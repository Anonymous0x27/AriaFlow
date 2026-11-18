//! Instantaneous and average speed tracking over a sliding window.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

const WINDOW: Duration = Duration::from_secs(5);

pub struct SpeedTracker {
    /// (sample time, bytes downloaded at that time)
    samples: VecDeque<(Instant, u64)>,
    started_at: Option<Instant>,
    bytes_at_start: u64,
}

impl SpeedTracker {
    pub fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(64),
            started_at: None,
            bytes_at_start: 0,
        }
    }

    /// Records the cumulative downloaded byte count.
    pub fn record(&mut self, total_downloaded: u64) {
        let now = Instant::now();
        if self.started_at.is_none() {
            self.started_at = Some(now);
            self.bytes_at_start = total_downloaded;
        }
        self.samples.push_back((now, total_downloaded));
        while let Some((t, _)) = self.samples.front() {
            if t.elapsed() > WINDOW {
                self.samples.pop_front();
            } else {
                break;
            }
        }
    }

    /// Bytes per second over the trailing 5-second window.
    pub fn instant_speed(&self) -> u64 {
        let (first_t, first_bytes) = match self.samples.front() {
            Some(v) => *v,
            None => return 0,
        };
        let (last_t, last_bytes) = match self.samples.back() {
            Some(v) => *v,
            None => return 0,
        };
        let secs = last_t.duration_since(first_t).as_secs_f64();
        if secs < 0.05 {
            return 0;
        }
        ((last_bytes.saturating_sub(first_bytes)) as f64 / secs) as u64
    }

    /// Bytes per second over the whole active period.
    pub fn average_speed(&self, current_total: u64) -> u64 {
        match self.started_at {
            Some(start) => {
                let secs = start.elapsed().as_secs_f64();
                if secs < 0.05 {
                    return 0;
                }
                ((current_total.saturating_sub(self.bytes_at_start)) as f64 / secs) as u64
            }
            None => 0,
        }
    }
}

impl Default for SpeedTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn average_of_zero_elapsed_is_zero() {
        let t = SpeedTracker::new();
        assert_eq!(t.average_speed(1000), 0);
    }

    #[test]
    fn records_and_reports() {
        let mut t = SpeedTracker::new();
        t.record(0);
        std::thread::sleep(std::time::Duration::from_millis(80));
        t.record(8192);
        assert!(t.instant_speed() > 0);
        assert!(t.average_speed(8192) > 0);
    }
}
