#[cfg(test)]
mod tests {
    use kube_forward::backoff::{MAX_BACKOFF, backoff_delay, backoff_delay_with};
    use std::time::Duration;

    #[test]
    fn test_attempt_zero_is_zero() {
        assert_eq!(
            backoff_delay_with(Duration::from_secs(5), 0, MAX_BACKOFF, 0.5),
            Duration::ZERO
        );
    }

    #[test]
    fn test_first_attempt_equals_base() {
        let base = Duration::from_secs(5);
        // attempt 1 has no jitter window (floor == ceiling == base).
        assert_eq!(backoff_delay_with(base, 1, MAX_BACKOFF, 0.0), base);
        assert_eq!(backoff_delay_with(base, 1, MAX_BACKOFF, 1.0), base);
    }

    #[test]
    fn test_grows_exponentially_within_window() {
        let base = Duration::from_secs(1);
        // attempt 3 -> ceiling = base * 2^2 = 4s; fraction picks within [1s, 4s].
        assert_eq!(
            backoff_delay_with(base, 3, MAX_BACKOFF, 0.0),
            Duration::from_secs(1)
        );
        assert_eq!(
            backoff_delay_with(base, 3, MAX_BACKOFF, 1.0),
            Duration::from_secs(4)
        );
        let mid = backoff_delay_with(base, 3, MAX_BACKOFF, 0.5);
        assert!(mid >= Duration::from_secs(1) && mid <= Duration::from_secs(4));
    }

    #[test]
    fn test_capped() {
        let base = Duration::from_secs(10);
        let cap = Duration::from_secs(30);
        // attempt 10 would be huge; ceiling clamps to cap.
        let d = backoff_delay_with(base, 10, cap, 1.0);
        assert_eq!(d, cap);
        let d0 = backoff_delay_with(base, 10, cap, 0.0);
        assert_eq!(d0, base);
    }

    #[test]
    fn test_no_overflow_on_large_attempt() {
        // Must not panic on overflow for very large attempt counts.
        let d = backoff_delay_with(Duration::from_secs(5), 1000, MAX_BACKOFF, 1.0);
        assert_eq!(d, MAX_BACKOFF);
    }

    #[test]
    fn test_public_fn_within_bounds() {
        let base = Duration::from_secs(2);
        for attempt in 1..8 {
            let d = backoff_delay(base, attempt);
            assert!(d >= base, "delay {:?} below base at attempt {}", d, attempt);
            assert!(d <= MAX_BACKOFF, "delay {:?} above cap", d);
        }
    }
}
