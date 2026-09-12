pub const SUBUNITS_PER_WORLD_UNIT: i32 = 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
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
        let dx = i64::from(self.x) - i64::from(other.x);
        let dy = i64::from(self.y) - i64::from(other.y);
        (dx * dx + dy * dy) as u64
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
