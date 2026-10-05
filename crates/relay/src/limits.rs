//! In-memory connection counters. Nothing here outlives the process.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct Limits {
    max_total: usize,
    max_per_ip: usize,
    inner: Mutex<Counts>,
}

#[derive(Debug, Default)]
struct Counts {
    total: usize,
    per_ip: HashMap<IpAddr, usize>,
}

/// Holds one connection slot; releases it on drop.
#[derive(Debug)]
pub struct Slot {
    limits: Arc<Limits>,
    ip: IpAddr,
}

impl Limits {
    pub fn new(max_total: usize, max_per_ip: usize) -> Arc<Limits> {
        Arc::new(Limits {
            max_total,
            max_per_ip,
            inner: Mutex::new(Counts::default()),
        })
    }

    /// Takes a slot for `ip`, or `None` if a limit is reached.
    pub fn acquire(self: &Arc<Self>, ip: IpAddr) -> Option<Slot> {
        let mut c = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mine = c.per_ip.get(&ip).copied().unwrap_or(0);
        if c.total >= self.max_total || mine >= self.max_per_ip {
            return None;
        }
        c.total += 1;
        c.per_ip.insert(ip, mine + 1);
        Some(Slot {
            limits: Arc::clone(self),
            ip,
        })
    }

    pub fn total(&self) -> usize {
        self.inner.lock().unwrap_or_else(|e| e.into_inner()).total
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        let mut c = self.limits.inner.lock().unwrap_or_else(|e| e.into_inner());
        c.total = c.total.saturating_sub(1);
        if let Some(n) = c.per_ip.get_mut(&self.ip) {
            *n -= 1;
            if *n == 0 {
                c.per_ip.remove(&self.ip);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn per_ip_and_total() {
        let l = Limits::new(3, 2);
        let a: IpAddr = "203.0.113.1".parse().unwrap();
        let b: IpAddr = "203.0.113.2".parse().unwrap();
        let a1 = l.acquire(a).unwrap();
        let _a2 = l.acquire(a).unwrap();
        assert!(l.acquire(a).is_none(), "per-IP limit");
        let _b1 = l.acquire(b).unwrap();
        assert!(l.acquire(b).is_none(), "total limit");
        assert_eq!(l.total(), 3);
        drop(a1);
        assert_eq!(l.total(), 2);
        let _b2 = l.acquire(b).unwrap();
        assert!(l.acquire(b).is_none());
    }

    #[test]
    fn map_is_cleaned_up() {
        let l = Limits::new(10, 10);
        let a: IpAddr = "203.0.113.1".parse().unwrap();
        drop(l.acquire(a).unwrap());
        assert!(l.inner.lock().unwrap().per_ip.is_empty());
        assert_eq!(l.total(), 0);
    }
}
