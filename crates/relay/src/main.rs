use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::process::ExitCode;
use std::time::Duration;

use spool_relay::config::Config;
use spool_relay::VERSION;

/// The relay's only output: one line on stderr when it starts (or fails to).
fn startup_line(msg: &str) {
    eprintln!("spool-relay {VERSION} {msg}");
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    // `local`: a relay for this computer only. It's the default on Windows and
    // macOS, where the binary is double-clicked rather than run by Docker.
    let local = match args.first().map(String::as_str) {
        Some("healthcheck") => return healthcheck(),
        Some("--version") | Some("-V") => {
            startup_line("");
            return ExitCode::SUCCESS;
        }
        Some("local") => true,
        Some(_) => {
            startup_line("usage: spool-relay [local|healthcheck]");
            return ExitCode::from(2);
        }
        None => !cfg!(target_os = "linux"),
    };
    let cfg = match Config::from_lookup(|k| {
        std::env::var(k).ok().or_else(|| match (local, k) {
            (true, "SPOOL_LISTEN") => Some("127.0.0.1:8080".into()),
            (true, "SPOOL_ALLOW") => Some("*".into()),
            _ => None,
        })
    }) {
        Ok(cfg) => cfg,
        Err(e) => {
            startup_line(&format!("failed to start: {e}"));
            return ExitCode::from(2);
        }
    };
    let rt = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(_) => return ExitCode::FAILURE,
    };
    rt.block_on(run(cfg, local))
}

async fn run(cfg: Config, local: bool) -> ExitCode {
    let listener = match tokio::net::TcpListener::bind(cfg.listen).await {
        Ok(l) => l,
        Err(e) => {
            startup_line(&format!("failed to listen on {}: {e}", cfg.listen));
            return ExitCode::FAILURE;
        }
    };
    let addr = listener.local_addr().unwrap_or(cfg.listen);
    if local {
        startup_line(&format!(
            "listening on {addr}. In Spool, set the relay to ws://localhost:{}. Press Ctrl+C or close this window to stop it.",
            addr.port()
        ));
    } else {
        startup_line(&format!("listening on {addr}"));
    }

    let (tx, rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        wait_for_signal().await;
        let _ = tx.send(true);
    });
    spool_relay::serve(listener, cfg, rx, Duration::from_secs(10)).await;
    ExitCode::SUCCESS
}

#[cfg(unix)]
async fn wait_for_signal() {
    use tokio::signal::unix::{signal, SignalKind};
    let mut term = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(_) => return std::future::pending().await,
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

#[cfg(not(unix))]
async fn wait_for_signal() {
    let _ = tokio::signal::ctrl_c().await;
}

/// `spool-relay healthcheck`: exit 0 if the local relay answers `/healthz`.
/// Used by Docker's HEALTHCHECK, since a scratch image has no curl.
fn healthcheck() -> ExitCode {
    let listen: SocketAddr = std::env::var("SPOOL_LISTEN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| "0.0.0.0:8080".parse().expect("valid address"));
    let ip = if listen.ip().is_unspecified() {
        if listen.is_ipv6() {
            "::1".parse().expect("valid ip")
        } else {
            "127.0.0.1".parse().expect("valid ip")
        }
    } else {
        listen.ip()
    };
    let target = SocketAddr::new(ip, listen.port());
    let ok = (|| -> std::io::Result<bool> {
        let mut s = TcpStream::connect_timeout(&target, Duration::from_secs(3))?;
        s.set_read_timeout(Some(Duration::from_secs(3)))?;
        s.write_all(b"GET /healthz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")?;
        let mut out = Vec::new();
        s.read_to_end(&mut out)?;
        let text = String::from_utf8_lossy(&out);
        Ok(text.starts_with("HTTP/1.1 200") && text.ends_with("\r\n\r\nok"))
    })();
    if matches!(ok, Ok(true)) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}
