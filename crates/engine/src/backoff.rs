//! Restart backoff for a supervised engine.
//!
//! A managed engine that crashes should be restarted, but an engine that
//! crashes *immediately and repeatedly* is broken in a way restarting will not
//! fix — spinning on it burns CPU and buries the real error. The delay grows
//! and the attempts are capped.

use std::time::Duration;

pub const MAX_ATTEMPTS: u32 = 5;
const BASE_MS: u64 = 500;
const CEILING_MS: u64 = 30_000;

#[derive(Debug, Default, Clone)]
pub struct Backoff {
    attempts: u32,
}

impl Backoff {
    pub fn new() -> Self {
        Self::default()
    }

    /// The delay before attempt `n`, doubling and capped.
    pub fn delay_for(attempt: u32) -> Duration {
        let shift = attempt.saturating_sub(1).min(16);
        let ms = BASE_MS.saturating_mul(1u64 << shift).min(CEILING_MS);
        Duration::from_millis(ms)
    }

    /// Record a failure and report how long to wait, or `None` when we have
    /// given up and the user needs to see the error instead.
    pub fn next_delay(&mut self) -> Option<Duration> {
        self.attempts += 1;
        (self.attempts <= MAX_ATTEMPTS).then(|| Self::delay_for(self.attempts))
    }

    /// A successful start clears the history, so a later unrelated crash gets
    /// the full retry budget rather than inheriting an old one.
    pub fn reset(&mut self) {
        self.attempts = 0;
    }

    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    pub fn exhausted(&self) -> bool {
        self.attempts > MAX_ATTEMPTS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_delay_doubles() {
        assert_eq!(Backoff::delay_for(1), Duration::from_millis(500));
        assert_eq!(Backoff::delay_for(2), Duration::from_millis(1000));
        assert_eq!(Backoff::delay_for(3), Duration::from_millis(2000));
    }

    #[test]
    fn the_delay_is_capped_so_a_recovering_engine_is_still_retried_promptly() {
        assert_eq!(Backoff::delay_for(20), Duration::from_millis(CEILING_MS));
    }

    #[test]
    fn attempts_are_capped_and_then_give_up() {
        let mut b = Backoff::new();
        for _ in 0..MAX_ATTEMPTS {
            assert!(b.next_delay().is_some());
        }
        assert!(
            b.next_delay().is_none(),
            "an engine that will not start must surface its error, not spin"
        );
        assert!(b.exhausted());
    }

    #[test]
    fn a_successful_start_restores_the_full_budget() {
        let mut b = Backoff::new();
        b.next_delay();
        b.next_delay();
        assert_eq!(b.attempts(), 2);
        b.reset();
        assert_eq!(b.attempts(), 0);
        assert!(!b.exhausted());
    }
}
