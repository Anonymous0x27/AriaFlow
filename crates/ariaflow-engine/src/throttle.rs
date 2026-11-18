//! Re-exported from `ariaflow-protocol` so existing `ariaflow_engine::throttle`
//! paths keep working. The limiter itself lives in the protocol crate so both
//! providers and the daemon can share one token bucket across tasks.

pub use ariaflow_protocol::throttle::RateLimiter;

use std::sync::Arc;

/// Resolves the limiter for one transfer: the daemon-supplied shared
/// (aggregate, global) bucket when configured, otherwise a fresh per-task
/// bucket paced at `options.rate_limit_bytes_per_sec` (`0` = unlimited).
pub fn resolve_limiter(options: &ariaflow_protocol::ProviderOptions) -> Arc<RateLimiter> {
    options
        .shared_limiter
        .clone()
        .unwrap_or_else(|| Arc::new(RateLimiter::new(options.rate_limit_bytes_per_sec)))
}
