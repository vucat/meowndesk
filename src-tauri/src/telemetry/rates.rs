//! Counter delta accounting for rates; resets, wraps and suspend discontinuities stay explicit.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RateOutcome {
    /// First observation; no delta is exposed until a second sample exists.
    Warmup,
    Sample {
        delta: u64,
        interval_ms: u64,
    },
    CounterReset,
    Discontinuity,
}

impl RateOutcome {
    pub fn per_second(self) -> Option<f64> {
        match self {
            Self::Sample { delta, interval_ms } if interval_ms > 0 => {
                Some(delta as f64 * 1000.0 / interval_ms as f64)
            }
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct CounterBaseline {
    last_value: Option<u64>,
    last_elapsed_ms: Option<u64>,
}

impl CounterBaseline {
    pub fn new() -> Self {
        Self::default()
    }

    /// Observes a monotonic counter. A poll gap beyond twice the configured interval is a
    /// discontinuity: rates are discarded and a fresh baseline is established.
    pub fn observe(
        &mut self,
        value: u64,
        elapsed_ms: u64,
        configured_interval_ms: u64,
    ) -> RateOutcome {
        let outcome = match (self.last_value, self.last_elapsed_ms) {
            (Some(previous_value), Some(previous_elapsed)) => {
                if elapsed_ms < previous_elapsed {
                    RateOutcome::Discontinuity
                } else {
                    let interval_ms = elapsed_ms - previous_elapsed;
                    if interval_ms > configured_interval_ms.saturating_mul(2) {
                        RateOutcome::Discontinuity
                    } else if value < previous_value {
                        RateOutcome::CounterReset
                    } else {
                        RateOutcome::Sample {
                            delta: value - previous_value,
                            interval_ms,
                        }
                    }
                }
            }
            _ => RateOutcome::Warmup,
        };
        self.last_value = Some(value);
        self.last_elapsed_ms = Some(elapsed_ms);
        outcome
    }

    pub fn reset(&mut self) {
        self.last_value = None;
        self.last_elapsed_ms = None;
    }

    pub fn has_baseline(&self) -> bool {
        self.last_value.is_some()
    }
}

/// Accumulated-energy wrap for RAPL-style counters, including a zero-range guard.
pub fn energy_delta(previous_uj: u64, current_uj: u64, max_energy_range_uj: u64) -> Option<u64> {
    if max_energy_range_uj == 0
        || previous_uj > max_energy_range_uj
        || current_uj > max_energy_range_uj
    {
        return None;
    }
    if current_uj >= previous_uj {
        return Some(current_uj - previous_uj);
    }
    Some(
        max_energy_range_uj
            .saturating_sub(previous_uj)
            .saturating_add(current_uj),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deltas_are_nonnegative_and_rates_use_elapsed_time() {
        let mut baseline = CounterBaseline::new();
        assert_eq!(baseline.observe(1000, 0, 1000), RateOutcome::Warmup);
        let sample = baseline.observe(4000, 2000, 1000);
        assert_eq!(
            sample,
            RateOutcome::Sample {
                delta: 3000,
                interval_ms: 2000
            }
        );
        assert_eq!(sample.per_second(), Some(1500.0));
    }

    #[test]
    fn counter_reset_establishes_a_new_baseline() {
        let mut baseline = CounterBaseline::new();
        baseline.observe(9000, 0, 1000);
        assert_eq!(baseline.observe(10, 1000, 1000), RateOutcome::CounterReset);
        assert_eq!(
            baseline.observe(510, 2000, 1000),
            RateOutcome::Sample {
                delta: 500,
                interval_ms: 1000
            }
        );
    }

    #[test]
    fn suspend_gap_beyond_two_intervals_is_a_discontinuity() {
        let mut baseline = CounterBaseline::new();
        baseline.observe(100, 0, 1000);
        assert_eq!(
            baseline.observe(200, 2001, 1000),
            RateOutcome::Discontinuity
        );
        assert_eq!(
            baseline.observe(300, 3000, 1000),
            RateOutcome::Sample {
                delta: 100,
                interval_ms: 999
            }
        );
        assert_eq!(
            baseline.observe(400, 2000, 1000),
            RateOutcome::Discontinuity
        );
    }

    #[test]
    fn wrapping_energy_meters_use_the_documented_range() {
        assert_eq!(energy_delta(100, 500, 10_000), Some(400));
        assert_eq!(energy_delta(9_900, 200, 10_000), Some(300));
        assert_eq!(energy_delta(9_900, 200, 0), None);
        assert_eq!(energy_delta(0, 10_001, 10_000), None);
    }
}
