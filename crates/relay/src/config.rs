//! Configuration from environment variables.

use std::net::SocketAddr;
use std::time::Duration;

use crate::allow::HostPattern;

/// Usenet provider domains that the public relay forwards to by default.
/// Self-hosters can set `SPOOL_ALLOW=*` to allow any public host.
pub const DEFAULT_ALLOW: &[&str] = &[
    "*.newshosting.com",
    "*.eweka.nl",
    "*.easynews.com",
    "*.usenetexpress.com",
    "*.newsdemon.com",
    "*.tweaknews.eu",
    "*.giganews.com",
    "*.usenetserver.com",
    "*.frugalusenet.com",
    "*.newsgroupdirect.com",
    "*.thundernews.com",
    "*.vipernews.com",
    "*.bulknews.eu",
    "*.xsnews.nl",
    "*.usenet.farm",
    "*.blocknews.net",
    "*.astraweb.com",
    "*.newsgroup.ninja",
    "*.usenight.com",
    "*.pureusenet.nl",
    "*.sunnyusenet.com",
    "*.ngd.news",
    "*.cheapnews.eu",
    "*.hitnews.com",
    "*.usenetprime.com",
    "*.supernews.com",
    "*.highwinds-media.com",
];

pub const DEFAULT_PORTS: &[u16] = &[563, 443];

#[derive(Clone, Debug)]
pub struct Config {
    pub listen: SocketAddr,
    pub allow: Vec<HostPattern>,
    pub ports: Vec<u16>,
    pub max_conns: usize,
    pub max_conns_per_ip: usize,
    pub idle: Duration,
    /// Exact `Origin` values that may connect. Empty means any origin.
    pub origins: Vec<String>,
    pub trust_proxy: bool,
    /// Test-only: allow loopback and private destinations.
    pub allow_private: bool,
    pub connect_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            listen: "0.0.0.0:8080".parse().expect("valid default address"),
            allow: DEFAULT_ALLOW.iter().map(|p| HostPattern::new(p)).collect(),
            ports: DEFAULT_PORTS.to_vec(),
            max_conns: 1024,
            max_conns_per_ip: 64,
            idle: Duration::from_secs(180),
            origins: Vec::new(),
            trust_proxy: false,
            allow_private: false,
            connect_timeout: Duration::from_secs(10),
        }
    }
}

fn list(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn flag(value: &str) -> bool {
    matches!(value.trim(), "1" | "true" | "yes" | "on")
}

impl Config {
    /// Builds a config from a lookup function (normally `std::env::var`).
    pub fn from_lookup(get: impl Fn(&str) -> Option<String>) -> Result<Config, String> {
        let mut c = Config::default();
        if let Some(v) = get("SPOOL_LISTEN") {
            c.listen = v
                .trim()
                .parse()
                .map_err(|_| format!("invalid SPOOL_LISTEN {v:?}"))?;
        }
        if let Some(v) = get("SPOOL_ALLOW") {
            let patterns: Vec<HostPattern> = list(&v).map(HostPattern::new).collect();
            if patterns.is_empty() {
                return Err("SPOOL_ALLOW is empty".into());
            }
            c.allow = patterns;
        }
        if let Some(v) = get("SPOOL_PORTS") {
            let ports: Result<Vec<u16>, _> = list(&v).map(str::parse::<u16>).collect();
            c.ports = ports.map_err(|_| format!("invalid SPOOL_PORTS {v:?}"))?;
            if c.ports.is_empty() || c.ports.contains(&0) {
                return Err(format!("invalid SPOOL_PORTS {v:?}"));
            }
        }
        let num = |name: &str, default: usize| -> Result<usize, String> {
            match get(name) {
                None => Ok(default),
                Some(v) => match v.trim().parse::<usize>() {
                    Ok(n) if n > 0 => Ok(n),
                    _ => Err(format!("invalid {name} {v:?}")),
                },
            }
        };
        c.max_conns = num("SPOOL_MAX_CONNS", c.max_conns)?;
        c.max_conns_per_ip = num("SPOOL_MAX_CONNS_PER_IP", c.max_conns_per_ip)?;
        c.idle = Duration::from_secs(num("SPOOL_IDLE_SECS", c.idle.as_secs() as usize)? as u64);
        if let Some(v) = get("SPOOL_ORIGINS") {
            c.origins = list(&v)
                .map(|s| s.trim_end_matches('/').to_string())
                .collect();
        }
        c.trust_proxy = get("SPOOL_TRUST_PROXY").is_some_and(|v| flag(&v));
        c.allow_private = get("SPOOL_ALLOW_PRIVATE").is_some_and(|v| flag(&v));
        Ok(c)
    }

    pub fn from_env() -> Result<Config, String> {
        Config::from_lookup(|k| std::env::var(k).ok())
    }

    pub fn port_allowed(&self, port: u16) -> bool {
        self.ports.contains(&port)
    }

    pub fn host_allowed(&self, host: &str) -> bool {
        self.allow.iter().any(|p| p.matches(host))
    }

    pub fn origin_allowed(&self, origin: Option<&str>) -> bool {
        if self.origins.is_empty() {
            return true;
        }
        match origin {
            Some(o) => {
                let o = o.trim_end_matches('/');
                self.origins.iter().any(|a| a.eq_ignore_ascii_case(o))
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn cfg(pairs: &[(&str, &str)]) -> Result<Config, String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Config::from_lookup(|k| map.get(k).cloned())
    }

    #[test]
    fn defaults() {
        let c = cfg(&[]).unwrap();
        assert_eq!(c.listen.port(), 8080);
        assert_eq!(c.ports, vec![563, 443]);
        assert_eq!(c.max_conns, 1024);
        assert_eq!(c.max_conns_per_ip, 64);
        assert_eq!(c.idle.as_secs(), 180);
        assert!(!c.trust_proxy);
        assert!(!c.allow_private);
        assert!(c.host_allowed("news.newshosting.com"));
        assert!(!c.host_allowed("example.com"));
        assert!(c.origin_allowed(None));
    }

    #[test]
    fn overrides() {
        let c = cfg(&[
            ("SPOOL_ALLOW", "*"),
            ("SPOOL_PORTS", "563, 119"),
            ("SPOOL_MAX_CONNS", "10"),
            ("SPOOL_ORIGINS", "https://a.example/,https://b.example"),
            ("SPOOL_TRUST_PROXY", "1"),
        ])
        .unwrap();
        assert!(c.host_allowed("anything.example"));
        assert!(c.port_allowed(119));
        assert!(!c.port_allowed(443));
        assert_eq!(c.max_conns, 10);
        assert!(c.trust_proxy);
        assert!(c.origin_allowed(Some("https://a.example")));
        assert!(c.origin_allowed(Some("https://b.example/")));
        assert!(!c.origin_allowed(Some("https://c.example")));
        assert!(!c.origin_allowed(None));
    }

    #[test]
    fn rejects_bad_values() {
        assert!(cfg(&[("SPOOL_PORTS", "abc")]).is_err());
        assert!(cfg(&[("SPOOL_PORTS", "0")]).is_err());
        assert!(cfg(&[("SPOOL_MAX_CONNS", "0")]).is_err());
        assert!(cfg(&[("SPOOL_LISTEN", "nope")]).is_err());
        assert!(cfg(&[("SPOOL_ALLOW", " , ")]).is_err());
    }
}
