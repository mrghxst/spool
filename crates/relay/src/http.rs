//! Just enough HTTP/1.1 to route a request and perform the WebSocket upgrade.

use std::net::IpAddr;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const MAX_HEAD: usize = 8 * 1024;
const MAX_HEADERS: usize = 32;

#[derive(Debug, Default)]
pub struct Request {
    pub method: String,
    pub path: String,
    pub upgrade_websocket: bool,
    pub ws_key: Option<String>,
    pub ws_version: Option<String>,
    pub origin: Option<String>,
    pub forwarded_for: Option<String>,
    /// Bytes read after the end of the head.
    pub rest: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum HeadError {
    TooLarge,
    Malformed,
    Closed,
    Timeout,
}

/// Reads and parses a request head.
pub async fn read_request(stream: &mut TcpStream, timeout: Duration) -> Result<Request, HeadError> {
    let mut buf = Vec::with_capacity(1024);
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Some(req) = parse(&buf)? {
            return Ok(req);
        }
        if buf.len() >= MAX_HEAD {
            return Err(HeadError::TooLarge);
        }
        let mut chunk = [0u8; 2048];
        let n = tokio::time::timeout_at(deadline, stream.read(&mut chunk))
            .await
            .map_err(|_| HeadError::Timeout)?
            .map_err(|_| HeadError::Closed)?;
        if n == 0 {
            return Err(HeadError::Closed);
        }
        buf.extend_from_slice(&chunk[..n]);
    }
}

/// Parses a complete head, `Ok(None)` if more bytes are needed.
pub fn parse(buf: &[u8]) -> Result<Option<Request>, HeadError> {
    let mut headers = [httparse::EMPTY_HEADER; MAX_HEADERS];
    let mut req = httparse::Request::new(&mut headers);
    let len = match req.parse(buf) {
        Ok(httparse::Status::Complete(n)) => n,
        Ok(httparse::Status::Partial) => return Ok(None),
        Err(httparse::Error::TooManyHeaders) => return Err(HeadError::TooLarge),
        Err(_) => return Err(HeadError::Malformed),
    };
    let mut out = Request {
        method: req.method.unwrap_or("").to_string(),
        path: req.path.unwrap_or("").to_string(),
        rest: buf[len..].to_vec(),
        ..Request::default()
    };
    let mut upgrade = false;
    let mut connection_upgrade = false;
    for h in req.headers.iter() {
        let value = match std::str::from_utf8(h.value) {
            Ok(v) => v.trim(),
            Err(_) => continue,
        };
        match h.name.to_ascii_lowercase().as_str() {
            "upgrade" => upgrade = value.eq_ignore_ascii_case("websocket"),
            "connection" => {
                connection_upgrade = value
                    .split(',')
                    .any(|t| t.trim().eq_ignore_ascii_case("upgrade"))
            }
            "sec-websocket-key" => out.ws_key = Some(value.to_string()),
            "sec-websocket-version" => out.ws_version = Some(value.to_string()),
            "origin" => out.origin = Some(value.to_string()),
            "x-forwarded-for" => out.forwarded_for = Some(value.to_string()),
            _ => {}
        }
    }
    out.upgrade_websocket = upgrade && connection_upgrade;
    Ok(Some(out))
}

/// The client address as seen by the proxy: the right-most valid entry of
/// `X-Forwarded-For`, which is the one our own reverse proxy appended.
pub fn forwarded_ip(header: &str) -> Option<IpAddr> {
    header
        .rsplit(',')
        .map(str::trim)
        .find(|s| !s.is_empty())
        .and_then(|s| s.parse().ok())
}

/// Splits `/v1/tcp/{host}/{port}` into host and port.
pub fn tcp_target(path: &str) -> Option<(String, u16)> {
    let path = path.split('?').next().unwrap_or(path);
    let rest = path.strip_prefix("/v1/tcp/")?;
    let mut parts = rest.split('/');
    let host = parts.next()?.to_ascii_lowercase();
    let port = parts.next()?.parse::<u16>().ok()?;
    if parts.next().is_some_and(|p| !p.is_empty()) {
        return None;
    }
    Some((host, port))
}

pub async fn respond(stream: &mut TcpStream, status: &str, content_type: &str, body: &str) {
    let resp = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: {content_type}\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Cache-Control: no-store\r\n\
         X-Content-Type-Options: nosniff\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(resp.as_bytes()).await;
    let _ = stream.shutdown().await;
}

pub async fn respond_preflight(stream: &mut TcpStream) {
    let resp = "HTTP/1.1 204 No Content\r\n\
                Access-Control-Allow-Origin: *\r\n\
                Access-Control-Allow-Methods: GET, OPTIONS\r\n\
                Access-Control-Max-Age: 86400\r\n\
                Content-Length: 0\r\n\
                Connection: close\r\n\r\n";
    let _ = stream.write_all(resp.as_bytes()).await;
    let _ = stream.shutdown().await;
}

pub fn switching_protocols(accept: &str) -> String {
    format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\r\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_upgrade() {
        let raw = b"GET /v1/tcp/news.example.com/563 HTTP/1.1\r\nHost: r\r\nUpgrade: websocket\r\n\
Connection: keep-alive, Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\n\
Sec-WebSocket-Version: 13\r\nOrigin: https://app.example\r\n\r\nxyz";
        let r = parse(raw).unwrap().unwrap();
        assert_eq!(r.method, "GET");
        assert!(r.upgrade_websocket);
        assert_eq!(r.ws_key.as_deref(), Some("dGhlIHNhbXBsZSBub25jZQ=="));
        assert_eq!(r.origin.as_deref(), Some("https://app.example"));
        assert_eq!(r.rest, b"xyz");
    }

    #[test]
    fn partial_and_malformed() {
        assert!(parse(b"GET / HTTP/1.1\r\nHost:").unwrap().is_none());
        assert_eq!(
            parse(b"\x16\x03\x01\x00\r\n\r\n").unwrap_err(),
            HeadError::Malformed
        );
    }

    #[test]
    fn targets() {
        assert_eq!(
            tcp_target("/v1/tcp/News.Example.com/563"),
            Some(("news.example.com".into(), 563))
        );
        assert_eq!(tcp_target("/v1/tcp/a/443?x=1"), Some(("a".into(), 443)));
        assert_eq!(tcp_target("/v1/tcp/a/99999"), None);
        assert_eq!(tcp_target("/v1/tcp/a"), None);
        assert_eq!(tcp_target("/v1/tcp/a/1/extra"), None);
        assert_eq!(tcp_target("/v2/tcp/a/1"), None);
    }

    #[test]
    fn xff() {
        assert_eq!(
            forwarded_ip("198.51.100.1, 203.0.113.7"),
            Some("203.0.113.7".parse().unwrap())
        );
        assert_eq!(
            forwarded_ip("2001:db8::1"),
            Some("2001:db8::1".parse().unwrap())
        );
        assert_eq!(forwarded_ip("garbage"), None);
    }
}
