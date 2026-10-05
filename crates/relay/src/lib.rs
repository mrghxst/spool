//! spool-relay: forwards WebSocket binary frames to a TCP host and back.
//!
//! The first client frame must be a TLS handshake record, so the relay only
//! ever carries encrypted traffic. Nothing is logged or stored; the only state
//! is a set of in-memory connection counters.

pub mod allow;
pub mod config;
pub mod http;
pub mod limits;

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::BytesMut;
use futures_util::{SinkExt, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Role, WebSocketConfig};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use crate::config::Config;
use crate::limits::Limits;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
pub const PROTOCOL: u32 = 1;

/// WebSocket close codes defined by protocol v1.
pub mod close {
    pub const NOT_ALLOWED: u16 = 4001;
    pub const CONNECT_FAILED: u16 = 4002;
    pub const LIMIT_REACHED: u16 = 4003;
    pub const NOT_TLS: u16 = 4004;
}

const BUF_SIZE: usize = 256 * 1024;
const HEAD_TIMEOUT: Duration = Duration::from_secs(10);
const FIRST_FRAME_TIMEOUT: Duration = Duration::from_secs(10);

/// True if `frame` starts with a TLS handshake record header
/// (content type 22, legacy version 3.1 to 3.4).
pub fn is_tls_handshake(frame: &[u8]) -> bool {
    frame.len() >= 3 && frame[0] == 0x16 && frame[1] == 0x03 && (0x01..=0x04).contains(&frame[2])
}

/// Body for `GET /`.
pub fn info_json(cfg: &Config) -> String {
    let ports: Vec<String> = cfg.ports.iter().map(u16::to_string).collect();
    let allow: Vec<String> = cfg
        .allow
        .iter()
        .map(|p| format!("\"{}\"", p.to_string().replace(['"', '\\'], "")))
        .collect();
    format!(
        "{{\"name\":\"spool-relay\",\"version\":\"{VERSION}\",\"protocol\":{PROTOCOL},\"ports\":[{}],\"allow\":[{}]}}",
        ports.join(","),
        allow.join(",")
    )
}

struct Shared {
    cfg: Config,
    limits: Arc<Limits>,
    info: String,
}

/// Accepts connections until `shutdown` turns true, then waits up to
/// `grace` for open sessions to finish.
pub async fn serve(
    listener: TcpListener,
    cfg: Config,
    mut shutdown: watch::Receiver<bool>,
    grace: Duration,
) {
    let shared = Arc::new(Shared {
        limits: Limits::new(cfg.max_conns, cfg.max_conns_per_ip),
        info: info_json(&cfg),
        cfg,
    });
    let mut tasks = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            res = listener.accept() => {
                let Ok((stream, peer)) = res else {
                    // Out of file descriptors or similar: back off briefly.
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                };
                let shared = Arc::clone(&shared);
                let stop = shutdown.clone();
                tasks.spawn(async move { handle(stream, peer, shared, stop).await });
            }
            _ = shutdown.changed() => break,
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {}
        }
    }
    drop(listener);
    let _ = tokio::time::timeout(grace, async { while tasks.join_next().await.is_some() {} }).await;
    tasks.abort_all();
}

async fn handle(
    mut stream: TcpStream,
    peer: SocketAddr,
    shared: Arc<Shared>,
    stop: watch::Receiver<bool>,
) {
    let _ = stream.set_nodelay(true);
    let Ok(req) = http::read_request(&mut stream, HEAD_TIMEOUT).await else {
        return;
    };
    let path = req.path.split('?').next().unwrap_or("");
    match (req.method.as_str(), path, req.upgrade_websocket) {
        ("OPTIONS", _, _) => http::respond_preflight(&mut stream).await,
        ("GET", "/", false) => {
            http::respond(&mut stream, "200 OK", "application/json", &shared.info).await
        }
        ("GET", "/healthz", false) => {
            http::respond(&mut stream, "200 OK", "text/plain", "ok").await
        }
        ("GET", p, true) if p.starts_with("/v1/tcp/") => {
            let client_ip = client_ip(&req, peer, &shared.cfg);
            upgrade(stream, req, client_ip, shared, stop).await
        }
        _ => http::respond(&mut stream, "404 Not Found", "text/plain", "not found").await,
    }
}

fn client_ip(req: &http::Request, peer: SocketAddr, cfg: &Config) -> IpAddr {
    if cfg.trust_proxy {
        if let Some(ip) = req.forwarded_for.as_deref().and_then(http::forwarded_ip) {
            return ip;
        }
    }
    peer.ip()
}

fn ws_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .read_buffer_size(BUF_SIZE)
        .write_buffer_size(BUF_SIZE)
        .max_write_buffer_size(4 * BUF_SIZE)
        .max_message_size(Some(4 * 1024 * 1024))
        .max_frame_size(Some(4 * 1024 * 1024))
}

type Ws = WebSocketStream<TcpStream>;

async fn close_with(ws: &mut Ws, code: u16, reason: &'static str) {
    let frame = CloseFrame {
        code: CloseCode::from(code),
        reason: reason.into(),
    };
    let _ = tokio::time::timeout(Duration::from_secs(2), ws.close(Some(frame))).await;
}

async fn upgrade(
    mut stream: TcpStream,
    req: http::Request,
    client_ip: IpAddr,
    shared: Arc<Shared>,
    stop: watch::Receiver<bool>,
) {
    let (Some(key), Some("13")) = (req.ws_key.as_deref(), req.ws_version.as_deref()) else {
        http::respond(
            &mut stream,
            "400 Bad Request",
            "text/plain",
            "bad websocket request",
        )
        .await;
        return;
    };
    let accept = derive_accept_key(key.as_bytes());
    if stream
        .write_all(http::switching_protocols(&accept).as_bytes())
        .await
        .is_err()
    {
        return;
    }
    let mut ws = WebSocketStream::from_partially_read(
        stream,
        req.rest.clone(),
        Role::Server,
        Some(ws_config()),
    )
    .await;

    let cfg = &shared.cfg;
    if !cfg.origin_allowed(req.origin.as_deref()) {
        return close_with(&mut ws, close::NOT_ALLOWED, "origin not allowed").await;
    }
    let Some((host, port)) = http::tcp_target(&req.path) else {
        return close_with(&mut ws, close::NOT_ALLOWED, "bad target").await;
    };
    if !allow::valid_hostname(&host) || !cfg.host_allowed(&host) || !cfg.port_allowed(port) {
        return close_with(&mut ws, close::NOT_ALLOWED, "destination not allowed").await;
    }
    let Some(_slot) = shared.limits.acquire(client_ip) else {
        return close_with(&mut ws, close::LIMIT_REACHED, "limit reached").await;
    };

    // The first frame must be a TLS ClientHello. Only then do we dial out.
    let first = match tokio::time::timeout(FIRST_FRAME_TIMEOUT, ws.next()).await {
        Ok(Some(Ok(Message::Binary(b)))) if is_tls_handshake(&b) => b,
        Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Ok(Some(Err(_))) => return,
        _ => return close_with(&mut ws, close::NOT_TLS, "first frame is not TLS").await,
    };

    let tcp = match dial(&host, port, cfg).await {
        Ok(tcp) => tcp,
        Err(DialError::NotAllowed) => {
            return close_with(&mut ws, close::NOT_ALLOWED, "destination not allowed").await
        }
        Err(DialError::Failed) => {
            return close_with(&mut ws, close::CONNECT_FAILED, "connect failed").await
        }
    };
    pump(ws, tcp, first.to_vec(), cfg.idle, stop).await;
}

#[derive(Debug, PartialEq, Eq)]
pub enum DialError {
    NotAllowed,
    Failed,
}

/// Resolves `host` and connects to the first reachable global address,
/// alternating address families, within the connect timeout.
pub async fn dial(host: &str, port: u16, cfg: &Config) -> Result<TcpStream, DialError> {
    let deadline = tokio::time::Instant::now() + cfg.connect_timeout;
    let addrs: Vec<SocketAddr> =
        tokio::time::timeout_at(deadline, tokio::net::lookup_host((host, port)))
            .await
            .map_err(|_| DialError::Failed)?
            .map_err(|_| DialError::Failed)?
            .collect();
    if addrs.is_empty() {
        return Err(DialError::Failed);
    }
    let mut usable: Vec<SocketAddr> = addrs
        .iter()
        .copied()
        .filter(|a| cfg.allow_private || allow::is_global(a.ip()))
        .collect();
    if usable.is_empty() {
        return Err(DialError::NotAllowed);
    }
    // Interleave IPv6 and IPv4 so one broken family can't stall the dial.
    let (v6, v4): (Vec<_>, Vec<_>) = usable.drain(..).partition(SocketAddr::is_ipv6);
    let mut ordered = Vec::with_capacity(v6.len() + v4.len());
    let (mut a, mut b) = (v6.into_iter(), v4.into_iter());
    loop {
        match (a.next(), b.next()) {
            (None, None) => break,
            (x, y) => ordered.extend(x.into_iter().chain(y)),
        }
    }
    for addr in ordered {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            break;
        }
        let per_try = remaining.min(Duration::from_secs(4));
        if let Ok(Ok(s)) = tokio::time::timeout(per_try, TcpStream::connect(addr)).await {
            let _ = s.set_nodelay(true);
            return Ok(s);
        }
    }
    Err(DialError::Failed)
}

fn now_secs(start: Instant) -> u64 {
    start.elapsed().as_secs()
}

async fn pump(
    ws: Ws,
    tcp: TcpStream,
    first: Vec<u8>,
    idle: Duration,
    mut stop: watch::Receiver<bool>,
) {
    let start = Instant::now();
    let last = AtomicU64::new(0);
    let touch = || last.store(now_secs(start), Ordering::Relaxed);
    let (mut ws_tx, mut ws_rx) = ws.split();
    let (mut tcp_rd, mut tcp_wr) = tcp.into_split();

    let mut close_code: Option<(u16, &'static str)> = None;

    let up = async {
        if tcp_wr.write_all(&first).await.is_err() {
            return Some((close::CONNECT_FAILED, "write failed"));
        }
        while let Some(msg) = ws_rx.next().await {
            match msg {
                Ok(Message::Binary(b)) => {
                    if tcp_wr.write_all(&b).await.is_err() {
                        return None;
                    }
                    touch();
                }
                Ok(Message::Text(_)) => return Some((1003, "binary frames only")),
                Ok(Message::Close(_)) | Err(_) => break,
                Ok(_) => {}
            }
        }
        let _ = tcp_wr.shutdown().await;
        None
    };

    let down = async {
        let mut buf = BytesMut::with_capacity(BUF_SIZE);
        loop {
            if buf.capacity() < BUF_SIZE / 4 {
                buf.reserve(BUF_SIZE);
            }
            match tcp_rd.read_buf(&mut buf).await {
                Ok(0) | Err(_) => return Some((1000u16, "upstream closed")),
                Ok(_) => {
                    let chunk = buf.split().freeze();
                    if ws_tx.send(Message::Binary(chunk)).await.is_err() {
                        return None;
                    }
                    touch();
                }
            }
        }
    };

    let idle_watch = async {
        let tick = Duration::from_secs(1).min(idle);
        loop {
            tokio::time::sleep(tick).await;
            let quiet = now_secs(start).saturating_sub(last.load(Ordering::Relaxed));
            if quiet >= idle.as_secs() {
                return Some((1000u16, "idle timeout"));
            }
        }
    };

    tokio::select! {
        r = up => close_code = r,
        r = down => close_code = close_code.or(r),
        r = idle_watch => close_code = r,
        _ = stop.wait_for(|s| *s) => close_code = Some((1001, "relay shutting down")),
    }

    if let Ok(mut ws) = ws_tx.reunite(ws_rx) {
        let (code, reason) = close_code.unwrap_or((1000, "closed"));
        close_with(&mut ws, code, reason).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tls_first_frame_rule() {
        assert!(is_tls_handshake(&[0x16, 0x03, 0x01, 0x02, 0x00]));
        assert!(is_tls_handshake(&[0x16, 0x03, 0x03]));
        assert!(is_tls_handshake(&[0x16, 0x03, 0x04]));
        assert!(!is_tls_handshake(&[0x16, 0x03, 0x05]));
        assert!(!is_tls_handshake(&[0x16, 0x03, 0x00]));
        assert!(!is_tls_handshake(&[0x17, 0x03, 0x03]));
        assert!(!is_tls_handshake(b"AUTHINFO USER x\r\n"));
        assert!(!is_tls_handshake(&[0x16, 0x03]));
        assert!(!is_tls_handshake(&[]));
    }

    #[test]
    fn info() {
        let cfg = Config {
            ports: vec![563],
            allow: vec![allow::HostPattern::new("*.example.com")],
            ..Config::default()
        };
        assert_eq!(
            info_json(&cfg),
            format!(
                "{{\"name\":\"spool-relay\",\"version\":\"{VERSION}\",\"protocol\":1,\"ports\":[563],\"allow\":[\"*.example.com\"]}}"
            )
        );
    }

    #[tokio::test]
    async fn dial_rejects_private_targets() {
        let cfg = Config::default();
        assert_eq!(
            dial("127.0.0.1", 563, &cfg).await.unwrap_err(),
            DialError::NotAllowed
        );
        assert_eq!(
            dial("10.0.0.1", 563, &cfg).await.unwrap_err(),
            DialError::NotAllowed
        );
        assert_eq!(
            dial("localhost", 563, &cfg).await.unwrap_err(),
            DialError::NotAllowed
        );
    }
}
