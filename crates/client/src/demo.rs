use castle_fight_sim::{
    AbilityEffect, AbilityId, AbilityTargetPolicy, AttackDelivery, AttackProfile, AttackTargetMask,
    AutomaticAbilityProfile, BuildingFootprint, BuildingGameplayProperties, BuildingPlacementError,
    BuildingSpawn, CollisionRadius, CombatRules, CorpseDefinitionId, CorpseProfile, ManaProfile,
    MovementClass, MovementProfile, NavCell, ProductionProfile, SUBUNITS_PER_WORLD_UNIT, SimId,
    SimPoint, Simulation, SimulationConfig, SpellcastingProfile, Team, TerrainElevationMap,
    UnitGameplayProperties, UnitSpawn, UnitTemplate,
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
const FOOTMAN_COLLISION_WORLD: i32 = 16;
const RANGER_COLLISION_WORLD: i32 = 16;
const CATAPULT_COLLISION_WORLD: i32 = 16;
const ICE_TROLL_PRIEST_COLLISION_WORLD: i32 = 16;
const GRYPHON_COLLISION_WORLD: i32 = 16;
const TICKS_PER_SECOND: u16 = SIMULATION_HZ_I32 as u16;
const ATTACK_COOLDOWN_TICKS: u16 = TICKS_PER_SECOND;
const PROJECTILE_SPEED_WORLD_PER_SECOND: i32 = 300;
const TOWER_PROJECTILE_SPEED_WORLD_PER_SECOND: i32 = 110;
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
        for team in [Team(0), Team(1)] {
            let x = if team.0 == 0 { -152 } else { 148 };
            for (y, kind) in [
                (-24, ProductionKind::Footman),
                (-14, ProductionKind::Ranger),
                (-4, ProductionKind::Catapult),
                (6, ProductionKind::IceTrollPriest),
                (16, ProductionKind::GryphonRider),
            ] {
                simulation.spawn_building_with_properties(
                    production_structure(team, BuildingFootprint::new(x, y, 4, 4), kind),
                    production_building_properties(kind),
                );
            }
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
        // Real Castle Fight acquisition ranges reach 1,200 world units; use a broad-phase cell
        // sized for imported content rather than the tiny placeholder ranges used previously.
        spatial_cell_size: 256 * SUBUNITS_PER_WORLD_UNIT,
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
    Footman,
    Ranger,
    Catapult,
    IceTrollPriest,
    GryphonRider,
}

impl ProductionKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Footman => "Barracks",
            Self::Ranger => "Ranger's Hall",
            Self::Catapult => "Orcish Siege Factory",
            Self::IceTrollPriest => "Ice Troll Hut",
            Self::GryphonRider => "Gryphon Rock",
        }
    }

    const fn gold_cost(self) -> u16 {
        match self {
            Self::Footman => 100,
            Self::Ranger => 200,
            Self::Catapult => 380,
            Self::IceTrollPriest => 175,
            Self::GryphonRider => 250,
        }
    }

    const fn building_health(self) -> i32 {
        match self {
            Self::Footman | Self::IceTrollPriest => 1_200,
            Self::Ranger | Self::Catapult => 1_400,
            Self::GryphonRider => 1_300,
        }
    }

    const fn spawn_seconds(self) -> u16 {
        match self {
            Self::Footman => 20,
            Self::Ranger => 32,
            Self::Catapult => 34,
            Self::IceTrollPriest => 23,
            Self::GryphonRider => 27,
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

    pub(crate) const fn gold_cost(self) -> Option<u16> {
        match self {
            Self::Production(kind) => Some(kind.gold_cost()),
            Self::GuaranteedTower | Self::ProjectileTower | Self::GlobalAreaSpell => None,
        }
    }
}

fn biological_corpse_profile(definition: u32) -> CorpseProfile {
    CorpseProfile {
        definition: CorpseDefinitionId(definition),
        // Extracted biological corpses persist for about 30 seconds including death/flesh/bone
        // phases. The sim currently models this as one authoritative corpse lifetime.
        lifetime_ticks: Some(u32::from(TICKS_PER_SECOND) * 30),
    }
}

fn production_building_properties(kind: ProductionKind) -> BuildingGameplayProperties {
    let (movement_class, attack_targets, corpse) = match kind {
        ProductionKind::Footman => (
            MovementClass::Ground,
            AttackTargetMask::GROUND_AND_BUILDINGS,
            Some(biological_corpse_profile(u32::from_be_bytes(*b"hfoo"))),
        ),
        ProductionKind::Ranger => (
            MovementClass::Ground,
            AttackTargetMask::ALL,
            Some(biological_corpse_profile(u32::from_be_bytes(*b"e003"))),
        ),
        ProductionKind::Catapult => (
            MovementClass::Ground,
            AttackTargetMask::GROUND_AND_BUILDINGS,
            None,
        ),
        ProductionKind::IceTrollPriest => (
            MovementClass::Ground,
            AttackTargetMask::ALL,
            Some(biological_corpse_profile(u32::from_be_bytes(*b"n015"))),
        ),
        ProductionKind::GryphonRider => (MovementClass::Air, AttackTargetMask::ALL, None),
    };
    let collision_world = match kind {
        ProductionKind::Footman => FOOTMAN_COLLISION_WORLD,
        ProductionKind::Ranger => RANGER_COLLISION_WORLD,
        ProductionKind::Catapult => CATAPULT_COLLISION_WORLD,
        ProductionKind::IceTrollPriest => ICE_TROLL_PRIEST_COLLISION_WORLD,
        ProductionKind::GryphonRider => GRYPHON_COLLISION_WORLD,
    };
    BuildingGameplayProperties {
        production_unit: UnitGameplayProperties {
            corpse,
            collision_radius: Some(CollisionRadius(collision_world * SUBUNITS_PER_WORLD_UNIT)),
            movement_class,
            attack_targets,
        },
        ..BuildingGameplayProperties::default()
    }
}

pub(crate) fn try_spawn_demo_building(
    simulation: &mut Simulation,
    team: Team,
    footprint: BuildingFootprint,
    kind: BuildKind,
) -> Result<SimId, BuildingPlacementError> {
    match kind {
        BuildKind::Production(kind) => simulation.try_spawn_building_with_properties(
            production_structure(team, footprint, kind),
            production_building_properties(kind),
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
    let spawn_ticks = kind.spawn_seconds() * TICKS_PER_SECOND;
    BuildingSpawn {
        team,
        footprint,
        health: kind.building_health(),
        production: Some(ProductionProfile {
            initial_delay_ticks: spawn_ticks,
            interval_ticks: spawn_ticks,
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
    match kind {
        ProductionKind::Footman => UnitTemplate {
            health: 250,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                // Extracted damage is 25-26 (25.5 average). The current sim has integer fixed
                // damage, so use the nearest integer while preserving the extracted cadence.
                damage: 26,
                range: 90 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 800 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 41, // 1.35 s at 30 Hz, rounded to nearest tick.
            },
            movement: movement_profile(270),
        },
        ProductionKind::Ranger => UnitTemplate {
            health: 500,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: projectile_speed_per_tick(1_000),
                },
                damage: 65,
                range: 425 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 800 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 31, // 1.04 s at 30 Hz.
            },
            movement: movement_profile(300),
        },
        ProductionKind::Catapult => UnitTemplate {
            health: 475,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedBallistic {
                    speed_per_tick: projectile_speed_per_tick(900),
                    // Warcraft uses 60/110/160 radii with 100%/70%/35% falloff. The current
                    // ballistic primitive has one radius, so preserve the extracted outer area;
                    // tiered falloff remains a separate combat-model extension.
                    impact_radius: 160 * SUBUNITS_PER_WORLD_UNIT,
                },
                damage: 135,
                range: 1_000 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 1_200 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 150, // 5.0 s.
            },
            movement: movement_profile(220),
        },
        ProductionKind::IceTrollPriest => UnitTemplate {
            health: 350,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: projectile_speed_per_tick(1_200),
                },
                damage: 50,
                range: 350 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 800 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 54, // 1.8 s.
            },
            movement: movement_profile(270),
        },
        ProductionKind::GryphonRider => UnitTemplate {
            health: 500,
            attack: AttackProfile {
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: projectile_speed_per_tick(1_100),
                },
                // Extracted damage is 45-50 (47.5 average).
                damage: 48,
                range: 450 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 800 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 60, // 2.0 s.
            },
            movement: movement_profile(320),
        },
    }
}

fn movement_profile(world_units_per_second: i32) -> MovementProfile {
    MovementProfile {
        speed_per_tick: world_units_per_second * SUBUNITS_PER_WORLD_UNIT / SIMULATION_HZ_I32,
    }
}

fn projectile_speed_per_tick(world_units_per_second: i32) -> i32 {
    world_units_per_second * SUBUNITS_PER_WORLD_UNIT / SIMULATION_HZ_I32
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
            BuildKind::Production(ProductionKind::Footman),
            BuildKind::Production(ProductionKind::Ranger),
            BuildKind::Production(ProductionKind::Catapult),
            BuildKind::Production(ProductionKind::IceTrollPriest),
            BuildKind::Production(ProductionKind::GryphonRider),
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
    fn production_roster_uses_extracted_castle_fight_values() {
        let cases = [
            (
                ProductionKind::Footman,
                100,
                1_200,
                20,
                250,
                270,
                90,
                800,
                41,
                MovementClass::Ground,
                AttackTargetMask::GROUND_AND_BUILDINGS,
            ),
            (
                ProductionKind::Ranger,
                200,
                1_400,
                32,
                500,
                300,
                425,
                800,
                31,
                MovementClass::Ground,
                AttackTargetMask::ALL,
            ),
            (
                ProductionKind::Catapult,
                380,
                1_400,
                34,
                475,
                220,
                1_000,
                1_200,
                150,
                MovementClass::Ground,
                AttackTargetMask::GROUND_AND_BUILDINGS,
            ),
            (
                ProductionKind::IceTrollPriest,
                175,
                1_200,
                23,
                350,
                270,
                350,
                800,
                54,
                MovementClass::Ground,
                AttackTargetMask::ALL,
            ),
            (
                ProductionKind::GryphonRider,
                250,
                1_300,
                27,
                500,
                320,
                450,
                800,
                60,
                MovementClass::Air,
                AttackTargetMask::ALL,
            ),
        ];

        for (
            kind,
            gold,
            building_health,
            spawn_seconds,
            unit_health,
            move_speed,
            range,
            acquisition,
            cooldown,
            movement_class,
            attack_targets,
        ) in cases
        {
            let building =
                production_structure(Team(0), BuildingFootprint::new(-220, 0, 4, 4), kind);
            let production = building.production.expect("production profile missing");
            let properties = production_building_properties(kind).production_unit;
            assert_eq!(kind.gold_cost(), gold);
            assert_eq!(building.health, building_health);
            assert_eq!(
                production.initial_delay_ticks,
                spawn_seconds * TICKS_PER_SECOND
            );
            assert_eq!(production.interval_ticks, spawn_seconds * TICKS_PER_SECOND);
            assert_eq!(production.unit.health, unit_health);
            assert_eq!(
                production.unit.movement.speed_per_tick,
                move_speed * SUBUNITS_PER_WORLD_UNIT / SIMULATION_HZ_I32
            );
            assert_eq!(
                production.unit.attack.range,
                range * SUBUNITS_PER_WORLD_UNIT
            );
            assert_eq!(
                production.unit.attack.acquisition_range,
                acquisition * SUBUNITS_PER_WORLD_UNIT
            );
            assert_eq!(production.unit.attack.cooldown_ticks, cooldown);
            assert_eq!(properties.movement_class, movement_class);
            assert_eq!(properties.attack_targets, attack_targets);
            assert_eq!(
                properties.collision_radius,
                Some(CollisionRadius(16 * SUBUNITS_PER_WORLD_UNIT))
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
