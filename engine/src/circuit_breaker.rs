use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Two independent breakers: loss-based and rejection-based.
/// When either trips, all trading pauses until cooldown expires.
pub struct CircuitBreaker {
    // Loss breaker
    loss_window: VecDeque<(Instant, f64)>,
    loss_threshold: f64,
    loss_window_duration: Duration,

    // Rejection breaker
    rejection_window: VecDeque<(Instant, bool)>, // (time, was_rejected)
    rejection_threshold: f64,
    rejection_window_size: usize,

    // Cooldown
    cooldown: Duration,
    tripped_at: Option<Instant>,
}

impl CircuitBreaker {
    pub fn new(
        loss_threshold: f64,
        loss_window_secs: u64,
        rejection_threshold: f64,
        rejection_window_size: usize,
        cooldown_secs: u64,
    ) -> Self {
        CircuitBreaker {
            loss_window: VecDeque::new(),
            loss_threshold,
            loss_window_duration: Duration::from_secs(loss_window_secs),
            rejection_window: VecDeque::new(),
            rejection_threshold,
            rejection_window_size,
            cooldown: Duration::from_secs(cooldown_secs),
            tripped_at: None,
        }
    }

    /// Check if trading is allowed. Returns Some(reason) if blocked.
    pub fn check(&mut self) -> Option<String> {
        // If tripped, check cooldown
        if let Some(tripped) = self.tripped_at {
            if tripped.elapsed() < self.cooldown {
                let remaining = self.cooldown.saturating_sub(tripped.elapsed());
                return Some(format!("cooldown {}s remaining", remaining.as_secs()));
            }
            // Cooldown expired, reset
            self.tripped_at = None;
        }

        let now = Instant::now();

        // Prune old loss entries
        while let Some((t, _)) = self.loss_window.front() {
            if now.duration_since(*t) > self.loss_window_duration {
                self.loss_window.pop_front();
            } else {
                break;
            }
        }

        // Check cumulative loss
        let total_loss: f64 = self.loss_window.iter().map(|(_, pnl)| pnl).sum();
        if total_loss < -self.loss_threshold {
            self.tripped_at = Some(now);
            return Some(format!(
                "loss breaker: ${:.2} loss in {}s window (threshold: ${:.2})",
                total_loss.abs(),
                self.loss_window_duration.as_secs(),
                self.loss_threshold
            ));
        }

        // Check rejection rate
        if self.rejection_window.len() >= self.rejection_window_size {
            let rejections = self.rejection_window.iter().filter(|(_, r)| *r).count();
            let rate = rejections as f64 / self.rejection_window.len() as f64;
            if rate > self.rejection_threshold {
                self.tripped_at = Some(now);
                return Some(format!(
                    "rejection breaker: {:.0}% rejected ({}/{}, threshold: {:.0}%)",
                    rate * 100.0,
                    rejections,
                    self.rejection_window.len(),
                    self.rejection_threshold * 100.0
                ));
            }
        }

        None
    }

    /// Record a trade result. pnl can be positive (profit) or negative (loss).
    /// rejected=true means the sportsbook rejected the bet.
    pub fn record(&mut self, pnl: f64, rejected: bool) {
        let now = Instant::now();
        self.loss_window.push_back((now, pnl));
        self.rejection_window.push_back((now, rejected));

        // Keep rejection window at configured size
        while self.rejection_window.len() > self.rejection_window_size * 2 {
            self.rejection_window.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_trip_initially() {
        let mut cb = CircuitBreaker::new(50.0, 1800, 0.5, 10, 300);
        assert!(cb.check().is_none());
    }

    #[test]
    fn test_loss_breaker_trips() {
        let mut cb = CircuitBreaker::new(50.0, 1800, 0.5, 10, 300);
        cb.record(-30.0, false);
        cb.record(-25.0, false);
        assert!(cb.check().is_some()); // $55 loss > $50 threshold
    }

    #[test]
    fn test_profits_offset_losses() {
        let mut cb = CircuitBreaker::new(50.0, 1800, 0.5, 10, 300);
        cb.record(-30.0, false);
        cb.record(20.0, false);
        cb.record(-15.0, false);
        // Net: -25, below threshold
        assert!(cb.check().is_none());
    }
}
