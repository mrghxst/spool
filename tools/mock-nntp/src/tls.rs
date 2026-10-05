//! A throwaway test CA and server certificate, generated at startup.

use std::sync::Arc;

use rcgen::{BasicConstraints, CertificateParams, DnType, IsCa, Issuer, KeyPair, KeyUsagePurpose};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio_rustls::TlsAcceptor;

pub struct TestCa {
    pub ca_der: Vec<u8>,
    pub ca_pem: String,
    pub acceptor: TlsAcceptor,
}

/// Creates a CA and a server certificate for `localhost`, `127.0.0.1` and
/// `::1`, valid for a day.
pub fn generate() -> TestCa {
    let ca_key = KeyPair::generate().expect("generate CA key");
    let mut ca_params = CertificateParams::new(Vec::<String>::new()).expect("CA params");
    ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    // A unique name, so certificates from another run never chain to it.
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    ca_params.distinguished_name.push(
        DnType::CommonName,
        format!("Spool mock-nntp test CA {nonce:x}"),
    );
    ca_params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let now = std::time::SystemTime::now();
    ca_params.not_before = (now - std::time::Duration::from_secs(3600)).into();
    ca_params.not_after = (now + std::time::Duration::from_secs(86400)).into();
    let ca_cert = ca_params.self_signed(&ca_key).expect("self-sign CA");
    let issuer = Issuer::new(ca_params, ca_key);

    let leaf_key = KeyPair::generate().expect("generate leaf key");
    let mut leaf_params = CertificateParams::new(vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ])
    .expect("leaf params");
    leaf_params
        .distinguished_name
        .push(DnType::CommonName, "localhost");
    leaf_params.not_before = (now - std::time::Duration::from_secs(3600)).into();
    leaf_params.not_after = (now + std::time::Duration::from_secs(86400)).into();
    let leaf_cert = leaf_params
        .signed_by(&leaf_key, &issuer)
        .expect("sign leaf");

    let chain = vec![
        CertificateDer::from(leaf_cert.der().to_vec()),
        CertificateDer::from(ca_cert.der().to_vec()),
    ];
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(leaf_key.serialize_der()));
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = rustls::ServerConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_no_client_auth()
        .with_single_cert(chain, key)
        .expect("server config");
    TestCa {
        ca_der: ca_cert.der().to_vec(),
        ca_pem: ca_cert.pem(),
        acceptor: TlsAcceptor::from(Arc::new(config)),
    }
}
