//! # AriaFlow Scheduler
//!
//! Decides *which* queued tasks may run, *how many* connections each may use and
//! *when* to retry. Pure decision logic lives here and is heavily unit-tested;
//! the daemon owns the actual event loop.

pub mod planner;
pub mod window;

pub use planner::{effective_max_connections, pick_tasks, retry_backoff, SchedulerConfig};
pub use window::TimeWindow;
