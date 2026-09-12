use castle_fight_sim::{
    AttackDelivery, AttackProfile, BuildingFootprint, BuildingSpawn, MovementProfile, NavCell,
    ProductionProfile, SUBUNITS_PER_WORLD_UNIT, SimPoint, Simulation, SimulationConfig, Team,
    UnitTemplate,
};

use crate::presentation::WorldMetrics;

const SIMULATION_HZ_I32: i32 = 30;
const NAV_CELL_WORLD: i32 = 10;
const NAV_CELL_SUBUNITS: i32 = NAV_CELL_WORLD * SUBUNITS_PER_WORLD_UNIT;
const NAV_MAX_X: i32 = 199;
const NAV_MAX_Y: i32 = 74;
const MIDDLE_MIN_X: i32 = 67;
const MIDDLE_MAX_X: i32 = 132;
const LANE_MIN_Y: i32 = 20;
const LANE_MAX_Y: i32 = 54;
const CASTLE_HEALTH: i32 = 20_000;
const PRODUCTION_HEALTH: i32 = 1_000;
const PRODUCTION_INTERVAL_TICKS: u16 = 120;
const ATTACK_COOLDOWN_TICKS: u16 = 30;
const PROJECTILE_SPEED_WORLD_PER_SECOND: i32 = 300;

const PLAYER_CASTLE: BuildingFootprint = BuildingFootprint::new(30, 34, 7, 7);
const ENEMY_CASTLE: BuildingFootprint = BuildingFootprint::new(163, 34, 7, 7);

pub struct DemoWorld {
    pub simulation: Simulation,
    pub metrics: WorldMetrics,
}

#[must_use]
pub fn create_demo_world(workers: usize) -> DemoWorld {
    let config = demo_config();
    let metrics = WorldMetrics::from_simulation_config(&config);
    let mut simulation = Simulation::new(config, workers);

    simulation.spawn_building(passive_structure(Team(0), PLAYER_CASTLE, CASTLE_HEALTH));
    simulation.spawn_building(passive_structure(Team(1), ENEMY_CASTLE, CASTLE_HEALTH));

    for (team, melee, ranged) in [
        (
            Team(0),
            BuildingFootprint::new(49, 27, 4, 4),
            BuildingFootprint::new(49, 44, 4, 4),
        ),
        (
            Team(1),
            BuildingFootprint::new(147, 27, 4, 4),
            BuildingFootprint::new(147, 44, 4, 4),
        ),
    ] {
        simulation.spawn_building(production_structure(team, melee, BuildKind::Melee));
        simulation.spawn_building(production_structure(team, ranged, BuildKind::Ranged));
    }

    DemoWorld {
        simulation,
        metrics,
    }
}

fn demo_config() -> SimulationConfig {
    SimulationConfig {
        match_seed: 0x4341_5354_4c45,
        spatial_cell_size: 40 * SUBUNITS_PER_WORLD_UNIT,
        navigation_cell_size: NAV_CELL_SUBUNITS,
        navigation_min: NavCell::new(0, 0),
        navigation_max: NavCell::new(NAV_MAX_X, NAV_MAX_Y),
        target_pursuit_extra_range: 30 * SUBUNITS_PER_WORLD_UNIT,
        unit_separation_distance: 8 * SUBUNITS_PER_WORLD_UNIT,
        max_separation_per_tick: SUBUNITS_PER_WORLD_UNIT,
        static_blockers: vec![
            BuildingFootprint::new(
                MIDDLE_MIN_X,
                0,
                (MIDDLE_MAX_X - MIDDLE_MIN_X + 1) as u16,
                LANE_MIN_Y as u16,
            ),
            BuildingFootprint::new(
                MIDDLE_MIN_X,
                LANE_MAX_Y + 1,
                (MIDDLE_MAX_X - MIDDLE_MIN_X + 1) as u16,
                (NAV_MAX_Y - LANE_MAX_Y) as u16,
            ),
        ],
        team_objective: [cell_center(162, 37), cell_center(37, 37)],
    }
}

fn passive_structure(team: Team, footprint: BuildingFootprint, health: i32) -> BuildingSpawn {
    BuildingSpawn {
        team,
        footprint,
        health,
        production: None,
        attack: None,
        spellcasting: None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildKind {
    Melee,
    Ranged,
}

impl BuildKind {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Melee => "Melee Hall",
            Self::Ranged => "Ranged Hall",
        }
    }
}

pub(crate) fn production_structure(
    team: Team,
    footprint: BuildingFootprint,
    kind: BuildKind,
) -> BuildingSpawn {
    BuildingSpawn {
        team,
        footprint,
        health: PRODUCTION_HEALTH,
        production: Some(ProductionProfile {
            initial_delay_ticks: 15,
            interval_ticks: PRODUCTION_INTERVAL_TICKS,
            search_radius_cells: 12,
            unit: unit_template(kind),
        }),
        attack: None,
        spellcasting: None,
    }
}

fn unit_template(kind: BuildKind) -> UnitTemplate {
    let movement = MovementProfile {
        speed_per_tick: 40 * SUBUNITS_PER_WORLD_UNIT / SIMULATION_HZ_I32,
    };
    match kind {
        BuildKind::Melee => UnitTemplate {
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 12,
                range: 14 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 80 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: ATTACK_COOLDOWN_TICKS,
            },
            movement,
        },
        BuildKind::Ranged => UnitTemplate {
            health: 80,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: PROJECTILE_SPEED_WORLD_PER_SECOND * SUBUNITS_PER_WORLD_UNIT
                        / SIMULATION_HZ_I32,
                },
                damage: 9,
                range: 120 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 180 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: ATTACK_COOLDOWN_TICKS,
            },
            movement,
        },
    }
}

fn cell_center(x: i32, y: i32) -> SimPoint {
    SimPoint::new(
        x * NAV_CELL_SUBUNITS + NAV_CELL_SUBUNITS / 2,
        y * NAV_CELL_SUBUNITS + NAV_CELL_SUBUNITS / 2,
    )
}
