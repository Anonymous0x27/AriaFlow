//! Simple per-IP token bucket rate limiter (in-memory, process-local).
//!
//! Loopback callers are exempt: the bundled UI polls the daemon every 1.5s
//! (`list` + `status`) and every 1s while the detail dialog is open, and
//! several local windows/tabs share `127.0.0.1`. Throttling your own machine's
//! UI behind the limit meant for abusive remote clients only produces
//! self-inflicted 429s ("rate limited") in the desktop app and the web UI.
//! Remote (non-loopback) callers still get a bounded budget.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const CAPACITY: f64 = 600.0; // burst for non-loopback callers (was 120)
const REFILL_PER_SEC: f64 = 10.0; // sustained: 600 req/min (was 2/s)
const MAX_TRACKED_IPS: usize = 4096;

struct Bucket {
    tokens: f64,
    last: Instant,
}

pub struct RateLimiter {
    buckets: Mutex<HashMap<IpAddr, Bucket>>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Returns true if the request is allowed, false if the caller is rate limited.
    pub fn allow(&self, ip: IpAddr) -> bool {
        if ip.is_loopback() {
            return true;
        }
        let mut guard = self.buckets.lock().unwrap_or_else(|p| p.into_inner());
        if guard.len() > MAX_TRACKED_IPS {
            guard.retain(|_, b| b.last.elapsed() < Duration::from_secs(300));
        }
        let now = Instant::now();
        let bucket = guard.entry(ip).or_insert(Bucket {
            tokens: CAPACITY,
            last: now,
        });
        let elapsed = now.duration_since(bucket.last).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * REFILL_PER_SEC).min(CAPACITY);
        bucket.last = now;
        if bucket.tokens >= 1.0 {
            bucket.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_is_always_allowed() {
        let limiter = RateLimiter::new();
        let v4 = IpAddr::V4(std::net::Ipv4Addr::LOCALHOST);
        let v6 = IpAddr::V6(std::net::Ipv6Addr::LOCALHOST);
        for _ in 0..(CAPACITY as usize + 100) {
            assert!(limiter.allow(v4));
            assert!(limiter.allow(v6));
        }
    }

    #[test]
    fn non_loopback_budget_drains_then_denies() {
        let limiter = RateLimiter::new();
        let remote = IpAddr::V4(std::net::Ipv4Addr::new(203, 0, 113, 7));
        for i in 0..CAPACITY as usize {
            assert!(
                limiter.allow(remote),
                "request {i} within burst should be allowed"
            );
        }
        assert!(
            !limiter.allow(remote),
            "burst exhausted — next request must be rejected"
        );
    }
}
