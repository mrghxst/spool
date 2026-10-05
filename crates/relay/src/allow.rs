//! Destination checks: host glob patterns, hostname syntax and global IPs.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

/// A host glob. `*` matches any host, `*.example.com` matches any
/// subdomain of example.com (at any depth, but not example.com itself),
/// anything else must match exactly. Matching is case-insensitive.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostPattern {
    Any,
    Suffix(String),
    Exact(String),
}

impl HostPattern {
    pub fn new(pattern: &str) -> HostPattern {
        let p = pattern.trim().trim_end_matches('.').to_ascii_lowercase();
        if p == "*" {
            HostPattern::Any
        } else if let Some(rest) = p.strip_prefix("*.") {
            HostPattern::Suffix(format!(".{rest}"))
        } else {
            HostPattern::Exact(p)
        }
    }

    pub fn matches(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        match self {
            HostPattern::Any => true,
            HostPattern::Suffix(s) => host.len() > s.len() && host.ends_with(s.as_str()),
            HostPattern::Exact(e) => host == *e,
        }
    }
}

impl std::fmt::Display for HostPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostPattern::Any => f.write_str("*"),
            HostPattern::Suffix(s) => write!(f, "*{s}"),
            HostPattern::Exact(e) => f.write_str(e),
        }
    }
}

/// Accepts DNS names and IPv4 literals: letters, digits, '-' and '.',
/// labels of 1..=63 characters, at most 253 characters in total.
pub fn valid_hostname(host: &str) -> bool {
    let host = host.strip_suffix('.').unwrap_or(host);
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    host.split('.').all(|label| {
        !label.is_empty()
            && label.len() <= 63
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    })
}

/// True if the address is publicly routable. Rejects loopback, private
/// (RFC 1918), link-local, CGNAT, ULA, multicast, unspecified, documentation,
/// benchmarking, reserved and translation ranges that could reach private space.
pub fn is_global(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_global_v4(v4),
        IpAddr::V6(v6) => is_global_v6(v6),
    }
}

fn is_global_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    let in_net = |net: [u8; 4], bits: u32| -> bool {
        let mask = if bits == 0 {
            0
        } else {
            u32::MAX << (32 - bits)
        };
        (u32::from(ip) & mask) == (u32::from_be_bytes(net) & mask)
    };
    !(o[0] == 0                                  // "this network"
        || o[0] == 10                            // RFC 1918
        || in_net([100, 64, 0, 0], 10)           // CGNAT
        || o[0] == 127                           // loopback
        || in_net([169, 254, 0, 0], 16)          // link-local
        || in_net([172, 16, 0, 0], 12)           // RFC 1918
        || in_net([192, 0, 0, 0], 24)            // IETF protocol assignments
        || in_net([192, 0, 2, 0], 24)            // TEST-NET-1
        || in_net([192, 88, 99, 0], 24)          // 6to4 relay anycast
        || in_net([192, 168, 0, 0], 16)          // RFC 1918
        || in_net([198, 18, 0, 0], 15)           // benchmarking
        || in_net([198, 51, 100, 0], 24)         // TEST-NET-2
        || in_net([203, 0, 113, 0], 24)          // TEST-NET-3
        || o[0] >= 224) // multicast, reserved, broadcast
}

fn is_global_v6(ip: Ipv6Addr) -> bool {
    if let Some(v4) = ip.to_ipv4_mapped() {
        return is_global_v4(v4);
    }
    let s = ip.segments();
    // NAT64 well-known prefix 64:ff9b::/96 embeds an IPv4 address.
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6] == [0, 0, 0, 0] {
        let v4 = Ipv4Addr::new((s[6] >> 8) as u8, s[6] as u8, (s[7] >> 8) as u8, s[7] as u8);
        return is_global_v4(v4);
    }
    !(ip.is_unspecified()
        || ip.is_loopback()
        || s[0..5] == [0, 0, 0, 0, 0]            // ::/80 incl. IPv4-compatible
        || (s[0] == 0x0100 && s[1..4] == [0, 0, 0]) // discard-only 100::/64
        || (s[0] & 0xfe00) == 0xfc00             // ULA fc00::/7
        || (s[0] & 0xffc0) == 0xfe80             // link-local fe80::/10
        || (s[0] & 0xffc0) == 0xfec0             // site-local fec0::/10
        || (s[0] & 0xff00) == 0xff00             // multicast
        || (s[0] == 0x2001 && s[1] < 0x0200)     // 2001::/23 IETF, Teredo, ORCHID
        || (s[0] == 0x2001 && s[1] == 0x0db8)    // documentation
        || s[0] == 0x2002                        // 6to4 can embed private IPv4
        || (s[0] == 0x3fff && s[1] < 0x1000)) // documentation 3fff::/20
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        let p = HostPattern::new("*.newshosting.com");
        assert!(p.matches("news.newshosting.com"));
        assert!(p.matches("NEWS.Newshosting.COM"));
        assert!(p.matches("a.b.newshosting.com"));
        assert!(p.matches("news.newshosting.com."));
        assert!(!p.matches("newshosting.com"));
        assert!(!p.matches("evilnewshosting.com"));
        assert!(!p.matches("newshosting.com.evil.net"));
        assert!(HostPattern::new("*").matches("anything.example"));
        let e = HostPattern::new("news.example.com");
        assert!(e.matches("news.example.com"));
        assert!(!e.matches("x.news.example.com"));
        assert_eq!(p.to_string(), "*.newshosting.com");
    }

    #[test]
    fn hostnames() {
        assert!(valid_hostname("news.example.com"));
        assert!(valid_hostname("203.0.113.5"));
        assert!(!valid_hostname(""));
        assert!(!valid_hostname("a..b"));
        assert!(!valid_hostname("-a.com"));
        assert!(!valid_hostname("a_b.com"));
        assert!(!valid_hostname("a b.com"));
        assert!(!valid_hostname("[::1]"));
        assert!(!valid_hostname(&"a".repeat(64)));
    }

    fn g(s: &str) -> bool {
        is_global(s.parse().unwrap())
    }

    #[test]
    fn rejects_non_global_v4() {
        for ip in [
            "0.0.0.0",
            "10.1.2.3",
            "100.64.0.1",
            "100.127.255.255",
            "127.0.0.1",
            "169.254.169.254",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.8",
            "192.0.2.1",
            "192.168.1.1",
            "198.18.0.1",
            "198.51.100.1",
            "203.0.113.9",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
        ] {
            assert!(!g(ip), "{ip} should be rejected");
        }
    }

    #[test]
    fn accepts_global_v4() {
        for ip in [
            "1.1.1.1",
            "8.8.8.8",
            "100.63.255.255",
            "100.128.0.1",
            "172.32.0.1",
        ] {
            assert!(g(ip), "{ip} should be allowed");
        }
    }

    #[test]
    fn rejects_non_global_v6() {
        for ip in [
            "::",
            "::1",
            "::ffff:127.0.0.1",
            "::ffff:10.0.0.1",
            "64:ff9b::a00:1",
            "fc00::1",
            "fd12:3456::1",
            "fe80::1",
            "ff02::1",
            "2001:db8::1",
            "2001::1",
            "2002:c0a8:101::1",
            "100::1",
            "3fff::1",
        ] {
            assert!(!g(ip), "{ip} should be rejected");
        }
    }

    #[test]
    fn accepts_global_v6() {
        for ip in [
            "2606:4700:4700::1111",
            "2a00:1450:4001::1",
            "::ffff:8.8.8.8",
            "64:ff9b::808:808",
        ] {
            assert!(g(ip), "{ip} should be allowed");
        }
    }
}
