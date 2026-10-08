//! A TLS NNTP server for tests. It posts fixture files as yEnc articles,
//! writes matching NZBs, and can play several providers at once, each with
//! its own credentials, missing articles and connection limit.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use md5::{Digest, Md5};
use serde::Deserialize;
use spool_engine::nntp::dot_stuff;
use spool_engine::yenc;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

pub mod tls;

/// One stored article.
#[derive(Debug, Clone)]
pub struct Article {
    pub message_id: String,
    /// The file it belongs to (as posted) and its 1-based part number.
    pub file: String,
    pub part: u32,
    /// PAR2 files are never made missing by rules.
    pub protected: bool,
    /// Dot-stuffed yEnc body, without the terminating ".\r\n".
    pub body: Arc<Vec<u8>>,
}

#[derive(Debug, Default, Clone)]
pub struct Store {
    pub articles: HashMap<String, Article>,
}

/// Options for posting a job.
#[derive(Debug, Clone, Deserialize)]
pub struct PostOptions {
    #[serde(default = "default_article_size")]
    pub article_size: usize,
    /// Post data files under random names (subject and yEnc `name=`).
    #[serde(default)]
    pub obfuscate: bool,
    /// Password to put in the NZB `<meta type="password">`.
    #[serde(default)]
    pub password: Option<String>,
}

fn default_article_size() -> usize {
    65536
}

impl Default for PostOptions {
    fn default() -> Self {
        PostOptions {
            article_size: default_article_size(),
            obfuscate: false,
            password: None,
        }
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

impl Store {
    /// Posts `files` (name, bytes) as one job and returns the NZB.
    pub fn post(&mut self, job: &str, files: &[(String, Vec<u8>)], opts: &PostOptions) -> String {
        let mut nzb = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
             <!DOCTYPE nzb PUBLIC \"-//newzBin//DTD NZB 1.1//EN\" \"http://www.newzbin.com/DTD/nzb/nzb-1.1.dtd\">\n\
             <nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\">\n<head>\n",
        );
        nzb.push_str(&format!("<meta type=\"name\">{}</meta>\n", xml_escape(job)));
        if let Some(p) = &opts.password {
            nzb.push_str(&format!(
                "<meta type=\"password\">{}</meta>\n",
                xml_escape(p)
            ));
        }
        nzb.push_str("</head>\n");
        let count = files.len();
        for (idx, (name, data)) in files.iter().enumerate() {
            let is_par2 = name.to_ascii_lowercase().ends_with(".par2");
            let posted_name = if opts.obfuscate && !is_par2 {
                hex(&Md5::digest(format!("{job}/{name}").as_bytes()))
            } else {
                name.clone()
            };
            let parts: Vec<&[u8]> = if data.is_empty() {
                vec![&[][..]]
            } else {
                data.chunks(opts.article_size).collect()
            };
            let total = parts.len() as u32;
            let subject = format!(
                "{} [{}/{}] - \"{}\" yEnc (1/{})",
                job,
                idx + 1,
                count,
                posted_name,
                total
            );
            nzb.push_str(&format!(
                "<file poster=\"mock@spool.test\" date=\"1700000000\" subject=\"{}\">\n<groups><group>alt.binaries.test</group></groups>\n<segments>\n",
                xml_escape(&subject)
            ));
            let mut begin = 0u64;
            for (i, part) in parts.iter().enumerate() {
                let n = i as u32 + 1;
                let file_hash = hex(&Md5::digest(format!("{job}/{name}").as_bytes()));
                let id = format!("{}.{n}@spool.test", &file_hash[..16]);
                let enc =
                    yenc::encode_part(&posted_name, data.len() as u64, n, total, begin, part, 128);
                let body = dot_stuff(&enc);
                nzb.push_str(&format!(
                    "<segment bytes=\"{}\" number=\"{}\">{}</segment>\n",
                    body.len(),
                    n,
                    id
                ));
                self.articles.insert(
                    id.clone(),
                    Article {
                        message_id: id,
                        file: name.clone(),
                        part: n,
                        protected: is_par2,
                        body: Arc::new(body),
                    },
                );
                begin += part.len() as u64;
            }
            nzb.push_str("</segments>\n</file>\n");
        }
        nzb.push_str("</nzb>\n");
        nzb
    }

    /// Posts every file in `dir` (sorted by name) as job `job`.
    pub fn post_dir(
        &mut self,
        job: &str,
        dir: &Path,
        opts: &PostOptions,
    ) -> std::io::Result<String> {
        let mut names: Vec<_> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        let files: Vec<(String, Vec<u8>)> = names
            .into_iter()
            .map(|n| {
                let data = std::fs::read(dir.join(&n))?;
                Ok((n, data))
            })
            .collect::<std::io::Result<_>>()?;
        Ok(self.post(job, &files, opts))
    }
}

/// Which articles a provider pretends not to have.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MissingRule {
    /// Part numbers `n` with `n % every == offset` are missing.
    pub every: u32,
    #[serde(default)]
    pub offset: u32,
    /// Only files whose posted name contains this text (default: all).
    #[serde(default)]
    pub file_contains: Option<String>,
}

impl MissingRule {
    fn hides(&self, a: &Article) -> bool {
        if self.every == 0 || a.protected {
            return false;
        }
        if let Some(f) = &self.file_contains {
            if !a.file.contains(f.as_str()) {
                return false;
            }
        }
        a.part % self.every == self.offset % self.every
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    #[serde(default)]
    pub port: u16,
    pub user: String,
    pub pass: String,
    #[serde(default)]
    pub missing: Vec<MissingRule>,
    /// Connections beyond this get "502 too many connections" after login.
    #[serde(default)]
    pub max_conns: Option<usize>,
    /// Delay before each BODY reply, to slow downloads down (screenshots).
    #[serde(default)]
    pub delay_ms: u64,
    /// After this many BODY replies on one connection, go silent: keep the
    /// connection open but never answer again, like a dropped route.
    #[serde(default)]
    pub hang_after: Option<usize>,
}

/// Counters a test can inspect.
#[derive(Debug, Default)]
pub struct Stats {
    pub connections: AtomicUsize,
    pub active: AtomicUsize,
    pub bodies_served: AtomicUsize,
    pub missing_served: AtomicUsize,
    /// Any command outside the read-only set the engine is allowed to send.
    pub forbidden_commands: Mutex<Vec<String>>,
}

pub struct Provider {
    pub config: ProviderConfig,
    pub store: Arc<Store>,
    pub stats: Arc<Stats>,
}

const READ_ONLY: &[&str] = &["AUTHINFO", "BODY", "STAT", "DATE", "CAPABILITIES", "QUIT"];

impl Provider {
    pub fn is_missing(&self, a: &Article) -> bool {
        self.config.missing.iter().any(|r| r.hides(a))
    }

    /// Runs one NNTP session.
    pub async fn session<S: AsyncRead + AsyncWrite + Unpin>(
        &self,
        stream: S,
    ) -> std::io::Result<()> {
        let stats = &self.stats;
        stats.connections.fetch_add(1, Ordering::SeqCst);
        let active = stats.active.fetch_add(1, Ordering::SeqCst) + 1;
        let _guard = ActiveGuard(Arc::clone(stats));
        let (rd, mut wr) = tokio::io::split(stream);
        let mut rd = BufReader::new(rd);
        wr.write_all(b"200 mock-nntp ready (posting not permitted)\r\n")
            .await?;
        wr.flush().await?;
        let mut user_ok = false;
        let mut authed = false;
        let mut bodies = 0usize;
        let mut line = String::new();
        loop {
            line.clear();
            if rd.read_line(&mut line).await? == 0 {
                return Ok(());
            }
            let cmd = line.trim_end();
            let mut words = cmd.split(' ');
            let verb = words.next().unwrap_or("").to_ascii_uppercase();
            if !READ_ONLY.contains(&verb.as_str()) {
                stats
                    .forbidden_commands
                    .lock()
                    .unwrap()
                    .push(cmd.to_string());
            }
            let reply: Vec<u8> = match verb.as_str() {
                "AUTHINFO" => {
                    let sub = words.next().unwrap_or("").to_ascii_uppercase();
                    let arg = words.collect::<Vec<_>>().join(" ");
                    match sub.as_str() {
                        "USER" => {
                            user_ok = arg == self.config.user;
                            b"381 password required\r\n".to_vec()
                        }
                        "PASS" if user_ok && arg == self.config.pass => {
                            if self.config.max_conns.is_some_and(|m| active > m) {
                                wr.write_all(b"502 too many connections\r\n").await?;
                                wr.flush().await?;
                                return Ok(());
                            }
                            authed = true;
                            b"281 authentication accepted\r\n".to_vec()
                        }
                        "PASS" => b"481 authentication failed\r\n".to_vec(),
                        _ => b"501 syntax error\r\n".to_vec(),
                    }
                }
                "CAPABILITIES" => {
                    b"101 capability list\r\nVERSION 2\r\nREADER\r\nAUTHINFO USER\r\n.\r\n".to_vec()
                }
                "DATE" => b"111 20260101000000\r\n".to_vec(),
                "QUIT" => {
                    wr.write_all(b"205 bye\r\n").await?;
                    wr.flush().await?;
                    return Ok(());
                }
                "BODY" | "STAT" if !authed => b"480 authentication required\r\n".to_vec(),
                "BODY" | "STAT" => {
                    if self.config.hang_after.is_some_and(|n| bodies >= n) {
                        // Swallow everything until the client gives up.
                        while rd.read_line(&mut line).await? > 0 {}
                        return Ok(());
                    }
                    bodies += 1;
                    if self.config.delay_ms > 0 {
                        wr.flush().await?;
                        tokio::time::sleep(std::time::Duration::from_millis(self.config.delay_ms))
                            .await;
                    }
                    let id = words
                        .next()
                        .unwrap_or("")
                        .trim_start_matches('<')
                        .trim_end_matches('>');
                    match self.store.articles.get(id) {
                        Some(a) if !self.is_missing(a) => {
                            if verb == "STAT" {
                                format!("223 0 <{id}>\r\n").into_bytes()
                            } else {
                                stats.bodies_served.fetch_add(1, Ordering::SeqCst);
                                let mut out = format!("222 0 <{id}> body follows\r\n").into_bytes();
                                out.extend_from_slice(&a.body);
                                out.extend_from_slice(b".\r\n");
                                out
                            }
                        }
                        _ => {
                            stats.missing_served.fetch_add(1, Ordering::SeqCst);
                            b"430 no such article\r\n".to_vec()
                        }
                    }
                }
                "POST" | "IHAVE" => b"440 posting not permitted\r\n".to_vec(),
                _ => b"500 command not recognized\r\n".to_vec(),
            };
            wr.write_all(&reply).await?;
            // Only flush when the client has nothing else queued, so
            // pipelined replies go out together.
            if rd.buffer().is_empty() {
                wr.flush().await?;
            }
        }
    }
}

struct ActiveGuard(Arc<Stats>);

impl Drop for ActiveGuard {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Serves a provider over TLS on `listener` until the task is dropped.
pub async fn serve(listener: TcpListener, provider: Arc<Provider>, acceptor: TlsAcceptor) {
    loop {
        let Ok((tcp, _)) = listener.accept().await else {
            continue;
        };
        let _ = tcp.set_nodelay(true);
        let p = Arc::clone(&provider);
        let acc = acceptor.clone();
        tokio::spawn(async move {
            if let Ok(tls) = acc.accept(tcp).await {
                let _ = p.session(tls).await;
            }
        });
    }
}
