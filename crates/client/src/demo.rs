use castle_fight_sim::{
    ArmorProfile, ArmorType, BuilderBuildError, BuilderSpawn, BuildingFootprint,
    BuildingGameplayProperties, BuildingSpawn, CastleFightBuilderRace, CastleFightProductionKind,
    CastleFightTowerKind, CastleFightUnitKind, CombatRules, ContentIdentity, DamageType, NavCell,
    SUBUNITS_PER_WORLD_UNIT, SimId, SimPoint, Simulation, SimulationConfig, Team,
    TerrainElevationMap, UnitSpawn, castle_fight_damage_rules,
};

use crate::presentation::WorldMetrics;

const NAV_CELL_WORLD: i32 = 32;
const NAV_CELL_SUBUNITS: i32 = NAV_CELL_WORLD * SUBUNITS_PER_WORLD_UNIT;
// W3I's unplayable-border tile counts leave this authoritative playable rectangle. The smaller
// camera rectangle below controls what WC3 lets the player see, but legal base cells extend beyond
// it behind the castles.
const PLAYABLE_MIN_X_WORLD: i32 = -6_400;
const PLAYABLE_MAX_X_WORLD: i32 = 6_400;
const PLAYABLE_MIN_Y_WORLD: i32 = -3_584;
const PLAYABLE_MAX_Y_WORLD: i32 = 3_584;
const CAMERA_MIN_X_WORLD: i32 = -5_888;
const CAMERA_MAX_X_WORLD: i32 = 5_888;
const CAMERA_MIN_Y_WORLD: i32 = -3_328;
const CAMERA_MAX_Y_WORLD: i32 = 3_328;
// The protected map creates these two base rectangles as NFb/MFb. Their edges line up with the
// outer wall pathing and are the actual standard build/movement regions, not the camera bounds.
const LEFT_BUILD_MIN_X_WORLD: i32 = -6_144;
const LEFT_BUILD_MAX_X_WORLD: i32 = -1_920;
const RIGHT_BUILD_MIN_X_WORLD: i32 = 1_920;
const RIGHT_BUILD_MAX_X_WORLD: i32 = 6_144;
const BUILD_MIN_Y_WORLD: i32 = -2_048;
const BUILD_MAX_Y_WORLD: i32 = 2_048;
// Outside the lane, only the central gap between the two protected-map base rectangles is
// invalid. The old verification mask started this blocker at +/-4096 world units, which cut away
// almost half of each real base and caused legal wall-adjacent placement to be rejected.
const CENTRAL_GAP_MIN_X: i32 = LEFT_BUILD_MAX_X_WORLD / NAV_CELL_WORLD;
const CENTRAL_GAP_MAX_X: i32 = RIGHT_BUILD_MIN_X_WORLD / NAV_CELL_WORLD - 1;
const LANE_MIN_Y: i32 = -24;
const LANE_MAX_Y: i32 = 23;
const CASTLE_HEALTH: i32 = 20_000;
const CASTLE_CENTER_X_WORLD: i32 = 4_992;
const CASTLE_PATHING_SIZE_CELLS: u16 = 16;
const WALL_DOODAD_RAWCODES: [&str; 6] = ["B002", "B003", "D000", "D001", "D002", "D003"];
// Visual-verification value only; the exact original Castle Fight uphill miss chance is still
// compatibility data to recover.
const DEMO_UPHILL_MISS_CHANCE_PER_10K: u16 = 2_500;

// The protected map creates the castles at +/-4992 world units. Their WC3 object data uses the
// 16x16 `16x16Simple.tga` pathing map, so keep the native authoritative footprint at that size too.
const PLAYER_CASTLE: BuildingFootprint = BuildingFootprint::new(
    -(CASTLE_CENTER_X_WORLD / NAV_CELL_WORLD) - CASTLE_PATHING_SIZE_CELLS as i32 / 2,
    -(CASTLE_PATHING_SIZE_CELLS as i32) / 2,
    CASTLE_PATHING_SIZE_CELLS,
    CASTLE_PATHING_SIZE_CELLS,
);
const ENEMY_CASTLE: BuildingFootprint = BuildingFootprint::new(
    CASTLE_CENTER_X_WORLD / NAV_CELL_WORLD - CASTLE_PATHING_SIZE_CELLS as i32 / 2,
    -(CASTLE_PATHING_SIZE_CELLS as i32) / 2,
    CASTLE_PATHING_SIZE_CELLS,
    CASTLE_PATHING_SIZE_CELLS,
);

pub struct DemoWorld {
    pub simulation: Simulation,
    pub metrics: WorldMetrics,
    pub terrain: TerrainElevationMap,
}

#[must_use]
pub fn create_demo_world(workers: usize, stress_units: Option<usize>) -> DemoWorld {
    let terrain = original_terrain();
    let config = demo_config(&terrain);
    let metrics = WorldMetrics::from_simulation_config(&config).with_camera_focus_bounds_world(
        CAMERA_MIN_X_WORLD as f32,
        CAMERA_MIN_Y_WORLD as f32,
        CAMERA_MAX_X_WORLD as f32,
        CAMERA_MAX_Y_WORLD as f32,
    );
    let combat_rules = CombatRules {
        terrain_elevation: Some(terrain.clone()),
        uphill_miss_chance_per_10k: DEMO_UPHILL_MISS_CHANCE_PER_10K,
        damage_rules: castle_fight_damage_rules(),
    };
    let mut simulation = Simulation::new_with_combat_rules(config, workers, combat_rules);

    let builder_definition = CastleFightBuilderRace::Human.definition();
    let demo_build_catalog = CastleFightProductionKind::ALL
        .into_iter()
        .map(|kind| kind.definition().rawcode)
        .chain(
            CastleFightTowerKind::ALL
                .into_iter()
                .map(|kind| kind.definition().rawcode),
        )
        .collect::<Vec<_>>();
    for (team, x) in [
        (Team(0), -CASTLE_CENTER_X_WORLD),
        (Team(1), CASTLE_CENTER_X_WORLD),
    ] {
        simulation.spawn_builder(BuilderSpawn {
            team,
            position: world_point(x, 0),
            profile: builder_definition.profile,
            // The verification client intentionally exposes a mixed-race slice of implemented
            // buildings. Model that as a draft-style variable menu while keeping one builder type.
            configuration: builder_definition
                .configuration_with_catalog(demo_build_catalog.clone()),
        });
    }

    let castle_properties = BuildingGameplayProperties {
        content: Some(ContentIdentity {
            rawcode: u32::from_be_bytes(*b"hcas"),
            name: "Main Castle",
        }),
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
            // Keep the verification producers on the lane-facing side of the now-exact castle
            // footprint rather than overlapping the original 16x16 castle pathing map.
            let x = if team.0 == 0 { -144 } else { 140 };
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
    let terrain_navigation_min = NavCell::new(
        origin.x.div_euclid(NAV_CELL_SUBUNITS),
        origin.y.div_euclid(NAV_CELL_SUBUNITS),
    );
    let terrain_navigation_max = NavCell::new(
        maximum.x.div_euclid(NAV_CELL_SUBUNITS) - 1,
        maximum.y.div_euclid(NAV_CELL_SUBUNITS) - 1,
    );

    // W3I's camera bounds are deliberately inset from the actual playable map. The unplayable
    // border counts, not the camera lock, define where navigation may exist. This distinction is
    // visible behind each castle, where legal corner cages sit outside the camera-focus rectangle.
    let navigation_min = NavCell::new(
        PLAYABLE_MIN_X_WORLD.div_euclid(NAV_CELL_WORLD),
        PLAYABLE_MIN_Y_WORLD.div_euclid(NAV_CELL_WORLD),
    );
    let navigation_max = NavCell::new(
        PLAYABLE_MAX_X_WORLD.div_euclid(NAV_CELL_WORLD) - 1,
        PLAYABLE_MAX_Y_WORLD.div_euclid(NAV_CELL_WORLD) - 1,
    );
    assert!(navigation_min.x >= terrain_navigation_min.x);
    assert!(navigation_min.y >= terrain_navigation_min.y);
    assert!(navigation_max.x <= terrain_navigation_max.x);
    assert!(navigation_max.y <= terrain_navigation_max.y);

    // Use the exact protected-map base rectangles (NFb/MFb). Their max edges are exclusive, just
    // like WC3 rects; converting them as edge-aligned footprints avoids losing a rear nav cell.
    let left_build_region = world_rect_footprint(
        LEFT_BUILD_MIN_X_WORLD,
        BUILD_MIN_Y_WORLD,
        LEFT_BUILD_MAX_X_WORLD,
        BUILD_MAX_Y_WORLD,
    );
    let right_build_region = world_rect_footprint(
        RIGHT_BUILD_MIN_X_WORLD,
        BUILD_MIN_Y_WORLD,
        RIGHT_BUILD_MAX_X_WORLD,
        BUILD_MAX_Y_WORLD,
    );

    let mut static_blockers = vec![
        BuildingFootprint::new(
            CENTRAL_GAP_MIN_X,
            navigation_min.y,
            (CENTRAL_GAP_MAX_X - CENTRAL_GAP_MIN_X + 1) as u16,
            (LANE_MIN_Y - navigation_min.y) as u16,
        ),
        BuildingFootprint::new(
            CENTRAL_GAP_MIN_X,
            LANE_MAX_Y + 1,
            (CENTRAL_GAP_MAX_X - CENTRAL_GAP_MIN_X + 1) as u16,
            (navigation_max.y - LANE_MAX_Y) as u16,
        ),
    ];
    static_blockers.extend(original_wall_blockers());

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
        static_blockers,
        team_build_regions: [vec![left_build_region], vec![right_build_region]],
        // Distance-field objectives must stay outside the castle's blocked 16x16 footprint. These
        // are the lane-facing cells immediately beyond each original castle pathing envelope.
        team_objective: [world_point(4_720, 0), world_point(-4_720, 0)],
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
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Production(kind) => kind.definition().name,
            Self::Tower(kind) => kind.definition().name,
        }
    }

    pub(crate) fn footprint_size(self) -> u16 {
        match self {
            Self::Production(kind) => kind.definition().footprint_size_cells,
            Self::Tower(kind) => kind.definition().footprint_size_cells,
        }
    }

    pub(crate) fn gold_cost(self) -> Option<u16> {
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
) -> Result<SimId, BuilderBuildError> {
    let builder = simulation
        .builder_for_team(team)
        .ok_or(BuilderBuildError::Builder(
            castle_fight_sim::BuilderCommandError::BuilderNotFound,
        ))?;
    match kind {
        BuildKind::Production(kind) => {
            let definition = kind.definition();
            simulation.try_builder_summon_building_with_properties(
                builder.id,
                definition.spawn(team, footprint),
                definition.gameplay_properties(),
            )
        }
        BuildKind::Tower(kind) => {
            let definition = kind.definition();
            simulation.try_builder_summon_building_with_properties(
                builder.id,
                definition.spawn(team, footprint),
                definition.gameplay_properties(),
            )
        }
    }
}

fn original_wall_blockers() -> Vec<BuildingFootprint> {
    include_str!("../../../docs/original_map/extracted/resolved/placed-doodads.tsv")
        .lines()
        .skip(1)
        .filter_map(|line| {
            let columns: Vec<_> = line.split('\t').collect();
            let rawcode = columns
                .get(2)
                .copied()
                .expect("placed doodad row must contain a rawcode");
            if !WALL_DOODAD_RAWCODES.contains(&rawcode) {
                return None;
            }

            let x = parse_wall_integer(columns[5], "x");
            let y = parse_wall_integer(columns[6], "y");
            let angle = columns[8]
                .parse::<f64>()
                .expect("wall angle must be a number");
            let texture_width = columns[18]
                .parse::<u16>()
                .expect("wall pathing width must be present");
            let texture_height = columns[19]
                .parse::<u16>()
                .expect("wall pathing height must be present");
            let unwalkable = columns[22]
                .parse::<u32>()
                .expect("wall unwalkable count must be present");
            let unbuildable = columns[24]
                .parse::<u32>()
                .expect("wall unbuildable count must be present");
            let pathing_cells = u32::from(texture_width) * u32::from(texture_height);
            assert_eq!(
                unwalkable, pathing_cells,
                "wall {rawcode} must have a fully unwalkable pathing mask"
            );
            assert_eq!(
                unbuildable, pathing_cells,
                "wall {rawcode} must have a fully unbuildable pathing mask"
            );

            // WC3 pathing-texture rows/columns map to world Y/X respectively. A quarter-turn
            // doodad rotation then swaps those world extents. The destructable wall variants use
            // a nominal 270-degree angle with the small editor float error seen below.
            let quarter_turns = (angle / 90.0).round() as i32;
            let snapped_angle = f64::from(quarter_turns) * 90.0;
            assert!(
                (angle - snapped_angle).abs() < 0.1,
                "wall {rawcode} has unsupported non-right-angle rotation {angle}"
            );
            let (mut width, mut height) = (texture_height, texture_width);
            if quarter_turns.rem_euclid(2) == 1 {
                (width, height) = (height, width);
            }

            Some(centered_world_footprint(x, y, width, height))
        })
        .collect()
}

fn parse_wall_integer(value: &str, axis: &str) -> i32 {
    let value = value
        .parse::<f64>()
        .unwrap_or_else(|_| panic!("wall {axis} coordinate must be a number"));
    let rounded = value.round();
    assert!(
        (value - rounded).abs() < 1.0e-6,
        "wall {axis} coordinate must be integral, got {value}"
    );
    rounded as i32
}

fn world_rect_footprint(
    min_x_world: i32,
    min_y_world: i32,
    max_x_world: i32,
    max_y_world: i32,
) -> BuildingFootprint {
    assert!(min_x_world < max_x_world && min_y_world < max_y_world);
    for edge in [min_x_world, min_y_world, max_x_world, max_y_world] {
        assert_eq!(
            edge.rem_euclid(NAV_CELL_WORLD),
            0,
            "world rectangle edge must align to the navigation grid"
        );
    }
    let width = (max_x_world - min_x_world).div_euclid(NAV_CELL_WORLD);
    let height = (max_y_world - min_y_world).div_euclid(NAV_CELL_WORLD);
    BuildingFootprint::new(
        min_x_world.div_euclid(NAV_CELL_WORLD),
        min_y_world.div_euclid(NAV_CELL_WORLD),
        u16::try_from(width).expect("world rectangle must fit a building footprint"),
        u16::try_from(height).expect("world rectangle must fit a building footprint"),
    )
}

fn centered_world_footprint(
    center_x: i32,
    center_y: i32,
    width: u16,
    height: u16,
) -> BuildingFootprint {
    let width_world = i32::from(width) * NAV_CELL_WORLD;
    let height_world = i32::from(height) * NAV_CELL_WORLD;
    let min_x_world = center_x - width_world / 2;
    let min_y_world = center_y - height_world / 2;
    assert_eq!(
        min_x_world.rem_euclid(NAV_CELL_WORLD),
        0,
        "wall pathing footprint must align to the navigation grid"
    );
    assert_eq!(
        min_y_world.rem_euclid(NAV_CELL_WORLD),
        0,
        "wall pathing footprint must align to the navigation grid"
    );
    BuildingFootprint::new(
        min_x_world.div_euclid(NAV_CELL_WORLD),
        min_y_world.div_euclid(NAV_CELL_WORLD),
        width,
        height,
    )
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
                -176 + index as i32 * 6,
                40,
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
    fn demo_bootstraps_one_builder_per_side_at_the_castles() {
        let DemoWorld { simulation, .. } = create_demo_world(1, Some(0));
        let builders = simulation.builders();
        assert_eq!(builders.len(), 2);
        let human = CastleFightBuilderRace::Human.definition();
        assert_eq!(builders[0].team, Team(0));
        assert_eq!(builders[0].position, world_point(-CASTLE_CENTER_X_WORLD, 0));
        assert_eq!(builders[0].profile, human.profile);
        assert_eq!(builders[0].configuration.appearance.rawcode, human.rawcode);
        assert_eq!(builders[0].configuration.locomotion, human.locomotion);
        assert_eq!(builders[0].configuration.build_catalog.len(), 7);
        assert_eq!(builders[1].team, Team(1));
        assert_eq!(builders[1].position, world_point(CASTLE_CENTER_X_WORLD, 0));
        assert_eq!(builders[1].profile, human.profile);
        assert_eq!(builders[1].configuration, builders[0].configuration);
    }

    #[test]
    fn original_map_castles_use_runtime_positions_and_pathing_size() {
        let center_world = |footprint: BuildingFootprint| {
            (
                footprint.min_x * NAV_CELL_WORLD + i32::from(footprint.width) * NAV_CELL_WORLD / 2,
                footprint.min_y * NAV_CELL_WORLD + i32::from(footprint.height) * NAV_CELL_WORLD / 2,
            )
        };

        assert_eq!(PLAYER_CASTLE.width, CASTLE_PATHING_SIZE_CELLS);
        assert_eq!(PLAYER_CASTLE.height, CASTLE_PATHING_SIZE_CELLS);
        assert_eq!(ENEMY_CASTLE.width, CASTLE_PATHING_SIZE_CELLS);
        assert_eq!(ENEMY_CASTLE.height, CASTLE_PATHING_SIZE_CELLS);
        assert_eq!(center_world(PLAYER_CASTLE), (-CASTLE_CENTER_X_WORLD, 0));
        assert_eq!(center_world(ENEMY_CASTLE), (CASTLE_CENTER_X_WORLD, 0));

        let DemoWorld { simulation, .. } = create_demo_world(1, Some(0));
        let buildings = simulation.buildings();
        let footprints: Vec<_> = buildings
            .iter()
            .map(|building| building.footprint)
            .collect();
        assert!(footprints.contains(&PLAYER_CASTLE));
        assert!(footprints.contains(&ENEMY_CASTLE));
        for castle in buildings.iter().filter(|building| {
            building.footprint == PLAYER_CASTLE || building.footprint == ENEMY_CASTLE
        }) {
            assert_eq!(
                castle.content.map(|content| content.rawcode),
                Some(u32::from_be_bytes(*b"hcas"))
            );
        }
    }

    #[test]
    fn original_map_playable_bounds_are_wider_than_camera_bounds() {
        let terrain = original_terrain();
        let config = demo_config(&terrain);
        assert_eq!(config.navigation_min, NavCell::new(-200, -112));
        assert_eq!(config.navigation_max, NavCell::new(199, 111));
        assert_eq!(
            config.team_build_regions[0],
            vec![BuildingFootprint::new(-192, -64, 132, 128)]
        );
        assert_eq!(
            config.team_build_regions[1],
            vec![BuildingFootprint::new(60, -64, 132, 128)]
        );

        let DemoWorld { simulation, .. } = create_demo_world(1, Some(0));
        assert!(!simulation.can_place_building(BuildingFootprint::new(-201, 0, 1, 1)));
        assert!(!simulation.can_place_building(BuildingFootprint::new(200, 0, 1, 1)));
        assert!(!simulation.can_place_building(BuildingFootprint::new(0, -113, 1, 1)));
        assert!(!simulation.can_place_building(BuildingFootprint::new(0, 112, 1, 1)));
    }

    #[test]
    fn original_map_wall_doodads_block_building_placement() {
        let blockers = original_wall_blockers();
        assert_eq!(blockers.len(), 92);

        // D000 at (4384, 2016) is a horizontal 10x2 WC3 wall footprint. This point is otherwise
        // inside the right team's build region, so the rejection specifically exercises the wall.
        let wall = BuildingFootprint::new(132, 62, 10, 2);
        assert!(blockers.contains(&wall));

        // The left rear cap is centered at x=-5984. Its footprint begins exactly at the authored
        // NFb base edge x=-6144, rather than bleeding one nav cell inward into the corner cage.
        assert!(blockers.contains(&BuildingFootprint::new(-192, 62, 10, 2)));
        assert!(blockers.contains(&BuildingFootprint::new(-192, -64, 10, 2)));

        let DemoWorld { simulation, .. } = create_demo_world(1, Some(0));
        assert!(
            !simulation.can_place_building_for_team(Team(1), BuildingFootprint::new(136, 62, 1, 1))
        );
    }

    #[test]
    fn original_map_corner_cages_and_wall_adjacent_cells_are_buildable() {
        let DemoWorld { simulation, .. } = create_demo_world(1, Some(0));

        // Four-cell buildings can sit flush against both the rear base edge and either horizontal
        // wall. Moving the same footprint one cell into a wall must still be rejected.
        let top_left_corner = BuildingFootprint::new(-192, 58, 4, 4);
        let bottom_left_corner = BuildingFootprint::new(-192, -62, 4, 4);
        assert!(simulation.can_place_building_for_team(Team(0), top_left_corner));
        assert!(simulation.can_place_building_for_team(Team(0), bottom_left_corner));
        assert!(
            !simulation
                .can_place_building_for_team(Team(0), BuildingFootprint::new(-192, 59, 4, 4))
        );
        assert!(
            !simulation
                .can_place_building_for_team(Team(0), BuildingFootprint::new(-192, -63, 4, 4))
        );

        // Check wall adjacency away from the cap too, so the regression does not depend only on
        // corner geometry.
        assert!(
            simulation.can_place_building_for_team(Team(0), BuildingFootprint::new(-120, 58, 4, 4))
        );
        assert!(
            simulation
                .can_place_building_for_team(Team(0), BuildingFootprint::new(-120, -62, 4, 4))
        );
    }

    #[test]
    fn original_map_rear_build_edge_keeps_its_last_legal_cell() {
        let DemoWorld { simulation, .. } = create_demo_world(1, Some(0));

        // NFb/MFb end at +/-6144. A 4x4 footprint may touch that edge exactly, but shifting one
        // nav cell farther outward must fail the team-region containment check.
        assert!(
            simulation.can_place_building_for_team(Team(0), BuildingFootprint::new(-192, 20, 4, 4))
        );
        assert!(
            !simulation
                .can_place_building_for_team(Team(0), BuildingFootprint::new(-193, 20, 4, 4))
        );
        assert!(
            simulation.can_place_building_for_team(Team(1), BuildingFootprint::new(188, 20, 4, 4))
        );
        assert!(
            !simulation.can_place_building_for_team(Team(1), BuildingFootprint::new(189, 20, 4, 4))
        );
    }

    #[test]
    fn original_map_central_lane_gap_is_not_buildable() {
        let DemoWorld { simulation, .. } = create_demo_world(1, Some(0));
        let middle = BuildingFootprint::new(0, 0, 4, 4);
        let left = BuildingFootprint::new(-140, 30, 4, 4);
        let right = BuildingFootprint::new(130, 30, 4, 4);

        assert!(!simulation.can_place_building_for_team(Team(0), middle));
        assert!(!simulation.can_place_building_for_team(Team(1), middle));
        assert!(simulation.can_place_building_for_team(Team(0), left));
        assert!(simulation.can_place_building_for_team(Team(1), right));
    }
}
