//! GPUI dashboard driving the host in the same process.
#[path = "gui_input.rs"]
mod input;

use crate::{
    config::{self, Config},
    macos_permissions,
    monitor::{HostController, HostSnapshot, Lifecycle, SessionSnapshot, SessionState},
};
use anyhow::{Context as _, Result};
use gpui::{prelude::*, *};
use std::time::Duration;

const BG: u32 = 0x20211f;
const PANEL: u32 = 0x2b2c29;
const BORDER: u32 = 0x3b3d35;
const TEXT: u32 = 0xf0f1e9;
const MUTED: u32 = 0xb0b5a9;
const ACCENT: u32 = 0xe9d86a;
const SOFT: u32 = 0x363831;
const ERROR: u32 = 0xe2b5a4;

actions!(dashboard, [Quit]);

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Overview,
    Devices,
    Settings,
    Diagnostics,
}
impl Page {
    fn title(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Devices => "Devices",
            Self::Settings => "Settings",
            Self::Diagnostics => "Diagnostics",
        }
    }
}

pub fn run(config: Config, handle: tokio::runtime::Handle) -> Result<HostController> {
    let host = HostController::new(handle);
    let app_host = host.clone();
    let startup_error = std::rc::Rc::new(std::cell::RefCell::new(None));
    let error = startup_error.clone();
    Application::new().run(move |cx: &mut App| {
        input::init(cx);
        cx.bind_keys([KeyBinding::new("secondary-q", Quit, None)]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        let quit_host = app_host.clone();
        cx.on_app_quit(move |_| {
            quit_host.stop();
            async {}
        })
        .detach();
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        let bounds = Bounds::centered(None, size(px(1120.), px(800.)), cx);
        if let Err(err) = cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(760.), px(600.))),
                titlebar: Some(TitlebarOptions {
                    title: Some("Quest Display".into()),
                    ..Default::default()
                }),
                app_id: Some("questdisplay".into()),
                ..Default::default()
            },
            |_, cx| cx.new(|cx| Dashboard::new(config, app_host, cx)),
        ) {
            *error.borrow_mut() = Some(err);
            cx.quit();
        }
        cx.activate(true);
    });
    host.stop();
    if let Some(err) = startup_error.borrow_mut().take() {
        return Err(err.context("opening GPUI dashboard"));
    }
    Ok(host)
}

struct Dashboard {
    host: HostController,
    snapshot: HostSnapshot,
    config: Config,
    page: Page,
    device_ids: Vec<String>,
    message: Option<String>,
    listen: Entity<input::Input>,
    port: Entity<input::Input>,
    fps: Entity<input::Input>,
    bitrate: Entity<input::Input>,
    audio: bool,
    screen_recording_access: Option<bool>,
    art: std::sync::Arc<Image>,
    _refresh: Task<()>,
}

impl Dashboard {
    fn new(config: Config, host: HostController, cx: &mut gpui::Context<Self>) -> Self {
        let listen = cx.new(|cx| input::Input::new(config.listen.to_string(), cx));
        let port = cx.new(|cx| input::Input::new(config.port.to_string(), cx));
        let fps = cx.new(|cx| input::Input::new(config.fps.to_string(), cx));
        let bitrate = cx.new(|cx| input::Input::new(config.bitrate_mbps.to_string(), cx));
        let executor = cx.background_executor().clone();
        let mut snapshots = host.monitor().subscribe();
        let refresh = cx.spawn(async move |this, cx| {
            let mut tick = 0u8;
            loop {
                executor.timer(Duration::from_millis(500)).await;
                if this
                    .update(cx, |view, cx| {
                        view.snapshot = snapshots.borrow_and_update().clone();
                        if let Some(address) = view.snapshot.effective_listen
                            && view.config.listen != address
                        {
                            view.config.listen = address;
                            view.listen = cx.new(|cx| input::Input::new(address.to_string(), cx));
                        }
                        tick = (tick + 1) % 4;
                        if tick == 0
                            && let Ok(config) = config::load()
                        {
                            view.device_ids = config.device_ids();
                        }
                        if tick == 0 {
                            view.screen_recording_access =
                                macos_permissions::screen_recording_access();
                        }
                        cx.notify();
                    })
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            snapshot: host.snapshot(),
            device_ids: config.device_ids(),
            audio: config.audio,
            screen_recording_access: macos_permissions::screen_recording_access(),
            config,
            host,
            page: Page::Overview,
            message: None,
            listen,
            port,
            fps,
            bitrate,
            art: std::sync::Arc::new(Image::from_bytes(
                ImageFormat::Png,
                include_bytes!("../assets/headset-studio.png").to_vec(),
            )),
            _refresh: refresh,
        }
    }

    fn start(&mut self, cx: &mut gpui::Context<Self>) {
        self.message = self
            .host
            .start(self.config.clone())
            .err()
            .map(|err| format!("Could not start host: {err:#}"));
        self.snapshot = self.host.snapshot();
        cx.notify();
    }

    fn request_screen_recording(&mut self, cx: &mut gpui::Context<Self>) {
        self.screen_recording_access = macos_permissions::request_screen_recording_access();
        self.message = Some(match self.screen_recording_access {
            Some(true) => "Screen Recording is granted. Restart Quest Display if macOS asks or capture still fails.".into(),
            Some(false) => "Enable Quest Display in System Settings > Privacy & Security > Screen Recording. Restart Quest Display if macOS asks or permission does not take effect.".into(),
            None => "Screen Recording permission status is unavailable on this platform.".into(),
        });
        cx.notify();
    }
    fn save(&mut self, cx: &mut gpui::Context<Self>) {
        // Reload credentials so saving settings cannot restore a revoked device.
        let result = config::update(|config| {
            config.listen = self
                .listen
                .read(cx)
                .value
                .parse()
                .context("Enter a valid IP address")?;
            config.port = self
                .port
                .read(cx)
                .value
                .parse()
                .context("Enter a port from 1 to 65535")?;
            config.fps = self
                .fps
                .read(cx)
                .value
                .parse()
                .context("Enter a whole number for FPS")?;
            config.bitrate_mbps = self
                .bitrate
                .read(cx)
                .value
                .parse()
                .context("Enter a whole number for bitrate")?;
            config.audio = self.audio;
            config.validate()?;
            Ok(())
        });
        match result {
            Ok(config) => {
                self.config = config;
                self.message = Some(
                    "Settings saved. They take effect the next time you start the host.".into(),
                );
            }
            Err(err) => self.message = Some(format!("Could not save settings: {err:#}")),
        }
        cx.notify();
    }
    fn revoke(&mut self, id: &str, cx: &mut gpui::Context<Self>) {
        let result = config::update(|config| config.remove_device(id));
        self.message = Some(match result {
            Ok(_) => "Device revoked. Active streaming stops at the next credential check.".into(),
            Err(err) => format!("Could not revoke device: {err:#}"),
        });
        if let Ok(config) = config::load() {
            self.device_ids = config.device_ids();
        }
        cx.notify();
    }

    fn active_streams(&self) -> usize {
        self.snapshot
            .sessions
            .iter()
            .filter(|session| matches!(session.state, SessionState::Streaming))
            .count()
    }

    fn host_action(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        let can_start = matches!(
            self.snapshot.lifecycle,
            Lifecycle::Stopped | Lifecycle::Error(_)
        );
        let can_stop = matches!(
            self.snapshot.lifecycle,
            Lifecycle::Starting | Lifecycle::Running
        );
        if can_start {
            button("start", "Start host")
                .bg(rgb(ACCENT))
                .text_color(rgb(BG))
                .hover(|style| style.bg(rgb(0xf1e38b)))
                .on_click(cx.listener(|this, _, _, cx| this.start(cx)))
                .into_any_element()
        } else {
            button("stop", if can_stop { "Stop host" } else { "Stopping…" })
                .when(!can_stop, |button| {
                    button.cursor(CursorStyle::Arrow).text_color(rgb(MUTED))
                })
                .when(can_stop, |button| {
                    button.on_click(cx.listener(|this, _, _, cx| {
                        this.host.stop();
                        this.snapshot = this.host.snapshot();
                        cx.notify();
                    }))
                })
                .into_any_element()
        }
    }

    fn connection(&self) -> Div {
        let mut connection = card(if self.snapshot.pending_pair.is_some() {
            "Pair this browser"
        } else {
            "Connect a device"
        });
        if let Some(url) = self
            .snapshot
            .url
            .as_ref()
            .filter(|_| matches!(self.snapshot.lifecycle, Lifecycle::Running))
        {
            let copy_url = url.clone();
            let open_url = url.clone();
            connection = connection.child(
                div()
                    .w_full()
                    .min_w_0()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(0x23261f))
                    .font_family("monospace")
                    .text_size(px(12.))
                    .child(url.clone()),
            );
            if let Some(pair) = self
                .snapshot
                .pending_pair
                .as_ref()
                .filter(|pair| pair.expires_at > std::time::Instant::now())
            {
                let seconds = pair
                    .expires_at
                    .saturating_duration_since(std::time::Instant::now())
                    .as_secs();
                connection = connection
                    .child(
                        div()
                            .text_3xl()
                            .font_family("monospace")
                            .text_color(rgb(ACCENT))
                            .child(pair.code.clone()),
                    )
                    .child(note(format!(
                        "Request from {} · expires in {seconds}s",
                        pair.peer
                    )))
                    .child(note("Enter this code in the requesting Quest Browser."));
            } else {
                connection = connection
                    .child(
                        div()
                            .text_size(px(12.))
                            .child("Open in Quest Browser · same Wi-Fi"),
                    )
                    .child(note(
                        "Check this address before accepting the local certificate warning.",
                    ))
                    .child(note(
                        "Request pairing in the browser to display a six-digit code here.",
                    ));
            }
            connection = connection.child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .mt_auto()
                    .pt_1()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_2()
                            .child(button("copy-url", "Copy URL").on_click(move |_, _, cx| {
                                cx.write_to_clipboard(ClipboardItem::new_string(copy_url.clone()))
                            }))
                            .child(
                                button("open-url", "Open in browser")
                                    .on_click(move |_, _, cx| cx.open_url(&open_url)),
                            ),
                    )
                    .child(
                        div()
                            .rounded_lg()
                            .overflow_hidden()
                            .flex_shrink_0()
                            .child(qr(url)),
                    ),
            );
        } else {
            connection = connection.child(note(match self.snapshot.lifecycle {
                Lifecycle::Starting => "Preparing the secure local host address…",
                Lifecycle::Stopping => "The host is closing its connections. Dismiss an open screen picker to finish.",
                Lifecycle::Error(_) => "A connection address is unavailable until the host starts.",
                _ => "Start the host to show its HTTPS address.",
            })).child(div().p_3().rounded_lg().bg(rgb(0x23261f)).text_color(rgb(MUTED)).child("Host address unavailable"))
                .child(note("Browser access requires a pairing code from this host. Your connection stays on the local network."))
                .child(div().mt_auto().pt_3().child(note("Use Quest Browser on the same Wi-Fi network.")));
        }
        connection
    }

    fn permission(&self, cx: &mut gpui::Context<Self>) -> Div {
        card("Screen recording")
            .child(chip(match self.screen_recording_access { Some(true) => "Granted", Some(false) => "Needs access", None => "Unavailable" }, self.screen_recording_access == Some(true)))
            .child(note(match self.screen_recording_access {
                Some(true) => "Access is granted. Restart Quest Display if macOS asks or capture still does not take effect.",
                Some(false) => "Request access, then enable Quest Display in System Settings → Privacy & Security → Screen Recording. Restart the app if permission does not take effect.",
                None => "Permission status is unavailable on this platform. The operating system may ask you to choose a screen when streaming starts.",
            }))
            .when(self.screen_recording_access == Some(false), |card| card.child(button("request-screen-recording", "Request permission")
                .on_click(cx.listener(|this, _, _, cx| this.request_screen_recording(cx)))))
    }

    fn hero(&self) -> Div {
        let title = if self.active_streams() > 0 {
            "Your desktop, a little further."
        } else if self.snapshot.pending_pair.is_some() {
            "A browser wants to pair."
        } else {
            match self.snapshot.lifecycle {
                Lifecycle::Stopped => "Ready for your headset.",
                Lifecycle::Starting => "Preparing your local host.",
                Lifecycle::Stopping => "Closing the connection.",
                Lifecycle::Error(_) => "The host needs attention.",
                Lifecycle::Running => {
                    if self
                        .snapshot
                        .sessions
                        .iter()
                        .any(|session| matches!(session.state, SessionState::Negotiating))
                    {
                        "Connecting to your browser."
                    } else {
                        "A private connection. Your own space."
                    }
                }
            }
        };
        div().w_full().flex().flex_col().flex_grow().min_h(px(320.)).rounded(px(16.)).bg(rgb(PANEL)).overflow_hidden()
            .child(art_stage(self.art.clone(), 220.))
            .child(div().p_5().flex().flex_col().gap_2()
                .child(div().text_size(px(23.)).font_weight(FontWeight::NORMAL).child(title))
                .child(note(if self.active_streams() > 0 { "A paired browser is streaming. Live desktop preview is unavailable in the host." } else { "Start the host, then open the connection address in Quest Browser." })))
    }

    fn overview(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        let active = self.active_streams();
        let sessions = card("Session")
            .child(chip(
                format!(
                    "{active} active stream{}",
                    if active == 1 { "" } else { "s" }
                ),
                active > 0,
            ))
            .when(self.snapshot.sessions.is_empty(), |card| {
                card.child(note(
                    "Stream health appears when a paired browser starts sharing a desktop.",
                ))
                .child(row("Audio track", "Not active"))
                .child(row("Effective quality", "Unavailable"))
            })
            .children(self.snapshot.sessions.iter().rev().map(session_card));
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(columns(self.hero(), self.connection()))
            .child(columns(sessions, self.permission(cx)))
            .into_any_element()
    }

    fn devices(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        let devices = card("Paired browsers")
            .when(self.device_ids.is_empty(), |card| card.child(div().flex().flex_col().items_center().justify_center().gap_3().py_12()
                .child(div().text_2xl().child("No paired browsers yet"))
                .child(note("Open the host address in Quest Browser and request a code to pair your browser."))))
            .children(self.device_ids.iter().map(|id| {
                let revoke_id = id.clone();
                div().flex().items_center().justify_between().gap_3().border_t_1().border_color(rgb(BORDER)).py_4()
                    .child(div().flex_1().min_w_0().flex().flex_col().gap_2().child("Browser")
                        .child(div().font_family("monospace").text_size(px(12.)).children(id.as_bytes().chunks(24).map(|chunk| div().child(String::from_utf8_lossy(chunk).into_owned()))))
                        .child(note("Device name and last-seen time are unavailable.")))
                    .child(button(SharedString::from(format!("revoke-{id}")), "Revoke").text_color(rgb(ERROR)).bg(rgb(0x423931))
                        .on_click(cx.listener(move |this, _, _, cx| this.revoke(&revoke_id, cx))))
            }));
        columns(devices, div().flex().flex_col().gap_4().child(self.connection())
            .child(card("About browser access").child(note("Each pairing saves a browser credential. Revoking it removes future access. An active stream stops at the next credential check.")))).into_any_element()
    }

    fn settings(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        let form = card("Host defaults")
            .child(field(
                "Listen address",
                "Use a private LAN or loopback IP address.",
                self.listen.clone(),
            ))
            .child(field(
                "HTTPS port",
                "A number from 1 to 65535.",
                self.port.clone(),
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap_4()
                    .child(div().flex_1().min_w(px(190.)).child(field(
                        "Default capture FPS",
                        "1–240 FPS",
                        self.fps.clone(),
                    )))
                    .child(div().flex_1().min_w(px(190.)).child(field(
                        "Default bitrate (Mbps)",
                        "1–100 Mbps",
                        self.bitrate.clone(),
                    ))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .py_3()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child("Desktop audio")
                            .child(note(if self.audio { "Audio on" } else { "Audio off" })),
                    )
                    .child(
                        button("audio-toggle", if self.audio { "On  ●" } else { "●  Off" })
                            .when(self.audio, |button| {
                                button.bg(rgb(ACCENT)).text_color(rgb(BG))
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.audio = !this.audio;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                button("save", "Save settings")
                    .bg(rgb(ACCENT))
                    .text_color(rgb(BG))
                    .hover(|style| style.bg(rgb(0xf1e38b)))
                    .on_click(cx.listener(|this, _, _, cx| this.save(cx))),
            );
        let aside = div().flex().flex_col().gap_4()
            .child(div().flex().flex_col().rounded(px(16.)).bg(rgb(PANEL)).overflow_hidden()
                .child(art_stage(self.art.clone(), 180.))
                .child(div().p_5().child(note("A local connection. Your own space."))))
            .child(card("When settings apply")
                .child(note("Saving updates the configuration. It does not restart the current host or change an existing stream."))
                .child(note("Stop the host, then start it to use saved values."))
                .child(note("FPS and bitrate defaults are targets, not measured throughput. Browser requests and adaptation can lower them."))
                .child(note("Desktop audio availability depends on capture and negotiation; video continues if audio is unavailable.")));
        columns(form, aside).into_any_element()
    }

    fn diagnostics(&self) -> AnyElement {
        let host = card("Host environment")
            .child(row("Platform", std::env::consts::OS))
            .child(row(
                "Capture source",
                "System-selected display / native OS picker",
            ))
            .child(row("Video encoder", "OpenH264 (software)"))
            .child(row(
                "Adaptive thresholds",
                if crate::adaptive::PROVISIONAL_POLICY.provisional {
                    "Provisional — not empirically calibrated"
                } else {
                    "Calibrated"
                },
            ))
            .child(row(
                "Glass-to-glass latency",
                "Unavailable — no physical measurement",
            ))
            .child(row(
                "Hardware encoding",
                "Unavailable — no native hardware encoder",
            ))
            .child(row(
                "Quest validation",
                "Unavailable — no physical Quest test recorded",
            ));
        let mut secure = card("Secure local connection").child(note("TLS certificate SHA-256"));
        if let Some(fingerprint) = &self.snapshot.fingerprint {
            let copy_fingerprint = fingerprint.clone();
            let components: Vec<_> = fingerprint.split(':').collect();
            secure = secure
                .child(
                    div()
                        .p_3()
                        .rounded_lg()
                        .bg(rgb(0x23261f))
                        .font_family("monospace")
                        .text_size(px(11.))
                        .children(
                            components
                                .chunks(8)
                                .map(|parts| div().child(parts.join(":"))),
                        ),
                )
                .child(
                    button("copy-fingerprint", "Copy fingerprint").on_click(move |_, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copy_fingerprint.clone()))
                    }),
                );
        } else {
            secure = secure.child(note(
                "Unavailable — start the host to load its certificate.",
            ));
        }
        let mut content = div().flex().flex_col().gap_4();
        if self.snapshot.sessions.is_empty() {
            content = content.child(balanced_columns(
                card("Host timings")
                    .child(chip("Unavailable", false))
                    .child(note("Unavailable — no browser session.")),
                card("Browser telemetry")
                    .child(chip("Unavailable", false))
                    .child(note("Unavailable — no browser session.")),
            ));
        }
        for session in self.snapshot.sessions.iter().rev() {
            let host_available = session
                .host_metrics
                .as_ref()
                .is_some_and(|sample| sample.observed_at.elapsed() < Duration::from_secs(3))
                && matches!(session.state, SessionState::Streaming);
            let browser_available = session
                .controller
                .as_ref()
                .is_some_and(|status| status.browser_feedback_available)
                && matches!(session.state, SessionState::Streaming);
            let mut timings = card("Host timings").child(chip(
                if host_available {
                    "Fresh"
                } else {
                    "Unavailable"
                },
                host_available,
            ));
            if let Some(sample) = session.host_metrics.as_ref().filter(|_| host_available) {
                timings = timings
                    .child(row(
                        "Capture delivery interval",
                        optional_duration(sample.capture_receive_interval),
                    ))
                    .child(row("Resize", duration(sample.resize_time)))
                    .child(row("Color conversion", duration(sample.color_convert_time)))
                    .child(row("H.264 encode", duration(sample.encode_time)))
                    .child(row(
                        "Encoded queue wait · previous frame",
                        optional_duration(sample.queue_wait),
                    ))
                    .child(row(
                        "Frame age since capture delivery",
                        duration(sample.frame_age_since_capture_delivery),
                    ))
                    .child(row(
                        "Queue saturated",
                        if sample.queue_saturated { "Yes" } else { "No" },
                    ))
                    .child(row(
                        "Cadence missed",
                        if sample.cadence_missed { "Yes" } else { "No" },
                    ));
            } else {
                timings = timings.child(note("Unavailable — no fresh frame."));
            }
            let mut browser = card("Browser telemetry").child(chip(
                if browser_available {
                    "Fresh"
                } else {
                    "Unavailable"
                },
                browser_available,
            ));
            if let Some(sample) = session
                .browser_telemetry
                .as_ref()
                .filter(|_| browser_available)
            {
                browser = browser
                    .child(row(
                        "Received bitrate",
                        sample
                            .received_bitrate_bps
                            .map(|bps| format!("{:.2} Mbps", bps as f64 / 1_000_000.))
                            .unwrap_or_else(unavailable),
                    ))
                    .child(row("Network round trip", optional_ms(sample.rtt_ms)))
                    .child(row("Jitter", optional_ms(sample.jitter_ms)))
                    .child(row(
                        "Lost packets · cumulative",
                        sample
                            .packets_lost
                            .map(|n| n.to_string())
                            .unwrap_or_else(unavailable),
                    ))
                    .child(row(
                        "Decoded frames · cumulative",
                        sample
                            .frames_decoded
                            .map(|n| n.to_string())
                            .unwrap_or_else(unavailable),
                    ))
                    .child(row(
                        "Dropped frames · cumulative",
                        sample
                            .frames_dropped
                            .map(|n| n.to_string())
                            .unwrap_or_else(unavailable),
                    ));
            } else {
                browser = browser.child(note("Unavailable — no fresh control-channel feedback."));
            }
            browser = browser.child(note("Network round trip is not display latency."));
            content = content
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(rgb(MUTED))
                        .child(format!(
                            "Session {} · {} · {}",
                            session.id,
                            session.peer,
                            session_state(&session.state)
                        )),
                )
                .when(matches!(session.state, SessionState::Error(_)), |content| {
                    if let SessionState::Error(error) = &session.state {
                        content.child(notice(error, true))
                    } else {
                        content
                    }
                })
                .child(balanced_columns(timings, browser));
        }
        content
            .child(balanced_columns(host, secure))
            .into_any_element()
    }
}

impl Render for Dashboard {
    fn render(&mut self, window: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let content = match self.page {
            Page::Overview => self.overview(cx),
            Page::Devices => self.devices(cx),
            Page::Settings => self.settings(cx),
            Page::Diagnostics => self.diagnostics(),
        };
        let narrow = window.bounds().size.width < px(930.);
        let title = match self.page {
            Page::Overview => {
                if self.snapshot.pending_pair.is_some() {
                    "A browser wants to pair."
                } else {
                    "Host overview"
                }
            }
            Page::Devices => "Paired browsers",
            Page::Settings => "Host defaults",
            Page::Diagnostics => "Behind the stream",
        };
        let subtitle = match self.page {
            Page::Overview => "A private local connection, ready for your headset.",
            Page::Devices => "Manage the browser credentials allowed to connect.",
            Page::Settings => "Saved values take effect the next time you start the host.",
            Page::Diagnostics => "Timing, browser feedback, and secure connection details.",
        };
        let nav = div().flex().gap_1().children(
            [
                Page::Overview,
                Page::Devices,
                Page::Settings,
                Page::Diagnostics,
            ]
            .into_iter()
            .map(|page| {
                button(SharedString::from(page.title()), page.title())
                    .rounded_full()
                    .border_color(rgba(0x00000000))
                    .bg(rgb(BG))
                    .text_color(rgb(MUTED))
                    .when(self.page == page, |button| {
                        button.bg(rgb(SOFT)).text_color(rgb(ACCENT))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.page = page;
                        this.message = None;
                        cx.notify();
                    }))
            }),
        );
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .text_size(px(13.))
            .font_family(ui_font())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_shrink_0()
                    .px_6()
                    .py_4()
                    .gap_3()
                    .border_b_1()
                    .border_color(rgb(0x2b2c29))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_5()
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap_2()
                                    .flex_shrink_0()
                                    .child(div().text_2xl().text_color(rgb(ACCENT)).child("◈"))
                                    .child(
                                        div()
                                            .text_lg()
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child("Quest Display"),
                                    ),
                            )
                            .when(!narrow, |header| header.child(nav))
                            .child(div().flex_1())
                            .child(
                                chip(
                                    lifecycle_label(&self.snapshot.lifecycle),
                                    matches!(self.snapshot.lifecycle, Lifecycle::Running),
                                )
                                .map(|mut chip| {
                                    chip.style().align_self = Some(AlignSelf::Center);
                                    chip
                                }),
                            )
                            .child(self.host_action(cx)),
                    )
                    .when(narrow, |header| {
                        header.child(
                            div().flex().gap_1().children(
                                [
                                    Page::Overview,
                                    Page::Devices,
                                    Page::Settings,
                                    Page::Diagnostics,
                                ]
                                .into_iter()
                                .map(|page| {
                                    button(
                                        SharedString::from(format!("nav-{}", page.title())),
                                        page.title(),
                                    )
                                    .rounded_full()
                                    .border_color(rgba(0x00000000))
                                    .bg(rgb(BG))
                                    .text_color(rgb(MUTED))
                                    .when(self.page == page, |button| {
                                        button.bg(rgb(SOFT)).text_color(rgb(ACCENT))
                                    })
                                    .on_click(cx.listener(
                                        move |this, _, _, cx| {
                                            this.page = page;
                                            this.message = None;
                                            cx.notify();
                                        },
                                    ))
                                }),
                            ),
                        )
                    }),
            )
            .child(
                div()
                    .id("dashboard-scroll")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .items_start()
                            .justify_between()
                            .gap_4()
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap_2()
                                    .child(
                                        div()
                                            .text_size(px(10.))
                                            .text_color(rgb(MUTED))
                                            .child(self.page.title().to_uppercase()),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(27.))
                                            .font_weight(FontWeight::NORMAL)
                                            .child(title),
                                    )
                                    .child(note(subtitle)),
                            )
                            .child(chip(
                                if self.page == Page::Devices {
                                    format!("{} saved credentials", self.device_ids.len())
                                } else {
                                    "Local host · view only".into()
                                },
                                false,
                            )),
                    )
                    .children(self.message.iter().map(|message| notice(message, false)))
                    .when(
                        matches!(self.snapshot.lifecycle, Lifecycle::Error(_)),
                        |main| {
                            if let Lifecycle::Error(error) = &self.snapshot.lifecycle {
                                main.child(notice(error, true))
                            } else {
                                main
                            }
                        },
                    )
                    .children(
                        self.snapshot
                            .notice
                            .iter()
                            .map(|message| notice(message, false)),
                    )
                    .child(content)
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .gap_4()
                            .border_t_1()
                            .border_color(rgb(BORDER))
                            .pt_3()
                            .text_size(px(10.))
                            .text_color(rgb(MUTED))
                            .child("Local host · view only")
                            .child("Experimental build"),
                    ),
            )
    }
}

fn art_stage(art: std::sync::Arc<Image>, height: f32) -> Div {
    div()
        .relative()
        .w_full()
        .h(px(height))
        .flex_none()
        .overflow_hidden()
        .rounded_t(px(16.))
        .bg(rgb(PANEL))
        .child(
            img(art)
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .object_fit(ObjectFit::Cover)
                .rounded_t(px(16.)),
        )
        // GPUI's content mask is rectangular. Cover can paint a larger image whose rounded
        // corners lie outside the stage, so trim the two visible stage corners explicitly.
        .child(
            canvas(
                |_, _, _| (),
                |bounds, _, window, _| {
                    let radius = px(16.);
                    let control = radius * 0.44771525;
                    for (corner, direction) in [(bounds.origin, 1.), (bounds.top_right(), -1.)] {
                        let mut path = PathBuilder::fill();
                        path.move_to(corner);
                        path.line_to(corner + point(radius * direction, px(0.)));
                        path.cubic_bezier_to(
                            corner + point(px(0.), radius),
                            corner + point(control * direction, px(0.)),
                            corner + point(px(0.), control),
                        );
                        path.close();
                        if let Ok(path) = path.build() {
                            window.paint_path(path, rgb(BG));
                        }
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
        .child(
            div()
                .absolute()
                .top_3()
                .left_3()
                .child(chip("Illustrative headset art", false)),
        )
}

fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .map(|mut button| {
            button.style().align_self = Some(AlignSelf::FlexStart);
            button
        })
        .flex_none()
        .whitespace_nowrap()
        .px_3()
        .py_2()
        .min_h(px(36.))
        .rounded_lg()
        .bg(rgb(SOFT))
        .border_1()
        .border_color(rgb(BORDER))
        .cursor_pointer()
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .focusable()
        .tab_stop(true)
        .focus(|style| style.border_color(rgb(ACCENT)))
        .child(label.into())
        .hover(|style| style.bg(rgb(0x45483c)))
}
fn card(title: &str) -> Div {
    div()
        .flex_grow()
        .w_full()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_3()
        .p_5()
        .bg(rgb(PANEL))
        .border_1()
        .border_color(rgb(0x35372f))
        .rounded(px(16.))
        .child(
            div()
                .text_size(px(14.))
                .font_weight(FontWeight::MEDIUM)
                .child(title.to_owned()),
        )
}
fn columns(left: impl IntoElement, right: impl IntoElement) -> Div {
    // Flex wrapping also keeps unusually narrow windows and long session history scrollable.
    div()
        .flex()
        .flex_wrap()
        .map(|mut row| {
            row.style().align_items = Some(AlignItems::Stretch);
            row
        })
        .gap_4()
        .child(div().flex().flex_col().flex_1().min_w(px(340.)).child(left))
        .child(
            div()
                .flex()
                .flex_col()
                .w(relative(0.38))
                .flex_shrink_0()
                .min_w(px(280.))
                .child(right),
        )
}
fn balanced_columns(left: impl IntoElement, right: impl IntoElement) -> Div {
    div()
        .flex()
        .flex_wrap()
        .map(|mut row| {
            row.style().align_items = Some(AlignItems::Stretch);
            row
        })
        .gap_4()
        .child(div().flex().flex_col().flex_1().min_w(px(300.)).child(left))
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w(px(300.))
                .child(right),
        )
}
fn chip(text: impl Into<SharedString>, active: bool) -> Div {
    div()
        .map(|mut chip| {
            chip.style().align_self = Some(AlignSelf::FlexStart);
            chip
        })
        .flex_none()
        .whitespace_nowrap()
        .flex()
        .items_center()
        .gap_2()
        .rounded_full()
        .px_3()
        .py_1()
        .bg(rgb(SOFT))
        .text_size(px(11.))
        .text_color(rgb(if active { ACCENT } else { MUTED }))
        .child(
            div()
                .size(px(5.))
                .rounded_full()
                .bg(rgb(if active { ACCENT } else { MUTED })),
        )
        .child(text.into())
}
fn notice(text: &str, error: bool) -> Div {
    div()
        .w_full()
        .min_w_0()
        .p_3()
        .rounded_lg()
        .border_1()
        .border_color(rgb(if error { 0x725347 } else { 0x625d3a }))
        .bg(rgb(if error { 0x42312a } else { 0x3c3827 }))
        .text_color(rgb(if error { ERROR } else { 0xe7d8ab }))
        .child(text.to_owned())
}
fn note(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(px(12.))
        .text_color(rgb(MUTED))
        .child(text.into())
}
fn row(label: &str, value: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .justify_between()
        .items_start()
        .gap_4()
        .py_1()
        .child(div().flex_1().min_w_0().child(note(label.to_owned())))
        .child(div().flex_1().min_w_0().text_right().child(value.into()))
}
fn field(label: &str, help: &str, input: Entity<input::Input>) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .py_2()
        .child(label.to_owned())
        .child(input)
        .child(note(help.to_owned()))
}
fn lifecycle_label(state: &Lifecycle) -> &'static str {
    match state {
        Lifecycle::Stopped => "Stopped",
        Lifecycle::Starting => "Starting",
        Lifecycle::Running => "Running",
        Lifecycle::Stopping => "Stopping",
        Lifecycle::Error(_) => "Host error",
    }
}
fn session_state(state: &SessionState) -> String {
    match state {
        SessionState::Negotiating => "Negotiating".into(),
        SessionState::Streaming => "Streaming".into(),
        SessionState::Closed => "Closed".into(),
        SessionState::Error(_) => "Error".into(),
    }
}
fn quality_readout(label: &str, value: Option<crate::quality::StreamSettings>) -> Div {
    let mut readout = div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .gap_1()
        .child(note(label.to_owned()));
    if let Some(value) = value {
        readout = readout
            .child(div().text_size(px(21.)).child(match value.preset {
                crate::quality::Preset::Performance => "Performance",
                crate::quality::Preset::Balanced => "Balanced",
                crate::quality::Preset::Quality => "Quality",
            }))
            .child(note(format!(
                "{} FPS · {} Mbps target",
                value.fps, value.bitrate_mbps
            )));
    } else {
        readout = readout
            .child(div().text_lg().child("Unavailable"))
            .child(note("No confirmed quality"));
    }
    readout
}
fn session_card(session: &SessionSnapshot) -> Div {
    let streaming = matches!(session.state, SessionState::Streaming);
    let mut card = div()
        .w_full()
        .min_w_0()
        .border_t_1()
        .border_color(rgb(BORDER))
        .pt_3()
        .flex()
        .flex_col()
        .gap_3()
        .child(
            div()
                .flex()
                .flex_wrap()
                .justify_between()
                .gap_2()
                .child(div().child(format!("Session {} · {}", session.id, session.peer)))
                .child(chip(session_state(&session.state), streaming)),
        )
        .child(
            div()
                .flex()
                .gap_4()
                .child(quality_readout(
                    "Effective stream",
                    session.controller.as_ref().map(|status| status.effective),
                ))
                .child(quality_readout(
                    "Selected ceiling",
                    session.controller.as_ref().map(|status| status.ceiling),
                )),
        )
        .child(row(
            "Audio track",
            if session.audio_active {
                "Available"
            } else {
                "Not active"
            },
        ));
    if let SessionState::Error(error) = &session.state {
        card = card.child(notice(error, true));
    }
    if let Some(status) = &session.controller {
        let host_fresh = streaming
            && status.host_feedback_available
            && session
                .host_metrics
                .as_ref()
                .is_some_and(|sample| sample.observed_at.elapsed() < Duration::from_secs(3));
        let browser_fresh = streaming && status.browser_feedback_available;
        card = card
            .child(row(
                "Mode",
                match status.mode {
                    crate::adaptive::AdaptationMode::Adaptive => "Adaptive",
                    crate::adaptive::AdaptationMode::Fixed => "Fixed",
                },
            ))
            .child(row(
                "Host feedback",
                if host_fresh { "Fresh" } else { "Unavailable" },
            ))
            .child(row(
                "Browser feedback",
                if browser_fresh {
                    "Fresh"
                } else {
                    "Unavailable"
                },
            ))
            .child(
                div()
                    .p_3()
                    .rounded_lg()
                    .bg(rgb(if status.error.is_some() {
                        0x43312a
                    } else if status.reason != crate::adaptive::PressureReason::Stable {
                        0x3d3725
                    } else {
                        0x23281e
                    }))
                    .text_color(rgb(if status.error.is_some() {
                        ERROR
                    } else {
                        0xe6d79e
                    }))
                    .child(reason(status.reason)),
            );
        if let Some(error) = &status.error {
            card = card.child(note(error.clone()));
        }
        if !streaming {
            card = card.child(note("Quality readouts retain the latest confirmed session settings. Live feedback is unavailable."));
        }
    } else {
        card = card.child(note("Effective quality and health feedback are unavailable until negotiation completes. The operating system may show a screen picker."));
    }
    card
}
fn reason(reason: crate::adaptive::PressureReason) -> &'static str {
    use crate::adaptive::PressureReason::*;
    match reason {
        Stable => "Stable",
        NetworkPressure => "Network pressure",
        HostPressure => "Host processing pressure",
        CombinedPressure => "Host and network pressure",
        BrowserFeedbackUnavailable => "Browser feedback unavailable",
        HostFeedbackUnavailable => "Host feedback unavailable",
        AtLowestTier => "At lowest quality tier",
        ApplyFailed => "Quality change failed",
    }
}
fn unavailable() -> String {
    "Unavailable".into()
}
fn duration(value: Duration) -> String {
    format!("{:.2} ms", value.as_secs_f64() * 1000.)
}
fn optional_duration(value: Option<Duration>) -> String {
    value.map(duration).unwrap_or_else(unavailable)
}
fn optional_ms(value: Option<f64>) -> String {
    value
        .filter(|n| n.is_finite() && *n >= 0.)
        .map(|n| format!("{n:.2} ms"))
        .unwrap_or_else(unavailable)
}

fn qr(url: &str) -> AnyElement {
    match qrcode::QrCode::new(url.as_bytes()) {
        Ok(code) => {
            let width = code.width();
            let modules = code.to_colors();
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    window.paint_quad(fill(bounds, rgb(0xffffff)));
                    let module = (f32::from(bounds.size.width) / (width + 8) as f32).floor();
                    let offset = (f32::from(bounds.size.width) - module * (width + 8) as f32) / 2.;
                    for y in 0..width {
                        for x in 0..width {
                            if modules[y * width + x] == qrcode::Color::Dark {
                                window.paint_quad(fill(
                                    Bounds::new(
                                        point(
                                            bounds.left() + px(offset + (x + 4) as f32 * module),
                                            bounds.top() + px(offset + (y + 4) as f32 * module),
                                        ),
                                        size(px(module), px(module)),
                                    ),
                                    rgb(0x000000),
                                ));
                            }
                        }
                    }
                },
            )
            .size(px(104.))
            .flex_shrink_0()
            .into_any_element()
        }
        Err(_) => note("QR code unavailable").into_any_element(),
    }
}

fn ui_font() -> &'static str {
    if cfg!(target_os = "macos") {
        ".SystemUIFont"
    } else if cfg!(target_os = "windows") {
        "Segoe UI"
    } else {
        "sans-serif"
    }
}
