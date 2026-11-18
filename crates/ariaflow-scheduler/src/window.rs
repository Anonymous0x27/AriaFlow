//! Optional daily download window (e.g. "only between 02:00 and 06:30").

use serde::{Deserialize, Serialize};

/// Minutes after local midnight, inclusive start / exclusive end. Overnight windows wrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeWindow {
    pub start_minute: u32,
    pub end_minute: u32,
}

impl TimeWindow {
    pub fn new(start_minute: u32, end_minute: u32) -> Self {
        Self {
            start_minute: start_minute % 1440,
            end_minute: end_minute % 1440,
        }
    }

    /// `now_minute` = local hour*60 + minute.
    pub fn contains(&self, now_minute: u32) -> bool {
        let now = now_minute % 1440;
        if self.start_minute <= self.end_minute {
            now >= self.start_minute && now < self.end_minute
        } else {
            // Overnight window, e.g. 22:00 -> 06:00
            now >= self.start_minute || now < self.end_minute
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daytime_window() {
        let w = TimeWindow::new(120, 390); // 02:00 - 06:30
        assert!(w.contains(150));
        assert!(!w.contains(400));
    }

    #[test]
    fn overnight_window() {
        let w = TimeWindow::new(22 * 60, 6 * 60);
        assert!(w.contains(23 * 60));
        assert!(w.contains(5 * 60 + 59));
        assert!(!w.contains(12 * 60));
    }
}
