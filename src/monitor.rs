//! In-process host control and latest-value diagnostics for the desktop interface.
use crate::{
    adaptive::{BrowserTelemetry, ControllerUpdate, HostMetrics},
    config::Config,
};
use anyhow::Result;
use std::{
    net::{IpAddr, SocketAddr},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tokio::{runtime::Handle, sync::watch, task::JoinHandle};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Stopped,
    Starting,
    Running,
    Stopping,
    Error(String),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionState {
    Negotiating,
    Streaming,
    Closed,
    Error(String),
}
#[derive(Clone)]
pub struct PendingPair {
    pub peer: IpAddr,
    pub code: String,
    pub expires_at: Instant,
}
impl std::fmt::Debug for PendingPair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PendingPair")
            .field("peer", &self.peer)
            .field("code", &"[redacted]")
            .field("expires_at", &self.expires_at)
            .finish()
    }
}
#[derive(Clone, Debug)]
pub struct SessionSnapshot {
    pub id: u64,
    pub peer: SocketAddr,
    pub state: SessionState,
    pub host_metrics: Option<HostMetrics>,
    pub browser_telemetry: Option<BrowserTelemetry>,
    pub controller: Option<ControllerUpdate>,
    pub audio_active: bool,
}
#[derive(Clone, Debug)]
pub struct HostSnapshot {
    pub lifecycle: Lifecycle,
    pub url: Option<String>,
    pub fingerprint: Option<String>,
    pub effective_listen: Option<IpAddr>,
    pub notice: Option<String>,
    pub pending_pair: Option<PendingPair>,
    pub sessions: Vec<SessionSnapshot>,
}
#[derive(Clone)]
pub struct Monitor {
    snapshot: watch::Sender<HostSnapshot>,
    next_session: Arc<AtomicU64>,
}
impl Default for Monitor {
    fn default() -> Self {
        let (snapshot, _) = watch::channel(HostSnapshot {
            lifecycle: Lifecycle::Stopped,
            url: None,
            fingerprint: None,
            effective_listen: None,
            notice: None,
            pending_pair: None,
            sessions: Vec::new(),
        });
        Self {
            snapshot,
            next_session: Arc::new(AtomicU64::new(1)),
        }
    }
}
impl Monitor {
    pub fn snapshot(&self) -> HostSnapshot {
        self.expire_pair();
        self.snapshot.borrow().clone()
    }
    pub fn subscribe(&self) -> watch::Receiver<HostSnapshot> {
        self.snapshot.subscribe()
    }
    pub(crate) fn update(&self, update: impl FnOnce(&mut HostSnapshot)) {
        self.snapshot.send_modify(update);
    }
    pub(crate) fn expire_pair(&self) {
        self.snapshot.send_if_modified(|state| {
            if state
                .pending_pair
                .as_ref()
                .is_some_and(|pair| pair.expires_at <= Instant::now())
            {
                state.pending_pair = None;
                true
            } else {
                false
            }
        });
    }
    pub(crate) fn pair(&self, peer: IpAddr, code: String) {
        self.update(|state| {
            state.pending_pair = Some(PendingPair {
                peer,
                code,
                expires_at: Instant::now() + Duration::from_secs(120),
            })
        });
    }
    pub(crate) fn clear_pair(&self, peer: IpAddr) {
        self.update(|state| {
            if state
                .pending_pair
                .as_ref()
                .is_some_and(|pair| pair.peer == peer)
            {
                state.pending_pair = None;
            }
        });
    }
    pub(crate) fn session(&self, peer: SocketAddr) -> u64 {
        let id = self.next_session.fetch_add(1, Ordering::Relaxed);
        self.update(|state| {
            // Keep bounded history while retaining every active session.
            if state.sessions.len() >= 64 {
                state.sessions.retain(|session| {
                    matches!(
                        session.state,
                        SessionState::Negotiating | SessionState::Streaming
                    )
                });
            }
            state.sessions.push(SessionSnapshot {
                id,
                peer,
                state: SessionState::Negotiating,
                host_metrics: None,
                browser_telemetry: None,
                controller: None,
                audio_active: false,
            });
        });
        id
    }
    pub(crate) fn update_session(&self, id: u64, update: impl FnOnce(&mut SessionSnapshot)) {
        self.update(|state| {
            if let Some(session) = state.sessions.iter_mut().find(|session| session.id == id) {
                update(session);
            }
            let closed = state
                .sessions
                .iter()
                .filter(|session| {
                    matches!(session.state, SessionState::Closed | SessionState::Error(_))
                })
                .count();
            let mut remove = closed.saturating_sub(8);
            state.sessions.retain(|session| {
                if remove > 0
                    && matches!(session.state, SessionState::Closed | SessionState::Error(_))
                {
                    remove -= 1;
                    false
                } else {
                    true
                }
            });
        });
    }
}

/// Cancellation and task ownership shared with HTTP negotiations and media sessions.
pub(crate) struct SessionRuntime {
    pub monitor: Monitor,
    pub stop: watch::Sender<bool>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}
impl SessionRuntime {
    pub fn new(monitor: Monitor) -> Arc<Self> {
        let (stop, _) = watch::channel(false);
        Arc::new(Self {
            monitor,
            stop,
            tasks: Mutex::new(Vec::new()),
        })
    }
    pub fn spawn(&self, future: impl Future<Output = ()> + Send + 'static) {
        let mut tasks = self.tasks.lock().unwrap();
        tasks.retain(|task| !task.is_finished());
        tasks.push(tokio::spawn(future));
    }
    pub async fn shutdown(&self) {
        self.stop.send_replace(true);
        let tasks = std::mem::take(&mut *self.tasks.lock().unwrap());
        for task in tasks {
            let _ = task.await;
        }
    }
}
pub(crate) async fn cancelled(stop: &mut watch::Receiver<bool>) {
    loop {
        if *stop.borrow_and_update() {
            return;
        }
        if stop.changed().await.is_err() {
            return;
        }
    }
}

struct Run {
    stop: watch::Sender<bool>,
    done: watch::Receiver<bool>,
}
struct ControllerInner {
    handle: Handle,
    monitor: Monitor,
    run: Mutex<Option<Run>>,
}
impl Drop for ControllerInner {
    fn drop(&mut self) {
        if let Some(run) = self.run.get_mut().unwrap().as_ref() {
            run.stop.send_replace(true);
        }
    }
}
#[derive(Clone)]
pub struct HostController(Arc<ControllerInner>);
impl HostController {
    pub fn new(handle: Handle) -> Self {
        Self(Arc::new(ControllerInner {
            handle,
            monitor: Monitor::default(),
            run: Mutex::new(None),
        }))
    }
    pub fn monitor(&self) -> Monitor {
        self.0.monitor.clone()
    }
    pub fn snapshot(&self) -> HostSnapshot {
        self.0.monitor.snapshot()
    }
    /// Safe to call from a GUI thread outside the Tokio runtime.
    pub fn start(&self, config: Config) -> Result<()> {
        let mut run = self.0.run.lock().unwrap();
        if run.as_ref().is_some_and(|run| !*run.done.borrow()) {
            anyhow::bail!("host is already starting or running");
        }
        if let Err(error) = config.validate() {
            self.0
                .monitor
                .update(|state| state.lifecycle = Lifecycle::Error(error.to_string()));
            return Err(error);
        }
        let (stop, stop_rx) = watch::channel(false);
        let (done_tx, done) = watch::channel(false);
        *run = Some(Run { stop, done });
        let monitor = self.monitor();
        monitor.update(|state| {
            state.lifecycle = Lifecycle::Starting;
            state.url = None;
            state.fingerprint = None;
            state.effective_listen = None;
            state.notice = None;
            state.pending_pair = None;
            state.sessions.clear();
        });
        self.0.handle.spawn(async move {
            let result = crate::server::serve_embedded(config, monitor.clone(), stop_rx).await;
            monitor.update(|state| {
                state.lifecycle = match result {
                    Ok(()) => Lifecycle::Stopped,
                    Err(error) => Lifecycle::Error(format!("{error:#}")),
                };
                state.pending_pair = None;
            });
            done_tx.send_replace(true);
        });
        Ok(())
    }
    pub fn stop(&self) {
        let run = self.0.run.lock().unwrap();
        if let Some(run) = run.as_ref().filter(|run| !*run.done.borrow()) {
            self.0
                .monitor
                .update(|state| state.lifecycle = Lifecycle::Stopping);
            run.stop.send_replace(true);
        }
    }
    pub async fn shutdown(&self) {
        self.stop();
        let done = self
            .0
            .run
            .lock()
            .unwrap()
            .as_ref()
            .map(|run| run.done.clone());
        if let Some(mut done) = done {
            while !*done.borrow_and_update() {
                if done.changed().await.is_err() {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn expired_pairing_secret_is_cleared() {
        let monitor = Monitor::default();
        monitor.pair("127.0.0.1".parse().unwrap(), "123456".into());
        monitor.update(|state| {
            state.pending_pair.as_mut().unwrap().expires_at =
                Instant::now() - Duration::from_secs(1)
        });
        assert!(monitor.snapshot().pending_pair.is_none());
    }
    #[test]
    fn history_keeps_active_sessions_and_eight_closed_sessions() {
        let monitor = Monitor::default();
        let peer = "127.0.0.1:1234".parse().unwrap();
        let active = monitor.session(peer);
        for _ in 0..20 {
            let id = monitor.session(peer);
            monitor.update_session(id, |session| session.state = SessionState::Closed);
        }
        let snapshot = monitor.snapshot();
        assert_eq!(snapshot.sessions.len(), 9);
        assert!(
            snapshot
                .sessions
                .iter()
                .any(|session| session.id == active && session.state == SessionState::Negotiating)
        );
    }
    #[test]
    fn debug_output_redacts_pairing_code() {
        let monitor = Monitor::default();
        monitor.pair("127.0.0.1".parse().unwrap(), "123456".into());
        assert!(!format!("{:?}", monitor.snapshot()).contains("123456"));
    }
    #[tokio::test]
    async fn cancellation_observes_an_already_sent_stop() {
        let (sender, mut receiver) = watch::channel(false);
        sender.send_replace(true);
        tokio::time::timeout(Duration::from_millis(100), cancelled(&mut receiver))
            .await
            .unwrap();
    }
    #[tokio::test]
    async fn stopping_before_startup_finishes_returns_to_stopped() {
        let controller = HostController::new(Handle::current());
        controller.start(Config::default()).unwrap();
        assert_eq!(controller.snapshot().lifecycle, Lifecycle::Starting);
        controller.stop();
        assert_eq!(controller.snapshot().lifecycle, Lifecycle::Stopping);
        tokio::time::timeout(Duration::from_secs(1), controller.shutdown())
            .await
            .unwrap();
        assert_eq!(controller.snapshot().lifecycle, Lifecycle::Stopped);
    }
    #[tokio::test]
    async fn invalid_configuration_reports_error_without_starting() {
        let controller = HostController::new(Handle::current());
        let config = Config {
            port: 0,
            ..Config::default()
        };
        assert!(controller.start(config).is_err());
        assert!(matches!(
            controller.snapshot().lifecycle,
            Lifecycle::Error(_)
        ));
        controller.shutdown().await;
    }
}
