use std::time::Duration;

/// Upper bound on any single backoff delay.
pub const MAX_BACKOFF: Duration = Duration::from_secs(60);

/// Exponential backoff with jitter, capped at [`MAX_BACKOFF`].
///
/// `attempt` is 1-based (the first retry is `1`). The delay grows as
/// `base * 2^(attempt-1)`, clamped to the cap, with uniform jitter applied so
/// many forwards reconnecting at once do not synchronize into a thundering herd.
pub fn backoff_delay(base: Duration, attempt: u32) -> Duration {
    backoff_delay_with(base, attempt, MAX_BACKOFF, rand::random::<f64>())
}

/// Testable core of [`backoff_delay`]: `fraction` in `[0, 1]` picks a point in
/// the jitter window `[base, ceiling]`, where `ceiling = min(base*2^(attempt-1), cap)`.
pub fn backoff_delay_with(base: Duration, attempt: u32, cap: Duration, fraction: f64) -> Duration {
    if attempt == 0 {
        return Duration::ZERO;
    }
    let exp = attempt.saturating_sub(1).min(31);
    let scaled = base.saturating_mul(1u32 << exp);
    let ceiling = scaled.min(cap);
    let floor = base.min(ceiling);
    let span = ceiling.saturating_sub(floor);
    floor + span.mul_f64(fraction.clamp(0.0, 1.0))
}
