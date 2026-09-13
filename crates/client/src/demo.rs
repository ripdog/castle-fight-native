use castle_fight_sim::{
    AttackDelivery, AttackProfile, BuildingFootprint, BuildingSpawn, CombatRules,
    CorpseDefinitionId, CorpseProfile, MovementProfile, NavCell, ProductionProfile,
    SUBUNITS_PER_WORLD_UNIT, SimPoint, Simulation, SimulationConfig, Team, TerrainElevationMap,
    UnitSpawn, UnitTemplate,
};

use crate::presentation::WorldMetrics;

const SIMULATION_HZ_I32: i32 = 30;
const NAV_CELL_WORLD: i32 = 32;
const NAV_CELL_SUBUNITS: i32 = NAV_CELL_WORLD * SUBUNITS_PER_WORLD_UNIT;
const MIDDLE_MIN_X: i32 = -128;
const MIDDLE_MAX_X: i32 = 127;
const LANE_MIN_Y: i32 = -24;
const LANE_MAX_Y: i32 = 23;
const CASTLE_HEALTH: i32 = 20_000;
const PRODUCTION_HEALTH: i32 = 1_000;
const PRODUCTION_INTERVAL_TICKS: u16 = 120;
const ATTACK_COOLDOWN_TICKS: u16 = 30;
const PROJECTILE_SPEED_WORLD_PER_SECOND: i32 = 300;
const DEMO_CORPSE_LIFETIME_TICKS: u32 = 300;
// Visual-verification value only; the exact original Castle Fight uphill miss chance is still
// compatibility data to recover.
const DEMO_UPHILL_MISS_CHANCE_PER_10K: u16 = 2_500;

const PLAYER_CASTLE: BuildingFootprint = BuildingFootprint::new(-191, -4, 7, 7);
const ENEMY_CASTLE: BuildingFootprint = BuildingFootprint::new(184, -4, 7, 7);

pub struct DemoWorld {
    pub simulation: Simulation,
    pub metrics: WorldMetrics,
    pub terrain: TerrainElevationMap,
}

#[must_use]
pub fn create_demo_world(workers: usize, stress_units: Option<usize>) -> DemoWorld {
    let terrain = original_terrain();
    let config = demo_config(&terrain);
    let metrics = WorldMetrics::from_simulation_config(&config);
    let combat_rules = CombatRules {
        terrain_elevation: Some(terrain.clone()),
        uphill_miss_chance_per_10k: DEMO_UPHILL_MISS_CHANCE_PER_10K,
    };
    let mut simulation = Simulation::new_with_combat_rules(config, workers, combat_rules);

    simulation.spawn_building(passive_structure(Team(0), PLAYER_CASTLE, CASTLE_HEALTH));
    simulation.spawn_building(passive_structure(Team(1), ENEMY_CASTLE, CASTLE_HEALTH));

    if let Some(unit_count) = stress_units {
        populate_render_stress_units(&mut simulation, unit_count);
    } else {
        for (team, melee, ranged) in [
            (
                Team(0),
                BuildingFootprint::new(-152, -24, 4, 4),
                BuildingFootprint::new(-152, 20, 4, 4),
            ),
            (
                Team(1),
                BuildingFootprint::new(148, -24, 4, 4),
                BuildingFootprint::new(148, 20, 4, 4),
            ),
        ] {
            simulation.spawn_building_with_production_corpse(
                production_structure(team, melee, BuildKind::Melee),
                demo_corpse_profile(),
            );
            simulation.spawn_building_with_production_corpse(
                production_structure(team, ranged, BuildKind::Ranged),
                demo_corpse_profile(),
            );
        }
    }

    DemoWorld {
        simulation,
        metrics,
        terrain,
    }
}

fn populate_render_stress_units(simulation: &mut Simulation, unit_count: usize) {
    const COLUMNS: usize = 120;
    const SPACING_WORLD: i32 = 10;
    const START_X_WORLD: i32 = -600;
    const START_Y_WORLD: i32 = -300;

    for index in 0..unit_count {
        let column = index % COLUMNS;
        let row = index / COLUMNS;
        let position = SimPoint::new(
            (START_X_WORLD + column as i32 * SPACING_WORLD) * SUBUNITS_PER_WORLD_UNIT,
            (START_Y_WORLD + row as i32 * SPACING_WORLD) * SUBUNITS_PER_WORLD_UNIT,
        );
        let delivery = if index % 2 == 0 {
            AttackDelivery::Melee
        } else {
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: PROJECTILE_SPEED_WORLD_PER_SECOND * SUBUNITS_PER_WORLD_UNIT
                    / SIMULATION_HZ_I32,
            }
        };
        simulation.spawn_unit(UnitSpawn {
            team: Team((index & 1) as u8),
            position,
            health: 100,
            attack: AttackProfile {
                delivery,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
    }
}

fn demo_config(terrain: &TerrainElevationMap) -> SimulationConfig {
    let origin = terrain.origin();
    let maximum = terrain.max_point();
    assert_eq!(origin.x.rem_euclid(NAV_CELL_SUBUNITS), 0);
    assert_eq!(origin.y.rem_euclid(NAV_CELL_SUBUNITS), 0);
    assert_eq!(maximum.x.rem_euclid(NAV_CELL_SUBUNITS), 0);
    assert_eq!(maximum.y.rem_euclid(NAV_CELL_SUBUNITS), 0);
    let navigation_min = NavCell::new(
        origin.x.div_euclid(NAV_CELL_SUBUNITS),
        origin.y.div_euclid(NAV_CELL_SUBUNITS),
    );
    let navigation_max = NavCell::new(
        maximum.x.div_euclid(NAV_CELL_SUBUNITS) - 1,
        maximum.y.div_euclid(NAV_CELL_SUBUNITS) - 1,
    );

    SimulationConfig {
        match_seed: 0x4341_5354_4c45,
        spatial_cell_size: 40 * SUBUNITS_PER_WORLD_UNIT,
        navigation_cell_size: NAV_CELL_SUBUNITS,
        navigation_min,
        navigation_max,
        target_pursuit_extra_range: 30 * SUBUNITS_PER_WORLD_UNIT,
        unit_separation_distance: 8 * SUBUNITS_PER_WORLD_UNIT,
        max_separation_per_tick: SUBUNITS_PER_WORLD_UNIT,
        static_blockers: vec![
            BuildingFootprint::new(
                MIDDLE_MIN_X,
                navigation_min.y,
                (MIDDLE_MAX_X - MIDDLE_MIN_X + 1) as u16,
                (LANE_MIN_Y - navigation_min.y) as u16,
            ),
            BuildingFootprint::new(
                MIDDLE_MIN_X,
                LANE_MAX_Y + 1,
                (MIDDLE_MAX_X - MIDDLE_MIN_X + 1) as u16,
                (navigation_max.y - LANE_MAX_Y) as u16,
            ),
        ],
        team_objective: [world_point(6_000, 0), world_point(-6_000, 0)],
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

pub(crate) const fn demo_corpse_profile() -> CorpseProfile {
    CorpseProfile {
        definition: CorpseDefinitionId(1),
        lifetime_ticks: Some(DEMO_CORPSE_LIFETIME_TICKS),
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

fn original_terrain() -> TerrainElevationMap {
    TerrainElevationMap::from_wc3_terrain_json(include_str!(
        "../../../docs/original_map/extracted/terrain.json"
    ))
    .expect("committed original terrain must remain loadable")
}

fn world_point(x: i32, y: i32) -> SimPoint {
    SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, y * SUBUNITS_PER_WORLD_UNIT)
}
