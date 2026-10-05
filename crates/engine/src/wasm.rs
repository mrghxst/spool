//! JavaScript bindings (wasm-bindgen).

use js_sys::{Array, Object, Reflect, Uint8Array};
use wasm_bindgen::prelude::*;

use crate::nntp::Event;
use crate::{conn, nzb, par2};

fn set(o: &Object, k: &str, v: impl Into<JsValue>) {
    let _ = Reflect::set(o, &JsValue::from_str(k), &v.into());
}

fn event_to_js(ev: Event) -> JsValue {
    let o = Object::new();
    match ev {
        Event::Ready { posting } => {
            set(&o, "type", "ready");
            set(&o, "posting", posting);
        }
        Event::AuthOk => set(&o, "type", "authOk"),
        Event::AuthFailed { code } => {
            set(&o, "type", "authFailed");
            set(&o, "code", code);
        }
        Event::Segment(s) => {
            set(&o, "type", "segment");
            set(&o, "segId", s.seg_id);
            set(&o, "begin", s.begin as f64);
            set(&o, "data", Uint8Array::from(&s.data[..]));
            set(&o, "crcOk", s.crc_ok);
            set(&o, "name", s.name);
            set(&o, "fileSize", s.file_size as f64);
            set(&o, "part", s.part);
        }
        Event::Missing { seg_id } => {
            set(&o, "type", "missing");
            set(&o, "segId", seg_id);
        }
        Event::Stat { seg_id, exists } => {
            set(&o, "type", "stat");
            set(&o, "segId", seg_id);
            set(&o, "exists", exists);
        }
        Event::Date { value } => {
            set(&o, "type", "date");
            set(&o, "value", value);
        }
        Event::Capabilities { lines } => {
            set(&o, "type", "capabilities");
            let a = Array::new();
            for l in lines {
                a.push(&JsValue::from_str(&l));
            }
            set(&o, "lines", a);
        }
        Event::Busy { code } => {
            set(&o, "type", "busy");
            set(&o, "code", code);
        }
        Event::Closed { reason } => {
            set(&o, "type", "closed");
            set(&o, "reason", reason);
        }
    }
    o.into()
}

/// One NNTP-over-TLS connection. Move bytes between it and a WebSocket.
#[wasm_bindgen]
pub struct Conn {
    inner: conn::Conn,
}

#[wasm_bindgen]
impl Conn {
    #[wasm_bindgen(constructor)]
    pub fn new(host: &str, user: &str, pass: &str) -> Result<Conn, JsError> {
        conn::Conn::new(host, user, pass)
            .map(|inner| Conn { inner })
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// Ciphertext from the WebSocket.
    pub fn on_bytes(&mut self, data: &[u8]) {
        self.inner.on_bytes(data);
    }

    /// Ciphertext for the WebSocket, or undefined.
    pub fn take_outgoing(&mut self) -> Option<Vec<u8>> {
        self.inner.take_outgoing()
    }

    pub fn request(&mut self, seg_id: u32, message_id: &str) {
        self.inner.request(seg_id, message_id);
    }

    pub fn stat(&mut self, seg_id: u32, message_id: &str) {
        self.inner.stat(seg_id, message_id);
    }

    pub fn date(&mut self) {
        self.inner.date();
    }

    pub fn capabilities(&mut self) {
        self.inner.capabilities();
    }

    pub fn quit(&mut self) {
        self.inner.quit();
    }

    pub fn set_pipeline(&mut self, depth: usize) {
        self.inner.set_pipeline(depth);
    }

    pub fn outstanding(&self) -> usize {
        self.inner.outstanding()
    }

    pub fn transport_closed(&mut self, reason: &str) {
        self.inner.transport_closed(reason);
    }

    /// The next event object, or undefined.
    pub fn poll_event(&mut self) -> JsValue {
        match self.inner.poll_event() {
            Some(ev) => event_to_js(ev),
            None => JsValue::UNDEFINED,
        }
    }
}

// --- JSON helpers (tiny, to avoid pulling in serde) ---

fn js_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

fn opt_str(out: &mut String, s: &Option<String>) {
    match s {
        Some(s) => js_str(out, s),
        None => out.push_str("null"),
    }
}

/// Parses an NZB file and returns JSON (see `apps/web/src/lib/engine/types.ts`).
#[wasm_bindgen]
pub fn parse_nzb(xml: &[u8]) -> Result<String, JsError> {
    let nzb = nzb::parse(xml).map_err(|e| JsError::new(&e.to_string()))?;
    let mut o = String::with_capacity(xml.len() / 2);
    o.push_str("{\"title\":");
    opt_str(&mut o, &nzb.title);
    o.push_str(",\"name\":");
    opt_str(&mut o, &nzb.name);
    o.push_str(",\"password\":");
    opt_str(&mut o, &nzb.password);
    o.push_str(",\"files\":[");
    for (i, f) in nzb.files.iter().enumerate() {
        if i > 0 {
            o.push(',');
        }
        o.push_str("{\"subject\":");
        js_str(&mut o, &f.subject);
        o.push_str(",\"filename\":");
        js_str(&mut o, &f.filename);
        let (kind, blocks) = match f.kind {
            nzb::FileKind::Par2Index => ("par2", None),
            nzb::FileKind::Par2Recovery { blocks } => ("par2vol", blocks),
            nzb::FileKind::Archive => ("archive", None),
            nzb::FileKind::Other => ("other", None),
        };
        o.push_str(&format!(
            ",\"kind\":\"{kind}\",\"blocks\":{},\"bytes\":{},\"date\":{},\"groups\":[",
            blocks.map_or("null".to_string(), |b| b.to_string()),
            f.bytes,
            f.date
        ));
        for (j, g) in f.groups.iter().enumerate() {
            if j > 0 {
                o.push(',');
            }
            js_str(&mut o, g);
        }
        o.push_str("],\"segments\":[");
        for (j, s) in f.segments.iter().enumerate() {
            if j > 0 {
                o.push(',');
            }
            o.push_str(&format!("[{},{},", s.number, s.bytes));
            js_str(&mut o, &s.message_id);
            o.push(']');
        }
        o.push_str("]}");
    }
    o.push_str("]}");
    Ok(o)
}

/// Classifies a file name: "par2", "par2vol", "archive" or "other".
#[wasm_bindgen]
pub fn classify(name: &str) -> String {
    match nzb::classify(name) {
        nzb::FileKind::Par2Index => "par2",
        nzb::FileKind::Par2Recovery { .. } => "par2vol",
        nzb::FileKind::Archive => "archive",
        nzb::FileKind::Other => "other",
    }
    .into()
}

#[wasm_bindgen]
pub fn sanitize_filename(name: &str) -> String {
    nzb::sanitize(name)
}

/// A parsed PAR2 recovery set.
#[wasm_bindgen]
pub struct Par2Set {
    inner: par2::Set,
}

#[wasm_bindgen]
impl Par2Set {
    pub fn parse(bytes: &[u8]) -> Result<Par2Set, JsError> {
        par2::Set::parse(bytes)
            .map(|inner| Par2Set { inner })
            .map_err(|e| JsError::new(&e.to_string()))
    }

    /// JSON: { setId, sliceSize, totalSlices, files: [...] }.
    pub fn info(&self) -> String {
        let s = &self.inner;
        let mut o = format!(
            "{{\"setId\":\"{}\",\"sliceSize\":{},\"totalSlices\":{},\"missingDescriptions\":{},\"files\":[",
            par2::hex(&s.set_id),
            s.slice_size,
            s.total_slices(),
            s.missing_descriptions
        );
        for (i, f) in s.files.iter().enumerate() {
            if i > 0 {
                o.push(',');
            }
            o.push_str("{\"name\":");
            js_str(&mut o, &f.name);
            o.push_str(&format!(
                ",\"id\":\"{}\",\"md5\":\"{}\",\"md5_16k\":\"{}\",\"length\":{},\"firstSlice\":{},\"sliceCount\":{},\"hasChecks\":{}}}",
                par2::hex(&f.id),
                par2::hex(&f.md5),
                par2::hex(&f.md5_16k),
                f.length,
                f.first_slice,
                f.slice_count(s.slice_size),
                !f.slices.is_empty()
            ));
        }
        o.push_str("]}");
        o
    }

    /// Index of the file whose first-16-KiB MD5 (and length) match, or -1.
    pub fn match_16k(&self, md5_16k: &[u8], length: f64) -> i32 {
        let Ok(h) = <[u8; 16]>::try_from(md5_16k) else {
            return -1;
        };
        match self.inner.match_16k(&h, length as u64) {
            Some(f) => self
                .inner
                .files
                .iter()
                .position(|x| x.id == f.id)
                .map_or(-1, |i| i as i32),
            None => -1,
        }
    }

    /// Checks a whole recovery packet. JSON { exponent, dataOffset, dataLen }
    /// or undefined if it's not a valid recovery packet of this set.
    pub fn recovery_packet(&self, packet: &[u8]) -> Option<String> {
        par2::recovery_packet(packet, &self.inner.set_id).map(|r| {
            format!(
                "{{\"exponent\":{},\"dataOffset\":{},\"dataLen\":{}}}",
                r.exponent, r.data_offset, r.data_len
            )
        })
    }
}

/// Reads a 64-byte PAR2 packet header. JSON { len, type } or undefined.
#[wasm_bindgen]
pub fn par2_header(bytes: &[u8]) -> Option<String> {
    par2::parse_header(bytes).map(|h| {
        let kind = match &h.kind {
            k if k == par2::TYPE_MAIN => "main",
            k if k == par2::TYPE_FILE_DESC => "filedesc",
            k if k == par2::TYPE_IFSC => "ifsc",
            k if k == par2::TYPE_RECOVERY => "recovery",
            _ => "other",
        };
        format!(
            "{{\"len\":{},\"type\":\"{kind}\",\"setId\":\"{}\"}}",
            h.len,
            par2::hex(&h.set_id)
        )
    })
}

#[wasm_bindgen]
pub struct Par2Verifier {
    inner: Option<par2::FileVerifier>,
}

#[wasm_bindgen]
impl Par2Verifier {
    #[wasm_bindgen(constructor)]
    pub fn new(set: &Par2Set, file_index: usize, deep: bool) -> Result<Par2Verifier, JsError> {
        let f = set
            .inner
            .files
            .get(file_index)
            .ok_or_else(|| JsError::new("no such file in the PAR2 set"))?;
        Ok(Par2Verifier {
            inner: Some(par2::FileVerifier::new(&set.inner, f, deep)),
        })
    }

    pub fn update(&mut self, data: &[u8]) {
        if let Some(v) = self.inner.as_mut() {
            v.update(data);
        }
    }

    /// JSON { ok, damaged: [local slice indexes] }.
    pub fn finish(&mut self) -> String {
        match self.inner.take() {
            Some(v) => {
                let r = v.finish();
                let d: Vec<String> = r.damaged.iter().map(u32::to_string).collect();
                format!("{{\"ok\":{},\"damaged\":[{}]}}", r.ok, d.join(","))
            }
            None => "{\"ok\":false,\"damaged\":[]}".into(),
        }
    }
}

#[wasm_bindgen]
pub struct Par2Repairer {
    inner: Option<par2::Repairer>,
}

#[wasm_bindgen]
impl Par2Repairer {
    #[wasm_bindgen(constructor)]
    pub fn new(
        total_slices: u32,
        missing: Vec<u32>,
        exponents: Vec<u32>,
        stripe_len: usize,
    ) -> Result<Par2Repairer, JsError> {
        par2::Repairer::new(total_slices, &missing, &exponents, stripe_len)
            .map(|r| Par2Repairer { inner: Some(r) })
            .map_err(|e| JsError::new(&e.to_string()))
    }

    pub fn add_recovery(&mut self, exponent: u32, data: &[u8]) {
        if let Some(r) = self.inner.as_mut() {
            r.add_recovery(exponent, data);
        }
    }

    pub fn add_input(&mut self, index: u32, data: &[u8]) {
        if let Some(r) = self.inner.as_mut() {
            r.add_input(index, data);
        }
    }

    /// The rebuilt stripes, concatenated in `missing` order.
    pub fn solve(&mut self) -> Result<Vec<u8>, JsError> {
        let r = self
            .inner
            .take()
            .ok_or_else(|| JsError::new("already solved"))?;
        let parts = r.solve().map_err(|e| JsError::new(&e.to_string()))?;
        Ok(parts.concat())
    }
}

/// Streaming MD5.
#[wasm_bindgen]
pub struct Md5 {
    inner: md5::Md5,
}

#[wasm_bindgen]
impl Md5 {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Md5 {
        use md5::Digest;
        Md5 {
            inner: md5::Md5::new(),
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        use md5::Digest;
        self.inner.update(data);
    }

    pub fn finish(&mut self) -> Vec<u8> {
        use md5::Digest;
        std::mem::replace(&mut self.inner, md5::Md5::new())
            .finalize()
            .to_vec()
    }
}

impl Default for Md5 {
    fn default() -> Self {
        Md5::new()
    }
}

#[wasm_bindgen]
pub fn md5(data: &[u8]) -> Vec<u8> {
    par2::md5(data).to_vec()
}

#[wasm_bindgen]
pub fn crc32(data: &[u8]) -> u32 {
    crc32fast::hash(data)
}

#[wasm_bindgen]
pub fn engine_version() -> String {
    env!("CARGO_PKG_VERSION").into()
}

/// Test-only: trust an extra root CA (DER). Only exists in builds with the
/// `test-ca` feature, which release builds never enable.
#[cfg(feature = "test-ca")]
#[wasm_bindgen]
pub fn add_test_root(der: &[u8]) {
    crate::tls::add_test_root(der);
}
