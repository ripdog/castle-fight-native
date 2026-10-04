//! Exact mana rates without changing the legacy profile/state wire layout.
use super::ManaProfile;

/// The rate unit is explicit at the API boundary. Legacy struct literals remain per-tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ManaRegeneration {
    PerTickPer10k(u32),
    PerSecondPer10k(u32),
}

const PER_SECOND_TAG: u32 = 1 << 31;

impl ManaProfile {
    /// Construct an exact per-second profile. Do not divide the rate by simulation Hz.
    #[must_use]
    pub const fn per_second(maximum: i32, starting: i32, rate_per_10k: u32) -> Self {
        assert!(rate_per_10k < PER_SECOND_TAG);
        Self {
            maximum,
            starting,
            regen_per_tick_per_10k: PER_SECOND_TAG | rate_per_10k,
        }
    }

    #[must_use]
    pub const fn regeneration(self) -> ManaRegeneration {
        if self.regen_per_tick_per_10k & PER_SECOND_TAG == 0 {
            ManaRegeneration::PerTickPer10k(self.regen_per_tick_per_10k)
        } else {
            ManaRegeneration::PerSecondPer10k(self.regen_per_tick_per_10k & !PER_SECOND_TAG)
        }
    }

    /// Difference of the cumulative rational clock, avoiding both rounding drift and an
    /// additional mutable denominator remainder. `tick` is zero-based and authoritative.
    #[must_use]
    pub fn regeneration_at_tick(self, tick: u64, hz: u32) -> u32 {
        match self.regeneration() {
            ManaRegeneration::PerTickPer10k(rate) => rate,
            ManaRegeneration::PerSecondPer10k(rate) => per_second_increment(rate, tick, hz),
        }
    }
}

pub(crate) fn per_second_increment(rate: u32, tick: u64, hz: u32) -> u32 {
    assert!(hz > 0);
    let phase = tick % u64::from(hz);
    let rate = u64::from(rate);
    let hz = u64::from(hz);
    u32::try_from(((phase + 1) * rate / hz) - (phase * rate / hz))
        .expect("one tick cannot exceed the per-second rate")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rational_clock_is_exact_for_fractional_rates_and_arbitrary_phase() {
        for rate in [10_000, 35_000, 1, 999_999] {
            let profile = ManaProfile::per_second(100, 0, rate);
            for start in 0..30 {
                assert_eq!(
                    (start..start + 300)
                        .map(|t| profile.regeneration_at_tick(t, 30))
                        .sum::<u32>(),
                    rate * 10
                );
            }
        }
        let legacy = ManaProfile {
            maximum: 10,
            starting: 0,
            regen_per_tick_per_10k: 333,
        };
        assert_eq!(legacy.regeneration_at_tick(29, 30), 333);
        assert_eq!(legacy.regeneration(), ManaRegeneration::PerTickPer10k(333));
    }
}
