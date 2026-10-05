//! Arithmetic in GF(2^16) with the PAR2 generator x^16 + x^12 + x^3 + x + 1.

use std::sync::OnceLock;

const POLY: u32 = 0x1100B;
/// Order of the multiplicative group.
pub const LIMIT: u32 = 65535;

struct Tables {
    log: Vec<u16>,
    exp: Vec<u16>,
}

fn tables() -> &'static Tables {
    static T: OnceLock<Tables> = OnceLock::new();
    T.get_or_init(|| {
        let mut log = vec![0u16; 65536];
        let mut exp = vec![0u16; 2 * LIMIT as usize];
        let mut x: u32 = 1;
        for i in 0..LIMIT {
            exp[i as usize] = x as u16;
            exp[(i + LIMIT) as usize] = x as u16;
            log[x as usize] = i as u16;
            x <<= 1;
            if x & 0x10000 != 0 {
                x ^= POLY;
            }
        }
        Tables { log, exp }
    })
}

#[inline]
pub fn mul(a: u16, b: u16) -> u16 {
    if a == 0 || b == 0 {
        return 0;
    }
    let t = tables();
    t.exp[t.log[a as usize] as usize + t.log[b as usize] as usize]
}

#[inline]
pub fn div(a: u16, b: u16) -> u16 {
    assert!(b != 0, "division by zero in GF(2^16)");
    if a == 0 {
        return 0;
    }
    let t = tables();
    t.exp[(t.log[a as usize] as usize + LIMIT as usize - t.log[b as usize] as usize)
        % LIMIT as usize]
}

/// `a` raised to `e`.
pub fn pow(a: u16, e: u32) -> u16 {
    if e == 0 {
        return 1;
    }
    if a == 0 {
        return 0;
    }
    let t = tables();
    let l = (t.log[a as usize] as u64 * e as u64) % LIMIT as u64;
    t.exp[l as usize]
}

/// 2 raised to `e`.
pub fn exp2(e: u32) -> u16 {
    tables().exp[(e % LIMIT) as usize]
}

fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// The PAR2 input slice constants: 2^n for the n coprime to 65535,
/// in increasing order.
pub fn input_constants(count: usize) -> Vec<u16> {
    let mut out = Vec::with_capacity(count);
    let mut n = 0u32;
    while out.len() < count {
        if gcd(LIMIT, n) == 1 {
            out.push(exp2(n));
        }
        n += 1;
    }
    out
}

/// `dst ^= f * src`, element-wise over little-endian 16-bit words. A `src`
/// shorter than `dst` counts as zero-padded, including an odd final byte.
pub fn mul_add_region(dst: &mut [u8], src: &[u8], f: u16) {
    if f == 0 {
        return;
    }
    let len = dst.len().min(src.len());
    if f == 1 {
        for (d, s) in dst[..len].iter_mut().zip(&src[..len]) {
            *d ^= *s;
        }
        return;
    }
    let n = len & !1;
    if len > n {
        // Odd tail: the word's high byte is padding (zero).
        let p = mul(f, src[n] as u16);
        dst[n] ^= p as u8;
        if n + 1 < dst.len() {
            dst[n + 1] ^= (p >> 8) as u8;
        }
    }
    // Split tables: f*w = f*(w & 0xff) ^ f*(w & 0xff00).
    let mut lo = [0u16; 256];
    let mut hi = [0u16; 256];
    for i in 0..256u16 {
        lo[i as usize] = mul(f, i);
        hi[i as usize] = mul(f, i << 8);
    }
    for (d, s) in dst[..n].chunks_exact_mut(2).zip(src[..n].chunks_exact(2)) {
        let p = lo[s[0] as usize] ^ hi[s[1] as usize];
        d[0] ^= p as u8;
        d[1] ^= (p >> 8) as u8;
    }
}

/// Inverts a square matrix in place (row-major). Returns false if singular.
pub fn invert(m: &mut [u16], n: usize) -> bool {
    let mut inv = vec![0u16; n * n];
    for i in 0..n {
        inv[i * n + i] = 1;
    }
    for col in 0..n {
        let Some(pivot) = (col..n).find(|&r| m[r * n + col] != 0) else {
            return false;
        };
        if pivot != col {
            for k in 0..n {
                m.swap(pivot * n + k, col * n + k);
                inv.swap(pivot * n + k, col * n + k);
            }
        }
        let p = m[col * n + col];
        for k in 0..n {
            m[col * n + k] = div(m[col * n + k], p);
            inv[col * n + k] = div(inv[col * n + k], p);
        }
        for r in 0..n {
            if r == col {
                continue;
            }
            let f = m[r * n + col];
            if f == 0 {
                continue;
            }
            for k in 0..n {
                m[r * n + k] ^= mul(f, m[col * n + k]);
                inv[r * n + k] ^= mul(f, inv[col * n + k]);
            }
        }
    }
    m.copy_from_slice(&inv);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_axioms() {
        assert_eq!(mul(1, 0x1234), 0x1234);
        assert_eq!(mul(0, 0x1234), 0);
        assert_eq!(mul(2, 0x8000), (0x10000u32 ^ POLY) as u16);
        for a in [1u16, 2, 3, 0x1234, 0xffff, 0x8001] {
            assert_eq!(mul(a, div(1, a)), 1);
            assert_eq!(div(mul(a, 0x4321), 0x4321), a);
            assert_eq!(pow(a, 3), mul(a, mul(a, a)));
            assert_eq!(pow(a, LIMIT), 1);
        }
    }

    #[test]
    fn constants_match_par2_spec() {
        // n = 1, 2, 4, 7, 8, 11, 13, 14, 16, ... (coprime to 3, 5, 17, 257)
        let c = input_constants(9);
        let expect: Vec<u16> = [1u32, 2, 4, 7, 8, 11, 13, 14, 16]
            .iter()
            .map(|&n| exp2(n))
            .collect();
        assert_eq!(c, expect);
        assert_eq!(c[0], 2);
        assert_eq!(c[1], 4);
        assert_eq!(c[2], 16);
        assert_eq!(c[3], 128);
    }

    #[test]
    fn region_matches_scalar() {
        let src: Vec<u8> = (0..1000u32).map(|i| (i * 31 % 251) as u8).collect();
        let mut dst = vec![0x55u8; 1000];
        let f = 0xbeef;
        let mut want = dst.clone();
        for i in (0..1000).step_by(2) {
            let w = u16::from_le_bytes([src[i], src[i + 1]]);
            let d = u16::from_le_bytes([want[i], want[i + 1]]) ^ mul(f, w);
            want[i..i + 2].copy_from_slice(&d.to_le_bytes());
        }
        mul_add_region(&mut dst, &src, f);
        assert_eq!(dst, want);
    }

    #[test]
    fn inverts() {
        let n = 4;
        let base = input_constants(n);
        let mut m: Vec<u16> = (0..n)
            .flat_map(|r| {
                base.iter()
                    .map(move |&b| pow(b, r as u32))
                    .collect::<Vec<_>>()
            })
            .collect();
        let orig = m.clone();
        assert!(invert(&mut m, n));
        for r in 0..n {
            for c in 0..n {
                let mut s = 0u16;
                for k in 0..n {
                    s ^= mul(orig[r * n + k], m[k * n + c]);
                }
                assert_eq!(s, (r == c) as u16);
            }
        }
        let mut singular = vec![1u16, 1, 1, 1];
        assert!(!invert(&mut singular, 2));
    }
}
