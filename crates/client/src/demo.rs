use castle_fight_sim::{
    AbilityEffect, AbilityId, AbilityTargetPolicy, AttackDelivery, AttackProfile,
    AutomaticAbilityProfile, BuildingFootprint, BuildingPlacementError, BuildingSpawn, CombatRules,
    CorpseDefinitionId, CorpseProfile, ManaProfile, MovementProfile, NavCell, ProductionProfile,
    SUBUNITS_PER_WORLD_UNIT, SimId, SimPoint, Simulation, SimulationConfig, SpellcastingProfile,
    Team, TerrainElevationMap, UnitSpawn, UnitTemplate,
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
const ARTILLERY_PROJECTILE_SPEED_WORLD_PER_SECOND: i32 = 90;
const TOWER_PROJECTILE_SPEED_WORLD_PER_SECOND: i32 = 110;
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
                production_structure(team, melee, ProductionKind::Melee),
                demo_corpse_profile(),
            );
            simulation.spawn_building_with_production_corpse(
                production_structure(team, ranged, ProductionKind::Ranged),
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

    let navigation_width = navigation_max.x - navigation_min.x + 1;
    assert_eq!(navigation_width % 3, 0);
    let build_region_width = navigation_width / 3;
    let navigation_height = navigation_max.y - navigation_min.y + 1;
    let left_build_region = BuildingFootprint::new(
        navigation_min.x,
        navigation_min.y,
        build_region_width as u16,
        navigation_height as u16,
    );
    let right_build_region = BuildingFootprint::new(
        navigation_max.x - build_region_width + 1,
        navigation_min.y,
        build_region_width as u16,
        navigation_height as u16,
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
        team_build_regions: [vec![left_build_region], vec![right_build_region]],
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
pub(crate) enum ProductionKind {
    Melee,
    Ranged,
    Artillery,
    Spellcaster,
}

impl ProductionKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Melee => "Melee Hall",
            Self::Ranged => "Ranged Hall",
            Self::Artillery => "Artillery Foundry",
            Self::Spellcaster => "Spellcaster Hall",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildKind {
    Production(ProductionKind),
    GuaranteedTower,
    ProjectileTower,
    GlobalAreaSpell,
}

impl BuildKind {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Production(kind) => kind.label(),
            Self::GuaranteedTower => "Hit Tower",
            Self::ProjectileTower => "Splash Tower",
            Self::GlobalAreaSpell => "Global AOE Shrine",
        }
    }

    pub(crate) const fn footprint_size(self) -> u16 {
        match self {
            Self::GuaranteedTower | Self::ProjectileTower => 3,
            Self::Production(_) | Self::GlobalAreaSpell => 4,
        }
    }
}

pub(crate) const fn demo_corpse_profile() -> CorpseProfile {
    CorpseProfile {
        definition: CorpseDefinitionId(1),
        lifetime_ticks: Some(DEMO_CORPSE_LIFETIME_TICKS),
    }
}

pub(crate) fn try_spawn_demo_building(
    simulation: &mut Simulation,
    team: Team,
    footprint: BuildingFootprint,
    kind: BuildKind,
) -> Result<SimId, BuildingPlacementError> {
    match kind {
        BuildKind::Production(ProductionKind::Spellcaster) => simulation
            .try_spawn_building_with_production_spellcasting(
                production_structure(team, footprint, ProductionKind::Spellcaster),
                short_range_spellcaster_profile(),
            ),
        BuildKind::Production(kind) => simulation.try_spawn_building_with_production_corpse(
            production_structure(team, footprint, kind),
            demo_corpse_profile(),
        ),
        BuildKind::GuaranteedTower => simulation.try_spawn_building(attack_structure(
            team,
            footprint,
            guaranteed_tower_attack(),
        )),
        BuildKind::ProjectileTower => simulation.try_spawn_building(attack_structure(
            team,
            footprint,
            projectile_tower_attack(),
        )),
        BuildKind::GlobalAreaSpell => simulation.try_spawn_building(spell_structure(
            team,
            footprint,
            global_area_spell_profile(),
        )),
    }
}

pub(crate) fn production_structure(
    team: Team,
    footprint: BuildingFootprint,
    kind: ProductionKind,
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

fn attack_structure(
    team: Team,
    footprint: BuildingFootprint,
    attack: AttackProfile,
) -> BuildingSpawn {
    BuildingSpawn {
        team,
        footprint,
        health: 1_600,
        production: None,
        attack: Some(attack),
        spellcasting: None,
    }
}

fn spell_structure(
    team: Team,
    footprint: BuildingFootprint,
    spellcasting: SpellcastingProfile,
) -> BuildingSpawn {
    BuildingSpawn {
        team,
        footprint,
        health: 1_400,
        production: None,
        attack: None,
        spellcasting: Some(spellcasting),
    }
}

fn unit_template(kind: ProductionKind) -> UnitTemplate {
    let movement = MovementProfile {
        speed_per_tick: 40 * SUBUNITS_PER_WORLD_UNIT / SIMULATION_HZ_I32,
    };
    match kind {
        ProductionKind::Melee => UnitTemplate {
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
        ProductionKind::Ranged => UnitTemplate {
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
        ProductionKind::Artillery => UnitTemplate {
            health: 70,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedBallistic {
                    speed_per_tick: ARTILLERY_PROJECTILE_SPEED_WORLD_PER_SECOND
                        * SUBUNITS_PER_WORLD_UNIT
                        / SIMULATION_HZ_I32,
                    impact_radius: 35 * SUBUNITS_PER_WORLD_UNIT,
                },
                damage: 18,
                range: 260 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 340 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 2 * ATTACK_COOLDOWN_TICKS,
            },
            movement,
        },
        ProductionKind::Spellcaster => UnitTemplate {
            health: 80,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 9,
                range: 14 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 80 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: ATTACK_COOLDOWN_TICKS,
            },
            movement,
        },
    }
}

fn guaranteed_tower_attack() -> AttackProfile {
    AttackProfile {
        delivery: AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: PROJECTILE_SPEED_WORLD_PER_SECOND * SUBUNITS_PER_WORLD_UNIT
                / SIMULATION_HZ_I32,
        },
        damage: 18,
        range: 300 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 300 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: ATTACK_COOLDOWN_TICKS,
    }
}

fn projectile_tower_attack() -> AttackProfile {
    AttackProfile {
        delivery: AttackDelivery::RangedBallistic {
            speed_per_tick: TOWER_PROJECTILE_SPEED_WORLD_PER_SECOND * SUBUNITS_PER_WORLD_UNIT
                / SIMULATION_HZ_I32,
            impact_radius: 40 * SUBUNITS_PER_WORLD_UNIT,
        },
        damage: 30,
        range: 340 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 340 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 2 * ATTACK_COOLDOWN_TICKS,
    }
}

fn global_area_spell_profile() -> SpellcastingProfile {
    SpellcastingProfile {
        mana: ManaProfile {
            maximum: 180,
            starting: 0,
            regen_per_tick: 1,
        },
        ability: AutomaticAbilityProfile {
            id: AbilityId(1_001),
            mana_cost: 180,
            cooldown_ticks: 1,
            range: 0,
            target_policy: AbilityTargetPolicy::RandomEnemyUnitGlobal,
            effect: AbilityEffect::AreaDamage {
                amount: 30,
                radius: 50 * SUBUNITS_PER_WORLD_UNIT,
            },
        },
    }
}

fn short_range_spellcaster_profile() -> SpellcastingProfile {
    SpellcastingProfile {
        mana: ManaProfile {
            maximum: 120,
            starting: 0,
            regen_per_tick: 1,
        },
        ability: AutomaticAbilityProfile {
            id: AbilityId(1_002),
            mana_cost: 120,
            cooldown_ticks: 1,
            range: 90 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::RandomEnemyUnit,
            effect: AbilityEffect::AreaDamage {
                amount: 18,
                radius: 30 * SUBUNITS_PER_WORLD_UNIT,
            },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_exposes_every_verification_build_kind() {
        let DemoWorld { mut simulation, .. } = create_demo_world(1, None);
        let kinds = [
            BuildKind::Production(ProductionKind::Melee),
            BuildKind::Production(ProductionKind::Ranged),
            BuildKind::Production(ProductionKind::Artillery),
            BuildKind::Production(ProductionKind::Spellcaster),
            BuildKind::GuaranteedTower,
            BuildKind::ProjectileTower,
            BuildKind::GlobalAreaSpell,
        ];

        for (index, kind) in kinds.into_iter().enumerate() {
            let footprint = BuildingFootprint::new(
                -240 + index as i32 * 6,
                -100,
                kind.footprint_size(),
                kind.footprint_size(),
            );
            assert!(
                try_spawn_demo_building(&mut simulation, Team(0), footprint, kind).is_ok(),
                "failed to spawn {}",
                kind.label()
            );
        }
    }

    #[test]
    fn original_map_middle_third_is_not_buildable() {
        let DemoWorld { simulation, .. } = create_demo_world(1, None);
        let middle = BuildingFootprint::new(0, 0, 4, 4);
        let left = BuildingFootprint::new(-220, 0, 4, 4);
        let right = BuildingFootprint::new(200, 0, 4, 4);

        assert!(!simulation.can_place_building_for_team(Team(0), middle));
        assert!(!simulation.can_place_building_for_team(Team(1), middle));
        assert!(simulation.can_place_building_for_team(Team(0), left));
        assert!(simulation.can_place_building_for_team(Team(1), right));
    }
}
