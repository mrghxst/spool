//! The whole native path: engine → WebSocket → spool-relay → TCP → mock-nntp.

mod common;

use std::time::Duration;

use common::*;
use futures_util::{SinkExt, StreamExt};
use spool_engine::{Conn, Event};
use spool_relay::allow::HostPattern;
use spool_relay::config::Config;
use tokio_tungstenite::tungstenite::Message;

#[tokio::test]
async fn downloads_through_the_relay() {
    let content = data(200_000, 9);
    let f = fixture(vec![("ubuntu-24.04.iso".into(), content.clone())], 20_000);
    spool_engine::tls::add_test_root(&f.ca_der);
    let (port, stats) = provider(&f, vec![], None).await;

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay_addr = listener.local_addr().unwrap();
    let cfg = Config {
        allow: vec![HostPattern::new("localhost")],
        ports: vec![port],
        allow_private: true,
        ..Config::default()
    };
    let (_tx, rx) = tokio::sync::watch::channel(false);
    tokio::spawn(spool_relay::serve(
        listener,
        cfg,
        rx,
        Duration::from_secs(1),
    ));

    let url = format!("ws://{relay_addr}/v1/tcp/localhost/{port}");
    let (mut ws, _) = tokio_tungstenite::connect_async(url).await.unwrap();
    let mut conn = Conn::new("localhost", "user", "pass").unwrap();
    let segs = &f.nzb.files[0].segments;
    for (i, s) in segs.iter().enumerate() {
        conn.request(i as u32, &s.message_id);
    }
    let mut events = Vec::new();
    let mut done = 0;
    'outer: loop {
        while let Some(out) = conn.take_outgoing() {
            ws.send(Message::Binary(out.into())).await.unwrap();
        }
        while let Some(ev) = conn.poll_event() {
            if matches!(ev, Event::Segment(_)) {
                done += 1;
            }
            let closed = matches!(ev, Event::Closed { .. });
            events.push(ev);
            if done == segs.len() || closed {
                break 'outer;
            }
        }
        match tokio::time::timeout(Duration::from_secs(10), ws.next())
            .await
            .unwrap()
        {
            Some(Ok(Message::Binary(b))) => conn.on_bytes(&b),
            Some(Ok(_)) => {}
            other => panic!("relay closed: {other:?}"),
        }
    }
    let (out, ok) = assemble(&events, content.len());
    assert_eq!(ok, segs.len(), "{events:?}");
    assert_eq!(out, content);
    assert!(stats.forbidden_commands.lock().unwrap().is_empty());
}
