use serde::{Deserialize, Serialize};

pub const SUBUNITS_PER_WORLD_UNIT: i32 = 1024;

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct SimPoint {
    pub x: i32,
    pub y: i32,
}

impl SimPoint {
    pub const fn new(x: i32, y: i32) -> Self {
        Self { x, y }
    }

    #[must_use]
    pub fn distance_sq(self, other: Self) -> u64 {
        let dx = u128::from((i64::from(self.x) - i64::from(other.x)).unsigned_abs());
        let dy = u128::from((i64::from(self.y) - i64::from(other.y)).unsigned_abs());
        let squared = dx * dx + dy * dy;
        u64::try_from(squared).unwrap_or(u64::MAX)
    }

    #[must_use]
    pub fn step_towards(self, target: Self, distance: i32) -> Self {
        debug_assert!(distance >= 0);

        let dx = i64::from(target.x) - i64::from(self.x);
        let dy = i64::from(target.y) - i64::from(self.y);
        let scale = dx.abs().max(dy.abs());
        if scale == 0 || scale <= i64::from(distance) {
            return target;
        }

        let distance = i64::from(distance);
        let step_x = dx * distance / scale;
        let step_y = dy * distance / scale;

        Self {
            x: checked_i64_to_i32(i64::from(self.x) + step_x),
            y: checked_i64_to_i32(i64::from(self.y) + step_y),
        }
    }
}

fn checked_i64_to_i32(value: i64) -> i32 {
    i32::try_from(value).expect("simulation coordinate exceeded configured i32 bounds")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn squared_distance_uses_wide_intermediate() {
        let a = SimPoint::new(-1_000_000, 2_000_000);
        let b = SimPoint::new(1_000_000, -2_000_000);
        assert_eq!(a.distance_sq(b), 20_000_000_000_000);
        assert_eq!(
            SimPoint::new(i32::MIN, i32::MIN).distance_sq(SimPoint::new(i32::MAX, i32::MAX)),
            u64::MAX,
            "untrusted coordinates outside validated map bounds saturate instead of overflowing"
        );
    }

    #[test]
    fn step_towards_is_integer_and_deterministic() {
        let start = SimPoint::new(0, 0);
        assert_eq!(
            start.step_towards(SimPoint::new(100, 50), 20),
            SimPoint::new(20, 10)
        );
        assert_eq!(
            start.step_towards(SimPoint::new(-100, 50), 20),
            SimPoint::new(-20, 10)
        );
    }
}

/// Floor division by a fixed positive cell size. Power-of-two sizes divide with an arithmetic
/// shift, which is exactly `div_euclid` for a positive divisor; other sizes use `div_euclid`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CellDivisor {
    size: i32,
    shift: Option<u32>,
}

impl CellDivisor {
    #[must_use]
    pub(crate) const fn new(size: i32) -> Self {
        assert!(size > 0, "cell size must be positive");
        let shift = if size.count_ones() == 1 {
            Some(size.trailing_zeros())
        } else {
            None
        };
        Self { size, shift }
    }

    #[inline]
    #[must_use]
    pub(crate) fn floor_div(self, value: i32) -> i32 {
        let quotient = match self.shift {
            Some(shift) => value >> shift,
            None => value.div_euclid(self.size),
        };
        debug_assert_eq!(quotient, value.div_euclid(self.size));
        quotient
    }

    #[inline]
    #[must_use]
    pub(crate) fn floor_div_wide(self, value: i64) -> i64 {
        let quotient = match self.shift {
            Some(shift) => value >> shift,
            None => value.div_euclid(i64::from(self.size)),
        };
        debug_assert_eq!(quotient, value.div_euclid(i64::from(self.size)));
        quotient
    }
}

#[cfg(test)]
mod cell_divisor_tests {
    use super::*;

    #[test]
    fn shift_path_matches_euclidean_division_for_negative_values() {
        for size in [1, 2, 1024, 1 << 15, 1 << 16, 1 << 18, 1 << 30, 3, 1000] {
            let divisor = CellDivisor::new(size);
            for value in [i32::MIN, -1025, -1, 0, 1, 1023, 1024, 65_537, i32::MAX] {
                assert_eq!(divisor.floor_div(value), value.div_euclid(size));
                assert_eq!(
                    divisor.floor_div_wide(i64::from(value)),
                    i64::from(value).div_euclid(i64::from(size))
                );
            }
        }
    }
}
