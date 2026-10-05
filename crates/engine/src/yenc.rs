//! yEnc decoding (and an encoder for tests and the mock server).

use memchr::memmem;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decoded {
    pub name: String,
    /// Total file size from `=ybegin size=`.
    pub size: u64,
    pub part: u32,
    pub total: u32,
    /// 0-based offset of this part in the file.
    pub begin: u64,
    pub data: Vec<u8>,
    /// `pcrc32` (or `crc32` for single-part posts) matched. False if the
    /// trailer has no CRC or the decoded size disagrees with the header.
    pub crc_ok: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    NoHeader,
    BadHeader,
}

/// Finds `key=` in a header line and returns the value up to the next space.
fn param<'a>(line: &'a [u8], key: &str) -> Option<&'a [u8]> {
    let pat = format!(" {key}=");
    let at = memmem::find(line, pat.as_bytes())? + pat.len();
    let rest = &line[at..];
    let end = rest.iter().position(|&b| b == b' ').unwrap_or(rest.len());
    Some(&rest[..end])
}

fn num<T: std::str::FromStr>(v: Option<&[u8]>) -> Option<T> {
    std::str::from_utf8(v?).ok()?.trim().parse().ok()
}

fn hex32(v: Option<&[u8]>) -> Option<u32> {
    let s = std::str::from_utf8(v?).ok()?.trim();
    // Some posters pad or truncate; take the last 8 hex digits.
    let s = if s.len() > 8 { &s[s.len() - 8..] } else { s };
    u32::from_str_radix(s, 16).ok()
}

fn line_at(buf: &[u8], start: usize) -> (&[u8], usize) {
    let end = memchr::memchr(b'\n', &buf[start..]).map_or(buf.len(), |i| start + i + 1);
    let mut line = &buf[start..end];
    while let Some((&last, rest)) = line.split_last() {
        if last == b'\n' || last == b'\r' {
            line = rest;
        } else {
            break;
        }
    }
    (line, end)
}

/// Decodes one yEnc article body (already dot-unstuffed).
pub fn decode(body: &[u8]) -> Result<Decoded, Error> {
    let start = if body.starts_with(b"=ybegin ") {
        0
    } else {
        memmem::find(body, b"\n=ybegin ").ok_or(Error::NoHeader)? + 1
    };
    let (header, mut pos) = line_at(body, start);
    // `name=` is always last and may contain spaces.
    let name = memmem::find(header, b" name=")
        .map(|i| String::from_utf8_lossy(&header[i + 6..]).trim().to_string())
        .unwrap_or_default();
    let size: u64 = num(param(header, "size")).ok_or(Error::BadHeader)?;
    let part: u32 = num(param(header, "part")).unwrap_or(0);
    let total: u32 = num(param(header, "total")).unwrap_or(if part == 0 { 1 } else { 0 });

    let mut begin = 0u64;
    let mut end_off = size;
    if body[pos..].starts_with(b"=ypart ") {
        let (ypart, next) = line_at(body, pos);
        let b: u64 = num(param(ypart, "begin")).ok_or(Error::BadHeader)?;
        let e: u64 = num(param(ypart, "end")).ok_or(Error::BadHeader)?;
        if b == 0 || e < b {
            return Err(Error::BadHeader);
        }
        begin = b - 1;
        end_off = e;
        pos = next;
    }

    // Data runs until the =yend line.
    let (data_end, trailer) = match memmem::find(&body[pos..], b"\n=yend") {
        Some(i) => {
            let t = pos + i + 1;
            (t, Some(line_at(body, t).0))
        }
        None if body[pos..].starts_with(b"=yend") => (pos, Some(line_at(body, pos).0)),
        None => (body.len(), None),
    };
    let expected = end_off.saturating_sub(begin) as usize;
    let mut data = Vec::with_capacity(expected.min(16 << 20));
    decode_into(&body[pos..data_end.max(pos)], &mut data);

    let crc_ok = match trailer {
        Some(t) => {
            let want = if part > 0 {
                hex32(param(t, "pcrc32"))
            } else {
                hex32(param(t, "crc32")).or_else(|| hex32(param(t, "pcrc32")))
            };
            let size_ok = num::<u64>(param(t, "size")).is_none_or(|s| s == data.len() as u64);
            match want {
                Some(w) => size_ok && crc32fast::hash(&data) == w,
                None => size_ok && data.len() == expected,
            }
        }
        None => false,
    };
    Ok(Decoded {
        name,
        size,
        part,
        total,
        begin,
        data,
        crc_ok,
    })
}

const fn has_zero_byte(v: u64) -> bool {
    (v.wrapping_sub(0x0101_0101_0101_0101) & !v & 0x8080_8080_8080_8080) != 0
}

#[inline(always)]
const fn has_byte(v: u64, b: u8) -> bool {
    has_zero_byte(v ^ (0x0101_0101_0101_0101u64 * b as u64))
}

/// Subtracts 42 from each byte (mod 256) without carries between lanes.
#[inline(always)]
const fn sub42(v: u64) -> u64 {
    const H: u64 = 0x8080_8080_8080_8080;
    const K: u64 = 0x2a2a_2a2a_2a2a_2a2a;
    ((v | H).wrapping_sub(K & !H)) ^ ((v ^ !K) & H)
}

/// Decodes yEnc data lines, skipping CR and LF. An escape split across a
/// line break is still honoured.
pub fn decode_into(src: &[u8], out: &mut Vec<u8>) {
    out.reserve(src.len());
    let mut i = 0;
    let n = src.len();
    let mut escape = false;
    while i < n {
        if !escape {
            #[cfg(all(target_arch = "wasm32", target_feature = "simd128", feature = "simd"))]
            {
                i = simd::fast_run(src, i, out);
            }
            // Fast path: 8 bytes with no '=', CR or LF.
            while i + 8 <= n {
                let v = u64::from_le_bytes(src[i..i + 8].try_into().unwrap());
                if has_byte(v, b'=') || has_byte(v, b'\r') || has_byte(v, b'\n') {
                    break;
                }
                out.extend_from_slice(&sub42(v).to_le_bytes());
                i += 8;
            }
            if i >= n {
                break;
            }
        }
        let b = src[i];
        i += 1;
        match b {
            b'\r' | b'\n' => {}
            b'=' if !escape => escape = true,
            _ => {
                let off = if escape { 106u8 } else { 42u8 };
                out.push(b.wrapping_sub(off));
                escape = false;
            }
        }
    }
}

#[cfg(all(target_arch = "wasm32", target_feature = "simd128", feature = "simd"))]
mod simd {
    use core::arch::wasm32::*;

    /// Decodes 16-byte runs that contain no special characters.
    #[inline(always)]
    pub fn fast_run(src: &[u8], mut i: usize, out: &mut Vec<u8>) -> usize {
        let eq = u8x16_splat(b'=');
        let cr = u8x16_splat(b'\r');
        let lf = u8x16_splat(b'\n');
        let k = u8x16_splat(42);
        let mut buf = [0u8; 16];
        while i + 16 <= src.len() {
            // SAFETY: bounds checked above; v128_load allows unaligned reads.
            let v = unsafe { v128_load(src.as_ptr().add(i) as *const v128) };
            let special = v128_or(v128_or(u8x16_eq(v, eq), u8x16_eq(v, cr)), u8x16_eq(v, lf));
            if v128_any_true(special) {
                break;
            }
            let d = u8x16_sub(v, k);
            // SAFETY: buf is 16 bytes.
            unsafe { v128_store(buf.as_mut_ptr() as *mut v128, d) };
            out.extend_from_slice(&buf);
            i += 16;
        }
        i
    }
}

/// Encodes one part as a complete yEnc block (=ybegin, =ypart, data, =yend).
/// `begin` is 0-based. A single-part post (`total == 1`) omits `=ypart`.
pub fn encode_part(
    name: &str,
    file_size: u64,
    part: u32,
    total: u32,
    begin: u64,
    data: &[u8],
    line_len: usize,
) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() * 103 / 100 + 256);
    let crc = crc32fast::hash(data);
    if total > 1 {
        out.extend_from_slice(
            format!("=ybegin part={part} total={total} line={line_len} size={file_size} name={name}\r\n")
                .as_bytes(),
        );
        out.extend_from_slice(
            format!(
                "=ypart begin={} end={}\r\n",
                begin + 1,
                begin + data.len() as u64
            )
            .as_bytes(),
        );
    } else {
        out.extend_from_slice(
            format!("=ybegin line={line_len} size={file_size} name={name}\r\n").as_bytes(),
        );
    }
    let mut col = 0;
    for (i, &raw) in data.iter().enumerate() {
        let b = raw.wrapping_add(42);
        let last = i + 1 == data.len() || col + 1 >= line_len;
        let escape = matches!(b, 0 | b'\n' | b'\r' | b'=')
            || ((b == b'\t' || b == b' ') && (col == 0 || last))
            || (b == b'.' && col == 0);
        if escape {
            out.push(b'=');
            out.push(b.wrapping_add(64));
            col += 2;
        } else {
            out.push(b);
            col += 1;
        }
        if col >= line_len {
            out.extend_from_slice(b"\r\n");
            col = 0;
        }
    }
    if col > 0 {
        out.extend_from_slice(b"\r\n");
    }
    if total > 1 {
        out.extend_from_slice(
            format!("=yend size={} part={part} pcrc32={crc:08x}\r\n", data.len()).as_bytes(),
        );
    } else {
        out.extend_from_slice(format!("=yend size={} crc32={crc:08x}\r\n", data.len()).as_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{Rng, SeedableRng};

    fn random(len: usize, seed: u64) -> Vec<u8> {
        let mut r = rand::rngs::SmallRng::seed_from_u64(seed);
        (0..len).map(|_| r.random()).collect()
    }

    #[test]
    fn swar_helpers() {
        for b in 0..=255u8 {
            let v = u64::from_le_bytes([b; 8]);
            assert_eq!(sub42(v).to_le_bytes(), [b.wrapping_sub(42); 8]);
            assert_eq!(has_byte(v, b'='), b == b'=');
        }
        let v = u64::from_le_bytes([1, 2, 3, b'\n', 5, 6, 7, 8]);
        assert!(has_byte(v, b'\n'));
        assert!(!has_byte(v, b'\r'));
    }

    #[test]
    fn round_trip_all_bytes_and_sizes() {
        for len in [1usize, 2, 7, 8, 9, 127, 128, 129, 1000, 65536 + 3] {
            for seed in 0..3 {
                let data = random(len, seed + len as u64);
                for line in [16, 128, 255] {
                    let enc = encode_part("ubuntu-24.04.iso", 1 << 30, 3, 9, 4096, &data, line);
                    let d = decode(&enc).unwrap();
                    assert_eq!(d.data, data, "len={len} line={line}");
                    assert!(d.crc_ok);
                    assert_eq!(d.begin, 4096);
                    assert_eq!(d.part, 3);
                    assert_eq!(d.total, 9);
                    assert_eq!(d.size, 1 << 30);
                    assert_eq!(d.name, "ubuntu-24.04.iso");
                }
            }
        }
    }

    #[test]
    fn every_byte_value_round_trips() {
        let data: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
        let enc = encode_part("a b c.bin", 4096, 1, 1, 0, &data, 128);
        let d = decode(&enc).unwrap();
        assert_eq!(d.data, data);
        assert!(d.crc_ok);
        assert_eq!(d.name, "a b c.bin");
        assert_eq!(d.begin, 0);
    }

    #[test]
    fn crc_failure_detected() {
        let data = random(5000, 9);
        let mut enc = encode_part("x.bin", 10000, 1, 2, 0, &data, 128);
        let at = memmem::find(&enc, b"=ypart").unwrap() + 40;
        enc[at] ^= 0x01;
        let d = decode(&enc).unwrap();
        assert!(!d.crc_ok);
    }

    #[test]
    fn truncated_article_fails_crc() {
        let data = random(5000, 10);
        let enc = encode_part("x.bin", 10000, 1, 2, 0, &data, 128);
        let cut = memmem::find(&enc, b"=yend").unwrap() - 200;
        let d = decode(&enc[..cut]).unwrap();
        assert!(!d.crc_ok);
    }

    #[test]
    fn wrong_size_in_trailer_fails() {
        let data = random(100, 11);
        let mut enc = encode_part("x.bin", 100, 1, 1, 0, &data, 128);
        let at = memmem::find(&enc, b"=yend size=100").unwrap();
        enc.splice(at..at + 14, b"=yend size=099".iter().copied());
        assert!(!decode(&enc).unwrap().crc_ok);
    }

    #[test]
    fn missing_header_is_an_error() {
        assert_eq!(decode(b"hello\r\n").unwrap_err(), Error::NoHeader);
        assert_eq!(decode(b"=ybegin name=x\r\n").unwrap_err(), Error::BadHeader);
    }

    #[test]
    fn tolerates_leading_text_and_lf_only() {
        let data = random(300, 12);
        let enc = encode_part("x.bin", 300, 1, 1, 0, &data, 64);
        let mut raw = b"some preamble\n".to_vec();
        raw.extend(enc.iter().filter(|&&b| b != b'\r'));
        let d = decode(&raw).unwrap();
        assert_eq!(d.data, data);
        assert!(d.crc_ok);
    }

    #[test]
    fn escape_split_across_line_break() {
        // "=" at the end of a line followed by the escaped char on the next.
        let mut out = Vec::new();
        decode_into(b"=\r\n}", &mut out);
        assert_eq!(out, vec![b'='.wrapping_sub(42)]);
    }
}
