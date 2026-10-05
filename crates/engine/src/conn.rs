//! One NNTP-over-TLS connection, sans-IO: ciphertext in, ciphertext out,
//! NNTP events up.

use std::io::{ErrorKind, Read, Write};

use rustls::ClientConnection;
use rustls_pki_types::ServerName;

use crate::nntp::{Client, Event};
use crate::tls;

pub struct Conn {
    tls: ClientConnection,
    nntp: Client,
    scratch: Vec<u8>,
    closed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnError {
    BadHost,
    BadCredentials,
    Tls(String),
}

impl std::fmt::Display for ConnError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConnError::BadHost => f.write_str("invalid server name"),
            ConnError::BadCredentials => {
                f.write_str("username or password contains invalid characters")
            }
            ConnError::Tls(e) => write!(f, "TLS error: {e}"),
        }
    }
}

fn valid_credential(s: &str) -> bool {
    !s.bytes().any(|b| b < 0x20 || b == 0x7f)
}

impl Conn {
    /// Starts a TLS client for `host` (used for SNI and certificate checks).
    /// An empty `user` skips authentication.
    pub fn new(host: &str, user: &str, pass: &str) -> Result<Conn, ConnError> {
        let name = ServerName::try_from(host.trim().to_ascii_lowercase())
            .map_err(|_| ConnError::BadHost)?;
        if !valid_credential(user) || !valid_credential(pass) {
            return Err(ConnError::BadCredentials);
        }
        let tls = ClientConnection::new(tls::client_config(), name)
            .map_err(|e| ConnError::Tls(e.to_string()))?;
        Ok(Conn {
            tls,
            nntp: Client::new(user, pass),
            scratch: vec![0u8; 64 * 1024],
            closed: false,
        })
    }

    pub fn set_pipeline(&mut self, depth: usize) {
        self.nntp.set_pipeline(depth);
    }

    /// Ciphertext received from the transport.
    pub fn on_bytes(&mut self, mut data: &[u8]) {
        if self.closed {
            return;
        }
        while !data.is_empty() {
            match self.tls.read_tls(&mut data) {
                Ok(0) => break,
                Ok(_) => {}
                Err(e) => return self.fail(format!("TLS error: {e}")),
            }
            if let Err(e) = self.tls.process_new_packets() {
                return self.fail(tls_reason(&e));
            }
            self.drain_plaintext();
            if self.closed {
                return;
            }
        }
    }

    fn drain_plaintext(&mut self) {
        loop {
            match self.tls.reader().read(&mut self.scratch) {
                Ok(0) => {
                    // Clean close_notify from the server.
                    self.nntp.transport_closed("server closed the connection");
                    self.closed = true;
                    return;
                }
                Ok(n) => self.nntp.feed(&self.scratch[..n]),
                Err(e) if e.kind() == ErrorKind::WouldBlock => return,
                Err(e) => return self.fail(format!("connection error: {e}")),
            }
        }
    }

    fn fail(&mut self, reason: String) {
        self.closed = true;
        self.nntp.transport_closed(&reason);
    }

    /// Ciphertext to send to the transport, if any.
    pub fn take_outgoing(&mut self) -> Option<Vec<u8>> {
        if let Some(plain) = self.nntp.take_output() {
            if !self.closed && self.tls.writer().write_all(&plain).is_err() {
                self.fail("TLS write failed".into());
            }
        }
        if !self.tls.wants_write() {
            return None;
        }
        let mut out = Vec::new();
        while self.tls.wants_write() {
            if self.tls.write_tls(&mut out).is_err() {
                break;
            }
        }
        if out.is_empty() {
            None
        } else {
            Some(out)
        }
    }

    pub fn request(&mut self, seg_id: u32, message_id: &str) {
        self.nntp.request_body(seg_id, message_id);
    }

    pub fn stat(&mut self, seg_id: u32, message_id: &str) {
        self.nntp.request_stat(seg_id, message_id);
    }

    pub fn date(&mut self) {
        self.nntp.request_date();
    }

    pub fn capabilities(&mut self) {
        self.nntp.request_capabilities();
    }

    /// Sends QUIT and a TLS close_notify.
    pub fn quit(&mut self) {
        self.nntp.quit();
        if let Some(plain) = self.nntp.take_output() {
            let _ = self.tls.writer().write_all(&plain);
        }
        self.tls.send_close_notify();
    }

    pub fn poll_event(&mut self) -> Option<Event> {
        self.nntp.poll_event()
    }

    /// The transport closed (WebSocket close or TCP EOF).
    pub fn transport_closed(&mut self, reason: &str) {
        self.closed = true;
        self.nntp.transport_closed(reason);
    }

    pub fn is_closed(&self) -> bool {
        self.closed || self.nntp.is_closed()
    }

    /// Requests not yet answered.
    pub fn outstanding(&self) -> usize {
        self.nntp.outstanding()
    }

    pub fn handshake_done(&self) -> bool {
        !self.tls.is_handshaking()
    }
}

fn tls_reason(e: &rustls::Error) -> String {
    use rustls::{CertificateError, Error};
    match e {
        Error::InvalidCertificate(CertificateError::NotValidForName)
        | Error::InvalidCertificate(CertificateError::NotValidForNameContext { .. }) => {
            "TLS error: the server's certificate doesn't match its name".into()
        }
        Error::InvalidCertificate(CertificateError::Expired)
        | Error::InvalidCertificate(CertificateError::ExpiredContext { .. }) => {
            "TLS error: the server's certificate has expired".into()
        }
        Error::InvalidCertificate(CertificateError::UnknownIssuer) => {
            "TLS error: the server's certificate isn't trusted".into()
        }
        other => format!("TLS error: {other}"),
    }
}
