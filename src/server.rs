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
    io::IsTerminal,
    net::{IpAddr, SocketAddr},
    sync::Arc,
};
use tokio::sync::Mutex;

pub struct AppState {
    pub config: Mutex<Config>,
    pub pairing: Mutex<Pairing>,
    pub address: SocketAddr,
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
}
#[derive(Serialize)]
struct Answer {
    sdp: String,
}

pub async fn serve(config: Config) -> Result<()> {
    config.validate()?;
    let address = SocketAddr::new(config.listen, config.port);
    let (tls, fingerprint) = certificate(address.ip()).await?;
    let state = Arc::new(AppState {
        config: Mutex::new(config),
        pairing: Mutex::new(Pairing::new()),
        address,
    });
    let app = Router::new()
        .route("/", get(index))
        .route("/api/status", get(status))
        .route("/api/displays", get(displays))
        .route("/api/pair/start", post(pair_start))
        .route("/api/pair/confirm", post(pair_confirm))
        .route("/api/offer", post(offer))
        .with_state(state);
    let url = format!("https://{address}");
    println!("Quest Display running at {url}");
    println!("TLS certificate SHA-256: {fingerprint}");
    if let Ok(qr) = qrcode::QrCode::new(url.as_bytes()) {
        println!(
            "{}",
            qr.render::<qrcode::render::unicode::Dense1x2>().build()
        );
    }
    println!("Accept the local self-signed certificate warning in your browser.");
    if std::io::stderr().is_terminal() {
        println!("Pairing codes will appear in this host terminal.");
    } else {
        println!(
            "Pairing requires an interactive host terminal; service output is not used for secrets."
        );
    }
    axum_server::bind_rustls(address, tls)
        .serve(app.into_make_service_with_connect_info::<SocketAddr>())
        .await?;
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
    if !std::io::stderr().is_terminal() {
        return reject(
            StatusCode::SERVICE_UNAVAILABLE,
            "pairing requires an interactive host terminal",
        );
    }
    let Some((response, code)) = state.pairing.lock().await.start(peer.ip()) else {
        return reject(StatusCode::TOO_MANY_REQUESTS, "try again later");
    };
    eprintln!(
        "Pairing request from {}: {} (expires in 2 minutes)",
        peer.ip(),
        code
    );
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
    let token = random_hex(32);
    let mut config = state.config.lock().await;
    match crate::config::load() {
        Ok(persisted) => config.paired_tokens = persisted.paired_tokens,
        Err(err) => {
            tracing::error!("could not load device credentials: {err:#}");
            return reject(
                StatusCode::INTERNAL_SERVER_ERROR,
                "could not load pairing store",
            );
        }
    }
    config.paired_tokens.push(token_digest(&token));
    if let Err(err) = config.save() {
        tracing::error!("failed to persist paired device: {err:#}");
        return reject(StatusCode::INTERNAL_SERVER_ERROR, "could not save pairing");
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
    match crate::rtc::answer(
        &request.sdp,
        &config.listen.to_string(),
        settings,
        &display,
        credential_digest,
        config.audio,
    )
    .await
    {
        Ok((sdp, audio_active)) => {
            if config.audio && !audio_active {
                state.config.lock().await.audio = false;
            }
            Json(Answer { sdp }).into_response()
        }
        Err(err) => {
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
