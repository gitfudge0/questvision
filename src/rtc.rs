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
    sync::{
        Arc, Weak,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, Notify, mpsc, oneshot, watch};
use webrtc::{
    data_channel::{DataChannel, DataChannelEvent},
    media_stream::Track,
    media_stream::track_local::{TrackLocal, static_sample::TrackLocalStaticSample},
    peer_connection::{
        PeerConnection, PeerConnectionBuilder, PeerConnectionEventHandler, RTCIceGatheringState,
    },
    rtp_transceiver::RtpSender,
};

const CONTROL_CHANNEL_LABEL: &str = "questdisplay-control";
const CONTROL_MESSAGE_MAX_BYTES: usize = 4096;
const STATUS_ERROR_MAX_BYTES: usize = 256;
const STATUS_REFRESH_INTERVAL: Duration = Duration::from_millis(500);
const STATUS_SEND_RETRY_INTERVAL: Duration = Duration::from_millis(100);

struct ReceivedControl {
    bytes: Vec<u8>,
    received_at: Instant,
}

// Each kind has its own capacity-one inbox. A telemetry burst cannot overwrite
// the latest desired mode or ceiling; newer commands of the same kind supersede.
struct LatestMessage {
    sender: mpsc::Sender<ReceivedControl>,
    receiver: Mutex<mpsc::Receiver<ReceivedControl>>,
}

impl LatestMessage {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel(1);
        Self {
            sender,
            receiver: Mutex::new(receiver),
        }
    }

    async fn replace(&self, message: ReceivedControl) {
        let mut receiver = self.receiver.lock().await;
        if let Err(mpsc::error::TrySendError::Full(message)) = self.sender.try_send(message) {
            let _ = receiver.try_recv();
            let _ = self.sender.try_send(message);
        }
    }

    async fn clear(&self) {
        let _ = self.receiver.lock().await.try_recv();
    }

    async fn take(&self) -> Option<ReceivedControl> {
        self.receiver.lock().await.try_recv().ok()
    }
}

struct ChannelTask(tokio::task::JoinHandle<()>);

impl Drop for ChannelTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct ControlSession {
    accepted: AtomicBool,
    channel: Mutex<Option<Arc<dyn DataChannel>>>,
    poll_task: Mutex<Option<ChannelTask>>,
    available: watch::Sender<bool>,
    incoming: Notify,
    ingress: Mutex<()>,
    telemetry: LatestMessage,
    mode: LatestMessage,
    ceiling: LatestMessage,
}

impl ControlSession {
    fn new() -> Arc<Self> {
        let (available, _) = watch::channel(false);
        Arc::new(Self {
            accepted: AtomicBool::new(false),
            channel: Mutex::new(None),
            poll_task: Mutex::new(None),
            available,
            incoming: Notify::new(),
            ingress: Mutex::new(()),
            telemetry: LatestMessage::new(),
            mode: LatestMessage::new(),
            ceiling: LatestMessage::new(),
        })
    }

    async fn feedback_closed(&self) {
        self.available.send_replace(false);
        self.telemetry.clear().await;
        self.channel.lock().await.take();
        self.incoming.notify_one();
        tracing::debug!("browser control feedback unavailable; media session continues");
    }

    async fn stop(&self) {
        self.poll_task.lock().await.take();
        self.feedback_closed().await;
    }
}

async fn poll_control(channel: Arc<dyn DataChannel>, session: Weak<ControlSession>) {
    while let Some(event) = channel.poll().await {
        let Some(session) = session.upgrade() else {
            break;
        };
        match event {
            DataChannelEvent::OnOpen => {
                session.available.send_replace(true);
                session.incoming.notify_one();
            }
            DataChannelEvent::OnMessage(message) => {
                if !message.is_string || message.data.len() > CONTROL_MESSAGE_MAX_BYTES {
                    tracing::debug!("ignored non-text or oversized browser control message");
                    continue;
                }
                let received_at = Instant::now();
                let Ok(control) =
                    serde_json::from_slice::<crate::adaptive::ControlMessage>(&message.data)
                else {
                    tracing::debug!("ignored malformed browser control message");
                    continue;
                };
                let inbox = match control {
                    crate::adaptive::ControlMessage::BrowserStats(_) => &session.telemetry,
                    crate::adaptive::ControlMessage::SetMode(_) => &session.mode,
                    crate::adaptive::ControlMessage::SetCeiling(_) => &session.ceiling,
                };
                {
                    let _ingress = session.ingress.lock().await;
                    inbox
                        .replace(ReceivedControl {
                            bytes: message.data.to_vec(),
                            received_at,
                        })
                        .await;
                }
                session.incoming.notify_one();
            }
            DataChannelEvent::OnClosing | DataChannelEvent::OnClose => break,
            DataChannelEvent::OnError => {
                tracing::debug!("browser control channel reported an error");
            }
            _ => {}
        }
    }
    if let Some(session) = session.upgrade() {
        session.feedback_closed().await;
    }
}

#[derive(Default)]
struct NewerCommands {
    mode: Option<(Instant, crate::adaptive::AdaptationMode)>,
    ceiling: Option<(Instant, crate::quality::StreamSettings)>,
}

impl NewerCommands {
    fn retain(&mut self, message: &crate::adaptive::ControlMessage, received_at: Instant) {
        match message {
            crate::adaptive::ControlMessage::SetMode(mode) => {
                self.mode = Some((received_at, *mode));
            }
            crate::adaptive::ControlMessage::SetCeiling(ceiling) => {
                self.ceiling = Some((received_at, *ceiling));
            }
            _ => {}
        }
    }

    fn take(&mut self) -> Vec<(Instant, crate::adaptive::ControlMessage)> {
        let mut commands = Vec::with_capacity(2);
        if let Some((at, mode)) = self.mode.take() {
            commands.push((at, crate::adaptive::ControlMessage::SetMode(mode)));
        }
        if let Some((at, ceiling)) = self.ceiling.take() {
            commands.push((at, crate::adaptive::ControlMessage::SetCeiling(ceiling)));
        }
        commands.sort_by_key(|(at, _)| *at);
        commands
    }
}

fn route_control(
    controller: &mut crate::adaptive::AdaptiveController,
    message: crate::adaptive::ControlMessage,
    received_at: Instant,
) {
    match message {
        crate::adaptive::ControlMessage::BrowserStats(sample) => {
            controller.observe_browser(sample, received_at);
        }
        crate::adaptive::ControlMessage::SetMode(mode) => {
            controller.set_mode(mode);
        }
        crate::adaptive::ControlMessage::SetCeiling(ceiling) => {
            controller.set_ceiling(ceiling);
        }
    }
}

async fn run_controller(
    mut controller: crate::adaptive::AdaptiveController,
    control: Arc<ControlSession>,
    policy: watch::Sender<crate::quality::StreamSettings>,
    mut applied: watch::Receiver<Option<crate::capture::PolicyApplyResult>>,
    mut host_metrics: watch::Receiver<Option<crate::adaptive::HostMetrics>>,
    status: watch::Sender<crate::adaptive::ControllerUpdate>,
) {
    let mut available = control.available.subscribe();
    let mut browser_was_available = false;
    let mut applied_open = true;
    let mut metrics_open = true;
    let mut in_flight = None;
    let mut newer_commands = NewerCommands::default();
    let mut refresh = tokio::time::interval(STATUS_REFRESH_INTERVAL);
    refresh.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        let mut apply_changed = false;
        let mut metrics_changed = false;
        tokio::select! {
            _ = control.incoming.notified() => {}
            result = available.changed() => {
                if result.is_err() {
                    break;
                }
            }
            result = applied.changed(), if applied_open => {
                applied_open = result.is_ok();
                apply_changed = applied_open;
            }
            result = host_metrics.changed(), if metrics_open => {
                metrics_open = result.is_ok();
                metrics_changed = metrics_open;
            }
            _ = refresh.tick() => {}
        }
        let browser_available = *available.borrow_and_update();
        if browser_was_available && !browser_available {
            controller.mark_browser_unavailable();
        }
        browser_was_available = browser_available;

        // Short try_recv locks let the channel poller keep draining library events.
        let mut commands = Vec::with_capacity(3);
        {
            // Drain one consistent snapshot so a newer mode cannot slip behind
            // a later ceiling merely because their inbox locks were read apart.
            let _ingress = control.ingress.lock().await;
            for inbox in [&control.mode, &control.ceiling, &control.telemetry] {
                if let Some(message) = inbox.take().await
                    && let Ok(parsed) =
                        serde_json::from_slice::<crate::adaptive::ControlMessage>(&message.bytes)
                {
                    commands.push((message.received_at, parsed));
                }
            }
        }
        commands.sort_by_key(|(received_at, _)| *received_at);
        for (received_at, command) in commands {
            if matches!(command, crate::adaptive::ControlMessage::BrowserStats(_))
                && !browser_available
            {
                continue;
            }
            if in_flight.is_some() {
                newer_commands.retain(&command, received_at);
            }
            route_control(&mut controller, command, received_at);
        }
        if apply_changed {
            let result = applied.borrow_and_update().clone();
            if let Some(result) = result {
                if in_flight != Some(result.settings) {
                    tracing::warn!(
                        ?result,
                        ?in_flight,
                        "ignored unexpected capture acknowledgement"
                    );
                } else {
                    in_flight = None;
                    if let Some(error) = result.error {
                        status.send_replace(controller.report_apply_failure(error));
                        // A failed older request must not erase a user's newer choice.
                        for (received_at, command) in newer_commands.take() {
                            route_control(&mut controller, command, received_at);
                        }
                    } else {
                        controller.report_apply_success(result.settings);
                        newer_commands = NewerCommands::default();
                    }
                }
            }
        }
        if metrics_changed {
            let sample = host_metrics.borrow_and_update().clone();
            if let Some(sample) = sample {
                controller.observe_host(sample);
            }
        }
        if !applied_open && in_flight.take().is_some() {
            controller.report_apply_failure("capture apply feedback unavailable".into());
        }
        // Serialize applies so the latest-result watch cannot hide an intermediate
        // success behind a later failure. New desired state stays in the controller.
        if in_flight.is_none()
            && let Some(desired) = controller.desired_settings()
        {
            if policy.is_closed() || !applied_open {
                controller.report_apply_failure("capture policy control unavailable".into());
            } else {
                policy.send_replace(desired);
                in_flight = Some(desired);
                newer_commands = NewerCommands::default();
            }
        }
        status.send_replace(controller.current());
    }
}

fn encode_status(mut update: crate::adaptive::ControllerUpdate) -> Result<String> {
    if let Some(error) = update.error.as_mut()
        && error.len() > STATUS_ERROR_MAX_BYTES
    {
        let mut end = STATUS_ERROR_MAX_BYTES;
        while !error.is_char_boundary(end) {
            end -= 1;
        }
        error.truncate(end);
    }
    let text = serde_json::to_string(&crate::adaptive::HostMessage::Status(update))?;
    anyhow::ensure!(
        text.len() <= CONTROL_MESSAGE_MAX_BYTES,
        "host control status exceeded its message limit"
    );
    Ok(text)
}

async fn publish_status(
    control: Arc<ControlSession>,
    mut status: watch::Receiver<crate::adaptive::ControllerUpdate>,
) {
    let mut available = control.available.subscribe();
    let mut last_sent = None;
    let mut retry = tokio::time::interval(STATUS_SEND_RETRY_INTERVAL);
    retry.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            result = status.changed() => {
                if result.is_err() {
                    break;
                }
            }
            result = available.changed() => {
                if result.is_err() {
                    break;
                }
            }
            _ = retry.tick() => {}
        }
        if !*available.borrow_and_update() {
            continue;
        }
        let update = status.borrow_and_update().clone();
        let text = match encode_status(update) {
            Ok(text) => text,
            Err(error) => {
                tracing::warn!(%error, "could not serialize host control status");
                continue;
            }
        };
        if last_sent.as_ref() == Some(&text) {
            continue;
        }
        let channel = control.channel.lock().await.clone();
        let Some(channel) = channel else {
            continue;
        };
        // This is the only host writer: one outstanding status in SCTP and one
        // newest pending replacement. Never accumulate statuses for a stalled peer.
        match channel.outstanding_bytes().await {
            Ok(0) => match channel.try_send_text(&text).await {
                Ok(()) => last_sent = Some(text),
                Err(webrtc::error::Error::ErrSendBufferFull) => {}
                Err(error) => {
                    tracing::debug!(%error, "host status send failed; browser feedback unavailable");
                    control.feedback_closed().await;
                }
            },
            Ok(_) => {}
            Err(error) => {
                tracing::debug!(%error, "host status channel unavailable");
                control.feedback_closed().await;
            }
        }
    }
}

struct Handler {
    gathered: tokio::sync::Mutex<Option<oneshot::Sender<()>>>,
    // Avoid a PeerConnection -> handler -> channel -> PeerConnection Arc cycle.
    control: Weak<ControlSession>,
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

    async fn on_data_channel(&self, channel: Arc<dyn DataChannel>) {
        let Some(session) = self.control.upgrade() else {
            let _ = channel.close().await;
            return;
        };
        if channel.label().await.ok().as_deref() != Some(CONTROL_CHANNEL_LABEL)
            || session.accepted.swap(true, Ordering::Relaxed)
        {
            let _ = channel.close().await;
            return;
        }
        *session.channel.lock().await = Some(Arc::clone(&channel));
        *session.poll_task.lock().await = Some(ChannelTask(tokio::spawn(poll_control(
            channel,
            Arc::downgrade(&session),
        ))));
    }
}

pub async fn answer(
    offer_sdp: &str,
    lan_ip: &str,
    settings: crate::quality::StreamSettings,
    mode: crate::adaptive::AdaptationMode,
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
    let control = ControlSession::new();
    let controller = crate::adaptive::AdaptiveController::new(settings, mode);
    let (status_tx, status_rx) = watch::channel(controller.current());
    let (metrics_tx, metrics_rx) = watch::channel(None);
    let pc = PeerConnectionBuilder::new()
        .with_configuration(RTCConfigurationBuilder::new().build())
        .with_media_engine(media)
        .with_interceptor_registry(registry)
        .with_data_channel_send_buffer_limit(CONTROL_MESSAGE_MAX_BYTES)
        .with_handler(Arc::new(Handler {
            gathered: tokio::sync::Mutex::new(Some(tx)),
            control: Arc::downgrade(&control),
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
        let controller_task = ChannelTask(tokio::spawn(run_controller(
            controller,
            Arc::clone(&control),
            streams.policy,
            streams.applied,
            metrics_rx,
            status_tx,
        )));
        let status_task = ChannelTask(tokio::spawn(publish_status(
            Arc::clone(&control),
            status_rx,
        )));
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
        if let Err(err) = stream(track, sender, streams.video, credential_digest, metrics_tx).await
        {
            tracing::warn!("stream stopped: {err:#}");
        }
        if let Some(task) = audio_task {
            task.abort();
        }
        drop(controller_task);
        drop(status_task);
        control.stop().await;
        let _ = pc.close().await;
    });
    Ok((sdp, audio_available))
}

async fn stream(
    track: Arc<TrackLocalStaticSample>,
    sender: Arc<dyn RtpSender>,
    mut frames: tokio::sync::mpsc::Receiver<crate::capture::EncodedFrame>,
    credential_digest: String,
    metrics: watch::Sender<Option<crate::adaptive::HostMetrics>>,
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
        // Latest-only diagnostics never block media or discard encoded frames.
        metrics.send_replace(Some(frame.metrics));
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
