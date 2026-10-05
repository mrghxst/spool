//! PAR2 verify and repair against fixtures made by par2cmdline
//! (`scripts/make-fixtures.sh`).

use std::path::PathBuf;

use spool_engine::par2::{self, FileVerifier, Repairer, Set};

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/par2")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap()
}

fn set() -> Set {
    Set::parse(&read("set.par2")).unwrap()
}

fn verify(set: &Set, name: &str, data: &[u8], deep: bool) -> par2::Verified {
    let f = set.files.iter().find(|f| f.name == name).unwrap();
    let mut v = FileVerifier::new(set, f, deep);
    // Odd chunk sizes to exercise slice boundaries.
    for chunk in data.chunks(1000) {
        v.update(chunk);
    }
    v.finish()
}

/// (exponent, data) for every recovery slice in the volume files.
fn recovery(set: &Set) -> Vec<(u32, Vec<u8>)> {
    let mut out = Vec::new();
    for vol in ["set.vol0+3.par2", "set.vol3+3.par2", "set.vol6+2.par2"] {
        let buf = read(vol);
        for p in par2::packets(&buf) {
            if &p.header.kind == par2::TYPE_RECOVERY {
                assert_eq!(p.header.set_id, set.set_id);
                let exp = u32::from_le_bytes(p.body[..4].try_into().unwrap());
                out.push((exp, p.body[4..].to_vec()));
            }
        }
    }
    out.sort_by_key(|r| r.0);
    out
}

#[test]
fn parses_index() {
    let s = set();
    assert_eq!(s.slice_size, 4096);
    assert_eq!(s.files.len(), 3);
    assert_eq!(s.missing_descriptions, 0);
    let mut names: Vec<_> = s
        .files
        .iter()
        .map(|f| (f.name.as_str(), f.length))
        .collect();
    names.sort();
    assert_eq!(
        names,
        vec![("a.bin", 40000), ("b.bin", 12345), ("c.bin", 8192)]
    );
    assert_eq!(s.total_slices(), 10 + 4 + 2);
    for f in &s.files {
        assert_eq!(f.slices.len() as u32, f.slice_count(4096));
        let data = read(&f.name);
        assert_eq!(par2::md5(&data), f.md5);
        assert_eq!(par2::md5(&data[..data.len().min(16384)]), f.md5_16k);
    }
    // The main packet's order defines contiguous slice numbering.
    let mut next = 0;
    for f in &s.files {
        assert_eq!(f.first_slice, next);
        next += f.slice_count(4096);
    }
}

#[test]
fn volumes_carry_the_same_set() {
    let s = set();
    let v = Set::parse(&read("set.vol3+3.par2")).unwrap();
    assert_eq!(v.set_id, s.set_id);
    assert_eq!(v.files, s.files);
    assert_eq!(
        recovery(&s).iter().map(|r| r.0).collect::<Vec<_>>(),
        (0..8).collect::<Vec<_>>()
    );
}

#[test]
fn intact_files_verify() {
    let s = set();
    for name in ["a.bin", "b.bin", "c.bin"] {
        for deep in [false, true] {
            let r = verify(&s, name, &read(name), deep);
            assert!(r.ok, "{name}");
            assert!(r.damaged.is_empty());
        }
    }
}

#[test]
fn finds_damaged_slices() {
    let s = set();
    let mut a = read("a.bin");
    a[2 * 4096 + 5] ^= 0xff;
    a[9 * 4096 + 1] ^= 0x01; // last, partial slice
    let r = verify(&s, "a.bin", &a, false);
    assert!(!r.ok);
    assert_eq!(r.damaged, vec![2, 9]);
    assert_eq!(verify(&s, "a.bin", &a, true).damaged, vec![2, 9]);

    // A truncated file: the partial slice and everything after is damaged.
    let b = read("b.bin");
    let r = verify(&s, "b.bin", &b[..5000], false);
    assert!(!r.ok);
    assert_eq!(r.damaged, vec![1, 2, 3]);

    // Zeroed segments, as left by missing articles.
    let mut c = read("c.bin");
    c[4096..].fill(0);
    assert_eq!(verify(&s, "c.bin", &c, false).damaged, vec![1]);
}

#[test]
fn our_recovery_math_matches_par2cmdline() {
    let s = set();
    let mut inputs: Vec<Vec<u8>> = Vec::new();
    for f in &s.files {
        let data = read(&f.name);
        for chunk in data.chunks(4096) {
            let mut v = chunk.to_vec();
            v.resize(4096, 0);
            inputs.push(v);
        }
    }
    let refs: Vec<&[u8]> = inputs.iter().map(Vec::as_slice).collect();
    for (exp, data) in recovery(&s) {
        assert_eq!(
            par2::compute_recovery(&refs, 4096, exp),
            data,
            "exponent {exp}"
        );
    }
}

/// Damages slices across files, repairs them from the volumes, and checks
/// the MD5 of every file.
fn repair_case(damage: &[(&str, u32)], use_exps: &[u32], stripe: usize) {
    let s = set();
    let mut files: Vec<(String, Vec<u8>)> = s
        .files
        .iter()
        .map(|f| (f.name.clone(), read(&f.name)))
        .collect();
    for &(name, slice) in damage {
        let (_, data) = files.iter_mut().find(|(n, _)| n == name).unwrap();
        let start = slice as usize * 4096;
        let end = (start + 4096).min(data.len());
        data[start..end].fill(0);
    }
    // Verify to find the damage, as the app does.
    let mut missing = Vec::new();
    for (f, (name, data)) in s.files.iter().zip(&files) {
        let r = verify(&s, name, data, false);
        missing.extend(r.damaged.iter().map(|d| f.first_slice + d));
    }
    assert_eq!(missing.len(), damage.len());
    let rec = recovery(&s);
    let exps = &use_exps[..missing.len()];

    let mut rebuilt = vec![Vec::new(); missing.len()];
    for off in (0..4096).step_by(stripe) {
        let mut r = Repairer::new(s.total_slices(), &missing, exps, stripe).unwrap();
        for (e, data) in &rec {
            r.add_recovery(*e, &data[off..off + stripe]);
        }
        for (f, (_, data)) in s.files.iter().zip(&files) {
            for i in 0..f.slice_count(4096) {
                let start = i as usize * 4096 + off;
                let end = (start + stripe).min(data.len());
                let part = if start < data.len() {
                    &data[start..end]
                } else {
                    &[][..]
                };
                r.add_input(f.first_slice + i, part);
            }
        }
        for (k, part) in r.solve().unwrap().into_iter().enumerate() {
            rebuilt[k].extend(part);
        }
    }
    // Write rebuilt slices back.
    for (k, &g) in missing.iter().enumerate() {
        let f = s
            .files
            .iter()
            .find(|f| g >= f.first_slice && g < f.first_slice + f.slice_count(4096))
            .unwrap();
        let (_, data) = files.iter_mut().find(|(n, _)| *n == f.name).unwrap();
        let start = (g - f.first_slice) as usize * 4096;
        let end = (start + 4096).min(data.len());
        data[start..end].copy_from_slice(&rebuilt[k][..end - start]);
    }
    for (f, (name, data)) in s.files.iter().zip(&files) {
        assert_eq!(par2::md5(data), f.md5, "{name} after repair");
        assert_eq!(data, &read(name));
    }
}

#[test]
fn repairs_one_slice() {
    repair_case(&[("a.bin", 3)], &[0], 4096);
}

#[test]
fn repairs_across_files_with_mixed_exponents() {
    repair_case(
        &[("a.bin", 0), ("a.bin", 9), ("b.bin", 3), ("c.bin", 1)],
        &[7, 2, 5, 0],
        4096,
    );
}

#[test]
fn repairs_with_all_recovery_in_stripes() {
    repair_case(
        &[
            ("a.bin", 1),
            ("a.bin", 2),
            ("a.bin", 5),
            ("b.bin", 0),
            ("b.bin", 1),
            ("b.bin", 2),
            ("c.bin", 0),
            ("c.bin", 1),
        ],
        &[0, 1, 2, 3, 4, 5, 6, 7],
        512,
    );
}

#[test]
fn deobfuscation_by_16k_hash() {
    let s = set();
    let data = read("b.bin");
    let h = par2::md5(&data[..data.len().min(16384)]);
    assert_eq!(s.match_16k(&h, data.len() as u64).unwrap().name, "b.bin");
    assert!(s.match_16k(&[0u8; 16], 1).is_none());
}
