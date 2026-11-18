//! Simple token-bucket rate limiter shared by all streaming transports.
//!
//! One bucket per transfer; refilled continuously at `rate_bytes_per_sec`.
//! Burst equals one second of allowance.

use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug)]
pub struct RateLimiter {
    rate: f64,
    burst: f64,
    state: Mutex<Bucket>,
}

#[derive(Debug)]
struct Bucket {
    /// Signed balance. Positive = unused burst budget (capped at `burst`);
    /// negative = bytes reserved by waiters that have not refilled yet.
    allowance: f64,
    last: Instant,
}

impl RateLimiter {
    /// `rate_bytes_per_sec == 0` yields an unlimited limiter.
    pub fn new(rate_bytes_per_sec: u64) -> Self {
        let burst = rate_bytes_per_sec.max(1) as f64;
        Self {
            rate: rate_bytes_per_sec as f64,
            burst,
            state: Mutex::new(Bucket {
                allowance: burst,
                last: Instant::now(),
            }),
        }
    }

    pub fn is_limited(&self) -> bool {
        self.rate > 0.0
    }

    /// Computes how long the caller must wait for `bytes` of budget.
    ///
    /// Bytes are *reserved* at call time: the bucket balance is allowed to go
    /// negative (a debt the limiter will spend with future refills). This is
    /// what keeps concurrent acquirers honest — each byte is accounted exactly
    /// once, so the aggregate across all callers can never exceed `rate` plus
    /// the initial burst, regardless of chunk size or task interleaving.
    fn plan_delay(&self, bytes: u64) -> std::time::Duration {
        let mut bucket = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last).as_secs_f64();
        bucket.last = now;
        bucket.allowance = (bucket.allowance + elapsed * self.rate).min(self.burst);
        bucket.allowance -= bytes as f64;
        if bucket.allowance >= 0.0 {
            std::time::Duration::ZERO
        } else {
            std::time::Duration::from_secs_f64(-bucket.allowance / self.rate)
        }
    }

    /// Delays the caller so that `bytes` worth of traffic fits the bucket.
    pub async fn acquire(&self, bytes: u64) {
        if !self.is_limited() {
            return;
        }
        let wait = self.plan_delay(bytes);
        if wait.is_zero() {
            return;
        }
        tokio::time::sleep(wait).await;
    }

    /// Blocking variant for synchronous transports (FTP/SFTP on dedicated OS threads).
    pub fn acquire_blocking(&self, bytes: u64) {
        if !self.is_limited() {
            return;
        }
        let wait = self.plan_delay(bytes);
        if wait.is_zero() {
            return;
        }
        std::thread::sleep(wait);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn unlimited_never_waits() {
        let limiter = RateLimiter::new(0);
        let start = Instant::now();
        for _ in 0..100 {
            limiter.acquire(1024).await;
        }
        assert!(start.elapsed().as_millis() < 50);
    }

    #[tokio::test]
    async fn aggregate_across_callers_respects_rate() {
        use std::sync::Arc;
        let limiter = Arc::new(RateLimiter::new(1_000_000)); // 1 MB/s, burst = 1 MB
        let start = Instant::now();
        let mut handles = Vec::new();
        for _ in 0..4 {
            let limiter = Arc::clone(&limiter);
            handles.push(tokio::spawn(async move {
                // 1 MB each = 4 MB total. Initial burst 1 MB + 1 MB/s refill => >= 3s.
                limiter.acquire(1_000_000).await;
            }));
        }
        for handle in handles {
            handle.await.unwrap();
        }
        assert!(
            start.elapsed().as_millis() >= 2900,
            "aggregate must respect the shared rate"
        );
    }

    #[tokio::test]
    async fn limits_throughput() {
        let limiter = RateLimiter::new(1_000_000); // 1 MB/s, burst = 1 MB
        limiter.acquire(1_000_000).await; // drain the burst
        let start = Instant::now();
        limiter.acquire(1_000_000).await; // must wait ~1s
        assert!(start.elapsed().as_millis() >= 900);
    }
}
