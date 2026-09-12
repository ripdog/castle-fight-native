use std::collections::BTreeMap;

use bevy::prelude::Resource;
use castle_fight_sim::{
    AttackDelivery, BuildingFootprint, ProjectileView, SimId, SimPoint, Simulation, Team,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitVisualKind {
    Melee,
    Ranged,
    Ballistic,
    Bounce,
}

impl UnitVisualKind {
    fn from_delivery(delivery: AttackDelivery) -> Self {
        match delivery {
            AttackDelivery::Melee => Self::Melee,
            AttackDelivery::RangedGuaranteedHit { .. } => Self::Ranged,
            AttackDelivery::RangedBallistic { .. } => Self::Ballistic,
            AttackDelivery::Bounce { .. } => Self::Bounce,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingVisualKind {
    Structure,
    Production,
    Attack,
    Spellcaster,
}

#[derive(Debug, Clone, Copy)]
pub struct UnitSample {
    pub id: SimId,
    pub team: Team,
    pub position: SimPoint,
    pub collision_radius: i32,
    pub health: i32,
    pub target: Option<SimId>,
    pub cooldown_remaining: u16,
    pub stunned_until_tick: u64,
    pub visual_kind: UnitVisualKind,
}

#[derive(Debug, Clone, Copy)]
pub struct BuildingSample {
    pub id: SimId,
    pub team: Team,
    pub footprint: BuildingFootprint,
    pub health: i32,
    pub target: Option<SimId>,
    pub next_spawn_tick: Option<u64>,
    pub cooldown_remaining: Option<u16>,
    pub mana_current: Option<i32>,
    pub mana_maximum: Option<i32>,
    pub ability_ready_tick: Option<u64>,
    pub stunned_until_tick: Option<u64>,
    pub visual_kind: BuildingVisualKind,
}

#[derive(Debug, Clone)]
pub struct PresentationSnapshot {
    pub tick: u64,
    pub units: BTreeMap<SimId, UnitSample>,
    pub buildings: BTreeMap<SimId, BuildingSample>,
    pub projectiles: BTreeMap<SimId, ProjectileView>,
}

impl PresentationSnapshot {
    #[must_use]
    pub fn capture(simulation: &Simulation) -> Self {
        let units = simulation
            .units()
            .into_iter()
            .map(|unit| {
                (
                    unit.id,
                    UnitSample {
                        id: unit.id,
                        team: unit.team,
                        position: unit.position,
                        collision_radius: unit.collision_radius,
                        health: unit.health,
                        target: unit.target,
                        cooldown_remaining: unit.cooldown_remaining,
                        stunned_until_tick: unit.stunned_until_tick,
                        visual_kind: UnitVisualKind::from_delivery(unit.attack_delivery),
                    },
                )
            })
            .collect();
        let buildings = simulation
            .buildings()
            .into_iter()
            .map(|building| {
                let visual_kind = if building.production.is_some() {
                    BuildingVisualKind::Production
                } else if building.attack_delivery.is_some() {
                    BuildingVisualKind::Attack
                } else if building.mana_maximum.is_some() {
                    BuildingVisualKind::Spellcaster
                } else {
                    BuildingVisualKind::Structure
                };
                (
                    building.id,
                    BuildingSample {
                        id: building.id,
                        team: building.team,
                        footprint: building.footprint,
                        health: building.health,
                        target: building.target,
                        next_spawn_tick: building.next_spawn_tick,
                        cooldown_remaining: building.cooldown_remaining,
                        mana_current: building.mana_current,
                        mana_maximum: building.mana_maximum,
                        ability_ready_tick: building.ability_ready_tick,
                        stunned_until_tick: building.stunned_until_tick,
                        visual_kind,
                    },
                )
            })
            .collect();
        let projectiles = simulation
            .projectiles()
            .into_iter()
            .map(|projectile| (projectile.id, projectile))
            .collect();

        Self {
            tick: simulation.tick(),
            units,
            buildings,
            projectiles,
        }
    }
}

#[derive(Resource, Debug, Clone)]
pub struct PresentationSamples {
    pub previous: PresentationSnapshot,
    pub current: PresentationSnapshot,
}

impl PresentationSamples {
    #[must_use]
    pub fn new(initial: PresentationSnapshot) -> Self {
        Self {
            previous: initial.clone(),
            current: initial,
        }
    }

    pub fn publish(&mut self, next: PresentationSnapshot) {
        self.previous = std::mem::replace(&mut self.current, next);
    }
}

#[cfg(test)]
mod tests {
    use castle_fight_sim::{
        AttackProfile, MovementProfile, SUBUNITS_PER_WORLD_UNIT, SimulationConfig, UnitSpawn,
    };

    use super::*;

    #[test]
    fn repeated_presentation_capture_does_not_change_authoritative_checksums() {
        fn fixture() -> Simulation {
            let mut simulation = Simulation::new(SimulationConfig::default(), 1);
            simulation.spawn_unit(UnitSpawn {
                team: Team(0),
                position: SimPoint::new(2 * SUBUNITS_PER_WORLD_UNIT, 0),
                health: 100,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 1,
                    range: SUBUNITS_PER_WORLD_UNIT,
                    acquisition_range: 4 * SUBUNITS_PER_WORLD_UNIT,
                    cooldown_ticks: 30,
                },
                movement: MovementProfile { speed_per_tick: 1 },
            });
            simulation
        }

        let mut headless = fixture();
        let mut presented = fixture();
        for _ in 0..120 {
            headless.step();
            presented.step();
            let snapshot = PresentationSnapshot::capture(&presented);

            assert_eq!(snapshot.tick, presented.tick());
            assert_eq!(snapshot.units.len(), 1);
            assert_eq!(headless.checksum(), presented.checksum());
        }
    }
}
