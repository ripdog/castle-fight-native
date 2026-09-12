use bevy_ecs::prelude::Component;

use crate::math::SimPoint;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimId(pub u64);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Team(pub u8);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position(pub SimPoint);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Health {
    pub current: i32,
    pub max: i32,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackProfile {
    pub damage: i32,
    pub range: i32,
    pub acquisition_range: i32,
    pub cooldown_ticks: u16,
}

impl AttackProfile {
    #[must_use]
    pub fn range_sq(self) -> u64 {
        let range = i64::from(self.range);
        (range * range) as u64
    }

    #[must_use]
    pub fn acquisition_range_sq(self) -> u64 {
        let range = i64::from(self.acquisition_range);
        (range * range) as u64
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AttackCooldown {
    pub remaining: u16,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TargetState {
    pub current: Option<SimId>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovementProfile {
    pub speed_per_tick: i32,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnTick(pub u64);

#[derive(Debug, Clone, Copy)]
pub struct UnitSpawn {
    pub team: Team,
    pub position: SimPoint,
    pub health: i32,
    pub attack: AttackProfile,
    pub movement: MovementProfile,
}
