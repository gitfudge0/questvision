use crate::{
    adaptive::HostMetrics,
    audio::{EncodedAudio, Packetizer},
    quality::{Preset, StreamSettings},
};
use anyhow::{Context, Result};
use fast_image_resize::{FilterType, PixelType, ResizeAlg, ResizeOptions, Resizer, images::Image};
use openh264::{
    OpenH264API,
    encoder::{
        BitRate, Complexity, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, Profile,
        RateControlMode, UsageType,
    },
    formats::{BgraSliceU8, RgbaSliceU8, YUVBuffer, YUVSource},
};
use scrcap::{
    AudioConfig, CaptureConfig, CaptureDescriptor as _, PixFmt, Target, VideoConfig, VideoFrame,
    error::{CaptureError, Unsupported},
};
use std::{
    thread,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc, oneshot};

pub struct EncodedFrame {
    pub data: Vec<u8>,
    pub duration: Duration,
    pub resize_time: Duration,
    pub color_convert_time: Duration,
    pub encode_time: Duration,
    pub metrics: HostMetrics,
    pub output_size: (u32, u32),
}

pub struct CaptureStreams {
    pub video: mpsc::Receiver<EncodedFrame>,
    pub audio: Option<mpsc::Receiver<EncodedAudio>>,
}

pub async fn start(
    settings: StreamSettings,
    display: &str,
    audio_requested: bool,
) -> Result<CaptureStreams> {
    let (frames_tx, frames_rx) = mpsc::channel(1);
    let (audio_tx, audio_rx) = mpsc::channel(8);
    let (ready_tx, ready_rx) = oneshot::channel();
    let display = display.to_owned();
    let fps = settings.fps;
    let bitrate_mbps = settings.bitrate_mbps;
    thread::spawn(move || {
        let target = if display == "primary" {
            Target::Primary
        } else if let Ok(n) = display.parse::<isize>() {
            Target::Monitor(n)
        } else {
            let _ = ready_tx.send(Err(anyhow::anyhow!("unknown display")));
            return;
        };
        let capture_config = |with_audio: bool| CaptureConfig {
            video: VideoConfig {
                channel_capacity: 1,
                hide: vec![],
                target: target.clone(),
                fps: Some(fps),
            },
            audio: with_audio.then_some(AudioConfig {
                channel_capacity: 64,
            }),
        };
        let capture = match capture_config(audio_requested).create() {
            Err(CaptureError::Unsupported(Unsupported::Audio)) if audio_requested => {
                tracing::warn!("native desktop audio unavailable; continuing video only");
                capture_config(false).create()
            }
            other => other,
        };
        let capture = match capture {
            Ok(c) => c,
            Err(e) => {
                let _ = ready_tx.send(Err(anyhow::anyhow!("capture failed: {e}")));
                return;
            }
        };
        let config = EncoderConfig::new()
            .bitrate(BitRate::from_bps(bitrate_mbps * 1_000_000))
            .max_frame_rate(FrameRate::from_hz(fps as f32))
            .usage_type(UsageType::ScreenContentRealTime)
            .profile(Profile::Baseline)
            .rate_control_mode(RateControlMode::Bitrate)
            .complexity(if settings.preset == Preset::Quality {
                Complexity::Medium
            } else {
                Complexity::Low
            })
            .adaptive_quantization(false)
            .background_detection(false)
            .intra_frame_period(IntraFramePeriod::from_num_frames(fps * 2))
            .skip_frames(true);
        let encoder = Encoder::with_api_config(OpenH264API::from_source(), config);
        let mut encoder = match encoder {
            Ok(e) => e,
            Err(e) => {
                let _ = ready_tx.send(Err(anyhow::anyhow!("H.264 encoder failed: {e}")));
                return;
            }
        };
        let audio_capture = capture.audio().cloned();
        let mut audio_encoder = audio_capture.as_ref().and_then(|_| {
            match opus::Encoder::new(48_000, opus::Channels::Stereo, opus::Application::Audio) {
                Ok(encoder) => Some(encoder),
                Err(error) => {
                    tracing::warn!("Opus encoder unavailable; continuing video only: {error}");
                    None
                }
            }
        });
        let audio_available = audio_encoder.is_some();
        if let (Some(receiver), Some(mut encoder)) = (audio_capture, audio_encoder.take()) {
            if let Err(error) = encoder.set_bitrate(opus::Bitrate::Bits(128_000)) {
                tracing::warn!("Opus bitrate configuration failed: {error}");
            }
            thread::spawn(move || encode_audio(receiver, audio_tx, encoder));
        }
        if ready_tx.send(Ok(audio_available)).is_err() {
            capture.terminate();
            return;
        }
        let mut resizer = Resizer::new();
        let mut yuv_buffer = None;
        let mut previous: Option<Instant> = None;
        let frame_budget = Duration::from_secs_f64(1.0 / fps as f64);
        let mut previous_queue_observation: Option<(Duration, bool)> = None;
        while let Ok(frame) = capture.video().recv() {
            // Delivery is when recv returns, not the native desktop capture time.
            let capture_delivered_at = Instant::now();
            let capture_receive_interval =
                previous.map(|prev| capture_delivered_at.duration_since(prev));
            let duration = capture_receive_interval.unwrap_or(frame_budget);
            previous = Some(capture_delivered_at);
            match encode(&mut encoder, &mut resizer, &mut yuv_buffer, frame, settings) {
                Ok(result) if !result.data.is_empty() => {
                    let observed_at = Instant::now();
                    let frame_age_since_capture_delivery =
                        observed_at.duration_since(capture_delivered_at);
                    // A completed send's wait is only known after it consumes the frame.
                    // Carry it forward; the first sample has no previous observation.
                    let (queue_wait, queue_saturated) =
                        previous_queue_observation.unwrap_or((Duration::ZERO, false));
                    let encoded_frame = EncodedFrame {
                        data: result.data,
                        duration,
                        resize_time: result.resize_time,
                        color_convert_time: result.color_convert_time,
                        encode_time: result.encode_time,
                        metrics: HostMetrics {
                            observed_at,
                            capture_receive_interval,
                            resize_time: result.resize_time,
                            color_convert_time: result.color_convert_time,
                            encode_time: result.encode_time,
                            queue_wait,
                            queue_saturated,
                            // Allow receive jitter until a whole expected interval is missed.
                            cadence_missed: frame_age_since_capture_delivery > frame_budget
                                || capture_receive_interval
                                    .is_some_and(|interval| interval >= frame_budget * 2),
                            frame_age_since_capture_delivery,
                        },
                        output_size: result.output_size,
                    };
                    let queue_saturated = frames_tx.capacity() == 0;
                    let send_started = Instant::now();
                    if frames_tx.blocking_send(encoded_frame).is_err() {
                        break;
                    }
                    previous_queue_observation = Some((send_started.elapsed(), queue_saturated));
                }
                Ok(_) => {}
                Err(e) => {
                    tracing::warn!("encode failed: {e}");
                }
            }
        }
        capture.terminate();
    });
    let audio_available = tokio::time::timeout(Duration::from_secs(110), ready_rx)
        .await
        .context("screen capture permission timed out")?
        .context("capture thread exited")??;
    Ok(CaptureStreams {
        video: frames_rx,
        audio: audio_available.then_some(audio_rx),
    })
}

struct EncodeResult {
    data: Vec<u8>,
    resize_time: Duration,
    color_convert_time: Duration,
    encode_time: Duration,
    output_size: (u32, u32),
}

fn encode(
    encoder: &mut Encoder,
    resizer: &mut Resizer,
    yuv_buffer: &mut Option<YUVBuffer>,
    mut frame: VideoFrame,
    settings: StreamSettings,
) -> Result<EncodeResult> {
    let (w, h) = frame.size;
    anyhow::ensure!(w > 0 && h > 0, "capture dimensions must be positive");
    anyhow::ensure!(
        frame.vframe.len() == w as usize * h as usize * 4,
        "invalid frame length"
    );
    let (out_w, out_h) = settings.output_size(w, h).context("invalid capture size")?;
    let resize_started = Instant::now();
    let resized = if (w, h) != (out_w, out_h) {
        let source = Image::from_slice_u8(w, h, &mut frame.vframe, PixelType::U8x4)?;
        let mut output = Image::new(out_w, out_h, PixelType::U8x4);
        let filter = match settings.preset {
            Preset::Performance => FilterType::Hamming,
            Preset::Balanced => FilterType::CatmullRom,
            Preset::Quality => FilterType::Lanczos3,
        };
        let options = ResizeOptions::new()
            .use_alpha(false)
            .resize_alg(ResizeAlg::Convolution(filter));
        resizer.resize(&source, &mut output, &options)?;
        Some(output)
    } else {
        None
    };
    let resize_time = resize_started.elapsed();
    let convert_started = Instant::now();
    let pixels = resized
        .as_ref()
        .map(|image| image.buffer())
        .unwrap_or(&frame.vframe);
    let size = (out_w as usize, out_h as usize);
    if yuv_buffer.as_ref().map(|yuv| yuv.dimensions()) != Some(size) {
        *yuv_buffer = Some(YUVBuffer::new(size.0, size.1));
    }
    let yuv = yuv_buffer.as_mut().context("YUV buffer unavailable")?;
    match frame.pix_fmt {
        PixFmt::Bgra | PixFmt::Bgr0 => yuv.read_bgra8(BgraSliceU8::new(pixels, size)),
        PixFmt::Rgba | PixFmt::Rgb0 => yuv.read_rgba8(RgbaSliceU8::new(pixels, size)),
        _ => {
            let mut rgba = vec![0u8; pixels.len()];
            for (src, dst) in pixels.chunks_exact(4).zip(rgba.chunks_exact_mut(4)) {
                let (r, g, b) = match frame.pix_fmt {
                    PixFmt::Argb => (src[1], src[2], src[3]),
                    PixFmt::Abgr => (src[3], src[2], src[1]),
                    _ => unreachable!(),
                };
                dst.copy_from_slice(&[r, g, b, 255]);
            }
            yuv.read_rgba8(RgbaSliceU8::new(&rgba, size));
        }
    };
    let color_convert_time = convert_started.elapsed();
    let started = Instant::now();
    let data = encoder.encode(yuv)?.to_vec();
    Ok(EncodeResult {
        data,
        resize_time,
        color_convert_time,
        encode_time: started.elapsed(),
        output_size: (out_w, out_h),
    })
}

fn encode_audio(
    receiver: crossbeam_channel::Receiver<scrcap::AudioFrame>,
    sender: mpsc::Sender<EncodedAudio>,
    mut encoder: opus::Encoder,
) {
    let mut packetizer = Packetizer::new();
    while let Ok(frame) = receiver.recv() {
        let pcm_packets = match packetizer.push(&frame) {
            Ok(packets) => packets,
            Err(error) => {
                tracing::warn!("audio frame discarded: {error}");
                continue;
            }
        };
        for pcm in pcm_packets {
            let mut encoded = vec![0_u8; 4000];
            match encoder.encode_float(&pcm, &mut encoded) {
                Ok(length) => {
                    encoded.truncate(length);
                    if sender
                        .blocking_send(EncodedAudio { data: encoded })
                        .is_err()
                    {
                        return;
                    }
                }
                Err(error) => tracing::warn!("Opus encoding failed: {error}"),
            }
        }
    }
}
