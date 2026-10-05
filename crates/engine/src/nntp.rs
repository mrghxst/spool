//! Sans-IO NNTP client: a read-only command whitelist, a pipelined request
//! queue and a response parser that works on arbitrarily split input.

use std::collections::VecDeque;

use crate::yenc;

/// Maximum `BODY` (or `STAT`) requests on the wire per connection.
pub const DEFAULT_PIPELINE: usize = 8;

/// The only verbs the engine can ever send. Posting (`POST`, `IHAVE`) and
/// anything else is impossible: there is no `Command` variant for it, and
/// every encoded line is checked against this list before it's queued.
pub const ALLOWED_VERBS: &[&str] = &["AUTHINFO", "BODY", "STAT", "DATE", "CAPABILITIES", "QUIT"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command<'a> {
    AuthUser(&'a str),
    AuthPass(&'a str),
    Body(&'a str),
    Stat(&'a str),
    Date,
    Capabilities,
    Quit,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandError {
    /// The verb isn't on the read-only whitelist.
    NotAllowed(String),
    /// An argument contains characters that could smuggle a second command.
    BadArgument,
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::NotAllowed(v) => write!(f, "command not allowed: {v}"),
            CommandError::BadArgument => f.write_str("invalid command argument"),
        }
    }
}

fn clean_arg(arg: &str) -> Result<&str, CommandError> {
    if arg.is_empty() || arg.len() > 480 || arg.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Err(CommandError::BadArgument);
    }
    Ok(arg)
}

/// Normalises a message-id to `<...>` form and validates it.
pub fn message_id(raw: &str) -> Result<String, CommandError> {
    let id = raw.trim();
    let id = id.strip_prefix('<').unwrap_or(id);
    let id = id.strip_suffix('>').unwrap_or(id);
    if id.is_empty()
        || id.len() > 250
        || id
            .bytes()
            .any(|b| b <= 0x20 || b == 0x7f || b == b'<' || b == b'>')
    {
        return Err(CommandError::BadArgument);
    }
    Ok(format!("<{id}>"))
}

/// Encodes a command as a CRLF-terminated line, checked by [`check_line`].
pub fn encode(cmd: &Command) -> Result<Vec<u8>, CommandError> {
    let line = match cmd {
        Command::AuthUser(u) => format!("AUTHINFO USER {}\r\n", clean_arg(u)?),
        Command::AuthPass(p) => format!("AUTHINFO PASS {}\r\n", clean_arg(p)?),
        Command::Body(id) => format!("BODY {}\r\n", message_id(id)?),
        Command::Stat(id) => format!("STAT {}\r\n", message_id(id)?),
        Command::Date => "DATE\r\n".to_string(),
        Command::Capabilities => "CAPABILITIES\r\n".to_string(),
        Command::Quit => "QUIT\r\n".to_string(),
    };
    let bytes = line.into_bytes();
    check_line(&bytes)?;
    Ok(bytes)
}

/// The single gate every outgoing line passes through: exactly one
/// CRLF-terminated line whose verb is on [`ALLOWED_VERBS`].
pub fn check_line(line: &[u8]) -> Result<(), CommandError> {
    let body = line
        .strip_suffix(b"\r\n")
        .ok_or(CommandError::BadArgument)?;
    if body.iter().any(|&b| b == b'\r' || b == b'\n' || b == 0) {
        return Err(CommandError::BadArgument);
    }
    let text = std::str::from_utf8(body).map_err(|_| CommandError::BadArgument)?;
    let mut words = text.split(' ');
    let verb = words.next().unwrap_or("").to_ascii_uppercase();
    if !ALLOWED_VERBS.contains(&verb.as_str()) {
        return Err(CommandError::NotAllowed(verb));
    }
    if verb == "AUTHINFO" {
        let sub = words.next().unwrap_or("").to_ascii_uppercase();
        if sub != "USER" && sub != "PASS" {
            return Err(CommandError::NotAllowed(format!("AUTHINFO {sub}")));
        }
    }
    Ok(())
}

/// A decoded article.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub seg_id: u32,
    /// 0-based offset of `data` in the file.
    pub begin: u64,
    pub data: Vec<u8>,
    /// Part CRC32 matched (or, for single-part posts, the file CRC32).
    pub crc_ok: bool,
    /// yEnc `name=`, the authoritative file name.
    pub name: String,
    /// yEnc total file size.
    pub file_size: u64,
    pub part: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The server greeted us and accepts commands.
    Ready {
        posting: bool,
    },
    AuthOk,
    AuthFailed {
        code: u16,
    },
    Segment(Segment),
    Missing {
        seg_id: u32,
    },
    Stat {
        seg_id: u32,
        exists: bool,
    },
    Date {
        value: String,
    },
    Capabilities {
        lines: Vec<String>,
    },
    /// 400 or 502: the server is busy or we hit a connection limit.
    Busy {
        code: u16,
    },
    Closed {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Greeting,
    AuthUser,
    AuthPass,
    Ready,
    Closed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Kind {
    Body,
    Stat,
    Date,
    Capabilities,
    Quit,
}

#[derive(Debug, Clone)]
struct Pending {
    kind: Kind,
    seg_id: u32,
    message_id: String,
}

#[derive(Debug)]
enum Reading {
    Status,
    /// Multi-line block for the front in-flight request.
    Block {
        lines: Vec<u8>,
    },
}

/// Pipelined NNTP client over a plaintext byte stream.
#[derive(Debug)]
pub struct Client {
    state: State,
    user: String,
    pass: String,
    pipeline: usize,
    inbuf: Vec<u8>,
    pos: usize,
    reading: Reading,
    out: Vec<u8>,
    queue: VecDeque<Pending>,
    inflight: VecDeque<Pending>,
    events: VecDeque<Event>,
}

impl Client {
    pub fn new(user: &str, pass: &str) -> Client {
        Client {
            state: State::Greeting,
            user: user.to_string(),
            pass: pass.to_string(),
            pipeline: DEFAULT_PIPELINE,
            inbuf: Vec::with_capacity(64 * 1024),
            pos: 0,
            reading: Reading::Status,
            out: Vec::new(),
            queue: VecDeque::new(),
            inflight: VecDeque::new(),
            events: VecDeque::new(),
        }
    }

    pub fn set_pipeline(&mut self, depth: usize) {
        self.pipeline = depth.max(1);
    }

    pub fn is_closed(&self) -> bool {
        self.state == State::Closed
    }

    /// Requests not yet answered, in order (queued and in flight).
    pub fn outstanding(&self) -> usize {
        self.queue.len() + self.inflight.len()
    }

    pub fn request_body(&mut self, seg_id: u32, message_id: &str) {
        self.enqueue(Kind::Body, seg_id, message_id);
    }

    pub fn request_stat(&mut self, seg_id: u32, message_id: &str) {
        self.enqueue(Kind::Stat, seg_id, message_id);
    }

    pub fn request_date(&mut self) {
        self.enqueue(Kind::Date, 0, "");
    }

    pub fn request_capabilities(&mut self) {
        self.enqueue(Kind::Capabilities, 0, "");
    }

    pub fn quit(&mut self) {
        self.enqueue(Kind::Quit, 0, "");
    }

    fn enqueue(&mut self, kind: Kind, seg_id: u32, message_id: &str) {
        if self.state == State::Closed {
            return;
        }
        if matches!(kind, Kind::Body | Kind::Stat) && self::message_id(message_id).is_err() {
            // A malformed id can never be fetched; report it as missing.
            self.events.push_back(match kind {
                Kind::Stat => Event::Stat {
                    seg_id,
                    exists: false,
                },
                _ => Event::Missing { seg_id },
            });
            return;
        }
        self.queue.push_back(Pending {
            kind,
            seg_id,
            message_id: message_id.to_string(),
        });
        self.pump();
    }

    /// The one place that queues bytes for the server.
    fn send(&mut self, cmd: Command) -> bool {
        match encode(&cmd) {
            Ok(line) => {
                self.out.extend_from_slice(&line);
                true
            }
            Err(_) => false,
        }
    }

    fn pump(&mut self) {
        if self.state != State::Ready {
            return;
        }
        while self.inflight.len() < self.pipeline {
            let Some(p) = self.queue.pop_front() else {
                break;
            };
            let sent = match p.kind {
                Kind::Body => self.send(Command::Body(&p.message_id)),
                Kind::Stat => self.send(Command::Stat(&p.message_id)),
                Kind::Date => self.send(Command::Date),
                Kind::Capabilities => self.send(Command::Capabilities),
                Kind::Quit => self.send(Command::Quit),
            };
            if sent {
                let quit = p.kind == Kind::Quit;
                self.inflight.push_back(p);
                if quit {
                    break;
                }
            }
        }
    }

    /// Plaintext bytes to send to the server.
    pub fn take_output(&mut self) -> Option<Vec<u8>> {
        if self.out.is_empty() {
            None
        } else {
            Some(std::mem::take(&mut self.out))
        }
    }

    pub fn poll_event(&mut self) -> Option<Event> {
        self.events.pop_front()
    }

    /// The transport closed. Requests that weren't answered are dropped; the
    /// caller requeues them elsewhere.
    pub fn transport_closed(&mut self, reason: &str) {
        if self.state != State::Closed {
            self.close(reason.to_string());
        }
    }

    fn close(&mut self, reason: String) {
        self.state = State::Closed;
        self.queue.clear();
        self.inflight.clear();
        self.events.push_back(Event::Closed { reason });
    }

    /// Feeds plaintext from the server.
    pub fn feed(&mut self, data: &[u8]) {
        if self.state == State::Closed {
            return;
        }
        if self.pos > 0 && self.pos == self.inbuf.len() {
            self.inbuf.clear();
            self.pos = 0;
        }
        self.inbuf.extend_from_slice(data);
        self.process();
        // Compact once the consumed prefix dominates the buffer.
        if self.pos > 0 && (self.pos >= self.inbuf.len() / 2 || self.pos > 1 << 20) {
            self.inbuf.drain(..self.pos);
            self.pos = 0;
        }
        self.pump();
    }

    fn next_line(&mut self) -> Option<(usize, usize)> {
        let rest = &self.inbuf[self.pos..];
        let nl = memchr::memchr(b'\n', rest)?;
        let start = self.pos;
        self.pos += nl + 1;
        Some((start, self.pos))
    }

    fn process(&mut self) {
        while self.state != State::Closed {
            match &mut self.reading {
                Reading::Status => {
                    let Some((s, e)) = self.next_line() else {
                        return;
                    };
                    let line = trim_crlf(&self.inbuf[s..e]).to_vec();
                    self.on_status(&line);
                }
                Reading::Block { .. } => {
                    if !self.read_block() {
                        return;
                    }
                }
            }
        }
    }

    /// Reads block lines until the terminator. Returns false if more input is
    /// needed.
    fn read_block(&mut self) -> bool {
        loop {
            let rest = &self.inbuf[self.pos..];
            let Some(nl) = memchr::memchr(b'\n', rest) else {
                return false;
            };
            let line = &rest[..=nl];
            let Reading::Block { lines } = &mut self.reading else {
                return true;
            };
            if line == b".\r\n" || line == b".\n" {
                self.pos += nl + 1;
                let block = std::mem::take(lines);
                self.reading = Reading::Status;
                self.finish_block(block);
                return true;
            }
            // Dot-unstuffing: a leading ".." becomes ".".
            if line.first() == Some(&b'.') {
                lines.extend_from_slice(&line[1..]);
            } else {
                lines.extend_from_slice(line);
            }
            self.pos += nl + 1;
        }
    }

    fn on_status(&mut self, line: &[u8]) {
        let code = parse_code(line);
        let text = String::from_utf8_lossy(line).into_owned();
        match self.state {
            State::Greeting => match code {
                Some(200) | Some(201) => {
                    self.events.push_back(Event::Ready {
                        posting: code == Some(200),
                    });
                    if self.user.is_empty() {
                        self.authenticated();
                    } else {
                        self.state = State::AuthUser;
                        let user = self.user.clone();
                        if !self.send(Command::AuthUser(&user)) {
                            self.events.push_back(Event::AuthFailed { code: 0 });
                            self.close("invalid username".into());
                        }
                    }
                }
                Some(c @ (400 | 502)) => {
                    self.events.push_back(Event::Busy { code: c });
                    self.close(text);
                }
                _ => self.close(format!("unexpected greeting: {text}")),
            },
            State::AuthUser | State::AuthPass => match code {
                Some(381) if self.state == State::AuthUser => {
                    self.state = State::AuthPass;
                    let pass = self.pass.clone();
                    if !self.send(Command::AuthPass(&pass)) {
                        self.events.push_back(Event::AuthFailed { code: 0 });
                        self.close("invalid password".into());
                    }
                }
                Some(281) => self.authenticated(),
                Some(c @ (400 | 502)) => {
                    self.events.push_back(Event::Busy { code: c });
                    self.close(text);
                }
                Some(c) => {
                    self.events.push_back(Event::AuthFailed { code: c });
                    self.close(text);
                }
                None => self.close(format!("unexpected reply: {text}")),
            },
            State::Ready => self.on_reply(code, text),
            State::Closed => {}
        }
    }

    fn authenticated(&mut self) {
        self.state = State::Ready;
        self.events.push_back(Event::AuthOk);
        self.pump();
    }

    fn on_reply(&mut self, code: Option<u16>, text: String) {
        let Some(front) = self.inflight.front() else {
            // Unsolicited reply, e.g. "400 idle timeout" or "205 bye".
            match code {
                Some(c @ (400 | 502)) => {
                    self.events.push_back(Event::Busy { code: c });
                    self.close(text);
                }
                _ => self.close(text),
            }
            return;
        };
        let (kind, seg_id) = (front.kind.clone(), front.seg_id);
        match (kind, code) {
            (Kind::Body, Some(222)) | (Kind::Capabilities, Some(101)) => {
                self.reading = Reading::Block {
                    lines: Vec::with_capacity(if code == Some(222) { 800 * 1024 } else { 1024 }),
                };
            }
            (Kind::Body, Some(420 | 423 | 430 | 451)) => {
                self.inflight.pop_front();
                self.events.push_back(Event::Missing { seg_id });
            }
            (Kind::Stat, Some(223)) => {
                self.inflight.pop_front();
                self.events.push_back(Event::Stat {
                    seg_id,
                    exists: true,
                });
            }
            (Kind::Stat, Some(420 | 423 | 430 | 451)) => {
                self.inflight.pop_front();
                self.events.push_back(Event::Stat {
                    seg_id,
                    exists: false,
                });
            }
            (Kind::Date, Some(111)) => {
                self.inflight.pop_front();
                let value = text.get(4..).unwrap_or("").trim().to_string();
                self.events.push_back(Event::Date { value });
            }
            (Kind::Quit, _) => {
                self.inflight.pop_front();
                self.close("quit".into());
            }
            (_, Some(c @ (400 | 502))) => {
                self.events.push_back(Event::Busy { code: c });
                self.close(text);
            }
            (_, Some(c @ 480..=482)) => {
                self.events.push_back(Event::AuthFailed { code: c });
                self.close(text);
            }
            _ => self.close(format!("unexpected reply: {text}")),
        }
    }

    fn finish_block(&mut self, block: Vec<u8>) {
        let Some(p) = self.inflight.pop_front() else {
            return;
        };
        match p.kind {
            Kind::Body => {
                let ev = match yenc::decode(&block) {
                    Ok(d) => Event::Segment(Segment {
                        seg_id: p.seg_id,
                        begin: d.begin,
                        crc_ok: d.crc_ok,
                        name: d.name,
                        file_size: d.size,
                        part: d.part,
                        data: d.data,
                    }),
                    Err(_) => Event::Segment(Segment {
                        seg_id: p.seg_id,
                        begin: 0,
                        data: Vec::new(),
                        crc_ok: false,
                        name: String::new(),
                        file_size: 0,
                        part: 0,
                    }),
                };
                self.events.push_back(ev);
            }
            Kind::Capabilities => {
                let lines = String::from_utf8_lossy(&block)
                    .lines()
                    .map(|l| l.trim_end().to_string())
                    .filter(|l| !l.is_empty())
                    .collect();
                self.events.push_back(Event::Capabilities { lines });
            }
            _ => {}
        }
    }
}

fn trim_crlf(line: &[u8]) -> &[u8] {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    line.strip_suffix(b"\r").unwrap_or(line)
}

fn parse_code(line: &[u8]) -> Option<u16> {
    if line.len() < 3 || !line[..3].iter().all(u8::is_ascii_digit) {
        return None;
    }
    if line.len() > 3 && line[3] != b' ' && line[3] != b'-' {
        return None;
    }
    std::str::from_utf8(&line[..3]).ok()?.parse().ok()
}

/// Dot-stuffs a block for transmission (used by tests and the mock server).
pub fn dot_stuff(block: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(block.len() + block.len() / 64);
    let mut at_line_start = true;
    for &b in block {
        if at_line_start && b == b'.' {
            out.push(b'.');
        }
        out.push(b);
        at_line_start = b == b'\n';
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn whitelist_rejects_posting() {
        for line in [
            &b"POST\r\n"[..],
            b"IHAVE <a@b>\r\n",
            b"post\r\n",
            b"GROUP alt.test\r\n",
            b"ARTICLE <a@b>\r\n",
            b"AUTHINFO SASL PLAIN\r\n",
            b"BODY <a@b>\r\nPOST\r\n",
            b"BODY <a@b>",
            b"BODY <a@b>\nPOST\r\n",
        ] {
            assert!(
                check_line(line).is_err(),
                "{:?} must be rejected",
                String::from_utf8_lossy(line)
            );
        }
        for line in [
            &b"BODY <a@b>\r\n"[..],
            b"STAT <a@b>\r\n",
            b"AUTHINFO USER me\r\n",
            b"AUTHINFO PASS secret\r\n",
            b"DATE\r\n",
            b"CAPABILITIES\r\n",
            b"QUIT\r\n",
        ] {
            assert!(check_line(line).is_ok());
        }
    }

    #[test]
    fn arguments_cannot_inject_commands() {
        assert!(encode(&Command::Body("a@b>\r\nPOST")).is_err());
        assert!(encode(&Command::Body("a@b\r\nPOST")).is_err());
        assert!(encode(&Command::Stat("a b@c")).is_err());
        assert!(encode(&Command::AuthUser("me\r\nPOST")).is_err());
        assert!(encode(&Command::AuthPass("pw\nIHAVE <x>")).is_err());
        assert!(encode(&Command::AuthPass("")).is_err());
        assert_eq!(encode(&Command::Body("a@b")).unwrap(), b"BODY <a@b>\r\n");
        assert_eq!(encode(&Command::Body("<a@b>")).unwrap(), b"BODY <a@b>\r\n");
    }

    #[test]
    fn client_never_emits_disallowed_commands() {
        // Drive a full session and check every byte the client writes.
        let mut c = Client::new("user", "pass");
        let mut sent = Vec::new();
        c.request_body(1, "x\r\nPOST");
        c.request_body(2, "ok@test");
        c.request_stat(3, "ok2@test");
        c.request_date();
        c.request_capabilities();
        c.feed(b"200 hi\r\n");
        sent.extend(c.take_output().unwrap());
        c.feed(b"381 more\r\n");
        sent.extend(c.take_output().unwrap());
        c.feed(b"281 ok\r\n");
        sent.extend(c.take_output().unwrap());
        c.quit();
        sent.extend(c.take_output().unwrap());
        for line in sent.split_inclusive(|&b| b == b'\n') {
            check_line(line).unwrap();
            let verb = line.split(|&b| b == b' ' || b == b'\r').next().unwrap();
            assert!(verb != b"POST" && verb != b"IHAVE");
        }
        assert_eq!(c.poll_event(), Some(Event::Missing { seg_id: 1 }));
    }

    fn article(name: &str, data: &[u8]) -> Vec<u8> {
        let enc = yenc::encode_part(name, data.len() as u64, 1, 1, 0, data, 128);
        let mut out = b"222 0 <x@y> body\r\n".to_vec();
        out.extend_from_slice(&crate::nntp::dot_stuff(&enc));
        out.extend_from_slice(b".\r\n");
        out
    }

    fn authed() -> Client {
        let mut c = Client::new("u", "p");
        c.feed(b"201 ready\r\n381 pass\r\n281 ok\r\n");
        assert_eq!(
            c.take_output().unwrap(),
            b"AUTHINFO USER u\r\nAUTHINFO PASS p\r\n".to_vec()
        );
        let evs: Vec<_> = std::iter::from_fn(|| c.poll_event()).collect();
        assert_eq!(evs, vec![Event::Ready { posting: false }, Event::AuthOk]);
        c
    }

    #[test]
    fn auth_sequence() {
        let mut c = Client::new("u", "p");
        c.feed(b"200 welcome\r\n");
        assert_eq!(c.take_output().unwrap(), b"AUTHINFO USER u\r\n");
        c.feed(b"381 PASS required\r\n");
        assert_eq!(c.take_output().unwrap(), b"AUTHINFO PASS p\r\n");
        c.feed(b"281 Ok\r\n");
        assert_eq!(c.poll_event(), Some(Event::Ready { posting: true }));
        assert_eq!(c.poll_event(), Some(Event::AuthOk));
    }

    #[test]
    fn auth_failures() {
        for (reply, ev) in [
            (&b"481 bad\r\n"[..], Event::AuthFailed { code: 481 }),
            (b"482 bad\r\n", Event::AuthFailed { code: 482 }),
            (b"502 too many\r\n", Event::Busy { code: 502 }),
        ] {
            let mut c = Client::new("u", "p");
            c.feed(b"200 hi\r\n381 more\r\n");
            c.feed(reply);
            let evs: Vec<_> = std::iter::from_fn(|| c.poll_event()).collect();
            assert_eq!(evs[1], ev);
            assert!(matches!(evs[2], Event::Closed { .. }));
            assert!(c.is_closed());
        }
    }

    #[test]
    fn busy_greeting() {
        let mut c = Client::new("u", "p");
        c.feed(b"400 service unavailable\r\n");
        assert_eq!(c.poll_event(), Some(Event::Busy { code: 400 }));
    }

    #[test]
    fn no_auth_when_no_user() {
        let mut c = Client::new("", "");
        c.request_body(7, "a@b");
        c.feed(b"200 hi\r\n");
        assert_eq!(c.take_output().unwrap(), b"BODY <a@b>\r\n");
    }

    #[test]
    fn pipelines_up_to_limit() {
        let mut c = authed();
        for i in 0..20 {
            c.request_body(i, &format!("m{i}@t"));
        }
        let out = c.take_output().unwrap();
        assert_eq!(
            out.split(|&b| b == b'\n').filter(|l| !l.is_empty()).count(),
            8
        );
        c.feed(b"430 no\r\n430 no\r\n");
        let out = c.take_output().unwrap();
        assert_eq!(out, b"BODY <m8@t>\r\nBODY <m9@t>\r\n");
        assert_eq!(c.poll_event(), Some(Event::Missing { seg_id: 0 }));
        assert_eq!(c.poll_event(), Some(Event::Missing { seg_id: 1 }));
    }

    /// Server bytes for a pipelined exchange: two articles, a missing one, a
    /// STAT, DATE and CAPABILITIES. Includes lines that need dot-unstuffing.
    fn script() -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        let a: Vec<u8> = (0..5000u32).map(|i| (i * 7 % 256) as u8).collect();
        // Bytes that encode to '.' at line starts ('.' - 42 = 4).
        let b: Vec<u8> = std::iter::repeat_n(4u8, 3000).collect();
        let mut s = Vec::new();
        s.extend(article("a.bin", &a));
        s.extend(b"430 No such article\r\n");
        s.extend(article("b.bin", &b));
        s.extend(b"223 0 <s@t>\r\n");
        s.extend(b"111 20260105123456\r\n");
        s.extend(b"101 Capability list:\r\nVERSION 2\r\nREADER\r\n.\r\n");
        (s, a, b)
    }

    fn run_split(chunks: &[&[u8]], a: &[u8], b: &[u8]) {
        let mut c = authed();
        c.request_body(1, "a@t");
        c.request_body(2, "m@t");
        c.request_body(3, "b@t");
        c.request_stat(4, "s@t");
        c.request_date();
        c.request_capabilities();
        for ch in chunks {
            c.feed(ch);
        }
        let evs: Vec<_> = std::iter::from_fn(|| c.poll_event()).collect();
        assert_eq!(evs.len(), 6, "{evs:?}");
        match &evs[0] {
            Event::Segment(s) => {
                assert_eq!(s.seg_id, 1);
                assert!(s.crc_ok);
                assert_eq!(s.data, a);
                assert_eq!(s.name, "a.bin");
            }
            e => panic!("{e:?}"),
        }
        assert_eq!(evs[1], Event::Missing { seg_id: 2 });
        match &evs[2] {
            Event::Segment(s) => {
                assert_eq!(s.seg_id, 3);
                assert!(s.crc_ok);
                assert_eq!(s.data, b);
            }
            e => panic!("{e:?}"),
        }
        assert_eq!(
            evs[3],
            Event::Stat {
                seg_id: 4,
                exists: true
            }
        );
        assert_eq!(
            evs[4],
            Event::Date {
                value: "20260105123456".into()
            }
        );
        assert_eq!(
            evs[5],
            Event::Capabilities {
                lines: vec!["VERSION 2".into(), "READER".into()]
            }
        );
    }

    #[test]
    fn pipelined_responses_in_one_chunk() {
        let (s, a, b) = script();
        run_split(&[&s], &a, &b);
    }

    #[test]
    fn pipelined_responses_split_at_every_byte_boundary() {
        let (s, a, b) = script();
        for i in 0..=s.len() {
            run_split(&[&s[..i], &s[i..]], &a, &b);
        }
    }

    #[test]
    fn pipelined_responses_byte_by_byte() {
        let (s, a, b) = script();
        let chunks: Vec<&[u8]> = s.chunks(1).collect();
        run_split(&chunks, &a, &b);
    }

    #[test]
    fn transport_close_reports_once() {
        let mut c = authed();
        c.request_body(1, "a@t");
        c.transport_closed("gone");
        c.transport_closed("again");
        assert_eq!(
            c.poll_event(),
            Some(Event::Closed {
                reason: "gone".into()
            })
        );
        assert_eq!(c.poll_event(), None);
    }

    #[test]
    fn crc_mismatch_is_reported() {
        let mut c = authed();
        c.request_body(1, "a@t");
        let mut art = article("a.bin", &[1, 2, 3, 4, 5, 6]);
        // Corrupt the first encoded data byte (after the =ybegin line).
        let pos = art.windows(8).position(|w| w == b"=ybegin ").unwrap();
        let nl = pos + art[pos..].iter().position(|&b| b == b'\n').unwrap();
        art[nl + 1] = art[nl + 1].wrapping_add(1);
        c.feed(&art);
        match c.poll_event() {
            Some(Event::Segment(s)) => assert!(!s.crc_ok),
            e => panic!("{e:?}"),
        }
    }
}
