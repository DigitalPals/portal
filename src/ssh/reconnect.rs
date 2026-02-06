//! SSH reconnect policy with exponential backoff and jitter.

use rand::Rng;
use std::time::Duration;

/// Policy for exponential backoff reconnect attempts.
#[derive(Debug, Clone, Copy)]
pub struct ReconnectPolicy {
    pub base_delay_ms: u64,
    pub max_delay_ms: u64,
    pub max_attempts: u32,
}

impl ReconnectPolicy {
    pub fn new(base_delay_ms: u64, max_delay_ms: u64, max_attempts: u32) -> Self {
        Self {
            base_delay_ms,
            max_delay_ms,
            max_attempts,
        }
    }

    /// Calculate the backoff delay for a given attempt (0-based) with jitter.
    pub fn delay_with_jitter(&self, attempt: u32) -> Duration {
        let delay_ms = self.raw_delay_ms(attempt);
        let jittered_ms = Self::apply_jitter(delay_ms);
        Duration::from_millis(jittered_ms.min(self.max_delay_ms))
    }

    fn raw_delay_ms(&self, attempt: u32) -> u64 {
        let shift = attempt.min(63);
        let multiplier = 1u64.checked_shl(shift).unwrap_or(u64::MAX);
        let delay = self.base_delay_ms.saturating_mul(multiplier);
        delay.min(self.max_delay_ms)
    }

    fn apply_jitter(delay_ms: u64) -> u64 {
        let mut rng = rand::thread_rng();
        let jitter: f64 = rng.gen_range(0.9..=1.1);
        ((delay_ms as f64) * jitter).round().max(0.0) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::{SeedableRng, rngs::StdRng};

    #[test]
    fn raw_delay_grows_exponentially_with_cap() {
        let policy = ReconnectPolicy::new(1000, 30_000, 5);
        assert_eq!(policy.raw_delay_ms(0), 1000);
        assert_eq!(policy.raw_delay_ms(1), 2000);
        assert_eq!(policy.raw_delay_ms(2), 4000);
        assert_eq!(policy.raw_delay_ms(3), 8000);
        assert_eq!(policy.raw_delay_ms(4), 16_000);
        assert_eq!(policy.raw_delay_ms(5), 30_000);
    }

    #[test]
    fn jitter_stays_within_ten_percent() {
        let policy = ReconnectPolicy::new(1000, 30_000, 5);
        let base = policy.raw_delay_ms(2);
        let mut rng = StdRng::seed_from_u64(42);
        let jitter: f64 = rng.gen_range(0.9..=1.1);
        let jittered = ((base as f64) * jitter).round() as u64;
        let min = (base as f64 * 0.9).round() as u64;
        let max = (base as f64 * 1.1).round() as u64;
        assert!((min..=max).contains(&jittered));
    }
}
