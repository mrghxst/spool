//! The sans-IO engine against mock-nntp over real TLS sockets.

mod common;

use std::sync::atomic::Ordering;

use common::*;
use mock_nntp::MissingRule;
use spool_engine::{Conn, Event};
use tokio::net::TcpStream;

fn trust(f: &Fixture) {
    spool_engine::tls::add_test_root(&f.ca_der);
}

async fn connect(port: u16, user: &str, pass: &str) -> (Conn, TcpStream) {
    let sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    sock.set_nodelay(true).unwrap();
    (Conn::new("localhost", user, pass).unwrap(), sock)
}

fn request_all(conn: &mut Conn, f: &Fixture, file: usize) -> usize {
    let segs = &f.nzb.files[file].segments;
    for (i, s) in segs.iter().enumerate() {
        conn.request(i as u32, &s.message_id);
    }
    segs.len()
}

#[tokio::test]
async fn downloads_a_file_with_pipelining() {
    let content = data(300_001, 1);
    let f = fixture(
        vec![("ubuntu-24.04.iso".into(), content.clone())],
        16 * 1024,
    );
    trust(&f);
    let (port, stats) = provider(&f, vec![], None).await;
    let (mut conn, mut sock) = connect(port, "user", "pass").await;
    let total = request_all(&mut conn, &f, 0);
    assert!(total > 8, "needs more requests than the pipeline depth");
    let mut done = 0;
    let events = drive(&mut conn, &mut sock, |ev, _| {
        if matches!(ev, Event::Segment(_)) {
            done += 1;
        }
        done == total
    })
    .await;
    assert!(matches!(events[0], Event::Ready { .. }));
    assert_eq!(events[1], Event::AuthOk);
    let (out, ok) = assemble(&events, content.len());
    assert_eq!(ok, total);
    assert_eq!(out, content);
    if let Event::Segment(s) = &events[2] {
        assert_eq!(s.name, "ubuntu-24.04.iso");
        assert_eq!(s.file_size, content.len() as u64);
    }
    assert!(stats.forbidden_commands.lock().unwrap().is_empty());
    assert_eq!(stats.bodies_served.load(Ordering::SeqCst), total);
}

#[tokio::test]
async fn reports_missing_articles() {
    let content = data(100_000, 2);
    let f = fixture(vec![("data.bin".into(), content)], 10_000);
    trust(&f);
    let rule = MissingRule {
        every: 3,
        offset: 1,
        file_contains: None,
    };
    let (port, _) = provider(&f, vec![rule], None).await;
    let (mut conn, mut sock) = connect(port, "user", "pass").await;
    let total = request_all(&mut conn, &f, 0);
    let mut answered = 0;
    let events = drive(&mut conn, &mut sock, |ev, _| {
        if matches!(ev, Event::Segment(_) | Event::Missing { .. }) {
            answered += 1;
        }
        answered == total
    })
    .await;
    let missing: Vec<u32> = events
        .iter()
        .filter_map(|e| match e {
            Event::Missing { seg_id } => Some(*seg_id),
            _ => None,
        })
        .collect();
    // Part numbers are seg_id + 1; parts 1, 4, 7, 10 are hidden.
    assert_eq!(missing, vec![0, 3, 6, 9]);
}

#[tokio::test]
async fn wrong_password_is_481() {
    let f = fixture(vec![("a.bin".into(), data(1000, 3))], 1000);
    trust(&f);
    let (port, _) = provider(&f, vec![], None).await;
    let (mut conn, mut sock) = connect(port, "user", "wrong").await;
    let events = drive(&mut conn, &mut sock, |ev, _| {
        matches!(ev, Event::AuthFailed { .. })
    })
    .await;
    assert!(
        events.contains(&Event::AuthFailed { code: 481 }),
        "{events:?}"
    );
}

#[tokio::test]
async fn connection_limit_is_busy_502() {
    let f = fixture(vec![("a.bin".into(), data(1000, 4))], 1000);
    trust(&f);
    let (port, _) = provider(&f, vec![], Some(1)).await;
    let (mut c1, mut s1) = connect(port, "user", "pass").await;
    drive(&mut c1, &mut s1, |ev, _| *ev == Event::AuthOk).await;
    let (mut c2, mut s2) = connect(port, "user", "pass").await;
    let events = drive(&mut c2, &mut s2, |ev, _| matches!(ev, Event::Busy { .. })).await;
    assert!(events.contains(&Event::Busy { code: 502 }), "{events:?}");
    drop(c1);
}

#[tokio::test]
async fn stat_date_and_capabilities() {
    let f = fixture(vec![("a.bin".into(), data(1000, 5))], 1000);
    trust(&f);
    let (port, _) = provider(&f, vec![], None).await;
    let (mut conn, mut sock) = connect(port, "user", "pass").await;
    let id = f.nzb.files[0].segments[0].message_id.clone();
    conn.stat(1, &id);
    conn.stat(2, "nope@spool.test");
    conn.date();
    conn.capabilities();
    let events = drive(&mut conn, &mut sock, |ev, _| {
        matches!(ev, Event::Capabilities { .. })
    })
    .await;
    assert!(events.contains(&Event::Stat {
        seg_id: 1,
        exists: true
    }));
    assert!(events.contains(&Event::Stat {
        seg_id: 2,
        exists: false
    }));
    assert!(events.contains(&Event::Date {
        value: "20260101000000".into()
    }));
}

#[tokio::test]
async fn quit_closes_cleanly() {
    let f = fixture(vec![("a.bin".into(), data(1000, 6))], 1000);
    trust(&f);
    let (port, stats) = provider(&f, vec![], None).await;
    let (mut conn, mut sock) = connect(port, "user", "pass").await;
    drive(&mut conn, &mut sock, |ev, c| {
        if *ev == Event::AuthOk {
            c.quit();
        }
        false
    })
    .await;
    assert!(conn.is_closed());
    assert!(stats.forbidden_commands.lock().unwrap().is_empty());
}
