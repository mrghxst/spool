//! mock-nntp: serve fixture files as Usenet articles over TLS.
//!
//!   mock-nntp <config.json>
//!
//! The config lists jobs (directories to post) and providers (ports,
//! credentials, missing-article rules). On start it writes `ca.der`,
//! `ca.pem` and one `<job>.nzb` per job into `out`, then `ready.json`.

use std::path::PathBuf;
use std::sync::Arc;

use mock_nntp::{PostOptions, Provider, ProviderConfig, Stats, Store};
use serde::Deserialize;

#[derive(Deserialize)]
struct Job {
    name: String,
    dir: PathBuf,
    #[serde(default)]
    options: PostOptions,
}

#[derive(Deserialize)]
struct Config {
    out: PathBuf,
    #[serde(default = "localhost")]
    bind: String,
    jobs: Vec<Job>,
    providers: Vec<ProviderConfig>,
}

fn localhost() -> String {
    "127.0.0.1".into()
}

fn fail(msg: String) -> ! {
    eprintln!("mock-nntp: {msg}");
    std::process::exit(1)
}

#[tokio::main]
async fn main() {
    let path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| fail("usage: mock-nntp <config.json>".into()));
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| fail(format!("read {path}: {e}")));
    let cfg: Config =
        serde_json::from_str(&raw).unwrap_or_else(|e| fail(format!("parse {path}: {e}")));
    std::fs::create_dir_all(&cfg.out)
        .unwrap_or_else(|e| fail(format!("create {}: {e}", cfg.out.display())));

    let mut store = Store::default();
    for job in &cfg.jobs {
        let nzb = store
            .post_dir(&job.name, &job.dir, &job.options)
            .unwrap_or_else(|e| fail(format!("post {}: {e}", job.dir.display())));
        std::fs::write(cfg.out.join(format!("{}.nzb", job.name)), nzb).expect("write nzb");
    }
    let store = Arc::new(store);

    let ca = mock_nntp::tls::generate();
    std::fs::write(cfg.out.join("ca.der"), &ca.ca_der).expect("write ca.der");
    std::fs::write(cfg.out.join("ca.pem"), &ca.ca_pem).expect("write ca.pem");

    let mut ports = Vec::new();
    for pc in cfg.providers {
        let listener = tokio::net::TcpListener::bind((cfg.bind.as_str(), pc.port))
            .await
            .unwrap_or_else(|e| fail(format!("bind {}:{}: {e}", cfg.bind, pc.port)));
        let port = listener.local_addr().expect("local addr").port();
        ports.push(format!("{{\"name\":{:?},\"port\":{port}}}", pc.name));
        let provider = Arc::new(Provider {
            config: pc,
            store: Arc::clone(&store),
            stats: Arc::new(Stats::default()),
        });
        tokio::spawn(mock_nntp::serve(listener, provider, ca.acceptor.clone()));
    }
    std::fs::write(
        cfg.out.join("ready.json"),
        format!(
            "{{\"articles\":{},\"providers\":[{}]}}",
            store.articles.len(),
            ports.join(",")
        ),
    )
    .expect("write ready.json");
    eprintln!(
        "mock-nntp: serving {} articles on {}",
        store.articles.len(),
        ports.join(", ")
    );
    let _ = tokio::signal::ctrl_c().await;
}
