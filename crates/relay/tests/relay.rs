//! End-to-end tests for the relay over real sockets on loopback.

use std::net::SocketAddr;
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use spool_relay::allow::HostPattern;
use spool_relay::config::Config;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::Message;

const HELLO: &[u8] = &[0x16, 0x03, 0x01, 0x00, 0x05, 1, 2, 3, 4, 5];

async fn echo_server() -> u16 {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = l.local_addr().unwrap().port();
    tokio::spawn(async move {
        loop {
            let (mut s, _) = l.accept().await.unwrap();
            tokio::spawn(async move {
                let mut buf = vec![0u8; 65536];
                loop {
                    match s.read(&mut buf).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => {
                            if s.write_all(&buf[..n]).await.is_err() {
                                break;
                            }
                        }
                    }
                }
            });
        }
    });
    port
}

async fn relay(cfg: Config) -> SocketAddr {
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = l.local_addr().unwrap();
    let (tx, rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        spool_relay::serve(l, cfg, rx, Duration::from_secs(1)).await;
        drop(tx);
    });
    addr
}

fn test_cfg(ports: Vec<u16>) -> Config {
    Config {
        allow: vec![HostPattern::new("*")],
        ports,
        allow_private: true,
        ..Config::default()
    }
}

type Client = tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>;

async fn connect(relay: SocketAddr, host: &str, port: u16) -> Client {
    let url = format!("ws://{relay}/v1/tcp/{host}/{port}");
    let (ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    ws
}

/// Reads until the relay closes, returning the close code.
async fn close_code(ws: &mut Client) -> Option<u16> {
    let fut = async {
        while let Some(msg) = ws.next().await {
            match msg {
                Ok(Message::Close(Some(f))) => return Some(u16::from(f.code)),
                Ok(Message::Close(None)) => return None,
                Ok(_) => continue,
                Err(_) => return None,
            }
        }
        None
    };
    tokio::time::timeout(Duration::from_secs(5), fut)
        .await
        .unwrap()
}

#[tokio::test]
async fn forwards_tls_bytes_both_ways() {
    let echo = echo_server().await;
    let r = relay(test_cfg(vec![echo])).await;
    let mut ws = connect(r, "127.0.0.1", echo).await;
    ws.send(Message::Binary(HELLO.to_vec().into()))
        .await
        .unwrap();
    let mut got = Vec::new();
    while got.len() < HELLO.len() {
        match ws.next().await.unwrap().unwrap() {
            Message::Binary(b) => got.extend_from_slice(&b),
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!(got, HELLO);
    // Later frames don't need to look like TLS.
    ws.send(Message::Binary(b"more".to_vec().into()))
        .await
        .unwrap();
    match ws.next().await.unwrap().unwrap() {
        Message::Binary(b) => assert_eq!(&b[..], b"more"),
        other => panic!("unexpected {other:?}"),
    }
}

#[tokio::test]
async fn rejects_plaintext_first_frame() {
    let echo = echo_server().await;
    let r = relay(test_cfg(vec![echo])).await;
    let mut ws = connect(r, "127.0.0.1", echo).await;
    ws.send(Message::Binary(b"AUTHINFO USER me\r\n".to_vec().into()))
        .await
        .unwrap();
    assert_eq!(close_code(&mut ws).await, Some(4004));
}

#[tokio::test]
async fn rejects_text_first_frame() {
    let echo = echo_server().await;
    let r = relay(test_cfg(vec![echo])).await;
    let mut ws = connect(r, "127.0.0.1", echo).await;
    ws.send(Message::Text("hello".into())).await.unwrap();
    assert_eq!(close_code(&mut ws).await, Some(4004));
}

#[tokio::test]
async fn closes_on_text_frame_later() {
    let echo = echo_server().await;
    let r = relay(test_cfg(vec![echo])).await;
    let mut ws = connect(r, "127.0.0.1", echo).await;
    ws.send(Message::Binary(HELLO.to_vec().into()))
        .await
        .unwrap();
    ws.send(Message::Text("x".into())).await.unwrap();
    assert_eq!(close_code(&mut ws).await, Some(1003));
}

#[tokio::test]
async fn rejects_disallowed_port_and_host() {
    let echo = echo_server().await;
    let r = relay(test_cfg(vec![echo])).await;
    let mut ws = connect(r, "127.0.0.1", 9).await;
    assert_eq!(close_code(&mut ws).await, Some(4001));

    let mut cfg = test_cfg(vec![echo]);
    cfg.allow = vec![HostPattern::new("*.example.com")];
    let r = relay(cfg).await;
    let mut ws = connect(r, "127.0.0.1", echo).await;
    assert_eq!(close_code(&mut ws).await, Some(4001));
}

#[tokio::test]
async fn rejects_private_destination_by_default() {
    let echo = echo_server().await;
    let mut cfg = test_cfg(vec![echo]);
    cfg.allow_private = false;
    let r = relay(cfg).await;
    let mut ws = connect(r, "localhost", echo).await;
    ws.send(Message::Binary(HELLO.to_vec().into()))
        .await
        .unwrap();
    assert_eq!(close_code(&mut ws).await, Some(4001));
}

#[tokio::test]
async fn reports_connect_failure() {
    // Bind and drop to find a port with nothing listening.
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        l.local_addr().unwrap().port()
    };
    let r = relay(test_cfg(vec![port])).await;
    let mut ws = connect(r, "127.0.0.1", port).await;
    ws.send(Message::Binary(HELLO.to_vec().into()))
        .await
        .unwrap();
    assert_eq!(close_code(&mut ws).await, Some(4002));
}

#[tokio::test]
async fn enforces_per_ip_limit() {
    let echo = echo_server().await;
    let mut cfg = test_cfg(vec![echo]);
    cfg.max_conns_per_ip = 1;
    let r = relay(cfg).await;
    let mut first = connect(r, "127.0.0.1", echo).await;
    first
        .send(Message::Binary(HELLO.to_vec().into()))
        .await
        .unwrap();
    let _ = first.next().await; // wait until the first session is pumping
    let mut second = connect(r, "127.0.0.1", echo).await;
    assert_eq!(close_code(&mut second).await, Some(4003));
    drop(first);
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut third = connect(r, "127.0.0.1", echo).await;
    third
        .send(Message::Binary(HELLO.to_vec().into()))
        .await
        .unwrap();
    assert!(matches!(third.next().await, Some(Ok(Message::Binary(_)))));
}

#[tokio::test]
async fn enforces_origin_allowlist() {
    let echo = echo_server().await;
    let mut cfg = test_cfg(vec![echo]);
    cfg.origins = vec!["https://app.example".into()];
    let r = relay(cfg).await;

    let mut req = format!("ws://{r}/v1/tcp/127.0.0.1/{echo}")
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert("Origin", "https://evil.example".parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    assert_eq!(close_code(&mut ws).await, Some(4001));

    let mut req = format!("ws://{r}/v1/tcp/127.0.0.1/{echo}")
        .into_client_request()
        .unwrap();
    req.headers_mut()
        .insert("Origin", "https://app.example".parse().unwrap());
    let (mut ws, _) = tokio_tungstenite::connect_async(req).await.unwrap();
    ws.send(Message::Binary(HELLO.to_vec().into()))
        .await
        .unwrap();
    assert!(matches!(ws.next().await, Some(Ok(Message::Binary(_)))));
}

#[tokio::test]
async fn idle_sessions_are_closed() {
    let echo = echo_server().await;
    let mut cfg = test_cfg(vec![echo]);
    cfg.idle = Duration::from_secs(1);
    let r = relay(cfg).await;
    let mut ws = connect(r, "127.0.0.1", echo).await;
    ws.send(Message::Binary(HELLO.to_vec().into()))
        .await
        .unwrap();
    assert_eq!(close_code(&mut ws).await, Some(1000));
}

async fn http_get(addr: SocketAddr, path: &str) -> String {
    let mut s = TcpStream::connect(addr).await.unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
        .await
        .unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).await.unwrap();
    out
}

#[tokio::test]
async fn info_and_health_endpoints() {
    let r = relay(Config::default()).await;
    let info = http_get(r, "/").await;
    assert!(info.starts_with("HTTP/1.1 200"));
    assert!(info.contains("Access-Control-Allow-Origin: *"));
    assert!(info.contains("\"name\":\"spool-relay\""));
    assert!(info.contains("\"protocol\":1"));
    assert!(info.contains("\"ports\":[563,443]"));
    assert!(info.contains("\"*.newshosting.com\""));
    let health = http_get(r, "/healthz").await;
    assert!(health.starts_with("HTTP/1.1 200"));
    assert!(health.ends_with("\r\n\r\nok"));
    let missing = http_get(r, "/nope").await;
    assert!(missing.starts_with("HTTP/1.1 404"));
}
