use std::fmt;

use serde::Deserialize;

use crate::{
    ArmorProfile, ArmorType, BuilderSpawn, BuildingFootprint, BuildingGameplayProperties,
    BuildingSpawn, CASTLE_FIGHT_CONTENT_REVISION_927, CastleFightBuilderRace,
    CastleFightBuildingKind, CastleFightContentAvailability, CastleFightContentBundle,
    CastleFightContentError, CastleFightContentIdentity, CombatRules, ContentIdentity, DamageType,
    MapVersion, NavCell, SUBUNITS_PER_WORLD_UNIT, SimPoint, Simulation, SimulationConfig,
    TargetlessLane, Team, TerrainElevationMap, TerrainLoadError,
    castle_fight_content_bundle_for_revision,
};

const NAV_CELL_WORLD: i32 = 32;
const NAV_CELL_SUBUNITS: i32 = NAV_CELL_WORLD * SUBUNITS_PER_WORLD_UNIT;
const PLAYABLE_MIN_X_WORLD: i32 = -6_400;
const PLAYABLE_MAX_X_WORLD: i32 = 6_400;
const PLAYABLE_MIN_Y_WORLD: i32 = -3_584;
const PLAYABLE_MAX_Y_WORLD: i32 = 3_584;
const LEFT_BUILD_MIN_X_WORLD: i32 = -6_176;
const LEFT_BUILD_MAX_X_WORLD: i32 = -1_920;
const RIGHT_BUILD_MIN_X_WORLD: i32 = 1_920;
const RIGHT_BUILD_MAX_X_WORLD: i32 = 6_176;
const BUILD_MIN_Y_WORLD: i32 = -2_048;
const BUILD_MAX_Y_WORLD: i32 = 2_048;
const CENTRAL_GAP_MIN_X: i32 = LEFT_BUILD_MAX_X_WORLD / NAV_CELL_WORLD;
const CENTRAL_GAP_MAX_X: i32 = RIGHT_BUILD_MIN_X_WORLD / NAV_CELL_WORLD - 1;
const LANE_MIN_Y: i32 = -24;
const LANE_MAX_Y: i32 = 23;
const STRATEGIC_LANE_MIN_Y_WORLD: i32 = -384;
const STRATEGIC_LANE_MAX_Y_WORLD: i32 = 384;
const CASTLE_CENTER_X_WORLD: i32 = 4_992;
const BUILDER_START_X_WORLD: i32 = 4_352;
const CASTLE_PATHING_SIZE_CELLS: u16 = 16;
const DEVELOPMENT_CASTLE_HEALTH: i32 = 20_000;
const DEVELOPMENT_UPHILL_MISS_CHANCE_PER_10K: u16 = 2_500;
const WALL_DOODAD_RAWCODES: [&str; 6] = ["B002", "B003", "D000", "D001", "D002", "D003"];
const ENTRANCE_ARCH_RAWCODE: &str = "ZSas";
const PATHING_UNFLYABLE_BIT: u8 = 0x02;
const PATHING_UNBUILDABLE_BIT: u8 = 0x04;
const EXTRACTION_TREE_927_R1: &str = "bb38bb165fcee1371557208f99de6cf69c70ba1e";
const TERRAIN_927: &str = include_str!("../../../docs/original_map/extracted/terrain.json");
const PLACED_DOODADS_927: &str =
    include_str!("../../../docs/original_map/extracted/resolved/placed-doodads.tsv");
const MAP_SOURCE_927_R1: &str = include_str!("../data/castle-fight/9.27/map-source-r1.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastleFightReleaseDescriptor {
    pub map_version: MapVersion,
    pub release_revision: &'static str,
    pub content_revision: Option<&'static str>,
    pub availability: CastleFightContentAvailability,
}

pub const CASTLE_FIGHT_REGISTERED_RELEASES: [CastleFightReleaseDescriptor; 2] = [
    CastleFightReleaseDescriptor {
        map_version: MapVersion::CASTLE_FIGHT_9_27,
        release_revision: "r1",
        content_revision: Some(CASTLE_FIGHT_CONTENT_REVISION_927),
        availability: CastleFightContentAvailability::SupportedDevelopmentSubset,
    },
    CastleFightReleaseDescriptor {
        map_version: MapVersion::CASTLE_FIGHT_9_32,
        release_revision: "r1",
        content_revision: None,
        availability: CastleFightContentAvailability::Archived,
    },
];

#[must_use]
pub fn castle_fight_registered_releases() -> &'static [CastleFightReleaseDescriptor] {
    &CASTLE_FIGHT_REGISTERED_RELEASES
}

#[must_use]
pub fn castle_fight_release_descriptor(
    version: MapVersion,
    release_revision: &str,
) -> Option<CastleFightReleaseDescriptor> {
    CASTLE_FIGHT_REGISTERED_RELEASES
        .iter()
        .copied()
        .find(|release| {
            release.map_version == version && release.release_revision == release_revision
        })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastleFightMatchMode {
    DevelopmentSubset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastleFightParticipantConfig {
    pub team: Team,
    pub builder_race: CastleFightBuilderRace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastleFightMatchConfig {
    pub release: CastleFightReleaseDescriptor,
    pub content_identity: CastleFightContentIdentity,
    pub mode: CastleFightMatchMode,
    pub match_seed: u64,
    pub participants: [CastleFightParticipantConfig; 2],
}

impl CastleFightMatchConfig {
    pub fn development_subset(
        version: MapVersion,
        release_revision: &str,
        match_seed: u64,
    ) -> Result<Self, CastleFightMatchSetupError> {
        let release = castle_fight_release_descriptor(version, release_revision).ok_or(
            CastleFightMatchSetupError::UnregisteredReleaseRevision {
                map_version: version,
                release_revision: release_revision.to_owned(),
            },
        )?;
        let bundle = resolve_playable_bundle(release)?;
        Ok(Self {
            release,
            content_identity: bundle.identity,
            mode: CastleFightMatchMode::DevelopmentSubset,
            match_seed,
            participants: [
                CastleFightParticipantConfig {
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
            ],
        })
    }
}

#[derive(Debug, Clone)]
pub struct CastleFightResolvedMatch {
    pub match_config: CastleFightMatchConfig,
    pub content: &'static CastleFightContentBundle,
    pub terrain_source_json: &'static str,
    pub terrain: TerrainElevationMap,
    pub simulation_config: SimulationConfig,
    pub combat_rules: CombatRules,
    pub direct_buildings: Vec<CastleFightBuildingKind>,
}

pub struct CastleFightMatch {
    pub simulation: Simulation,
    pub terrain_source_json: &'static str,
    pub terrain: TerrainElevationMap,
    pub simulation_config: SimulationConfig,
    pub match_config: CastleFightMatchConfig,
    pub content: &'static CastleFightContentBundle,
    pub direct_buildings: Vec<CastleFightBuildingKind>,
}

#[derive(Debug)]
pub enum CastleFightMatchSetupError {
    UnregisteredRelease(MapVersion),
    UnregisteredReleaseRevision {
        map_version: MapVersion,
        release_revision: String,
    },
    ArchivedRelease(CastleFightReleaseDescriptor),
    UnsupportedContent(CastleFightContentError),
    ContentRevisionMismatch {
        expected: &'static str,
        actual: &'static str,
    },
    ContentIdentityMismatch {
        expected: CastleFightContentIdentity,
        actual: CastleFightContentIdentity,
    },
    UnsupportedParticipants,
    MapSource(String),
    Terrain(TerrainLoadError),
}

impl fmt::Display for CastleFightMatchSetupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnregisteredRelease(version) => {
                write!(formatter, "Castle Fight {version} is not registered")
            }
            Self::UnregisteredReleaseRevision {
                map_version,
                release_revision,
            } => write!(
                formatter,
                "Castle Fight {map_version}/{release_revision} is not a registered release revision"
            ),
            Self::ArchivedRelease(release) => write!(
                formatter,
                "Castle Fight {}/{} is archived but not playable by this build",
                release.map_version, release.release_revision
            ),
            Self::UnsupportedContent(error) => error.fmt(formatter),
            Self::ContentRevisionMismatch { expected, actual } => write!(
                formatter,
                "selected content revision {expected} resolved to incompatible revision {actual}"
            ),
            Self::ContentIdentityMismatch { expected, actual } => write!(
                formatter,
                "selected gameplay bundle identity {}:{:#018x} resolved to {}:{:#018x}",
                expected.schema_version,
                expected.gameplay_hash,
                actual.schema_version,
                actual.gameplay_hash
            ),
            Self::UnsupportedParticipants => formatter.write_str(
                "the current development match requires exactly team 0 and team 1 participants",
            ),
            Self::MapSource(error) => formatter.write_str(error),
            Self::Terrain(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for CastleFightMatchSetupError {}

impl From<TerrainLoadError> for CastleFightMatchSetupError {
    fn from(error: TerrainLoadError) -> Self {
        Self::Terrain(error)
    }
}

pub fn resolve_castle_fight_match(
    config: CastleFightMatchConfig,
) -> Result<CastleFightResolvedMatch, CastleFightMatchSetupError> {
    validate_participants(config.participants)?;
    let content = resolve_playable_bundle(config.release)?;
    if content.identity != config.content_identity {
        return Err(CastleFightMatchSetupError::ContentIdentityMismatch {
            expected: config.content_identity,
            actual: content.identity,
        });
    }
    let expected_revision = config
        .release
        .content_revision
        .expect("playable release must declare a content revision");
    if content.revision != expected_revision {
        return Err(CastleFightMatchSetupError::ContentRevisionMismatch {
            expected: expected_revision,
            actual: content.revision,
        });
    }
    validate_map_source_927()?;
    let terrain = TerrainElevationMap::from_wc3_terrain_json(TERRAIN_927)?;
    let simulation_config = simulation_config_927(&terrain, content, config.match_seed);
    let combat_rules = CombatRules {
        terrain_elevation: Some(terrain.clone()),
        uphill_miss_chance_per_10k: DEVELOPMENT_UPHILL_MISS_CHANCE_PER_10K,
        damage_rules: content.damage_rules,
    };
    Ok(CastleFightResolvedMatch {
        match_config: config,
        content,
        terrain_source_json: TERRAIN_927,
        terrain,
        simulation_config,
        combat_rules,
        direct_buildings: content.direct_building_kinds(),
    })
}

pub fn create_castle_fight_match(
    config: CastleFightMatchConfig,
    workers: usize,
) -> Result<CastleFightMatch, CastleFightMatchSetupError> {
    let resolved = resolve_castle_fight_match(config)?;
    let simulation_config = resolved.simulation_config.clone();
    let mut simulation = Simulation::new_with_gameplay_bundle(
        simulation_config.clone(),
        workers,
        resolved.combat_rules.clone(),
        resolved.content.identity.into(),
    );
    let direct_rawcodes = resolved
        .direct_buildings
        .iter()
        .map(|kind| {
            kind.rawcode(resolved.content)
                .expect("resolved direct building must belong to content bundle")
        })
        .collect::<Vec<_>>();

    for participant in resolved.match_config.participants {
        let builder = resolved
            .content
            .builder(participant.builder_race)
            .expect("selected builder race must belong to playable content bundle");
        let x = match participant.team.0 {
            0 => -BUILDER_START_X_WORLD,
            1 => BUILDER_START_X_WORLD,
            _ => unreachable!("participants validated before match construction"),
        };
        simulation.spawn_builder(BuilderSpawn {
            team: participant.team,
            position: world_point(x, 0),
            profile: builder.profile,
            configuration: builder.configuration_with_catalog(direct_rawcodes.clone()),
            repair_autocast_enabled: builder.repair_autocast_enabled_by_default,
        });
    }

    let castle_properties = BuildingGameplayProperties {
        content: Some(ContentIdentity {
            rawcode: u32::from_be_bytes(*b"hcas"),
            name: "Main Castle",
        }),
        repair_time_ticks: Some(resolved.content.main_castle_repair_time_ticks),
        damage_type: DamageType::Normal,
        armor: ArmorProfile::new(ArmorType::Fortified, 5),
        ..BuildingGameplayProperties::default()
    };
    simulation.spawn_building_with_properties(
        passive_structure(Team(0), castle_footprint(0), DEVELOPMENT_CASTLE_HEALTH),
        castle_properties,
    );
    simulation.spawn_building_with_properties(
        passive_structure(Team(1), castle_footprint(1), DEVELOPMENT_CASTLE_HEALTH),
        castle_properties,
    );

    Ok(CastleFightMatch {
        simulation,
        terrain_source_json: resolved.terrain_source_json,
        terrain: resolved.terrain,
        simulation_config,
        match_config: resolved.match_config,
        content: resolved.content,
        direct_buildings: resolved.direct_buildings,
    })
}

fn resolve_playable_bundle(
    release: CastleFightReleaseDescriptor,
) -> Result<&'static CastleFightContentBundle, CastleFightMatchSetupError> {
    match release.availability {
        CastleFightContentAvailability::Archived => {
            Err(CastleFightMatchSetupError::ArchivedRelease(release))
        }
        CastleFightContentAvailability::Unavailable => Err(
            CastleFightMatchSetupError::UnregisteredRelease(release.map_version),
        ),
        CastleFightContentAvailability::SupportedDevelopmentSubset
        | CastleFightContentAvailability::SupportedFull => {
            let content_revision = release
                .content_revision
                .expect("playable release must declare a content revision");
            castle_fight_content_bundle_for_revision(release.map_version, content_revision)
                .map_err(CastleFightMatchSetupError::UnsupportedContent)
        }
    }
}

fn validate_participants(
    participants: [CastleFightParticipantConfig; 2],
) -> Result<(), CastleFightMatchSetupError> {
    let mut teams = participants.map(|participant| participant.team.0);
    teams.sort_unstable();
    if teams != [0, 1] {
        return Err(CastleFightMatchSetupError::UnsupportedParticipants);
    }
    Ok(())
}

fn simulation_config_927(
    terrain: &TerrainElevationMap,
    content: &CastleFightContentBundle,
    match_seed: u64,
) -> SimulationConfig {
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
    let mut air_static_blockers = no_mans_land_blockers;
    air_static_blockers.extend(original_air_pathing_blockers());

    SimulationConfig {
        match_seed,
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
            STRATEGIC_LANE_MIN_Y_WORLD * SUBUNITS_PER_WORLD_UNIT,
            STRATEGIC_LANE_MAX_Y_WORLD * SUBUNITS_PER_WORLD_UNIT,
        )),
        team_objective: [world_point(4_720, 0), world_point(-4_720, 0)],
        economy: content.economy,
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

fn castle_footprint(team: u8) -> BuildingFootprint {
    let center = match team {
        0 => -CASTLE_CENTER_X_WORLD,
        1 => CASTLE_CENTER_X_WORLD,
        _ => unreachable!("Castle Fight development mode has exactly two teams"),
    };
    BuildingFootprint::new(
        center.div_euclid(NAV_CELL_WORLD) - i32::from(CASTLE_PATHING_SIZE_CELLS) / 2,
        -i32::from(CASTLE_PATHING_SIZE_CELLS) / 2,
        CASTLE_PATHING_SIZE_CELLS,
        CASTLE_PATHING_SIZE_CELLS,
    )
}

fn original_wall_blockers() -> Vec<BuildingFootprint> {
    PLACED_DOODADS_927
        .lines()
        .skip(1)
        .filter_map(|line| {
            let columns = line.split('\t').collect::<Vec<_>>();
            let rawcode = columns.get(2).copied()?;
            if !WALL_DOODAD_RAWCODES.contains(&rawcode) {
                return None;
            }
            let x = parse_pathing_integer(columns[5], "x");
            let y = parse_pathing_integer(columns[6], "y");
            let angle = columns[8]
                .parse::<f64>()
                .expect("wall angle must be numeric");
            let texture_width = columns[18]
                .parse::<u16>()
                .expect("wall pathing width must be present");
            let texture_height = columns[19]
                .parse::<u16>()
                .expect("wall pathing height must be present");
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
    for line in PLACED_DOODADS_927.lines().skip(1) {
        let columns = line.split('\t').collect::<Vec<_>>();
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
            .expect("doodad angle must be numeric");
        let texture_width = columns[18]
            .parse::<u16>()
            .expect("pathing width must be present");
        let texture_height = columns[19]
            .parse::<u16>()
            .expect("pathing height must be present");
        let pathing_cells = u32::from(texture_width) * u32::from(texture_height);
        let quarter_turns = pathing_quarter_turns(rawcode, angle, texture_width, texture_height);
        if blocked_count == pathing_cells {
            let (width, height) =
                rotated_pathing_dimensions(texture_width, texture_height, quarter_turns);
            blockers.push(centered_world_footprint(x, y, width, height));
            continue;
        }
        blockers.extend(pathing_mask_cell_blockers(
            x,
            y,
            texture_width,
            texture_height,
            quarter_turns,
            columns[25],
            pathing_bit,
        ));
    }
    blockers
}

fn original_entrance_arch_build_blockers() -> Vec<BuildingFootprint> {
    PLACED_DOODADS_927
        .lines()
        .skip(1)
        .filter_map(|line| {
            let columns = line.split('\t').collect::<Vec<_>>();
            (columns.get(2).copied() == Some(ENTRANCE_ARCH_RAWCODE)).then(|| {
                centered_world_footprint(
                    snap_pathing_center(columns[5], "x"),
                    snap_pathing_center(columns[6], "y"),
                    6,
                    28,
                )
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
    let rows = hex_rows.split('/').collect::<Vec<_>>();
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
            blockers.push(BuildingFootprint::new(
                (center_x + rotated_x2 * (NAV_CELL_WORLD / 2)).div_euclid(NAV_CELL_WORLD),
                (center_y + rotated_y2 * (NAV_CELL_WORLD / 2)).div_euclid(NAV_CELL_WORLD),
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
        .unwrap_or_else(|_| panic!("pathing {axis} coordinate must be numeric"));
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
        .unwrap_or_else(|_| panic!("pathing {axis} coordinate must be numeric"));
    ((value / f64::from(NAV_CELL_WORLD)).round() as i32) * NAV_CELL_WORLD
}

fn world_rect_footprint(
    min_x_world: i32,
    min_y_world: i32,
    max_x_world: i32,
    max_y_world: i32,
) -> BuildingFootprint {
    let width = (max_x_world - min_x_world).div_euclid(NAV_CELL_WORLD);
    let height = (max_y_world - min_y_world).div_euclid(NAV_CELL_WORLD);
    BuildingFootprint::new(
        min_x_world.div_euclid(NAV_CELL_WORLD),
        min_y_world.div_euclid(NAV_CELL_WORLD),
        u16::try_from(width).expect("world rectangle width overflow"),
        u16::try_from(height).expect("world rectangle height overflow"),
    )
}

fn centered_world_footprint(
    center_x: i32,
    center_y: i32,
    width: u16,
    height: u16,
) -> BuildingFootprint {
    let min_x_world = center_x - i32::from(width) * NAV_CELL_WORLD / 2;
    let min_y_world = center_y - i32::from(height) * NAV_CELL_WORLD / 2;
    assert_eq!(min_x_world.rem_euclid(NAV_CELL_WORLD), 0);
    assert_eq!(min_y_world.rem_euclid(NAV_CELL_WORLD), 0);
    BuildingFootprint::new(
        min_x_world.div_euclid(NAV_CELL_WORLD),
        min_y_world.div_euclid(NAV_CELL_WORLD),
        width,
        height,
    )
}

fn world_point(x: i32, y: i32) -> SimPoint {
    SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, y * SUBUNITS_PER_WORLD_UNIT)
}

#[derive(Debug, Deserialize)]
struct MapSourceManifest {
    schema_version: u32,
    map_version: String,
    release_revision: String,
    extraction_git_tree: String,
    terrain_fnv64: u64,
    placed_doodads_fnv64: u64,
}

fn validate_map_source_927() -> Result<(), CastleFightMatchSetupError> {
    let manifest: MapSourceManifest = serde_json::from_str(MAP_SOURCE_927_R1)
        .map_err(|error| CastleFightMatchSetupError::MapSource(error.to_string()))?;
    if manifest.schema_version != 1
        || manifest.map_version != "9.27"
        || manifest.release_revision != "r1"
        || manifest.extraction_git_tree != EXTRACTION_TREE_927_R1
    {
        return Err(CastleFightMatchSetupError::MapSource(
            "9.27/r1 map-source manifest identity is invalid".to_owned(),
        ));
    }
    let terrain = fnv64_canonical_text(TERRAIN_927);
    let doodads = fnv64_canonical_text(PLACED_DOODADS_927);
    if terrain != manifest.terrain_fnv64 || doodads != manifest.placed_doodads_fnv64 {
        return Err(CastleFightMatchSetupError::MapSource(format!(
            "retained 9.27/r1 map evidence drifted (terrain {terrain:#018x}, doodads {doodads:#018x}); register a new release/content revision instead"
        )));
    }
    Ok(())
}

fn fnv64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn fnv64_canonical_text(contents: &str) -> u64 {
    if contents
        .as_bytes()
        .windows(2)
        .any(|window| window == b"\r\n")
    {
        fnv64(contents.replace("\r\n", "\n").as_bytes())
    } else {
        fnv64(contents.as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_release_selector_matches_retained_release_registry() {
        let manifest: serde_json::Value =
            serde_json::from_str(include_str!("../../../docs/original_map/releases.json"))
                .expect("retained release registry must remain valid JSON");
        let releases = manifest["releases"]
            .as_array()
            .expect("retained release registry must contain releases");
        assert_eq!(releases.len(), CASTLE_FIGHT_REGISTERED_RELEASES.len());
        for descriptor in CASTLE_FIGHT_REGISTERED_RELEASES {
            let version = descriptor.map_version.to_string();
            let retained = releases
                .iter()
                .find(|release| {
                    release["map_version"].as_str() == Some(version.as_str())
                        && release["revision"].as_str() == Some(descriptor.release_revision)
                })
                .expect("runtime release must exist in retained registry");
            let expected_availability = match descriptor.availability {
                CastleFightContentAvailability::SupportedDevelopmentSubset => {
                    "supported-development-subset"
                }
                CastleFightContentAvailability::SupportedFull => "supported-full",
                CastleFightContentAvailability::Archived => "archived",
                CastleFightContentAvailability::Unavailable => "unavailable",
            };
            assert_eq!(
                retained["availability"].as_str(),
                Some(expected_availability)
            );
            assert_eq!(
                retained["runtime_content"]["content_revision"].as_str(),
                descriptor.content_revision
            );
        }
    }

    #[test]
    fn retained_map_hash_normalizes_checkout_line_endings() {
        assert_eq!(
            fnv64_canonical_text("a\nb\n"),
            fnv64_canonical_text("a\r\nb\r\n")
        );
    }

    #[test]
    fn registered_releases_expose_playability_without_substitution() {
        let current = castle_fight_release_descriptor(MapVersion::CASTLE_FIGHT_9_27, "r1").unwrap();
        assert_eq!(current.release_revision, "r1");
        assert_eq!(
            current.content_revision,
            Some(CASTLE_FIGHT_CONTENT_REVISION_927)
        );
        assert_eq!(
            current.availability,
            CastleFightContentAvailability::SupportedDevelopmentSubset
        );
        let archived =
            castle_fight_release_descriptor(MapVersion::CASTLE_FIGHT_9_32, "r1").unwrap();
        assert_eq!(
            archived.availability,
            CastleFightContentAvailability::Archived
        );
        assert!(matches!(
            CastleFightMatchConfig::development_subset(MapVersion::CASTLE_FIGHT_9_32, "r1", 1),
            Err(CastleFightMatchSetupError::ArchivedRelease(_))
        ));
        assert!(matches!(
            CastleFightMatchConfig::development_subset(MapVersion::CASTLE_FIGHT_9_27, "r2", 1),
            Err(CastleFightMatchSetupError::UnregisteredReleaseRevision { .. })
        ));
    }

    #[test]
    fn resolved_match_rejects_stale_content_identity() {
        let mut config = CastleFightMatchConfig::development_subset(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            0x4341_5354_4c45,
        )
        .unwrap();
        config.content_identity.gameplay_hash ^= 1;
        assert!(matches!(
            resolve_castle_fight_match(config),
            Err(CastleFightMatchSetupError::ContentIdentityMismatch { .. })
        ));
    }

    #[test]
    fn headless_match_construction_is_repeatable_and_content_pinned() {
        let config = CastleFightMatchConfig::development_subset(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            0x4341_5354_4c45,
        )
        .unwrap();
        let first = create_castle_fight_match(config, 1).unwrap();
        let second = create_castle_fight_match(config, 4).unwrap();
        assert_eq!(first.simulation.checksum(), second.simulation.checksum());
        assert_eq!(first.simulation.tick(), 0);
        assert_eq!(first.simulation.building_count(), 2);
        assert_eq!(first.content.identity, config.content_identity);
        assert_eq!(first.direct_buildings.len(), 7);
    }

    #[test]
    fn resolved_match_keeps_original_development_topology() {
        let config = CastleFightMatchConfig::development_subset(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            0x4341_5354_4c45,
        )
        .unwrap();
        let resolved = resolve_castle_fight_match(config).unwrap();
        assert_eq!(
            resolved.simulation_config.navigation_min,
            NavCell::new(-200, -112)
        );
        assert_eq!(
            resolved.simulation_config.navigation_max,
            NavCell::new(199, 111)
        );
        assert_eq!(
            resolved.simulation_config.targetless_lane,
            Some(TargetlessLane::new(
                -384 * SUBUNITS_PER_WORLD_UNIT,
                384 * SUBUNITS_PER_WORLD_UNIT
            ))
        );
        assert_eq!(resolved.simulation_config.team_build_regions[0].len(), 1);
        assert_eq!(resolved.simulation_config.team_build_regions[1].len(), 1);
    }
}
