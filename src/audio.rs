use anyhow::{Context, Result};
use scrcap::{AudioFrame, SampleFmt};
use std::{collections::VecDeque, time::Duration};

pub const SAMPLE_RATE: u32 = 48_000;
pub const SAMPLES_PER_PACKET: usize = 960;
pub const PACKET_DURATION: Duration = Duration::from_millis(20);

pub struct EncodedAudio {
    pub data: Vec<u8>,
}

/// Converts native PCM into continuous 48 kHz stereo 20 ms blocks.
/// The operating system may change its format or rate during a session.
pub struct Packetizer {
    pending: VecDeque<f32>,
    source_rate: u32,
    source_index: u64,
    next_output: f64,
    previous: Option<[f32; 2]>,
}
impl Packetizer {
    pub fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            source_rate: 0,
            source_index: 0,
            next_output: 0.0,
            previous: None,
        }
    }
    pub fn push(&mut self, frame: &AudioFrame) -> Result<Vec<Vec<f32>>> {
        let rate = u32::try_from(frame.sample_rate).context("invalid audio sample rate")?;
        anyhow::ensure!(
            (8_000..=384_000).contains(&rate),
            "invalid audio sample rate"
        );
        let channels = usize::try_from(frame.nb_channels).context("invalid audio channel count")?;
        let samples = usize::try_from(frame.nb_samples).context("invalid audio sample count")?;
        anyhow::ensure!(
            (1..=32).contains(&channels),
            "unsupported audio channel count"
        );
        anyhow::ensure!(samples <= 384_000, "audio frame too large");
        let (bytes_per_sample, planar) = match frame.sample_fmt {
            SampleFmt::U8 | SampleFmt::U8P => (1, frame.sample_fmt.is_planar()),
            SampleFmt::S16 | SampleFmt::S16P => (2, frame.sample_fmt.is_planar()),
            SampleFmt::S32 | SampleFmt::S32P | SampleFmt::F32 | SampleFmt::F32P => {
                (4, frame.sample_fmt.is_planar())
            }
            SampleFmt::F64 | SampleFmt::F64P => (8, frame.sample_fmt.is_planar()),
            _ => anyhow::bail!("unsupported audio sample format"),
        };
        let expected = samples
            .checked_mul(channels)
            .and_then(|n| n.checked_mul(bytes_per_sample))
            .context("audio frame length overflow")?;
        anyhow::ensure!(frame.aframe.len() == expected, "invalid audio frame length");
        if self.source_rate != rate {
            self.source_rate = rate;
            self.source_index = 0;
            self.next_output = 0.0;
            self.previous = None;
            self.pending.clear();
        }
        let ratio = rate as f64 / SAMPLE_RATE as f64;
        for i in 0..samples {
            let mut pair = [0.0_f32; 2];
            for (c, sample) in pair.iter_mut().enumerate().take(channels.min(2)) {
                let sample_index = if planar {
                    c * samples + i
                } else {
                    i * channels + c
                };
                let offset = sample_index * bytes_per_sample;
                *sample = decode_sample(
                    frame.sample_fmt,
                    &frame.aframe[offset..offset + bytes_per_sample],
                );
            }
            if channels == 1 {
                pair[1] = pair[0];
            }
            // Channels beyond stereo cannot be mapped correctly without a native channel layout.
            // Keep the front left/right pair and reject silent/non-finite input values.
            pair[0] = pair[0].clamp(-1.0, 1.0);
            pair[1] = pair[1].clamp(-1.0, 1.0);
            anyhow::ensure!(
                pair[0].is_finite() && pair[1].is_finite(),
                "non-finite audio sample"
            );
            let index = self.source_index as f64;
            if let Some(previous) = self.previous {
                while self.next_output <= index {
                    let t = (self.next_output - (index - 1.0)) as f32;
                    self.pending
                        .push_back(previous[0] + (pair[0] - previous[0]) * t);
                    self.pending
                        .push_back(previous[1] + (pair[1] - previous[1]) * t);
                    self.next_output += ratio;
                }
            } else {
                self.pending.extend(pair);
                self.next_output = ratio;
            }
            self.previous = Some(pair);
            self.source_index += 1;
        }
        let mut packets = Vec::new();
        while self.pending.len() >= SAMPLES_PER_PACKET * 2 {
            packets.push(self.pending.drain(..SAMPLES_PER_PACKET * 2).collect());
        }
        Ok(packets)
    }
}
fn decode_sample(format: SampleFmt, bytes: &[u8]) -> f32 {
    match format {
        SampleFmt::U8 | SampleFmt::U8P => (bytes[0] as f32 - 128.0) / 128.0,
        SampleFmt::S16 | SampleFmt::S16P => {
            i16::from_le_bytes(bytes.try_into().unwrap()) as f32 / 32768.0
        }
        SampleFmt::S32 | SampleFmt::S32P => {
            i32::from_le_bytes(bytes.try_into().unwrap()) as f32 / 2147483648.0
        }
        SampleFmt::F32 | SampleFmt::F32P => f32::from_le_bytes(bytes.try_into().unwrap()),
        SampleFmt::F64 | SampleFmt::F64P => f64::from_le_bytes(bytes.try_into().unwrap()) as f32,
        _ => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame(
        format: SampleFmt,
        channels: i32,
        samples: i32,
        rate: i32,
        data: Vec<u8>,
    ) -> AudioFrame {
        AudioFrame {
            aframe: data,
            nb_samples: samples,
            sample_rate: rate,
            nb_channels: channels,
            sample_fmt: format,
            ts: 0,
        }
    }
    #[test]
    fn planar_stereo_packet() {
        let mut bytes = vec![];
        for _ in 0..960 {
            bytes.extend_from_slice(&0.25_f32.to_le_bytes());
        }
        for _ in 0..960 {
            bytes.extend_from_slice(&(-0.5_f32).to_le_bytes());
        }
        let packets = Packetizer::new()
            .push(&frame(SampleFmt::F32P, 2, 960, 48_000, bytes))
            .unwrap();
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].len(), 1920);
        assert!((packets[0][0] - 0.25).abs() < 0.001);
        assert!((packets[0][1] + 0.5).abs() < 0.001);
    }
    #[test]
    fn mono_s16_and_resampling() {
        let bytes = [16384_i16.to_le_bytes(); 4410].concat();
        let packets = Packetizer::new()
            .push(&frame(SampleFmt::S16, 1, 4410, 44_100, bytes))
            .unwrap();
        assert_eq!(packets.len(), 4); // 100 ms minus the final interpolation boundary
        assert_eq!(packets[0][0], packets[0][1]);
        assert!((packets[0][0] - 0.5).abs() < 0.001);
    }
    #[test]
    fn bundled_opus_encodes_and_decodes_packet() {
        let mut encoder = opus::Encoder::new(
            SAMPLE_RATE,
            opus::Channels::Stereo,
            opus::Application::Audio,
        )
        .unwrap();
        let mut decoder = opus::Decoder::new(SAMPLE_RATE, opus::Channels::Stereo).unwrap();
        let pcm: Vec<f32> = (0..SAMPLES_PER_PACKET)
            .flat_map(|n| {
                let value =
                    ((n as f32 * 440.0 * std::f32::consts::TAU / SAMPLE_RATE as f32).sin()) * 0.2;
                [value, value]
            })
            .collect();
        let mut encoded = vec![0u8; 4000];
        let bytes = encoder.encode_float(&pcm, &mut encoded).unwrap();
        encoded.truncate(bytes);
        assert!(!encoded.is_empty());
        assert_eq!(
            decoder.get_nb_samples(&encoded).unwrap(),
            SAMPLES_PER_PACKET
        );
        let mut decoded = vec![0.0_f32; SAMPLES_PER_PACKET * 2];
        let samples = decoder.decode_float(&encoded, &mut decoded, false).unwrap();
        assert_eq!(samples, SAMPLES_PER_PACKET);
        assert!(decoded.iter().all(|sample| sample.is_finite()));
        assert!(decoded.iter().any(|sample| sample.abs() > 0.001));
    }
    #[test]
    fn rejects_short_frame() {
        assert!(
            Packetizer::new()
                .push(&frame(SampleFmt::F32P, 2, 960, 48_000, vec![0; 8]))
                .is_err()
        );
    }
}
