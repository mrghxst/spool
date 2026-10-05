//! Without the test CA, the engine refuses the mock server's certificate.
//! (Separate test binary: trusted roots are process-wide.)

mod common;

use common::*;
use spool_engine::{Conn, Event};
use tokio::net::TcpStream;

#[tokio::test]
async fn rejects_untrusted_certificate() {
    let f = fixture(vec![("a.bin".into(), data(1000, 7))], 1000);
    let (port, stats) = provider(&f, vec![], None).await;
    let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut conn = Conn::new("localhost", "user", "pass").unwrap();
    let events = drive(&mut conn, &mut sock, |_, _| false).await;
    match events.last() {
        Some(Event::Closed { reason }) => assert!(reason.contains("isn't trusted"), "{reason}"),
        e => panic!("{e:?}"),
    }
    assert_eq!(
        stats
            .bodies_served
            .load(std::sync::atomic::Ordering::SeqCst),
        0
    );
}

#[tokio::test]
async fn rejects_wrong_hostname() {
    let f = fixture(vec![("a.bin".into(), data(1000, 8))], 1000);
    spool_engine::tls::add_test_root(&f.ca_der);
    let (port, _) = provider(&f, vec![], None).await;
    let mut sock = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let mut conn = Conn::new("news.example.com", "user", "pass").unwrap();
    let events = drive(&mut conn, &mut sock, |_, _| false).await;
    match events.last() {
        Some(Event::Closed { reason }) => assert!(reason.contains("doesn't match"), "{reason}"),
        e => panic!("{e:?}"),
    }
}
