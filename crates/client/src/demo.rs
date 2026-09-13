use castle_fight_sim::{
    ArmorProfile, ArmorType, BuildingFootprint, BuildingGameplayProperties, BuildingPlacementError,
    BuildingSpawn, CastleFightProductionKind, CastleFightTowerKind, CastleFightUnitKind,
    CombatRules, DamageType, NavCell, SUBUNITS_PER_WORLD_UNIT, SimId, SimPoint, Simulation,
    SimulationConfig, Team, TerrainElevationMap, UnitSpawn, castle_fight_damage_rules,
};

use crate::presentation::WorldMetrics;

const NAV_CELL_WORLD: i32 = 32;
const NAV_CELL_SUBUNITS: i32 = NAV_CELL_WORLD * SUBUNITS_PER_WORLD_UNIT;
const MIDDLE_MIN_X: i32 = -128;
const MIDDLE_MAX_X: i32 = 127;
const LANE_MIN_Y: i32 = -24;
const LANE_MAX_Y: i32 = 23;
const CASTLE_HEALTH: i32 = 20_000;
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
        damage_rules: castle_fight_damage_rules(),
    };
    let mut simulation = Simulation::new_with_combat_rules(config, workers, combat_rules);

    let castle_properties = BuildingGameplayProperties {
        damage_type: DamageType::Normal,
        armor: ArmorProfile::new(ArmorType::Fortified, 5),
        ..BuildingGameplayProperties::default()
    };
    simulation.spawn_building_with_properties(
        passive_structure(Team(0), PLAYER_CASTLE, CASTLE_HEALTH),
        castle_properties,
    );
    simulation.spawn_building_with_properties(
        passive_structure(Team(1), ENEMY_CASTLE, CASTLE_HEALTH),
        castle_properties,
    );

    if let Some(unit_count) = stress_units {
        populate_render_stress_units(&mut simulation, unit_count);
    } else {
        for team in [Team(0), Team(1)] {
            let x = if team.0 == 0 { -152 } else { 148 };
            for (y, kind) in [
                (-24, ProductionKind::Barracks),
                (-14, ProductionKind::RangersHall),
                (-4, ProductionKind::OrcishSiegeFactory),
                (6, ProductionKind::IceTrollHut),
                (16, ProductionKind::GryphonRock),
            ] {
                let definition = kind.definition();
                simulation.spawn_building_with_properties(
                    definition.spawn(team, BuildingFootprint::new(x, y, 4, 4)),
                    definition.gameplay_properties(),
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
    const SPACING_WORLD: i32 = 40;
    const START_X_WORLD: i32 = -2_400;
    const START_Y_WORLD: i32 = -1_200;

    for index in 0..unit_count {
        let column = index % COLUMNS;
        let row = index / COLUMNS;
        let position = SimPoint::new(
            (START_X_WORLD + column as i32 * SPACING_WORLD) * SUBUNITS_PER_WORLD_UNIT,
            (START_Y_WORLD + row as i32 * SPACING_WORLD) * SUBUNITS_PER_WORLD_UNIT,
        );
        let kind = CastleFightUnitKind::ALL[index % CastleFightUnitKind::ALL.len()];
        let definition = kind.definition();
        simulation.spawn_unit_with_properties(
            UnitSpawn::from_template(Team((index & 1) as u8), position, definition.template()),
            definition.gameplay_properties(),
        );
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

pub(crate) type ProductionKind = CastleFightProductionKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildKind {
    Production(ProductionKind),
    Tower(CastleFightTowerKind),
}

impl BuildKind {
    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Production(kind) => kind.definition().name,
            Self::Tower(kind) => kind.definition().name,
        }
    }

    pub(crate) const fn footprint_size(self) -> u16 {
        match self {
            Self::Production(kind) => kind.definition().footprint_size_cells,
            Self::Tower(kind) => kind.definition().footprint_size_cells,
        }
    }

    pub(crate) const fn gold_cost(self) -> Option<u16> {
        Some(match self {
            Self::Production(kind) => kind.definition().gold_cost,
            Self::Tower(kind) => kind.definition().gold_cost,
        })
    }
}

pub(crate) fn try_spawn_demo_building(
    simulation: &mut Simulation,
    team: Team,
    footprint: BuildingFootprint,
    kind: BuildKind,
) -> Result<SimId, BuildingPlacementError> {
    match kind {
        BuildKind::Production(kind) => {
            let definition = kind.definition();
            simulation.try_spawn_building_with_properties(
                definition.spawn(team, footprint),
                definition.gameplay_properties(),
            )
        }
        BuildKind::Tower(kind) => {
            let definition = kind.definition();
            simulation.try_spawn_building_with_properties(
                definition.spawn(team, footprint),
                definition.gameplay_properties(),
            )
        }
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
    fn demo_exposes_every_imported_build_kind() {
        let DemoWorld { mut simulation, .. } = create_demo_world(1, None);
        let kinds = [
            BuildKind::Production(ProductionKind::Barracks),
            BuildKind::Production(ProductionKind::RangersHall),
            BuildKind::Production(ProductionKind::OrcishSiegeFactory),
            BuildKind::Production(ProductionKind::IceTrollHut),
            BuildKind::Production(ProductionKind::GryphonRock),
            BuildKind::Tower(CastleFightTowerKind::WatchTower),
            BuildKind::Tower(CastleFightTowerKind::PoofTower),
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
    fn build_selectors_read_names_and_costs_from_imported_content() {
        let barracks = BuildKind::Production(ProductionKind::Barracks);
        assert_eq!(barracks.label(), "Barracks");
        assert_eq!(barracks.gold_cost(), Some(100));
        assert_eq!(barracks.footprint_size(), 4);

        let watch = BuildKind::Tower(CastleFightTowerKind::WatchTower);
        assert_eq!(watch.label(), "Watch Tower");
        assert_eq!(watch.gold_cost(), Some(150));
        assert_eq!(watch.footprint_size(), 4);
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
