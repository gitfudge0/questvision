use anyhow::{Context, Result};
use bytes::Bytes;
use rtc::{
    interceptor::Registry,
    media::Sample,
    media_stream::MediaStreamTrack,
    peer_connection::{
        configuration::{
            RTCConfigurationBuilder,
            interceptor_registry::register_default_interceptors,
            media_engine::{MIME_TYPE_H264, MIME_TYPE_OPUS, MediaEngine},
        },
        sdp::RTCSessionDescription,
    },
    rtp_transceiver::rtp_sender::{
        RTCRtpCodec, RTCRtpCodecParameters, RTCRtpCodingParameters, RTCRtpEncodingParameters,
        RtpCodecKind,
    },
};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::oneshot;
use webrtc::{
    media_stream::Track,
    media_stream::track_local::{TrackLocal, static_sample::TrackLocalStaticSample},
    peer_connection::{
        PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCIceGatheringState,
    },
    rtp_transceiver::RtpSender,
};

struct Handler {
    gathered: tokio::sync::Mutex<Option<oneshot::Sender<()>>>,
}
#[async_trait::async_trait]
impl PeerConnectionEventHandler for Handler {
    async fn on_ice_gathering_state_change(&self, state: RTCIceGatheringState) {
        if state == RTCIceGatheringState::Complete
            && let Some(tx) = self.gathered.lock().await.take()
        {
            let _ = tx.send(());
        }
    }
}

pub async fn answer(
    offer_sdp: &str,
    lan_ip: &str,
    settings: crate::quality::StreamSettings,
    display: &str,
    credential_digest: String,
    audio_requested: bool,
) -> Result<(String, bool)> {
    let streams = crate::capture::start(settings, display, audio_requested).await?;
    let audio_available = streams.audio.is_some();
    let mut media = MediaEngine::default();
    let codec = RTCRtpCodecParameters {
        rtp_codec: RTCRtpCodec {
            mime_type: MIME_TYPE_H264.to_owned(),
            clock_rate: 90_000,
            channels: 0,
            sdp_fmtp_line: "level-asymmetry-allowed=1;packetization-mode=1;profile-level-id=42e01f"
                .into(),
            rtcp_feedback: vec![],
        },
        payload_type: 102,
    };
    media.register_codec(codec.clone(), RtpCodecKind::Video)?;
    let audio_codec = RTCRtpCodecParameters {
        rtp_codec: RTCRtpCodec {
            mime_type: MIME_TYPE_OPUS.to_owned(),
            clock_rate: 48_000,
            channels: 2,
            sdp_fmtp_line: "minptime=10;useinbandfec=1".into(),
            rtcp_feedback: vec![],
        },
        payload_type: 111,
    };
    if audio_available {
        media.register_codec(audio_codec.clone(), RtpCodecKind::Audio)?;
    }
    let registry = register_default_interceptors(Registry::new(), &mut media)?;
    let (tx, rx) = oneshot::channel();
    let pc = PeerConnectionBuilder::new()
        .with_configuration(RTCConfigurationBuilder::new().build())
        .with_media_engine(media)
        .with_interceptor_registry(registry)
        .with_handler(Arc::new(Handler {
            gathered: tokio::sync::Mutex::new(Some(tx)),
        }))
        .with_udp_addrs(vec![format!("{lan_ip}:0")])
        .build()
        .await?;
    let track = Arc::new(TrackLocalStaticSample::new(
        Instant::now(),
        MediaStreamTrack::new(
            "questdisplay".into(),
            "display-primary".into(),
            "Quest Display".into(),
            RtpCodecKind::Video,
            vec![RTCRtpEncodingParameters {
                rtp_coding_parameters: RTCRtpCodingParameters {
                    ssrc: Some(rand::random::<u32>()),
                    ..Default::default()
                },
                codec: codec.rtp_codec.clone(),
                ..Default::default()
            }],
        ),
    )?);
    let sender = pc
        .add_track(Arc::clone(&track) as Arc<dyn TrackLocal>)
        .await?;
    let audio_track_and_sender = if audio_available {
        let audio_track = Arc::new(TrackLocalStaticSample::new(
            Instant::now(),
            MediaStreamTrack::new(
                "questdisplay".into(),
                "desktop-audio".into(),
                "Quest Display Audio".into(),
                RtpCodecKind::Audio,
                vec![RTCRtpEncodingParameters {
                    rtp_coding_parameters: RTCRtpCodingParameters {
                        ssrc: Some(rand::random::<u32>()),
                        ..Default::default()
                    },
                    codec: audio_codec.rtp_codec.clone(),
                    ..Default::default()
                }],
            ),
        )?);
        let audio_sender = pc
            .add_track(Arc::clone(&audio_track) as Arc<dyn TrackLocal>)
            .await?;
        Some((audio_track, audio_sender))
    } else {
        None
    };
    let offer = RTCSessionDescription::offer(offer_sdp.to_owned())?;
    pc.set_remote_description(offer).await?;
    let answer = pc.create_answer(None).await?;
    pc.set_local_description(answer).await?;
    tokio::time::timeout(Duration::from_secs(8), rx)
        .await
        .context("ICE gathering timed out")??;
    let local = pc
        .local_description()
        .await
        .context("missing local answer")?;
    let sdp = local.sdp;
    tokio::spawn(async move {
        let audio_task = if let (Some((audio_track, audio_sender)), Some(audio_frames)) =
            (audio_track_and_sender, streams.audio)
        {
            Some(tokio::spawn(async move {
                if let Err(err) = stream_audio(audio_track, audio_sender, audio_frames).await {
                    tracing::warn!("audio stream stopped: {err:#}");
                }
            }))
        } else {
            None
        };
        if let Err(err) = stream(track, sender, streams.video, credential_digest).await {
            tracing::warn!("stream stopped: {err:#}");
        }
        if let Some(task) = audio_task {
            task.abort();
        }
        let _ = pc.close().await;
    });
    Ok((sdp, audio_available))
}

async fn stream(
    track: Arc<TrackLocalStaticSample>,
    sender: Arc<dyn RtpSender>,
    mut frames: tokio::sync::mpsc::Receiver<crate::capture::EncodedFrame>,
    credential_digest: String,
) -> Result<()> {
    let payload = sender
        .get_parameters()
        .await?
        .rtp_parameters
        .codecs
        .first()
        .context("browser did not negotiate H.264")?
        .payload_type;
    let ssrc = *track
        .ssrcs()
        .await
        .first()
        .context("video track has no SSRC")?;
    let mut last_credential_check = Instant::now();
    while let Some(frame) = frames.recv().await {
        if last_credential_check.elapsed() >= Duration::from_secs(1) {
            let active = crate::config::load()
                .map(|config| {
                    config.paired_tokens.iter().any(|saved| {
                        crate::security::constant_time_eq(
                            saved.as_bytes(),
                            credential_digest.as_bytes(),
                        )
                    })
                })
                .unwrap_or(false);
            if !active {
                anyhow::bail!("paired device revoked");
            }
            last_credential_check = Instant::now();
        }
        track
            .sample_writer(ssrc, payload)
            .write_sample(&Sample {
                data: Bytes::from(frame.data),
                duration: frame.duration,
                ..Sample::new(Instant::now())
            })
            .await?;
    }
    Ok(())
}

async fn stream_audio(
    track: Arc<TrackLocalStaticSample>,
    sender: Arc<dyn RtpSender>,
    mut frames: tokio::sync::mpsc::Receiver<crate::audio::EncodedAudio>,
) -> Result<()> {
    let payload = sender
        .get_parameters()
        .await?
        .rtp_parameters
        .codecs
        .first()
        .context("browser did not negotiate Opus")?
        .payload_type;
    let ssrc = *track
        .ssrcs()
        .await
        .first()
        .context("audio track has no SSRC")?;
    // Skip packets accumulated while ICE and SDP were negotiating.
    while frames.try_recv().is_ok() {}
    while let Some(frame) = frames.recv().await {
        track
            .sample_writer(ssrc, payload)
            .write_sample(&Sample {
                data: Bytes::from(frame.data),
                duration: crate::audio::PACKET_DURATION,
                ..Sample::new(Instant::now())
            })
            .await?;
    }
    Ok(())
}
