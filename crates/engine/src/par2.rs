//! PAR2 2.0: packet parsing, verification and Reed-Solomon repair.
//!
//! Written from the public "Parity Volume Set Specification 2.0". All entry
//! points are streaming so files never have to sit in memory whole.

use md5::{Digest, Md5};

use crate::gf16;

pub const MAGIC: &[u8; 8] = b"PAR2\0PKT";
pub const HEADER_LEN: usize = 64;
pub const TYPE_MAIN: &[u8; 16] = b"PAR 2.0\0Main\0\0\0\0";
pub const TYPE_FILE_DESC: &[u8; 16] = b"PAR 2.0\0FileDesc";
pub const TYPE_IFSC: &[u8; 16] = b"PAR 2.0\0IFSC\0\0\0\0";
pub const TYPE_RECOVERY: &[u8; 16] = b"PAR 2.0\0RecvSlic";

pub type Hash = [u8; 16];

pub fn md5(data: &[u8]) -> Hash {
    Md5::digest(data).into()
}

pub fn hex(h: &[u8]) -> String {
    h.iter().map(|b| format!("{b:02x}")).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    pub len: u64,
    pub hash: Hash,
    pub set_id: Hash,
    pub kind: Hash,
}

/// Parses a 64-byte packet header. Doesn't check the packet hash.
pub fn parse_header(b: &[u8]) -> Option<Header> {
    if b.len() < HEADER_LEN || &b[..8] != MAGIC {
        return None;
    }
    let len = u64::from_le_bytes(b[8..16].try_into().ok()?);
    if len < HEADER_LEN as u64 || len % 4 != 0 {
        return None;
    }
    Some(Header {
        len,
        hash: b[16..32].try_into().ok()?,
        set_id: b[32..48].try_into().ok()?,
        kind: b[48..64].try_into().ok()?,
    })
}

/// A complete packet whose MD5 checks out.
#[derive(Debug, Clone, Copy)]
pub struct Packet<'a> {
    pub header: Header,
    pub body: &'a [u8],
}

/// Checks a packet starting at `buf[0]`; `None` if truncated or damaged.
pub fn packet_at(buf: &[u8]) -> Option<Packet<'_>> {
    let header = parse_header(buf)?;
    let len = usize::try_from(header.len).ok()?;
    if buf.len() < len {
        return None;
    }
    if md5(&buf[32..len]) != header.hash {
        return None;
    }
    Some(Packet {
        header,
        body: &buf[HEADER_LEN..len],
    })
}

/// All valid packets in a buffer, resynchronising on the magic after damage.
pub fn packets(buf: &[u8]) -> Vec<Packet<'_>> {
    let finder = memchr::memmem::Finder::new(MAGIC);
    let mut out = Vec::new();
    let mut pos = 0;
    while let Some(i) = finder.find(&buf[pos..]) {
        let at = pos + i;
        match packet_at(&buf[at..]) {
            Some(p) => {
                pos = at + p.header.len as usize;
                out.push(p);
            }
            None => pos = at + 1,
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SliceCheck {
    pub md5: Hash,
    pub crc: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileInfo {
    pub id: Hash,
    pub md5: Hash,
    pub md5_16k: Hash,
    pub length: u64,
    pub name: String,
    /// From the IFSC packet; empty if it wasn't found.
    pub slices: Vec<SliceCheck>,
    /// Global index of this file's first input slice.
    pub first_slice: u32,
}

impl FileInfo {
    pub fn slice_count(&self, slice_size: u64) -> u32 {
        self.length.div_ceil(slice_size) as u32
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Set {
    pub set_id: Hash,
    pub slice_size: u64,
    /// Recovery set files in main-packet order, which defines slice numbering.
    pub files: Vec<FileInfo>,
    /// File ids from the main packet with no description packet.
    pub missing_descriptions: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    NoMainPacket,
    BadMainPacket,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::NoMainPacket => f.write_str("no valid PAR2 main packet found"),
            Error::BadMainPacket => f.write_str("the PAR2 main packet is malformed"),
        }
    }
}

fn read_name(b: &[u8]) -> String {
    let end = b.iter().position(|&c| c == 0).unwrap_or(b.len());
    String::from_utf8_lossy(&b[..end]).into_owned()
}

impl Set {
    /// Builds the set from any PAR2 file's bytes (index or volume).
    pub fn parse(buf: &[u8]) -> Result<Set, Error> {
        let pkts = packets(buf);
        let main = pkts
            .iter()
            .find(|p| &p.header.kind == TYPE_MAIN)
            .ok_or(Error::NoMainPacket)?;
        let body = main.body;
        if body.len() < 12 {
            return Err(Error::BadMainPacket);
        }
        let slice_size = u64::from_le_bytes(body[0..8].try_into().unwrap());
        let count = u32::from_le_bytes(body[8..12].try_into().unwrap()) as usize;
        if slice_size == 0 || slice_size % 4 != 0 || body.len() < 12 + count * 16 {
            return Err(Error::BadMainPacket);
        }
        let set_id = main.header.set_id;
        let ids: Vec<Hash> = (0..count)
            .map(|i| body[12 + i * 16..28 + i * 16].try_into().unwrap())
            .collect();
        let mut files = Vec::with_capacity(count);
        let mut missing = 0;
        let mut first = 0u32;
        for id in ids {
            let desc = pkts.iter().find(|p| {
                p.header.set_id == set_id
                    && &p.header.kind == TYPE_FILE_DESC
                    && p.body.len() >= 56
                    && p.body[..16] == id
            });
            let Some(desc) = desc else {
                missing += 1;
                continue;
            };
            let b = desc.body;
            let length = u64::from_le_bytes(b[48..56].try_into().unwrap());
            let mut info = FileInfo {
                id,
                md5: b[16..32].try_into().unwrap(),
                md5_16k: b[32..48].try_into().unwrap(),
                length,
                name: read_name(&b[56..]),
                slices: Vec::new(),
                first_slice: first,
            };
            let n = info.slice_count(slice_size) as usize;
            if let Some(ifsc) = pkts.iter().find(|p| {
                p.header.set_id == set_id
                    && &p.header.kind == TYPE_IFSC
                    && p.body.len() >= 16
                    && p.body[..16] == id
            }) {
                let entries = &ifsc.body[16..];
                if entries.len() >= n * 20 {
                    info.slices = (0..n)
                        .map(|i| SliceCheck {
                            md5: entries[i * 20..i * 20 + 16].try_into().unwrap(),
                            crc: u32::from_le_bytes(
                                entries[i * 20 + 16..i * 20 + 20].try_into().unwrap(),
                            ),
                        })
                        .collect();
                }
            }
            first += n as u32;
            files.push(info);
        }
        Ok(Set {
            set_id,
            slice_size,
            files,
            missing_descriptions: missing,
        })
    }

    pub fn total_slices(&self) -> u32 {
        self.files
            .iter()
            .map(|f| f.slice_count(self.slice_size))
            .sum()
    }

    /// Finds the file whose first 16 KiB hash and length match.
    pub fn match_16k(&self, md5_16k: &Hash, length: u64) -> Option<&FileInfo> {
        self.files
            .iter()
            .find(|f| &f.md5_16k == md5_16k && f.length == length)
            .or_else(|| self.files.iter().find(|f| &f.md5_16k == md5_16k))
    }
}

/// A recovery slice packet: its exponent and where its data is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecoveryRef {
    pub exponent: u32,
    /// Offset of the recovery data within the packet.
    pub data_offset: usize,
    pub data_len: usize,
}

/// Checks a full recovery-slice packet (header included).
pub fn recovery_packet(buf: &[u8], set_id: &Hash) -> Option<RecoveryRef> {
    let p = packet_at(buf)?;
    if &p.header.kind != TYPE_RECOVERY || &p.header.set_id != set_id || p.body.len() < 4 {
        return None;
    }
    Some(RecoveryRef {
        exponent: u32::from_le_bytes(p.body[..4].try_into().unwrap()),
        data_offset: HEADER_LEN + 4,
        data_len: p.body.len() - 4,
    })
}

/// Streams one file and finds damaged slices. Feed the bytes in order with
/// [`FileVerifier::update`], then call [`FileVerifier::finish`].
pub struct FileVerifier {
    slice_size: u64,
    expected: FileInfo,
    deep: bool,
    file_md5: Md5,
    slice_md5: Md5,
    slice_crc: crc32fast::Hasher,
    in_slice: u64,
    slice: u32,
    pos: u64,
    damaged: Vec<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    /// Length and MD5 of the whole file match.
    pub ok: bool,
    /// Damaged slices, as indexes local to the file.
    pub damaged: Vec<u32>,
}

impl FileVerifier {
    /// With `deep`, each slice is checked by MD5 as well as CRC32.
    pub fn new(set: &Set, file: &FileInfo, deep: bool) -> FileVerifier {
        FileVerifier {
            slice_size: set.slice_size,
            expected: file.clone(),
            deep,
            file_md5: Md5::new(),
            slice_md5: Md5::new(),
            slice_crc: crc32fast::Hasher::new(),
            in_slice: 0,
            slice: 0,
            pos: 0,
            damaged: Vec::new(),
        }
    }

    pub fn update(&mut self, mut data: &[u8]) {
        let len = self.expected.length;
        while !data.is_empty() {
            // Bytes past the expected length don't belong to any slice.
            if self.pos >= len {
                self.file_md5.update(data);
                self.pos += data.len() as u64;
                return;
            }
            let room = (self.slice_size - self.in_slice).min(len - self.pos) as usize;
            let take = room.min(data.len());
            let (chunk, rest) = data.split_at(take);
            self.file_md5.update(chunk);
            self.slice_crc.update(chunk);
            if self.deep {
                self.slice_md5.update(chunk);
            }
            self.in_slice += take as u64;
            self.pos += take as u64;
            data = rest;
            if self.in_slice == self.slice_size || self.pos == len {
                self.end_slice();
            }
        }
    }

    fn end_slice(&mut self) {
        let pad = (self.slice_size - self.in_slice) as usize;
        if pad > 0 {
            let zeros = vec![0u8; pad.min(1 << 16)];
            let mut left = pad;
            while left > 0 {
                let n = left.min(zeros.len());
                self.slice_crc.update(&zeros[..n]);
                if self.deep {
                    self.slice_md5.update(&zeros[..n]);
                }
                left -= n;
            }
        }
        let crc = std::mem::replace(&mut self.slice_crc, crc32fast::Hasher::new()).finalize();
        let md5: Hash = std::mem::replace(&mut self.slice_md5, Md5::new())
            .finalize()
            .into();
        let good = match self.expected.slices.get(self.slice as usize) {
            Some(c) => c.crc == crc && (!self.deep || c.md5 == md5),
            None => false,
        };
        if !good {
            self.damaged.push(self.slice);
        }
        self.slice += 1;
        self.in_slice = 0;
    }

    pub fn finish(mut self) -> Verified {
        let total = self.expected.slice_count(self.slice_size);
        let length_ok = self.pos == self.expected.length;
        // A short file: the partial slice and everything after it is damaged.
        if self.in_slice > 0 {
            self.damaged.push(self.slice);
            self.slice += 1;
        }
        while self.slice < total {
            self.damaged.push(self.slice);
            self.slice += 1;
        }
        let md5: Hash = self.file_md5.finalize().into();
        let ok = length_ok && md5 == self.expected.md5;
        if ok {
            self.damaged.clear();
        } else if self.damaged.is_empty() && !self.expected.slices.is_empty() && length_ok {
            // MD5 mismatch with every CRC matching: rare, so call every
            // slice suspect and let a deep pass narrow it down.
            self.damaged = (0..total).collect();
        }
        Verified {
            ok,
            damaged: self.damaged,
        }
    }
}

/// Reed-Solomon repair over one stripe `[offset, offset + len)` of every
/// slice. Memory use is one stripe per missing slice, so large repairs can
/// run in several passes.
pub struct Repairer {
    consts: Vec<u16>,
    missing: Vec<u32>,
    exponents: Vec<u32>,
    stripe_len: usize,
    acc: Vec<Vec<u8>>,
    have: Vec<bool>,
    is_missing: Vec<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairError {
    BadParameters,
    NotEnoughRecovery,
    Singular,
}

impl std::fmt::Display for RepairError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RepairError::BadParameters => f.write_str("invalid repair parameters"),
            RepairError::NotEnoughRecovery => f.write_str("not enough recovery data"),
            RepairError::Singular => {
                f.write_str("these recovery blocks can't repair the damage; try others")
            }
        }
    }
}

impl Repairer {
    /// `missing` are global input slice indexes to rebuild; `exponents` are
    /// the recovery slices to use, one per missing slice.
    pub fn new(
        total_slices: u32,
        missing: &[u32],
        exponents: &[u32],
        stripe_len: usize,
    ) -> Result<Repairer, RepairError> {
        if missing.is_empty()
            || missing.len() != exponents.len()
            || stripe_len == 0
            || stripe_len % 2 != 0
            || missing.iter().any(|&m| m >= total_slices)
        {
            return Err(RepairError::BadParameters);
        }
        let mut is_missing = vec![false; total_slices as usize];
        for &m in missing {
            is_missing[m as usize] = true;
        }
        Ok(Repairer {
            consts: gf16::input_constants(total_slices as usize),
            missing: missing.to_vec(),
            exponents: exponents.to_vec(),
            stripe_len,
            acc: vec![vec![0u8; stripe_len]; missing.len()],
            have: vec![false; missing.len()],
            is_missing,
        })
    }

    /// Adds this stripe of a recovery slice. Shorter data counts as zeros.
    pub fn add_recovery(&mut self, exponent: u32, data: &[u8]) {
        if let Some(j) = self.exponents.iter().position(|&e| e == exponent) {
            let n = data.len().min(self.stripe_len);
            for (a, d) in self.acc[j][..n].iter_mut().zip(&data[..n]) {
                *a ^= *d;
            }
            self.have[j] = true;
        }
    }

    /// Adds this stripe of an intact input slice. Shorter data (the end of
    /// a file) counts as zero padding. Missing slices are ignored.
    pub fn add_input(&mut self, index: u32, data: &[u8]) {
        let i = index as usize;
        if i >= self.consts.len() || self.is_missing[i] {
            return;
        }
        let c = self.consts[i];
        let data = &data[..data.len().min(self.stripe_len)];
        for (j, &e) in self.exponents.iter().enumerate() {
            gf16::mul_add_region(&mut self.acc[j], data, gf16::pow(c, e));
        }
    }

    /// Solves for the missing slices (this stripe of each, in `missing` order).
    pub fn solve(self) -> Result<Vec<Vec<u8>>, RepairError> {
        if self.have.iter().any(|h| !h) {
            return Err(RepairError::NotEnoughRecovery);
        }
        let m = self.missing.len();
        let mut mat = vec![0u16; m * m];
        for (j, &e) in self.exponents.iter().enumerate() {
            for (k, &idx) in self.missing.iter().enumerate() {
                mat[j * m + k] = gf16::pow(self.consts[idx as usize], e);
            }
        }
        if !gf16::invert(&mut mat, m) {
            return Err(RepairError::Singular);
        }
        let mut out = vec![vec![0u8; self.stripe_len]; m];
        for (k, o) in out.iter_mut().enumerate() {
            for j in 0..m {
                gf16::mul_add_region(o, &self.acc[j], mat[k * m + j]);
            }
        }
        Ok(out)
    }
}

/// Computes one recovery slice from all input slices (zero-padded). Used by
/// tests and the mock server; real recovery files come from the poster.
pub fn compute_recovery(inputs: &[&[u8]], slice_size: usize, exponent: u32) -> Vec<u8> {
    let consts = gf16::input_constants(inputs.len());
    let mut out = vec![0u8; slice_size];
    for (i, d) in inputs.iter().enumerate() {
        gf16::mul_add_region(
            &mut out,
            &d[..d.len().min(slice_size)],
            gf16::pow(consts[i], exponent),
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slices(data: &[u8], size: usize) -> Vec<Vec<u8>> {
        data.chunks(size)
            .map(|c| {
                let mut v = c.to_vec();
                v.resize(size, 0);
                v
            })
            .collect()
    }

    #[test]
    fn repair_round_trip_synthetic() {
        let size = 64;
        let data: Vec<u8> = (0..64 * 10 - 6).map(|i| (i * 13 % 256) as u8).collect();
        let s = slices(&data, size);
        let refs: Vec<&[u8]> = s.iter().map(Vec::as_slice).collect();
        let exps = [0u32, 1, 2, 5];
        let rec: Vec<Vec<u8>> = exps
            .iter()
            .map(|&e| compute_recovery(&refs, size, e))
            .collect();
        let missing = [1u32, 4, 9];
        let use_exps = [1u32, 2, 5];
        for stripe in [size, 16] {
            let mut rebuilt = vec![Vec::new(); missing.len()];
            for off in (0..size).step_by(stripe) {
                let mut r = Repairer::new(10, &missing, &use_exps, stripe).unwrap();
                for (k, &e) in exps.iter().enumerate() {
                    r.add_recovery(e, &rec[k][off..off + stripe]);
                }
                for (i, sl) in s.iter().enumerate() {
                    r.add_input(i as u32, &sl[off..off + stripe]);
                }
                for (k, part) in r.solve().unwrap().into_iter().enumerate() {
                    rebuilt[k].extend(part);
                }
            }
            for (k, &m) in missing.iter().enumerate() {
                assert_eq!(rebuilt[k], s[m as usize], "slice {m} stripe {stripe}");
            }
        }
    }

    #[test]
    fn repair_needs_all_recovery() {
        let mut r = Repairer::new(4, &[0, 1], &[0, 1], 8).unwrap();
        r.add_recovery(0, &[0; 8]);
        assert_eq!(r.solve().unwrap_err(), RepairError::NotEnoughRecovery);
        assert!(Repairer::new(4, &[0], &[0, 1], 8).is_err());
        assert!(Repairer::new(4, &[5], &[0], 8).is_err());
        assert!(Repairer::new(4, &[0], &[0], 7).is_err());
    }

    #[test]
    fn packet_scanning_skips_damage() {
        let mut body = vec![0u8; 16];
        body[..4].copy_from_slice(&7u32.to_le_bytes());
        let pkt = build_packet(&[9u8; 16], TYPE_RECOVERY, &body);
        let mut buf = b"junk".to_vec();
        buf.extend(&pkt);
        let mut broken = pkt.clone();
        broken[70] ^= 1;
        buf.extend(&broken);
        buf.extend(&pkt);
        let ps = packets(&buf);
        assert_eq!(ps.len(), 2);
        let r = recovery_packet(&pkt, &[9u8; 16]).unwrap();
        assert_eq!(r.exponent, 7);
        assert_eq!(r.data_len, 12);
        assert!(recovery_packet(&pkt, &[8u8; 16]).is_none());
    }

    /// Builds a packet (test helper).
    pub fn build_packet(set_id: &Hash, kind: &[u8; 16], body: &[u8]) -> Vec<u8> {
        let len = (HEADER_LEN + body.len()) as u64;
        let mut p = Vec::with_capacity(len as usize);
        p.extend_from_slice(MAGIC);
        p.extend_from_slice(&len.to_le_bytes());
        p.extend_from_slice(&[0u8; 16]);
        p.extend_from_slice(set_id);
        p.extend_from_slice(kind);
        p.extend_from_slice(body);
        let h = md5(&p[32..]);
        p[16..32].copy_from_slice(&h);
        p
    }
}
