//! NZB parsing, filename extraction and file classification.

use quick_xml::events::{BytesStart, Event};
use quick_xml::Reader;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Nzb {
    pub title: Option<String>,
    pub name: Option<String>,
    pub password: Option<String>,
    pub files: Vec<NzbFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NzbFile {
    pub subject: String,
    pub poster: String,
    pub date: u64,
    pub groups: Vec<String>,
    pub segments: Vec<NzbSegment>,
    /// Best guess from the subject; yEnc `name=` replaces it once known.
    pub filename: String,
    pub kind: FileKind,
    /// Sum of segment sizes (encoded bytes, so slightly above the file size).
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct NzbSegment {
    pub number: u32,
    pub bytes: u64,
    pub message_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FileKind {
    /// The small `.par2` with no recovery blocks.
    Par2Index,
    /// `.volNN+MM.par2`, with the block count when the name says it.
    Par2Recovery { blocks: Option<u32> },
    /// RAR, 7z, zip and split volumes.
    Archive,
    #[default]
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Xml(String),
    NoFiles,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Xml(e) => write!(f, "not a valid NZB file: {e}"),
            Error::NoFiles => f.write_str("the NZB file lists no files"),
        }
    }
}

fn attr(e: &BytesStart, name: &str) -> Option<String> {
    e.attributes()
        .flatten()
        .find(|a| a.key.local_name().as_ref() == name)
        .map(|a| {
            let raw: &str = a.value.as_ref();
            unescape(raw)
        })
}

/// Unescapes XML entities, including numeric ones. Unknown entities are kept.
fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest.find(';').filter(|&e| e <= 12) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let ent = &rest[1..end];
        let ch = match ent {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ if ent.starts_with("#x") || ent.starts_with("#X") => {
                u32::from_str_radix(&ent[2..], 16)
                    .ok()
                    .and_then(char::from_u32)
            }
            _ if ent.starts_with('#') => ent[1..].parse().ok().and_then(char::from_u32),
            _ => None,
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[derive(PartialEq)]
enum Text {
    None,
    Meta(String),
    Group,
    Segment,
}

pub fn parse(xml: &[u8]) -> Result<Nzb, Error> {
    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();
    let mut nzb = Nzb::default();
    let mut file: Option<NzbFile> = None;
    let mut seg = NzbSegment::default();
    let mut text = Text::None;
    let mut acc = String::new();
    let mut saw_root = false;

    loop {
        match reader.read_event_into(&mut buf) {
            Err(e) => return Err(Error::Xml(e.to_string())),
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                acc.clear();
                match e.local_name().as_ref() {
                    "nzb" => saw_root = true,
                    "meta" => {
                        text = Text::Meta(attr(&e, "type").unwrap_or_default().to_ascii_lowercase())
                    }
                    "file" => {
                        file = Some(NzbFile {
                            subject: attr(&e, "subject").unwrap_or_default(),
                            poster: attr(&e, "poster").unwrap_or_default(),
                            date: attr(&e, "date")
                                .and_then(|d| d.trim().parse().ok())
                                .unwrap_or(0),
                            ..NzbFile::default()
                        })
                    }
                    "group" => text = Text::Group,
                    "segment" => {
                        text = Text::Segment;
                        seg = NzbSegment {
                            number: attr(&e, "number")
                                .and_then(|n| n.trim().parse().ok())
                                .unwrap_or(0),
                            bytes: attr(&e, "bytes")
                                .and_then(|n| n.trim().parse().ok())
                                .unwrap_or(0),
                            message_id: String::new(),
                        };
                    }
                    _ => {}
                }
            }
            Ok(Event::Text(t)) => {
                if text != Text::None {
                    acc.push_str(&unescape(t.as_ref()));
                }
            }
            Ok(Event::GeneralRef(r)) => {
                if text != Text::None {
                    let ent = format!("&{};", r.as_ref() as &str);
                    acc.push_str(&unescape(&ent));
                }
            }
            Ok(Event::CData(t)) => {
                if text != Text::None {
                    acc.push_str(t.as_ref());
                }
            }
            Ok(Event::End(e)) => {
                match (e.local_name().as_ref(), &text) {
                    ("meta", Text::Meta(kind)) => {
                        let v = acc.trim().to_string();
                        if !v.is_empty() {
                            match kind.as_str() {
                                "password" => nzb.password = Some(v),
                                "name" => nzb.name = Some(v),
                                "title" => nzb.title = Some(v),
                                _ => {}
                            }
                        }
                    }
                    ("group", Text::Group) => {
                        if let Some(f) = file.as_mut() {
                            let g = acc.trim();
                            if !g.is_empty() {
                                f.groups.push(g.to_string());
                            }
                        }
                    }
                    ("segment", Text::Segment) => {
                        if let Some(f) = file.as_mut() {
                            let id = acc.trim();
                            let id = id.strip_prefix('<').unwrap_or(id);
                            let id = id.strip_suffix('>').unwrap_or(id);
                            if !id.is_empty() {
                                seg.message_id = id.to_string();
                                f.segments.push(std::mem::take(&mut seg));
                            }
                        }
                    }
                    ("file", _) => {
                        if let Some(mut f) = file.take() {
                            finish_file(&mut f);
                            if !f.segments.is_empty() {
                                nzb.files.push(f);
                            }
                        }
                    }
                    _ => {}
                }
                if e.local_name().as_ref() != "file" {
                    text = Text::None;
                }
                acc.clear();
            }
            Ok(_) => {}
        }
        buf.clear();
    }
    if !saw_root {
        return Err(Error::Xml("missing <nzb> element".into()));
    }
    if nzb.files.is_empty() {
        return Err(Error::NoFiles);
    }
    Ok(nzb)
}

fn finish_file(f: &mut NzbFile) {
    f.segments.sort_by_key(|s| s.number);
    f.segments.dedup_by_key(|s| s.number);
    f.bytes = f.segments.iter().map(|s| s.bytes).sum();
    f.filename = filename_from_subject(&f.subject);
    f.kind = classify(&f.filename);
}

/// Picks the filename from a subject: the first quoted string if there is
/// one, otherwise the token that looks most like a filename.
pub fn filename_from_subject(subject: &str) -> String {
    if let Some(start) = subject.find('"') {
        if let Some(len) = subject[start + 1..].find('"') {
            let name = sanitize(&subject[start + 1..start + 1 + len]);
            if !name.is_empty() {
                return name;
            }
        }
    }
    // Drop a trailing "yEnc (1/23)" and part counters.
    let lower = subject.to_ascii_lowercase();
    let cut = lower.rfind(" yenc").unwrap_or(subject.len());
    let head = &subject[..cut];
    let candidate = head
        .split_whitespace()
        .rev()
        .find(|t| looks_like_filename(t))
        .map(sanitize);
    match candidate {
        Some(c) if !c.is_empty() => c,
        _ => {
            let s = sanitize(head.trim());
            if s.is_empty() {
                "file".to_string()
            } else {
                s
            }
        }
    }
}

fn looks_like_filename(t: &str) -> bool {
    let t = t.trim_matches(|c: char| matches!(c, '[' | ']' | '(' | ')' | '-' | ','));
    match t.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && (1..=5).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
                && ext.chars().any(|c| c.is_ascii_alphabetic())
                || (ext.len() == 3 && ext.chars().all(|c| c.is_ascii_digit()) && stem.contains('.'))
        }
        None => false,
    }
}

/// Makes a name safe for any file system: no path separators, control or
/// reserved characters, no leading dots, at most 200 bytes.
pub fn sanitize(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let mut out: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect();
    out = out
        .trim()
        .trim_start_matches('.')
        .trim_end_matches(['.', ' '])
        .to_string();
    if out.len() > 200 {
        let mut cut = 200;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
    }
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if RESERVED.contains(&stem.as_str()) {
        out.insert(0, '_');
    }
    out
}

/// Classifies a file by name.
pub fn classify(name: &str) -> FileKind {
    let n = name.to_ascii_lowercase();
    if let Some(stem) = n.strip_suffix(".par2") {
        if let Some(i) = stem.rfind(".vol") {
            let spec = &stem[i + 4..];
            if let Some((a, b)) = spec.split_once(['+', '-']) {
                if a.chars().all(|c| c.is_ascii_digit()) && !a.is_empty() {
                    let blocks = b.parse::<u32>().ok();
                    return FileKind::Par2Recovery { blocks };
                }
            }
        }
        return FileKind::Par2Index;
    }
    let ext = n.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
    let numeric = |e: &str, len: std::ops::RangeInclusive<usize>| {
        len.contains(&e.len()) && e.chars().all(|c| c.is_ascii_digit())
    };
    let is_archive = matches!(ext, "rar" | "7z" | "zip")
        || (ext.len() >= 3 && ext.starts_with('r') && numeric(&ext[1..], 2..=3))
        || (ext.len() >= 3 && ext.starts_with('z') && numeric(&ext[1..], 2..=2))
        || (numeric(ext, 3..=3) && {
            let stem = &n[..n.len() - 4];
            stem.ends_with(".7z")
                || stem.ends_with(".zip")
                || stem.ends_with(".rar")
                || stem.contains('.')
        });
    if is_archive {
        FileKind::Archive
    } else {
        FileKind::Other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE nzb PUBLIC "-//newzBin//DTD NZB 1.1//EN" "http://www.newzbin.com/DTD/nzb/nzb-1.1.dtd">
<nzb xmlns="http://www.newzbin.com/DTD/2003/nzb">
  <head>
    <meta type="title">Ubuntu 24.04</meta>
    <meta type="password">s3cr&amp;t</meta>
    <meta type="name">ubuntu-24.04</meta>
  </head>
  <file poster="poster@example.com" date="1712345678" subject="[1/3] - &quot;ubuntu-24.04.iso&quot; yEnc (1/2)">
    <groups><group>alt.binaries.test</group><group>alt.binaries.misc</group></groups>
    <segments>
      <segment bytes="739811" number="2">part2of2.abc@example.com</segment>
      <segment bytes="739900" number="1">&lt;part1of2.abc@example.com&gt;</segment>
      <segment bytes="739900" number="1">dup@example.com</segment>
    </segments>
  </file>
  <file poster="p" date="1" subject="[2/3] - &quot;ubuntu-24.04.par2&quot; yEnc (1/1)">
    <groups><group>alt.binaries.test</group></groups>
    <segments><segment bytes="1000" number="1">p1@example.com</segment></segments>
  </file>
  <file poster="p" date="1" subject="[3/3] - &quot;ubuntu-24.04.vol00+12.par2&quot; yEnc (1/1)">
    <groups><group>alt.binaries.test</group></groups>
    <segments><segment bytes="1000" number="1">v1@example.com</segment></segments>
  </file>
  <file poster="p" date="1" subject="empty file">
    <groups/><segments/>
  </file>
</nzb>"#;

    #[test]
    fn parses_sample() {
        let nzb = parse(SAMPLE.as_bytes()).unwrap();
        assert_eq!(nzb.title.as_deref(), Some("Ubuntu 24.04"));
        assert_eq!(nzb.password.as_deref(), Some("s3cr&t"));
        assert_eq!(nzb.name.as_deref(), Some("ubuntu-24.04"));
        assert_eq!(nzb.files.len(), 3);
        let f = &nzb.files[0];
        assert_eq!(f.filename, "ubuntu-24.04.iso");
        assert_eq!(f.poster, "poster@example.com");
        assert_eq!(f.date, 1712345678);
        assert_eq!(f.groups, vec!["alt.binaries.test", "alt.binaries.misc"]);
        assert_eq!(f.segments.len(), 2);
        assert_eq!(f.segments[0].number, 1);
        assert_eq!(f.segments[0].message_id, "part1of2.abc@example.com");
        assert_eq!(f.segments[1].message_id, "part2of2.abc@example.com");
        assert_eq!(f.bytes, 739900 + 739811);
        assert_eq!(f.kind, FileKind::Other);
        assert_eq!(nzb.files[1].kind, FileKind::Par2Index);
        assert_eq!(
            nzb.files[2].kind,
            FileKind::Par2Recovery { blocks: Some(12) }
        );
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse(b"not xml at all <<<").is_err());
        assert!(parse(b"<html></html>").is_err());
        assert_eq!(
            parse(b"<nzb xmlns=\"http://www.newzbin.com/DTD/2003/nzb\"></nzb>").unwrap_err(),
            Error::NoFiles
        );
    }

    #[test]
    fn subjects() {
        assert_eq!(filename_from_subject("\"a.rar\" yEnc (1/5)"), "a.rar");
        assert_eq!(
            filename_from_subject("Linux ISO [01/10] - ubuntu-24.04.iso yEnc (1/100)"),
            "ubuntu-24.04.iso"
        );
        assert_eq!(
            filename_from_subject("[1/2] data.part01.rar (1/50)"),
            "data.part01.rar"
        );
        assert_eq!(filename_from_subject("\"../../etc/passwd\" yEnc"), "passwd");
        assert_eq!(
            filename_from_subject("no filename here"),
            "no filename here"
        );
        assert_eq!(filename_from_subject(""), "file");
        assert_eq!(
            filename_from_subject("backup.7z.001 yEnc (1/9)"),
            "backup.7z.001"
        );
    }

    #[test]
    fn sanitizes() {
        assert_eq!(sanitize("a/b\\c.txt"), "c.txt");
        assert_eq!(sanitize("..hidden"), "hidden");
        assert_eq!(sanitize("x:y*z?.bin"), "x_y_z_.bin");
        assert_eq!(sanitize("con.txt"), "_con.txt");
        assert_eq!(sanitize("trailing. "), "trailing");
        assert!(sanitize(&"é".repeat(300)).len() <= 200);
    }

    #[test]
    fn classifies() {
        use FileKind::*;
        assert_eq!(classify("x.par2"), Par2Index);
        assert_eq!(classify("x.PAR2"), Par2Index);
        assert_eq!(
            classify("x.vol000+01.par2"),
            Par2Recovery { blocks: Some(1) }
        );
        assert_eq!(
            classify("x.vol07+08.par2"),
            Par2Recovery { blocks: Some(8) }
        );
        assert_eq!(
            classify("x.vol07-08.par2"),
            Par2Recovery { blocks: Some(8) }
        );
        for a in [
            "x.rar",
            "x.part01.rar",
            "x.r00",
            "x.r123",
            "x.7z",
            "x.7z.001",
            "x.zip",
            "x.z01",
            "x.zip.002",
        ] {
            assert_eq!(classify(a), Archive, "{a}");
        }
        for o in ["ubuntu-24.04.iso", "readme.txt", "x.nfo", "x.mp3", "x.rtf"] {
            assert_eq!(classify(o), Other, "{o}");
        }
    }
}
