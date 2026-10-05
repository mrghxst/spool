//! rustls client configuration: Mozilla roots, a clock that works in the
//! browser, and ChaCha20-Poly1305 first because WASM has no AES instructions.

use std::sync::Arc;

use rustls::client::ClientConfig;
use rustls::crypto::CryptoProvider;
use rustls::time_provider::TimeProvider;
use rustls::{CipherSuite, RootCertStore};
use rustls_pki_types::UnixTime;

#[derive(Debug)]
struct Clock;

impl TimeProvider for Clock {
    fn current_time(&self) -> Option<UnixTime> {
        #[cfg(target_arch = "wasm32")]
        {
            let ms = js_sys::Date::now();
            Some(UnixTime::since_unix_epoch(
                std::time::Duration::from_millis(ms as u64),
            ))
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            Some(UnixTime::now())
        }
    }
}

fn is_chacha(s: CipherSuite) -> bool {
    matches!(
        s,
        CipherSuite::TLS13_CHACHA20_POLY1305_SHA256
            | CipherSuite::TLS_ECDHE_ECDSA_WITH_CHACHA20_POLY1305_SHA256
            | CipherSuite::TLS_ECDHE_RSA_WITH_CHACHA20_POLY1305_SHA256
    )
}

/// The ring provider with ChaCha20-Poly1305 suites ordered first.
pub fn provider() -> CryptoProvider {
    let mut p = rustls::crypto::ring::default_provider();
    // Stable sort keeps the TLS 1.3 before 1.2 order within each group.
    p.cipher_suites.sort_by_key(|s| !is_chacha(s.suite()));
    p
}

#[cfg(feature = "test-ca")]
static EXTRA_ROOTS: std::sync::Mutex<Vec<Vec<u8>>> = std::sync::Mutex::new(Vec::new());

/// Test-only: trust an extra root certificate (DER). Only compiled with the
/// `test-ca` feature, which release builds never enable.
#[cfg(feature = "test-ca")]
pub fn add_test_root(der: &[u8]) {
    EXTRA_ROOTS.lock().unwrap().push(der.to_vec());
}

fn build_config() -> Arc<ClientConfig> {
    let mut roots = RootCertStore {
        roots: webpki_roots::TLS_SERVER_ROOTS.to_vec(),
    };
    #[cfg(feature = "test-ca")]
    for der in EXTRA_ROOTS.lock().unwrap().iter() {
        let _ = roots.add(rustls_pki_types::CertificateDer::from(der.clone()));
    }
    #[cfg(not(feature = "test-ca"))]
    let _ = &mut roots;
    let config = ClientConfig::builder_with_details(Arc::new(provider()), Arc::new(Clock))
        .with_safe_default_protocol_versions()
        .expect("ring supports the default protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth();
    Arc::new(config)
}

/// Shared client config. Built once; with `test-ca`, rebuilt so roots added
/// after the first connection still apply.
pub fn client_config() -> Arc<ClientConfig> {
    #[cfg(feature = "test-ca")]
    {
        build_config()
    }
    #[cfg(not(feature = "test-ca"))]
    {
        static CONFIG: std::sync::OnceLock<Arc<ClientConfig>> = std::sync::OnceLock::new();
        CONFIG.get_or_init(build_config).clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chacha_first() {
        let p = provider();
        let suites: Vec<_> = p.cipher_suites.iter().map(|s| s.suite()).collect();
        assert_eq!(suites[0], CipherSuite::TLS13_CHACHA20_POLY1305_SHA256);
        let first_non_chacha = suites.iter().position(|&s| !is_chacha(s)).unwrap();
        assert!(suites[first_non_chacha..].iter().all(|&s| !is_chacha(s)));
    }

    #[test]
    fn config_has_mozilla_roots() {
        let c = client_config();
        assert!(c.crypto_provider().cipher_suites.len() >= 3);
    }
}
