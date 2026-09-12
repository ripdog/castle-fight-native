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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttackDelivery {
    Melee,
    RangedGuaranteedHit,
}

impl AttackDelivery {
    #[must_use]
    pub const fn stable_tag(self) -> u8 {
        match self {
            Self::Melee => 0,
            Self::RangedGuaranteedHit => 1,
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttackProfile {
    pub delivery: AttackDelivery,
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

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RetaliationState {
    pub attacker: Option<SimId>,
    pub attacked_tick: Option<u64>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovementProfile {
    pub speed_per_tick: i32,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnTick(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitTemplate {
    pub health: i32,
    pub attack: AttackProfile,
    pub movement: MovementProfile,
}

#[derive(Debug, Clone, Copy)]
pub struct UnitSpawn {
    pub team: Team,
    pub position: SimPoint,
    pub health: i32,
    pub attack: AttackProfile,
    pub movement: MovementProfile,
}

impl UnitSpawn {
    #[must_use]
    pub const fn from_template(team: Team, position: SimPoint, template: UnitTemplate) -> Self {
        Self {
            team,
            position,
            health: template.health,
            attack: template.attack,
            movement: template.movement,
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildingFootprint {
    pub min_x: i32,
    pub min_y: i32,
    pub width: u16,
    pub height: u16,
}

impl BuildingFootprint {
    #[must_use]
    pub const fn new(min_x: i32, min_y: i32, width: u16, height: u16) -> Self {
        Self {
            min_x,
            min_y,
            width,
            height,
        }
    }

    #[must_use]
    pub const fn max_x(self) -> i32 {
        self.min_x + self.width as i32 - 1
    }

    #[must_use]
    pub const fn max_y(self) -> i32 {
        self.min_y + self.height as i32 - 1
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionProfile {
    pub initial_delay_ticks: u16,
    pub interval_ticks: u16,
    pub search_radius_cells: u16,
    pub unit: UnitTemplate,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionState {
    pub next_spawn_tick: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct BuildingSpawn {
    pub team: Team,
    pub footprint: BuildingFootprint,
    pub health: i32,
    pub production: Option<ProductionProfile>,
}
