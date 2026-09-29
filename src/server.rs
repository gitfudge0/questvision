use crate::{
    config::{Config, LanAddress},
    security::{PairConfirm, PairToken, Pairing, constant_time_eq, random_hex},
};
use anyhow::Result;
use axum::{
    Json, Router,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode, Uri, header},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, IsTerminal},
    net::{IpAddr, SocketAddr, TcpListener},
    sync::Arc,
};
use tokio::sync::Mutex;

pub struct AppState {
    pub config: Mutex<Config>,
    pub pairing: Mutex<Pairing>,
    pub address: SocketAddr,
    pub runtime: Arc<crate::monitor::SessionRuntime>,
    pub embedded: bool,
}
type Shared = Arc<AppState>;

#[derive(Serialize)]
struct Status {
    paired: bool,
    quality_controls: bool,
    input_modes: &'static [&'static str],
    codec: &'static str,
    audio: bool,
}
#[derive(Serialize)]
struct Display {
    id: String,
    name: String,
}
#[derive(Serialize)]
struct Displays {
    displays: Vec<Display>,
}
#[derive(Deserialize)]
struct Offer {
    sdp: String,
    display_id: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    quality: Option<String>,
    fps: Option<u32>,
    bitrate_mbps: Option<u32>,
    mode: Option<crate::adaptive::AdaptationMode>,
}
#[derive(Serialize)]
struct Answer {
    sdp: String,
}

pub async fn serve(config: Config) -> Result<()> {
    let (stop, receiver) = tokio::sync::watch::channel(false);
    let serving = serve_inner(config, crate::monitor::Monitor::default(), receiver, false);
    tokio::pin!(serving);
    tokio::select! {
        result = &mut serving => result,
        result = tokio::signal::ctrl_c() => {
            result?;
            stop.send_replace(true);
            serving.await
        }
    }
}

pub(crate) async fn serve_embedded(
    config: Config,
    monitor: crate::monitor::Monitor,
    stop: tokio::sync::watch::Receiver<bool>,
) -> Result<()> {
    serve_inner(config, monitor, stop, true).await
}

async fn serve_inner(
    mut config: Config,
    monitor: crate::monitor::Monitor,
    mut stop: tokio::sync::watch::Receiver<bool>,
    embedded: bool,
) -> Result<()> {
    config.validate()?;
    if *stop.borrow() {
        return Ok(());
    }
    let configured_ip = config.listen;
    let (listener, address) = bind_host(config.listen, config.port)?;
    config.listen = address.ip();
    if address.ip() != configured_ip {
        let refreshed = address.ip();
        let notice = if embedded {
            match crate::config::update(|saved| {
                if saved.listen == configured_ip {
                    saved.listen = refreshed;
                }
                Ok(())
            }) {
                Ok(_) => format!(
                    "Saved host address {configured_ip} is no longer available. Refreshed it to {refreshed}; use this address to connect your Quest."
                ),
                Err(error) => format!(
                    "Saved host address {configured_ip} is no longer available. Using {refreshed} for this session, but could not save the refreshed address: {error:#}"
                ),
            }
        } else {
            format!(
                "Host address {configured_ip} is unavailable; using current LAN address {refreshed}."
            )
        };
        monitor.update(|state| {
            state.effective_listen = Some(refreshed);
            state.notice = Some(notice.clone());
        });
        if !embedded {
            eprintln!("{notice}");
        }
    }
    let (tls, fingerprint) = tokio::select! {
        result = certificate(address.ip()) => result?,
        _ = crate::monitor::cancelled(&mut stop) => return Ok(()),
    };
    serve_prepared(config, monitor, stop, embedded, listener, tls, fingerprint).await
}

fn current_private_lan_addresses() -> Vec<IpAddr> {
    let mut addresses: Vec<_> = local_ip_address::list_afinet_netifas()
        .unwrap_or_default()
        .into_iter()
        .map(|(_, address)| address)
        .filter(LanAddress::is_private_lan)
        .collect();
    if let Some(preferred) = local_ip_address::local_ip()
        .ok()
        .filter(LanAddress::is_private_lan)
    {
        addresses.retain(|address| *address != preferred);
        addresses.insert(0, preferred);
    }
    addresses.dedup();
    addresses
}

fn bind_with_fallback<L>(
    configured_ip: IpAddr,
    port: u16,
    candidates: impl IntoIterator<Item = IpAddr>,
    mut bind: impl FnMut(SocketAddr) -> io::Result<L>,
) -> io::Result<(L, SocketAddr)> {
    let requested = SocketAddr::new(configured_ip, port);
    match bind(requested) {
        Ok(listener) => Ok((listener, requested)),
        Err(error)
            if error.kind() == io::ErrorKind::AddrNotAvailable
                && configured_ip.is_private_lan() =>
        {
            let mut last_error = error;
            for candidate in candidates
                .into_iter()
                .filter(|ip| *ip != configured_ip && ip.is_private_lan())
            {
                let address = SocketAddr::new(candidate, port);
                match bind(address) {
                    Ok(listener) => return Ok((listener, address)),
                    Err(error) if error.kind() == io::ErrorKind::AddrNotAvailable => {
                        last_error = error;
                    }
                    Err(error) => return Err(error),
                }
            }
            Err(io::Error::new(
                io::ErrorKind::AddrNotAvailable,
                format!(
                    "saved host address {configured_ip} is unavailable and no current private LAN address could be bound; reconnect to a private network or choose an active address in Settings ({last_error})"
                ),
            ))
        }
        Err(error) => Err(error),
    }
}

fn bind_host(configured_ip: IpAddr, port: u16) -> Result<(TcpListener, SocketAddr)> {
    let (listener, address) = bind_with_fallback(
        configured_ip,
        port,
        current_private_lan_addresses(),
        TcpListener::bind,
    )
    .map_err(|error| anyhow::anyhow!(error).context("binding host address"))?;
    listener.set_nonblocking(true)?;
    Ok((listener, address))
}

async fn serve_prepared(
    config: Config,
    monitor: crate::monitor::Monitor,
    mut stop: tokio::sync::watch::Receiver<bool>,
    embedded: bool,
    listener: TcpListener,
    tls: axum_server::tls_rustls::RustlsConfig,
    fingerprint: String,
) -> Result<()> {
    let address = SocketAddr::new(config.listen, config.port);
    let runtime = crate::monitor::SessionRuntime::new(monitor.clone());
    let state = Arc::new(AppState {
        config: Mutex::new(config),
        pairing: Mutex::new(Pairing::new()),
        address,
        runtime: runtime.clone(),
        embedded,
    });
    let app = Router::new()
        .route("/", get(index))
        .route("/api/status", get(status))
        .route("/api/displays", get(displays))
        .route("/api/pair/start", post(pair_start))
        .route("/api/pair/confirm", post(pair_confirm))
        .route("/api/offer", post(offer))
        .with_state(state);
    let handle = axum_server::Handle::new();
    let server = axum_server::from_tcp_rustls(listener, tls)?
        .handle(handle.clone())
        .serve(app.into_make_service_with_connect_info::<SocketAddr>());
    tokio::pin!(server);
    let mut listening = false;
    let mut refresh = tokio::time::interval(std::time::Duration::from_secs(1));
    let result = loop {
        tokio::select! {
            result = &mut server => break result,
            _ = refresh.tick() => monitor.expire_pair(),
            bound = handle.listening(), if !listening => {
                listening = true;
                if bound.is_some() {
                    let url = format!("https://{address}");
                    monitor.update(|state| {
                        if state.lifecycle != crate::monitor::Lifecycle::Stopping {
                            state.lifecycle = crate::monitor::Lifecycle::Running;
                        }
                        state.url = Some(url.clone());
                        state.fingerprint = Some(fingerprint.clone());
                    });
                    if !embedded {
                        println!("Quest Display running at {url}");
                        println!("TLS certificate SHA-256: {fingerprint}");
                        if let Ok(qr) = qrcode::QrCode::new(url.as_bytes()) {
                            println!("{}", qr.render::<qrcode::render::unicode::Dense1x2>().build());
                        }
                        println!("Accept the local self-signed certificate warning in your browser.");
                        if std::io::stderr().is_terminal() {
                            println!("Pairing codes will appear in this host terminal.");
                        } else {
                            println!("Pairing requires an interactive host terminal; service output is not used for secrets.");
                        }
                    }
                }
            }
            _ = crate::monitor::cancelled(&mut stop) => {
                runtime.stop.send_replace(true);
                handle.graceful_shutdown(Some(std::time::Duration::from_secs(5)));
                break server.await;
            }
        }
    };
    runtime.shutdown().await;
    result?;
    Ok(())
}

fn reject(status: StatusCode, message: &'static str) -> Response {
    (status, Json(serde_json::json!({"error":message}))).into_response()
}
fn valid_host(headers: &HeaderMap, uri: &Uri, address: SocketAddr) -> bool {
    let expected = address.to_string();
    let header_host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    let authority = uri.authority().map(|a| a.as_str());
    (header_host.is_some() || authority.is_some())
        && header_host.is_none_or(|host| host == expected)
        && authority.is_none_or(|host| host == expected)
}
fn valid_origin(headers: &HeaderMap, address: SocketAddr) -> bool {
    let expected = format!("https://{address}");
    headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) == Some(expected.as_str())
}
fn valid_peer(peer: IpAddr) -> bool {
    peer.is_loopback() || peer.is_private_lan()
}
fn guard(
    headers: &HeaderMap,
    uri: &Uri,
    peer: SocketAddr,
    state: &AppState,
    post: bool,
) -> Option<Response> {
    if !valid_peer(peer.ip()) {
        return Some(reject(StatusCode::FORBIDDEN, "LAN clients only"));
    }
    if !valid_host(headers, uri, state.address) {
        return Some(reject(StatusCode::BAD_REQUEST, "invalid host"));
    }
    if post && !valid_origin(headers, state.address) {
        return Some(reject(StatusCode::FORBIDDEN, "invalid origin"));
    }
    None
}
fn authorized(headers: &HeaderMap) -> Option<String> {
    let value = headers
        .get(header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?;
    let digest = token_digest(value);
    let persisted = crate::config::load().ok()?;
    persisted
        .paired_tokens
        .iter()
        .any(|saved| constant_time_eq(saved.as_bytes(), digest.as_bytes()))
        .then_some(digest)
}
async fn index(
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Some(r) = guard(&headers, &uri, peer, &state, false) {
        return r;
    }
    let page = include_str!("../web/index.html");
    let mut r = Html(page).into_response();
    let policy = format!(
        "default-src 'self'; connect-src 'self'; script-src {}; style-src {}; img-src 'self' data:; media-src 'self' blob:; object-src 'none'; frame-ancestors 'none'; base-uri 'none'",
        inline_hash(page, "script"),
        inline_hash(page, "style")
    );
    r.headers_mut()
        .insert(header::CONTENT_SECURITY_POLICY, policy.parse().unwrap());
    r.headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    r
}
async fn status(
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Some(r) = guard(&headers, &uri, peer, &state, false) {
        return r;
    }
    let paired = authorized(&headers).is_some();
    Json(Status {
        paired,
        quality_controls: true,
        input_modes: &["view_only"],
        codec: "H264",
        audio: state.config.lock().await.audio,
    })
    .into_response()
}
async fn displays(
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Some(r) = guard(&headers, &uri, peer, &state, false) {
        return r;
    }
    if authorized(&headers).is_none() {
        return reject(StatusCode::UNAUTHORIZED, "pair first");
    }
    Json(Displays {
        displays: vec![Display {
            id: "primary".into(),
            name: "System-selected display".into(),
        }],
    })
    .into_response()
}
async fn pair_start(
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
) -> Response {
    if let Some(r) = guard(&headers, &uri, peer, &state, true) {
        return r;
    }
    if !state.embedded && !std::io::stderr().is_terminal() {
        return reject(
            StatusCode::SERVICE_UNAVAILABLE,
            "pairing requires an interactive host terminal",
        );
    }
    let Some((response, code)) = state.pairing.lock().await.start(peer.ip()) else {
        return reject(StatusCode::TOO_MANY_REQUESTS, "try again later");
    };
    if state.embedded {
        state.runtime.monitor.pair(peer.ip(), code);
    } else {
        eprintln!(
            "Pairing request from {}: {} (expires in 2 minutes)",
            peer.ip(),
            code
        );
    }
    Json(response).into_response()
}
async fn pair_confirm(
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<PairConfirm>,
) -> Response {
    if let Some(r) = guard(&headers, &uri, peer, &state, true) {
        return r;
    }
    if !state
        .pairing
        .lock()
        .await
        .confirm(peer.ip(), &request.request_id, &request.code)
    {
        return reject(StatusCode::UNAUTHORIZED, "invalid or expired code");
    }
    state.runtime.monitor.clear_pair(peer.ip());
    let token = random_hex(32);
    match crate::config::update(|config| {
        config.paired_tokens.push(token_digest(&token));
        Ok(())
    }) {
        Ok(persisted) => state.config.lock().await.paired_tokens = persisted.paired_tokens,
        Err(err) => {
            tracing::error!("failed to persist paired device: {err:#}");
            return reject(StatusCode::INTERNAL_SERVER_ERROR, "could not save pairing");
        }
    }
    Json(PairToken { token }).into_response()
}
async fn offer(
    State(state): State<Shared>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Json(request): Json<Offer>,
) -> Response {
    if let Some(r) = guard(&headers, &uri, peer, &state, true) {
        return r;
    }
    let config = state.config.lock().await.clone();
    let Some(credential_digest) = authorized(&headers) else {
        return reject(StatusCode::UNAUTHORIZED, "pair first");
    };
    if request.kind.as_deref() != Some("offer") {
        return reject(StatusCode::BAD_REQUEST, "expected an SDP offer");
    }
    if request.sdp.len() > 128 * 1024 {
        return reject(StatusCode::PAYLOAD_TOO_LARGE, "SDP too large");
    }
    let display = request.display_id.unwrap_or_else(|| config.display.clone());
    if display != "primary" {
        return reject(StatusCode::BAD_REQUEST, "unsupported display");
    }
    let settings = match crate::quality::StreamSettings::from_offer(
        &config,
        request.quality.as_deref(),
        request.fps,
        request.bitrate_mbps,
    ) {
        Ok(settings) => settings,
        Err(message) => return reject(StatusCode::BAD_REQUEST, message),
    };
    let session_id = state.runtime.monitor.session(peer);
    match crate::rtc::answer(crate::rtc::AnswerRequest {
        offer_sdp: &request.sdp,
        lan_ip: &config.listen.to_string(),
        settings,
        mode: request.mode.unwrap_or_default(),
        display: &display,
        credential_digest,
        audio_requested: config.audio,
        runtime: state.runtime.clone(),
        session_id,
    })
    .await
    {
        Ok((sdp, audio_active)) => {
            if config.audio && !audio_active {
                state.config.lock().await.audio = false;
            }
            Json(Answer { sdp }).into_response()
        }
        Err(err) => {
            state.runtime.monitor.update_session(session_id, |session| {
                session.state = crate::monitor::SessionState::Error(format!("{err:#}"))
            });
            tracing::warn!("offer failed: {err:#}");
            if err
                .to_string()
                .contains("screen capture permission timed out")
            {
                reject(
                    StatusCode::REQUEST_TIMEOUT,
                    "Host screen selection timed out; approve the OS picker and retry",
                )
            } else if err.to_string().starts_with("capture failed:") {
                reject(
                    StatusCode::UNPROCESSABLE_ENTITY,
                    "Host screen capture was denied or unavailable",
                )
            } else {
                reject(
                    StatusCode::BAD_REQUEST,
                    "capture or WebRTC negotiation failed",
                )
            }
        }
    }
}

fn token_digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}
fn inline_hash(page: &str, tag: &str) -> String {
    let start = format!("<{tag}>");
    let end = format!("</{tag}>");
    let content = page
        .split_once(&start)
        .and_then(|(_, tail)| tail.split_once(&end))
        .map(|(body, _)| body)
        .unwrap_or("");
    format!(
        "'sha256-{}'",
        STANDARD.encode(Sha256::digest(content.as_bytes()))
    )
}

async fn certificate(ip: IpAddr) -> Result<(axum_server::tls_rustls::RustlsConfig, String)> {
    let dir = crate::config::dir()?;
    fs::create_dir_all(&dir)?;
    let cert_path = dir.join("cert.der");
    let key_path = dir.join("key.der");
    let ip_path = dir.join("cert-ip");
    let current_ip = fs::read_to_string(&ip_path).unwrap_or_default();
    let (cert, key) = if cert_path.exists() && key_path.exists() && current_ip == ip.to_string() {
        (fs::read(cert_path)?, fs::read(key_path)?)
    } else {
        let generated =
            rcgen::generate_simple_self_signed(vec![ip.to_string(), "localhost".into()])?;
        let cert = generated.cert.der().to_vec();
        let key = generated.signing_key.serialize_der();
        fs::write(&cert_path, &cert)?;
        fs::write(&key_path, &key)?;
        fs::write(&ip_path, ip.to_string())?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))?;
        }
        (cert, key)
    };
    let fingerprint = Sha256::digest(&cert)
        .iter()
        .map(|b| format!("{b:02X}"))
        .collect::<Vec<_>>()
        .join(":");
    Ok((
        axum_server::tls_rustls::RustlsConfig::from_der(vec![cert], key).await?,
        fingerprint,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    async fn test_tls() -> axum_server::tls_rustls::RustlsConfig {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        let certificate = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
        axum_server::tls_rustls::RustlsConfig::from_der(
            vec![certificate.cert.der().to_vec()],
            certificate.signing_key.serialize_der(),
        )
        .await
        .unwrap()
    }
    #[test]
    fn address_in_use_does_not_trigger_fallback() {
        let occupied = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = occupied.local_addr().unwrap().port();
        let attempts = std::cell::Cell::new(0);
        let result = bind_with_fallback(
            "127.0.0.1".parse().unwrap(),
            port,
            ["192.168.1.7".parse().unwrap()],
            |_| {
                attempts.set(attempts.get() + 1);
                Err::<(), _>(io::Error::new(io::ErrorKind::AddrInUse, "occupied"))
            },
        );
        assert!(result.is_err());
        assert_eq!(
            attempts.get(),
            1,
            "address-in-use must not trigger fallback"
        );
    }

    #[test]
    fn unavailable_private_ip_retries_current_lan_ip_only() {
        let configured = "192.168.1.9".parse().unwrap();
        let current = "10.0.0.4".parse().unwrap();
        let attempts = std::cell::RefCell::new(Vec::new());
        let (bound, address) = bind_with_fallback(configured, 47990, [current], |address| {
            attempts.borrow_mut().push(address);
            if address.ip() == configured {
                Err(io::Error::new(io::ErrorKind::AddrNotAvailable, "stale"))
            } else {
                Ok(address)
            }
        })
        .unwrap();
        assert_eq!(bound, address);
        assert_eq!(address.ip(), current);
        assert_eq!(
            *attempts.borrow(),
            vec![SocketAddr::new(configured, 47990), address]
        );
    }

    #[test]
    fn loopback_unavailable_does_not_fall_back_to_lan() {
        let attempts = std::cell::Cell::new(0);
        let result = bind_with_fallback(
            "127.0.0.1".parse().unwrap(),
            47990,
            ["192.168.1.7".parse().unwrap()],
            |_| {
                attempts.set(attempts.get() + 1);
                Err::<(), _>(io::Error::new(io::ErrorKind::AddrNotAvailable, "missing"))
            },
        );
        assert!(result.is_err());
        assert_eq!(attempts.get(), 1);
    }

    #[test]
    fn stale_private_ip_without_current_lan_has_actionable_error() {
        let result = bind_with_fallback::<()>("192.168.1.9".parse().unwrap(), 47990, [], |_| {
            Err(io::Error::new(io::ErrorKind::AddrNotAvailable, "missing"))
        });
        let error = result.unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AddrNotAvailable);
        assert!(error.to_string().contains("no current private LAN address"));
        assert!(error.to_string().contains("reconnect to a private network"));
    }
    #[tokio::test]
    async fn reports_running_after_listen_and_releases_port_on_stop() {
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = reservation.local_addr().unwrap();
        drop(reservation);
        let config = Config {
            listen: address.ip(),
            port: address.port(),
            ..Config::default()
        };
        let monitor = crate::monitor::Monitor::default();
        monitor.update(|state| state.lifecycle = crate::monitor::Lifecycle::Starting);
        let mut events = monitor.subscribe();
        let (sender, stop) = tokio::sync::watch::channel(false);
        let listener = TcpListener::bind(address).unwrap();
        listener.set_nonblocking(true).unwrap();
        let task = tokio::spawn(serve_prepared(
            config,
            monitor.clone(),
            stop,
            true,
            listener,
            test_tls().await,
            "test".into(),
        ));
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if events.borrow_and_update().lifecycle == crate::monitor::Lifecycle::Running {
                    break;
                }
                events.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        assert_eq!(
            monitor.snapshot().url.as_deref(),
            Some(format!("https://{address}").as_str())
        );
        sender.send_replace(true);
        tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        let _released = std::net::TcpListener::bind(address).unwrap();
    }
    #[test]
    fn origin_and_host_exact() {
        let addr: SocketAddr = "192.168.1.20:47990".parse().unwrap();
        let mut h = HeaderMap::new();
        h.insert(header::HOST, "192.168.1.20:47990".parse().unwrap());
        h.insert(
            header::ORIGIN,
            "https://192.168.1.20:47990".parse().unwrap(),
        );
        assert!(valid_host(&h, &"/api/status".parse().unwrap(), addr));
        assert!(valid_origin(&h, addr));
        h.insert(header::HOST, "evil.example".parse().unwrap());
        assert!(!valid_host(&h, &"/api/status".parse().unwrap(), addr));
        h.remove(header::HOST);
        let h2_uri: Uri = "https://192.168.1.20:47990/api/status".parse().unwrap();
        assert!(valid_host(&h, &h2_uri, addr));
        h.insert(header::HOST, "evil.example".parse().unwrap());
        assert!(!valid_host(&h, &h2_uri, addr));
    }
}
