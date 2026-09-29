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

const BG: u32 = 0x11171c;
const PANEL: u32 = 0x1a232a;
const BORDER: u32 = 0x303d46;
const TEXT: u32 = 0xeaf0f3;
const MUTED: u32 = 0xa2b1bb;
const ACCENT: u32 = 0x56cdb4;

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
        let bounds = Bounds::centered(None, size(px(1080.), px(780.)), cx);
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

    fn overview(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        let active = self
            .snapshot
            .sessions
            .iter()
            .filter(|session| matches!(session.state, SessionState::Streaming))
            .count();
        let can_start = matches!(
            self.snapshot.lifecycle,
            Lifecycle::Stopped | Lifecycle::Error(_)
        );
        let can_stop = matches!(
            self.snapshot.lifecycle,
            Lifecycle::Starting | Lifecycle::Running
        );
        let mut runtime = card("HOST RUNTIME").child(
            div().flex().items_center().justify_between().gap_4().child(div().flex().flex_col().gap_2()
                .child(div().text_2xl().child(lifecycle_label(&self.snapshot.lifecycle)))
                .child(note(match self.snapshot.lifecycle { Lifecycle::Running => "Listening for a browser on your local network.", Lifecycle::Starting => "Preparing TLS and binding the host address…", Lifecycle::Stopping => "Closing connections and capture. Dismiss any open screen picker to finish.", _ => "Start the host, then open the connection URL in Quest Browser." })))
                .child(if can_start { button("start", "Start host").bg(rgb(ACCENT)).text_color(rgb(BG)).on_click(cx.listener(|this, _, _, cx| this.start(cx))).into_any_element() }
                       else { button("stop", if can_stop { "Stop host" } else { "Stopping…" }).when(can_stop, |button| button.on_click(cx.listener(|this, _, _, cx| { this.host.stop(); this.snapshot = this.host.snapshot(); cx.notify(); }))).into_any_element() })
        );
        if let Lifecycle::Error(error) = &self.snapshot.lifecycle {
            runtime = runtime.child(div().text_color(rgb(0xf0ae99)).child(error.clone()));
        }
        if let Some(notice) = &self.snapshot.notice {
            runtime = runtime.child(note(notice.clone()));
        }
        let mut connection = card("CONNECT YOUR QUEST");
        if let Some(url) = self
            .snapshot
            .url
            .as_ref()
            .filter(|_| matches!(self.snapshot.lifecycle, Lifecycle::Running))
        {
            let copy_url = url.clone();
            let open_url = url.clone();
            let mut details = div().flex().flex_col().gap_3().flex_1()
                .child(div().text_lg().text_color(rgb(ACCENT)).child(url.clone()))
                .child(note("Use the same LAN. Check this address before accepting the local certificate warning."))
                .child(div().flex().gap_2()
                    .child(button("copy-url", "Copy URL").on_click(move |_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(copy_url.clone()))))
                    .child(button("open-url", "Open in browser").on_click(move |_, _, cx| cx.open_url(&open_url))));
            if let Some(pair) = &self.snapshot.pending_pair {
                let seconds = pair
                    .expires_at
                    .saturating_duration_since(std::time::Instant::now())
                    .as_secs();
                details = details.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap_1()
                        .p_3()
                        .rounded_md()
                        .bg(rgb(0x20372f))
                        .child(note(format!(
                            "Pairing request from {} · expires in {seconds}s",
                            pair.peer
                        )))
                        .child(
                            div()
                                .text_3xl()
                                .text_color(rgb(ACCENT))
                                .child(pair.code.clone()),
                        )
                        .child(note("Enter this code in the requesting browser.")),
                );
            } else {
                details = details.child(note(
                    "Request pairing in the browser to display a six-digit code here.",
                ));
            }
            connection = connection.child(
                div()
                    .flex()
                    .items_start()
                    .gap_5()
                    .child(qr(url))
                    .child(details),
            );
        } else {
            connection = connection.child(note(
                "The connection URL and QR code appear after the host is listening.",
            ));
        }
        let sessions = card("STREAM HEALTH")
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .child(div().text_2xl().child(format!(
                        "{active} active stream{}",
                        if active == 1 { "" } else { "s" }
                    )))
                    .child(note("H.264 · OpenH264 software encoding")),
            )
            .when(self.snapshot.sessions.is_empty(), |card| {
                card.child(note(
                    "No browser session. Live telemetry appears when a browser starts streaming.",
                ))
            })
            .children(self.snapshot.sessions.iter().rev().map(session_card));
        let permission = card("SCREEN RECORDING")
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap_4()
                    .child(div().text_lg().child(match self.screen_recording_access {
                        Some(true) => "Granted",
                        Some(false) => "Not granted",
                        None => "Unavailable on this platform",
                    }))
                    .when(self.screen_recording_access == Some(false), |row| {
                        row.child(button("request-screen-recording", "Request permission").on_click(
                            cx.listener(|this, _, _, cx| this.request_screen_recording(cx)),
                        ))
                    }),
            )
            .child(note(match self.screen_recording_access {
                Some(true) => "Quest Display can request screen capture. Restart the app if macOS asks or capture still fails.",
                Some(false) => "Request access, then enable Quest Display in System Settings > Privacy & Security > Screen Recording. Restart the app if permission does not take effect.",
                None => "The operating system may ask you to choose a capture source when streaming starts.",
            }));
        div()
            .flex()
            .flex_col()
            .gap_4()
            .child(runtime)
            .child(permission)
            .child(connection)
            .child(sessions)
            .into_any_element()
    }

    fn devices(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        card("PAIRED BROWSERS")
            .child(note("Each ID identifies a paired browser credential. Revoking it requires the browser to pair again."))
            .when(self.device_ids.is_empty(), |card| card.child(div().py_4().child("No paired devices yet.")))
            .children(self.device_ids.iter().map(|id| {
                let revoke_id = id.clone();
                div().flex().items_center().justify_between().border_t_1().border_color(rgb(BORDER)).py_3()
                    .child(div().flex().flex_col().gap_1().child(format!("Browser {id}")).child(note("Device name and last-seen time are unavailable.")))
                    .child(button(SharedString::from(format!("revoke-{id}")), "Revoke").text_color(rgb(0xf0ae99)).on_click(cx.listener(move |this, _, _, cx| this.revoke(&revoke_id, cx))))
            })).into_any_element()
    }

    fn settings(&self, cx: &mut gpui::Context<Self>) -> AnyElement {
        card("HOST SETTINGS")
            .child(note("Save changes, then stop and start the host to apply them. Existing sessions keep their current settings until the host restarts."))
            .child(field("Listen address", "A private LAN or loopback IP address.", self.listen.clone()))
            .child(field("HTTPS port", "1–65535. The connection URL changes after restarting.", self.port.clone()))
            .child(field("Default capture FPS", "1–240. Browser requests and adaptive policy may choose a lower rate.", self.fps.clone()))
            .child(field("Default bitrate (Mbps)", "1–100. This is a configured target, not measured throughput.", self.bitrate.clone()))
            .child(div().flex().items_center().justify_between().py_2().gap_4().child(div().flex().flex_col().gap_1().child("Desktop audio").child(note("Requests native desktop audio; falls back to video if unavailable.")))
                .child(button("audio-toggle", if self.audio { "Audio on" } else { "Audio off" }).when(self.audio, |button| button.text_color(rgb(ACCENT))).on_click(cx.listener(|this, _, _, cx| { this.audio = !this.audio; cx.notify(); }))))
            .child(div().flex().justify_end().child(button("save", "Save settings").bg(rgb(ACCENT)).text_color(rgb(BG)).on_click(cx.listener(|this, _, _, cx| this.save(cx)))))
            .into_any_element()
    }

    fn diagnostics(&self) -> AnyElement {
        let mut host = card("HOST DIAGNOSTICS")
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
        if let Some(fingerprint) = &self.snapshot.fingerprint {
            let copy_fingerprint = fingerprint.clone();
            host = host.child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .pt_3()
                    .child(note("TLS certificate SHA-256"))
                    .child(
                        div().text_sm().child(
                            fingerprint
                                .split(':')
                                .take(16)
                                .collect::<Vec<_>>()
                                .join(":"),
                        ),
                    )
                    .child(
                        div().text_sm().child(
                            fingerprint
                                .split(':')
                                .skip(16)
                                .collect::<Vec<_>>()
                                .join(":"),
                        ),
                    )
                    .child(button("copy-fingerprint", "Copy fingerprint").on_click(
                        move |_, _, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(
                                copy_fingerprint.clone(),
                            ))
                        },
                    )),
            );
        }
        div().flex().flex_col().gap_4().child(host)
            .when(self.snapshot.sessions.is_empty(), |div| div.child(card("LIVE TELEMETRY").child(note("Unavailable — no browser session."))))
            .children(self.snapshot.sessions.iter().rev().map(|session| {
                let mut card = card(&format!("SESSION {} · {}", session.id, session.peer));
                let host_available = session.host_metrics.as_ref().is_some_and(|sample| sample.observed_at.elapsed() < Duration::from_secs(3)) && matches!(session.state, SessionState::Streaming);
                if let Some(sample) = session.host_metrics.as_ref().filter(|_| host_available) {
                    card = card.child(note("Host timings · latest encoded frame"))
                        .child(row("Capture delivery interval", optional_duration(sample.capture_receive_interval)))
                        .child(row("Resize", duration(sample.resize_time)))
                        .child(row("Color conversion", duration(sample.color_convert_time)))
                        .child(row("H.264 encode", duration(sample.encode_time)))
                        .child(row("Encoded queue wait (previous frame)", optional_duration(sample.queue_wait)))
                        .child(row("Queue saturated", if sample.queue_saturated { "Yes" } else { "No" }))
                        .child(row("Cadence missed", if sample.cadence_missed { "Yes" } else { "No" }))
                        .child(row("Frame age since capture delivery", duration(sample.frame_age_since_capture_delivery)));
                } else { card = card.child(note("Host timings: unavailable (no fresh frame).")); }
                let browser_available = session.controller.as_ref().is_some_and(|status| status.browser_feedback_available) && matches!(session.state, SessionState::Streaming);
                if let Some(sample) = session.browser_telemetry.as_ref().filter(|_| browser_available) {
                    card = card.child(div().pt_3().child(note("Browser receive telemetry · RTT is network round trip, not display latency")))
                        .child(row("Received bitrate", sample.received_bitrate_bps.map(|bps| format!("{:.2} Mbps", bps as f64 / 1_000_000.)).unwrap_or_else(unavailable)))
                        .child(row("Network RTT", optional_ms(sample.rtt_ms)))
                        .child(row("Jitter", optional_ms(sample.jitter_ms)))
                        .child(row("Packets lost (cumulative)", sample.packets_lost.map(|n| n.to_string()).unwrap_or_else(unavailable)))
                        .child(row("Frames decoded (cumulative)", sample.frames_decoded.map(|n| n.to_string()).unwrap_or_else(unavailable)))
                        .child(row("Frames dropped (cumulative)", sample.frames_dropped.map(|n| n.to_string()).unwrap_or_else(unavailable)));
                } else { card = card.child(note("Browser telemetry: unavailable (no fresh control-channel feedback).")); }
                card
            })).into_any_element()
    }
}

impl Render for Dashboard {
    fn render(&mut self, _: &mut Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let content = match self.page {
            Page::Overview => self.overview(cx),
            Page::Devices => self.devices(cx),
            Page::Settings => self.settings(cx),
            Page::Diagnostics => self.diagnostics(),
        };
        div()
            .flex()
            .size_full()
            .bg(rgb(BG))
            .text_color(rgb(TEXT))
            .text_size(px(14.))
            .font_family(ui_font())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(180.))
                    .flex_shrink_0()
                    .p_4()
                    .gap_2()
                    .bg(rgb(0x151d23))
                    .border_r_1()
                    .border_color(rgb(BORDER))
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .mb_1()
                            .child("Quest Display"),
                    )
                    .child(note("HOST CONSOLE"))
                    .child(div().h_4())
                    .children(
                        [
                            Page::Overview,
                            Page::Devices,
                            Page::Settings,
                            Page::Diagnostics,
                        ]
                        .into_iter()
                        .map(|page| {
                            button(SharedString::from(page.title()), page.title())
                                .w_full()
                                .when(self.page == page, |button| {
                                    button.bg(rgb(0x264138)).text_color(rgb(ACCENT))
                                })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.page = page;
                                    this.message = None;
                                    cx.notify();
                                }))
                        }),
                    )
                    .child(div().flex_1())
                    .child(note("Local host · view only"))
                    .child(note("Experimental build")),
            )
            .child(
                div()
                    .id("dashboard-scroll")
                    .flex_1()
                    .min_w_0()
                    .overflow_y_scroll()
                    .p_6()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .items_center()
                            .child(
                                div()
                                    .text_2xl()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(self.page.title()),
                            )
                            .child(
                                div()
                                    .rounded_full()
                                    .px_3()
                                    .py_1()
                                    .bg(rgb(PANEL))
                                    .text_color(rgb(
                                        if matches!(self.snapshot.lifecycle, Lifecycle::Running) {
                                            ACCENT
                                        } else {
                                            MUTED
                                        },
                                    ))
                                    .child(lifecycle_label(&self.snapshot.lifecycle)),
                            ),
                    )
                    .children(self.message.iter().map(|message| {
                        div()
                            .p_3()
                            .rounded_md()
                            .border_1()
                            .border_color(rgb(BORDER))
                            .bg(rgb(PANEL))
                            .child(message.clone())
                    }))
                    .child(content),
            )
    }
}

fn button(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Stateful<Div> {
    div()
        .id(id)
        .px_3()
        .py_2()
        .rounded_md()
        .bg(rgb(0x2a363f))
        .cursor_pointer()
        .text_sm()
        .child(label.into())
        .hover(|style| style.bg(rgb(0x364a54)))
}
fn card(title: &str) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_3()
        .p_4()
        .bg(rgb(PANEL))
        .border_1()
        .border_color(rgb(BORDER))
        .rounded_lg()
        .child(
            div()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(MUTED))
                .child(title.to_owned()),
        )
}
fn note(text: impl Into<SharedString>) -> Div {
    div().text_sm().text_color(rgb(MUTED)).child(text.into())
}
fn row(label: &str, value: impl Into<SharedString>) -> Div {
    div()
        .flex()
        .justify_between()
        .gap_4()
        .py_1()
        .child(note(label.to_owned()))
        .child(div().child(value.into()))
}
fn field(label: &str, help: &str, input: Entity<input::Input>) -> Div {
    div()
        .flex()
        .items_center()
        .gap_5()
        .py_2()
        .child(
            div()
                .flex_1()
                .flex()
                .flex_col()
                .gap_1()
                .child(label.to_owned())
                .child(note(help.to_owned())),
        )
        .child(div().w(px(240.)).child(input))
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
fn session_card(session: &SessionSnapshot) -> Div {
    let state = match &session.state {
        SessionState::Negotiating => "Negotiating".into(),
        SessionState::Streaming => "Streaming".into(),
        SessionState::Closed => "Closed".into(),
        SessionState::Error(error) => format!("Error: {error}"),
    };
    let mut card = div()
        .border_t_1()
        .border_color(rgb(BORDER))
        .pt_3()
        .flex()
        .flex_col()
        .gap_1()
        .child(row(
            &format!("Session {} · {}", session.id, session.peer),
            state,
        ))
        .child(row(
            "Audio track",
            if session.audio_active {
                "Available"
            } else {
                "Not active"
            },
        ));
    if let Some(status) = &session.controller {
        card = card
            .child(row("Effective quality", settings(status.effective)))
            .child(row("Selected ceiling", settings(status.ceiling)))
            .child(row(
                "Mode",
                match status.mode {
                    crate::adaptive::AdaptationMode::Adaptive => "Adaptive",
                    crate::adaptive::AdaptationMode::Fixed => "Fixed",
                },
            ))
            .child(row("Health / policy", reason(status.reason)))
            .child(row(
                "Fresh feedback",
                format!(
                    "Host: {} · Browser: {}",
                    if matches!(session.state, SessionState::Streaming)
                        && status.host_feedback_available
                        && session
                            .host_metrics
                            .as_ref()
                            .is_some_and(
                                |sample| sample.observed_at.elapsed() < Duration::from_secs(3)
                            )
                    {
                        "available"
                    } else {
                        "unavailable"
                    },
                    if matches!(session.state, SessionState::Streaming)
                        && status.browser_feedback_available
                    {
                        "available"
                    } else {
                        "unavailable"
                    }
                ),
            ));
        if let Some(error) = &status.error {
            card = card.child(note(error.clone()));
        }
    } else {
        card = card.child(note(
            "Effective quality and health feedback are unavailable until negotiation completes.",
        ));
    }
    card
}
fn settings(settings: crate::quality::StreamSettings) -> String {
    format!(
        "{} · {} FPS · {} Mbps target",
        match settings.preset {
            crate::quality::Preset::Performance => "Performance",
            crate::quality::Preset::Balanced => "Balanced",
            crate::quality::Preset::Quality => "Quality",
        },
        settings.fps,
        settings.bitrate_mbps
    )
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
            .size(px(164.))
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
