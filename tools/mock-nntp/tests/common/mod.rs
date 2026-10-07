//! Shared helpers: start mock providers and drive the sans-IO engine.

#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use mock_nntp::{MissingRule, PostOptions, Provider, ProviderConfig, Stats, Store};
use spool_engine::{Conn, Event};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

pub struct Fixture {
    pub nzb: spool_engine::nzb::Nzb,
    pub files: Vec<(String, Vec<u8>)>,
    pub store: Arc<Store>,
    pub ca_der: Vec<u8>,
    pub acceptor: tokio_rustls::TlsAcceptor,
}

pub fn data(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2654435761).max(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x >> 8) as u8
        })
        .collect()
}

pub fn fixture(files: Vec<(String, Vec<u8>)>, article_size: usize) -> Fixture {
    let mut store = Store::default();
    let opts = PostOptions {
        article_size,
        ..PostOptions::default()
    };
    let xml = store.post("ubuntu-24.04", &files, &opts);
    let nzb = spool_engine::nzb::parse(xml.as_bytes()).unwrap();
    let ca = mock_nntp::tls::generate();
    Fixture {
        nzb,
        files,
        store: Arc::new(store),
        ca_der: ca.ca_der,
        acceptor: ca.acceptor,
    }
}

pub async fn provider(
    f: &Fixture,
    missing: Vec<MissingRule>,
    max_conns: Option<usize>,
) -> (u16, Arc<Stats>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    let stats = Arc::new(Stats::default());
    let p = Arc::new(Provider {
        config: ProviderConfig {
            name: "test".into(),
            port,
            user: "user".into(),
            pass: "pass".into(),
            missing,
            max_conns,
            delay_ms: 0,
        },
        store: Arc::clone(&f.store),
        stats: Arc::clone(&stats),
    });
    tokio::spawn(mock_nntp::serve(listener, p, f.acceptor.clone()));
    (port, stats)
}

/// Runs the engine over a TCP socket until `done` says stop or the
/// connection closes. Returns every event seen.
pub async fn drive(
    conn: &mut Conn,
    sock: &mut TcpStream,
    mut on_event: impl FnMut(&Event, &mut Conn) -> bool,
) -> Vec<Event> {
    let mut events = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        while let Some(out) = conn.take_outgoing() {
            sock.write_all(&out).await.unwrap();
        }
        while let Some(ev) = conn.poll_event() {
            let stop = on_event(&ev, conn);
            let closed = matches!(ev, Event::Closed { .. });
            events.push(ev);
            if stop || closed {
                while let Some(out) = conn.take_outgoing() {
                    let _ = sock.write_all(&out).await;
                }
                return events;
            }
        }
        if let Some(out) = conn.take_outgoing() {
            sock.write_all(&out).await.unwrap();
            continue;
        }
        let n = tokio::time::timeout(Duration::from_secs(10), sock.read(&mut buf))
            .await
            .expect("timed out waiting for the server")
            .unwrap_or(0);
        if n == 0 {
            conn.transport_closed("eof");
        } else {
            conn.on_bytes(&buf[..n]);
        }
    }
}

/// Reassembles a file from segment events.
pub fn assemble(events: &[Event], size: usize) -> (Vec<u8>, usize) {
    let mut out = vec![0u8; size];
    let mut ok = 0;
    for ev in events {
        if let Event::Segment(s) = ev {
            assert!(s.crc_ok, "segment {} failed CRC", s.seg_id);
            let b = s.begin as usize;
            out[b..b + s.data.len()].copy_from_slice(&s.data);
            ok += 1;
        }
    }
    (out, ok)
}
