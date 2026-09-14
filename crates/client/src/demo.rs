use castle_fight_sim::{
    ArmorProfile, ArmorType, BuilderBuildError, BuilderSpawn, BuildingEconomyProfile,
    BuildingFootprint, BuildingGameplayProperties, BuildingSpawn,
    CASTLE_FIGHT_MAIN_CASTLE_REPAIR_TIME_TICKS, CastleFightBuilderRace, CastleFightProductionKind,
    CastleFightTowerKind, CastleFightUnitKind, CombatRules, ContentIdentity, DamageType, NavCell,
    SUBUNITS_PER_WORLD_UNIT, SimId, SimPoint, Simulation, SimulationConfig, TargetlessLane, Team,
    TerrainElevationMap, UnitSpawn, castle_fight_damage_rules, castle_fight_economy_rules,
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
// The protected map's NFb/MFb rectangles identify the two bases. WC3's snapped building grid
// admits one additional 32-world-unit column at each rear/outside edge, visible in the original
// placement grid, so native placement includes that final column as well. The lane-facing edges
// remain the protected-map rect edges.
const LEFT_BUILD_MIN_X_WORLD: i32 = -6_176;
const LEFT_BUILD_MAX_X_WORLD: i32 = -1_920;
const RIGHT_BUILD_MIN_X_WORLD: i32 = 1_920;
const RIGHT_BUILD_MAX_X_WORLD: i32 = 6_176;
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
// The original W3I player start locations sit at x = +/-4352, with the middle slot on each team
// at y = 0. The temporary one-builder-per-side demo uses those middle starts instead of spawning
// builders inside the castle models at +/-4992.
const BUILDER_START_X_WORLD: i32 = 4_352;
const CASTLE_PATHING_SIZE_CELLS: u16 = 16;
const WALL_DOODAD_RAWCODES: [&str; 6] = ["B002", "B003", "D000", "D001", "D002", "D003"];
const ENTRANCE_ARCH_RAWCODE: &str = "ZSas";
const PATHING_UNFLYABLE_BIT: u8 = 0x02;
const PATHING_UNBUILDABLE_BIT: u8 = 0x04;
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
        (Team(0), -BUILDER_START_X_WORLD),
        (Team(1), BUILDER_START_X_WORLD),
    ] {
        simulation.spawn_builder(BuilderSpawn {
            team,
            position: world_point(x, 0),
            profile: builder_definition.profile,
            // The verification client intentionally exposes a mixed-race slice of implemented
            // buildings. Model that as a draft-style variable menu while keeping one builder type.
            configuration: builder_definition
                .configuration_with_catalog(demo_build_catalog.clone()),
            repair_autocast_enabled: builder_definition.repair_autocast_enabled_by_default,
        });
    }

    let castle_properties = BuildingGameplayProperties {
        content: Some(ContentIdentity {
            rawcode: u32::from_be_bytes(*b"hcas"),
            name: "Main Castle",
        }),
        repair_time_ticks: Some(CASTLE_FIGHT_MAIN_CASTLE_REPAIR_TIME_TICKS),
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
    }

    DemoWorld {
        simulation,
        metrics,
        terrain,
    }
}

fn populate_render_stress_units(simulation: &mut Simulation, unit_count: usize) {
    // Keep the synthetic stress cloud inside the real centre lane. Stress mode deliberately packs
    // large populations densely, but authored ground/no-fly topology must still consider every
    // spawn point legal now that the verification map imports those masks.
    const COLUMNS: usize = 90;
    const ROWS: usize = 32;
    const SPACING_WORLD: i32 = 40;
    const START_X_WORLD: i32 = -1_800;
    const START_Y_WORLD: i32 = -640;

    for index in 0..unit_count {
        let column = index % COLUMNS;
        let row = (index / COLUMNS) % ROWS;
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

    let no_mans_land_blockers = vec![
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
    let mut static_blockers = no_mans_land_blockers.clone();
    static_blockers.extend(original_wall_blockers());

    // Flying combat units ignore ordinary walls/buildings, but not the authored no-fly map
    // boundary or the same no-man's-land that keeps ground units in the centre lane. Keeping this
    // as a separate topology preserves normal Warcraft air movement while still routing flyers
    // through the lane opening instead of letting them disappear across invalid terrain.
    let mut air_static_blockers = no_mans_land_blockers;
    air_static_blockers.extend(original_air_pathing_blockers());

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
        air_static_blockers,
        build_static_blockers: original_doodad_build_blockers(),
        team_build_regions: [vec![left_build_region], vec![right_build_region]],
        targetless_lane: Some(TargetlessLane::new(
            LANE_MIN_Y * NAV_CELL_SUBUNITS,
            (LANE_MAX_Y + 1) * NAV_CELL_SUBUNITS,
            [LEFT_BUILD_MAX_X_WORLD, RIGHT_BUILD_MIN_X_WORLD].map(|x| x * SUBUNITS_PER_WORLD_UNIT),
        )),
        // Distance-field objectives must stay outside the castle's blocked 16x16 footprint. These
        // are the lane-facing cells immediately beyond each original castle pathing envelope.
        team_objective: [world_point(4_720, 0), world_point(-4_720, 0)],
        economy: castle_fight_economy_rules(),
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

    pub(crate) fn economy(self) -> BuildingEconomyProfile {
        match self {
            Self::Production(kind) => kind.definition().economy,
            Self::Tower(kind) => kind.definition().economy,
        }
    }

    pub(crate) fn gold_cost(self) -> u32 {
        self.economy().gold_cost
    }

    pub(crate) fn lumber_cost(self) -> u32 {
        self.economy().lumber_cost
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
            simulation.try_builder_purchase_building_with_properties(
                builder.id,
                definition.spawn(team, footprint),
                definition.gameplay_properties(),
            )
        }
        BuildKind::Tower(kind) => {
            let definition = kind.definition();
            simulation.try_builder_purchase_building_with_properties(
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

            let x = parse_pathing_integer(columns[5], "x");
            let y = parse_pathing_integer(columns[6], "y");
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
            let quarter_turns =
                pathing_quarter_turns(rawcode, angle, texture_width, texture_height);
            let (width, height) =
                rotated_pathing_dimensions(texture_width, texture_height, quarter_turns);

            Some(centered_world_footprint(x, y, width, height))
        })
        .collect()
}

fn original_air_pathing_blockers() -> Vec<BuildingFootprint> {
    original_doodad_pathing_blockers(PATHING_UNFLYABLE_BIT, 23, false)
}

fn original_doodad_build_blockers() -> Vec<BuildingFootprint> {
    let mut blockers = original_doodad_pathing_blockers(PATHING_UNBUILDABLE_BIT, 24, true);
    blockers.extend(original_entrance_arch_build_blockers());
    blockers
}

fn original_doodad_pathing_blockers(
    pathing_bit: u8,
    count_column: usize,
    exclude_walls: bool,
) -> Vec<BuildingFootprint> {
    let mut blockers = Vec::new();
    for line in include_str!("../../../docs/original_map/extracted/resolved/placed-doodads.tsv")
        .lines()
        .skip(1)
    {
        let columns: Vec<_> = line.split('\t').collect();
        let rawcode = columns
            .get(2)
            .copied()
            .expect("placed doodad row must contain a rawcode");
        if exclude_walls && WALL_DOODAD_RAWCODES.contains(&rawcode) {
            continue;
        }
        let blocked_count = columns
            .get(count_column)
            .copied()
            .unwrap_or_default()
            .parse::<u32>()
            .unwrap_or(0);
        if blocked_count == 0 {
            continue;
        }

        let x = parse_pathing_integer(columns[5], "x");
        let y = parse_pathing_integer(columns[6], "y");
        let angle = columns[8]
            .parse::<f64>()
            .expect("doodad angle must be a number");
        let texture_width = columns[18]
            .parse::<u16>()
            .expect("pathing width must be present when pathing cells are blocked");
        let texture_height = columns[19]
            .parse::<u16>()
            .expect("pathing height must be present when pathing cells are blocked");
        let pathing_cells = u32::from(texture_width) * u32::from(texture_height);
        let quarter_turns = pathing_quarter_turns(rawcode, angle, texture_width, texture_height);

        if blocked_count == pathing_cells {
            let (width, height) =
                rotated_pathing_dimensions(texture_width, texture_height, quarter_turns);
            blockers.push(centered_world_footprint(x, y, width, height));
            continue;
        }

        let rows = columns
            .get(25)
            .copied()
            .expect("partial pathing mask must contain hex rows");
        blockers.extend(pathing_mask_cell_blockers(
            x,
            y,
            texture_width,
            texture_height,
            quarter_turns,
            rows,
            pathing_bit,
        ));
    }
    blockers
}

fn original_entrance_arch_build_blockers() -> Vec<BuildingFootprint> {
    // ZSas is overridden to `pathTex=none` in the map object table so ground/air units may pass
    // through the entrance, but WC3 still rejects building placement directly through the visible
    // arch span. Reproduce that placement-only exclusion with the inherited CityArch footprint
    // (28x6 pathing cells, whose texture axes map to world Y/X at zero rotation).
    include_str!("../../../docs/original_map/extracted/resolved/placed-doodads.tsv")
        .lines()
        .skip(1)
        .filter_map(|line| {
            let columns: Vec<_> = line.split('\t').collect();
            (columns.get(2).copied() == Some(ENTRANCE_ARCH_RAWCODE)).then(|| {
                let x = snap_pathing_center(columns[5], "x");
                let y = snap_pathing_center(columns[6], "y");
                centered_world_footprint(x, y, 6, 28)
            })
        })
        .collect()
}

fn pathing_mask_cell_blockers(
    center_x: i32,
    center_y: i32,
    texture_width: u16,
    texture_height: u16,
    quarter_turns: i32,
    hex_rows: &str,
    pathing_bit: u8,
) -> Vec<BuildingFootprint> {
    let rows: Vec<_> = hex_rows.split('/').collect();
    assert_eq!(rows.len(), usize::from(texture_height));
    let mut blockers = Vec::new();
    for (row, values) in rows.into_iter().enumerate() {
        assert_eq!(values.chars().count(), usize::from(texture_width));
        for (column, value) in values.chars().enumerate() {
            let flags = value
                .to_digit(16)
                .expect("pathing mask row must be hexadecimal") as u8;
            if flags & pathing_bit == 0 {
                continue;
            }

            // WC3 pathing-texture rows map to local world X and columns to local world Y. Work in
            // half-cell units so even-sized textures rotate exactly around the doodad centre.
            let local_x2 = i32::try_from(row).expect("pathing row overflow") * 2 + 1
                - i32::from(texture_height);
            let local_y2 = i32::try_from(column).expect("pathing column overflow") * 2 + 1
                - i32::from(texture_width);
            let (rotated_x2, rotated_y2) = match quarter_turns.rem_euclid(4) {
                0 => (local_x2, local_y2),
                1 => (-local_y2, local_x2),
                2 => (-local_x2, -local_y2),
                3 => (local_y2, -local_x2),
                _ => unreachable!(),
            };
            let cell_center_x = center_x + rotated_x2 * (NAV_CELL_WORLD / 2);
            let cell_center_y = center_y + rotated_y2 * (NAV_CELL_WORLD / 2);
            blockers.push(BuildingFootprint::new(
                cell_center_x.div_euclid(NAV_CELL_WORLD),
                cell_center_y.div_euclid(NAV_CELL_WORLD),
                1,
                1,
            ));
        }
    }
    blockers
}

fn pathing_quarter_turns(
    rawcode: &str,
    angle: f64,
    texture_width: u16,
    texture_height: u16,
) -> i32 {
    let quarter_turns = (angle / 90.0).round() as i32;
    if texture_width != texture_height {
        let snapped_angle = f64::from(quarter_turns) * 90.0;
        assert!(
            (angle - snapped_angle).abs() < 0.1,
            "non-square pathing texture for {rawcode} has unsupported rotation {angle}"
        );
    }
    quarter_turns
}

fn rotated_pathing_dimensions(
    texture_width: u16,
    texture_height: u16,
    quarter_turns: i32,
) -> (u16, u16) {
    let (mut width, mut height) = (texture_height, texture_width);
    if quarter_turns.rem_euclid(2) == 1 {
        (width, height) = (height, width);
    }
    (width, height)
}

fn parse_pathing_integer(value: &str, axis: &str) -> i32 {
    let value = value
        .parse::<f64>()
        .unwrap_or_else(|_| panic!("pathing {axis} coordinate must be a number"));
    let rounded = value.round();
    assert!(
        (value - rounded).abs() < 1.0e-6,
        "pathing {axis} coordinate must be integral, got {value}"
    );
    rounded as i32
}

fn snap_pathing_center(value: &str, axis: &str) -> i32 {
    let value = value
        .parse::<f64>()
        .unwrap_or_else(|_| panic!("pathing {axis} coordinate must be a number"));
    ((value / f64::from(NAV_CELL_WORLD)).round() as i32) * NAV_CELL_WORLD
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
        "pathing footprint must align to the navigation grid"
    );
    assert_eq!(
        min_y_world.rem_euclid(NAV_CELL_WORLD),
        0,
        "pathing footprint must align to the navigation grid"
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
    fn demo_builder_catalog_exposes_every_imported_build_kind() {
        let DemoWorld { simulation, .. } = create_demo_world(1, None);
        let builder = simulation.builder_for_team(Team(0)).expect("blue builder");
        let kinds = [
            BuildKind::Production(ProductionKind::Barracks),
            BuildKind::Production(ProductionKind::RangersHall),
            BuildKind::Production(ProductionKind::OrcishSiegeFactory),
            BuildKind::Production(ProductionKind::IceTrollHut),
            BuildKind::Production(ProductionKind::GryphonRock),
            BuildKind::Tower(CastleFightTowerKind::WatchTower),
            BuildKind::Tower(CastleFightTowerKind::PoofTower),
        ];

        for kind in kinds {
            let rawcode = match kind {
                BuildKind::Production(kind) => kind.definition().rawcode,
                BuildKind::Tower(kind) => kind.definition().rawcode,
            };
            assert!(
                builder.configuration.allows_building(rawcode),
                "builder catalog missing {}",
                kind.label()
            );
        }
    }

    #[test]
    fn build_selectors_read_names_and_costs_from_imported_content() {
        let barracks = BuildKind::Production(ProductionKind::Barracks);
        assert_eq!(barracks.label(), "Barracks");
        assert_eq!(barracks.gold_cost(), 100);
        assert_eq!(barracks.lumber_cost(), 0);
        assert_eq!(barracks.footprint_size(), 4);

        let watch = BuildKind::Tower(CastleFightTowerKind::WatchTower);
        assert_eq!(watch.label(), "Watch Tower");
        assert_eq!(watch.gold_cost(), 150);
        assert_eq!(watch.lumber_cost(), 300);
        assert_eq!(watch.footprint_size(), 4);
    }

    #[test]
    fn normal_demo_starts_with_original_resources_and_no_free_production_buildings() {
        let DemoWorld { simulation, .. } = create_demo_world(1, None);
        for team in [Team(0), Team(1)] {
            let economy = simulation.player_economy(team).expect("player economy");
            assert_eq!(economy.resources.gold, 250);
            assert_eq!(economy.resources.lumber, 125);
            assert_eq!(economy.resources.legendary_points_used, 0);
            assert_eq!(economy.resources.legendary_points_cap, 1);
            assert_eq!(economy.income, 5);
        }
        assert_eq!(
            simulation.building_count(),
            2,
            "only the two castles should be preplaced"
        );
    }

    #[test]
    fn demo_bootstraps_one_builder_per_side_at_original_middle_start_locations() {
        let DemoWorld { simulation, .. } = create_demo_world(1, Some(0));
        let builders = simulation.builders();
        assert_eq!(builders.len(), 2);
        let human = CastleFightBuilderRace::Human.definition();
        assert_eq!(builders[0].team, Team(0));
        assert_eq!(builders[0].position, world_point(-BUILDER_START_X_WORLD, 0));
        assert_eq!(builders[0].profile, human.profile);
        assert_eq!(builders[0].configuration.appearance.rawcode, human.rawcode);
        assert_eq!(builders[0].configuration.locomotion, human.locomotion);
        assert_eq!(builders[0].configuration.build_catalog.len(), 7);
        assert!(builders[0].repair_autocast_enabled);
        assert_eq!(builders[1].team, Team(1));
        assert_eq!(builders[1].position, world_point(BUILDER_START_X_WORLD, 0));
        assert_eq!(builders[1].profile, human.profile);
        assert_eq!(builders[1].configuration, builders[0].configuration);
    }

    #[test]
    fn stress_population_stays_inside_imported_movement_masks() {
        let DemoWorld { simulation, .. } = create_demo_world(1, Some(2_000));
        assert_eq!(simulation.units().len(), 2_000);
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
            vec![BuildingFootprint::new(-193, -64, 133, 128)]
        );
        assert_eq!(
            config.team_build_regions[1],
            vec![BuildingFootprint::new(60, -64, 133, 128)]
        );
        assert_eq!(
            config.targetless_lane,
            Some(TargetlessLane::new(
                -768 * SUBUNITS_PER_WORLD_UNIT,
                768 * SUBUNITS_PER_WORLD_UNIT,
                [
                    -1_920 * SUBUNITS_PER_WORLD_UNIT,
                    1_920 * SUBUNITS_PER_WORLD_UNIT,
                ],
            ))
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

        // The left rear cap is centered at x=-5984 and begins at x=-6144. WC3's build grid has
        // one additional legal column behind that cap, so the wall itself must not be padded into
        // the x=-6176 column.
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
        let top_left_corner = BuildingFootprint::new(-193, 58, 4, 4);
        let bottom_left_corner = BuildingFootprint::new(-193, -62, 4, 4);
        assert!(simulation.can_place_building_for_team(Team(0), top_left_corner));
        assert!(simulation.can_place_building_for_team(Team(0), bottom_left_corner));
        assert!(
            !simulation
                .can_place_building_for_team(Team(0), BuildingFootprint::new(-193, 59, 4, 4))
        );
        assert!(
            !simulation
                .can_place_building_for_team(Team(0), BuildingFootprint::new(-193, -63, 4, 4))
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

        // WC3 admits one snapped nav column beyond the protected NFb/MFb rear edge. A 4x4
        // footprint may use that column, but shifting one more cell outward must still fail.
        assert!(
            simulation.can_place_building_for_team(Team(0), BuildingFootprint::new(-193, 20, 4, 4))
        );
        assert!(
            !simulation
                .can_place_building_for_team(Team(0), BuildingFootprint::new(-194, 20, 4, 4))
        );
        assert!(
            simulation.can_place_building_for_team(Team(1), BuildingFootprint::new(189, 20, 4, 4))
        );
        assert!(
            !simulation.can_place_building_for_team(Team(1), BuildingFootprint::new(190, 20, 4, 4))
        );
    }

    #[test]
    fn original_map_doodads_and_entrance_arch_block_building_placement() {
        let build_blockers = original_doodad_build_blockers();

        // A placed Ruins Firepot inside the right base has a 4x4 default pathing texture.
        assert!(build_blockers.contains(&BuildingFootprint::new(104, 18, 4, 4)));

        // The two large base-entrance ZSas arches deliberately have movement pathing disabled, but
        // the original build cursor still rejects footprints directly through their visible span.
        assert!(build_blockers.contains(&BuildingFootprint::new(-66, -12, 6, 28)));
        assert!(build_blockers.contains(&BuildingFootprint::new(59, -13, 6, 28)));

        let DemoWorld { simulation, .. } = create_demo_world(1, Some(0));
        assert!(
            !simulation.can_place_building_for_team(Team(1), BuildingFootprint::new(60, 0, 4, 4))
        );
        assert!(
            simulation.can_place_building_for_team(Team(1), BuildingFootprint::new(65, 0, 4, 4))
        );
        assert!(
            !simulation.can_place_building_for_team(Team(1), BuildingFootprint::new(104, 18, 1, 1))
        );
    }

    #[test]
    fn original_map_air_pathing_includes_no_mans_land_and_authored_no_fly_blockers() {
        let terrain = original_terrain();
        let config = demo_config(&terrain);

        assert!(config.air_static_blockers.contains(&BuildingFootprint::new(
            CENTRAL_GAP_MIN_X,
            config.navigation_min.y,
            (CENTRAL_GAP_MAX_X - CENTRAL_GAP_MIN_X + 1) as u16,
            (LANE_MIN_Y - config.navigation_min.y) as u16,
        )));
        // YTab at (-1952, 800) is one of the original 2x2 air/build pathing blockers around the
        // lane entrance and must remain part of the independent flying topology.
        assert!(
            config
                .air_static_blockers
                .contains(&BuildingFootprint::new(-62, 24, 2, 2))
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
