use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Per-venue sliding window rate limiter.
/// Tracks bet timestamps per venue and rejects if hourly limit exceeded.
pub struct RateLimiter {
    windows: HashMap<String, VecDeque<Instant>>,
    limits: HashMap<String, u32>,
    default_limit: u32,
    window: Duration,
}

impl RateLimiter {
    pub fn new(limits: HashMap<String, u32>, default_limit: u32) -> Self {
        RateLimiter {
            windows: HashMap::new(),
            limits,
            default_limit,
            window: Duration::from_secs(3600), // 1-hour sliding window
        }
    }

    /// Check if a bet can be placed on this venue. Returns Ok(()) or Err((current_count, max)).
    pub fn check(&self, venue: &str) -> Result<(), (u32, u32)> {
        let max = self.limits.get(venue).copied().unwrap_or(self.default_limit);
        let count = self.windows.get(venue).map(|w| w.len() as u32).unwrap_or(0);
        if count >= max {
            Err((count, max))
        } else {
            Ok(())
        }
    }

    /// Record a bet placed on a venue.
    pub fn record(&mut self, venue: &str) {
        self.windows
            .entry(venue.to_string())
            .or_default()
            .push_back(Instant::now());
    }

    /// Remove expired entries from all windows.
    pub fn prune(&mut self) {
        let now = Instant::now();
        for window in self.windows.values_mut() {
            while let Some(front) = window.front() {
                if now.duration_since(*front) > self.window {
                    window.pop_front();
                } else {
                    break;
                }
            }
        }
        self.windows.retain(|_, w| !w.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allows_within_limit() {
        let mut rl = RateLimiter::new(HashMap::from([("dk".into(), 3)]), 3);
        rl.record("dk");
        rl.record("dk");
        assert!(rl.check("dk").is_ok());
    }

    #[test]
    fn test_blocks_at_limit() {
        let mut rl = RateLimiter::new(HashMap::from([("dk".into(), 3)]), 3);
        rl.record("dk");
        rl.record("dk");
        rl.record("dk");
        assert!(rl.check("dk").is_err());
    }

    #[test]
    fn test_per_venue_isolation() {
        let mut rl = RateLimiter::new(
            HashMap::from([("dk".into(), 2), ("fd".into(), 2)]),
            2,
        );
        rl.record("dk");
        rl.record("dk");
        assert!(rl.check("dk").is_err());
        assert!(rl.check("fd").is_ok());
    }
}
