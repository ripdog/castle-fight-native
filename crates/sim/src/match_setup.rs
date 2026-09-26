use std::fmt;

use serde::Deserialize;

use crate::{
    ArmorProfile, ArmorType, BuilderSpawn, BuildingFootprint, BuildingGameplayProperties,
    BuildingSpawn, CASTLE_FIGHT_CONTENT_REVISION_927, CastleFightBuilderRace,
    CastleFightBuildingKind, CastleFightContentAvailability, CastleFightContentBundle,
    CastleFightContentError, CastleFightContentIdentity, CombatRules, ContentIdentity, DamageType,
    MapVersion, NavCell, PlayerConfig, PlayerId, SUBUNITS_PER_WORLD_UNIT, SimPoint, Simulation,
    SimulationConfig, TargetlessLane, Team, TerrainElevationMap, TerrainLoadError,
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
// The 9.27 road is seven 128-world-unit building rows wide. Behind each castle,
// its middle three rows are mossy ground where buildings are allowed.
const ROAD_HALF_WIDTH_WORLD: i32 = 448;
const REAR_MOSS_HALF_WIDTH_WORLD: i32 = 192;
const REAR_MOSS_CENTER_X_WORLD: i32 = 5_760;
const DEVELOPMENT_CASTLE_HEALTH: i32 = 20_000;
const DEVELOPMENT_UPHILL_MISS_CHANCE_PER_10K: u16 = 2_500;
const WALL_DOODAD_RAWCODES: [&str; 6] = ["B002", "B003", "D000", "D001", "D002", "D003"];
const ENTRANCE_ARCH_RAWCODE: &str = "ZSas";
const PATHING_UNFLYABLE_BIT: u8 = 0x02;
const PATHING_UNBUILDABLE_BIT: u8 = 0x04;
const EXTRACTION_TREE_927_R1: &str = "8ea806dca331ff254995e94e6f0baf225a14bf10";
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
    pub id: PlayerId,
    pub team: Team,
    pub builder_race: CastleFightBuilderRace,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastleFightMatchConfig {
    pub release: CastleFightReleaseDescriptor,
    pub content_identity: CastleFightContentIdentity,
    pub mode: CastleFightMatchMode,
    pub match_seed: u64,
    pub participants: Vec<CastleFightParticipantConfig>,
}

impl CastleFightMatchConfig {
    pub fn development_subset(
        version: MapVersion,
        release_revision: &str,
        match_seed: u64,
    ) -> Result<Self, CastleFightMatchSetupError> {
        Self::development_subset_with_participants(
            version,
            release_revision,
            match_seed,
            vec![
                CastleFightParticipantConfig {
                    id: PlayerId(0),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(6),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
            ],
        )
    }

    pub fn development_subset_with_participants(
        version: MapVersion,
        release_revision: &str,
        match_seed: u64,
        mut participants: Vec<CastleFightParticipantConfig>,
    ) -> Result<Self, CastleFightMatchSetupError> {
        let release = castle_fight_release_descriptor(version, release_revision).ok_or(
            CastleFightMatchSetupError::UnregisteredReleaseRevision {
                map_version: version,
                release_revision: release_revision.to_owned(),
            },
        )?;
        let bundle = resolve_playable_bundle(release)?;
        validate_participants(&participants)?;
        participants.sort_unstable_by_key(|participant| participant.id);
        Ok(Self {
            release,
            content_identity: bundle.identity,
            mode: CastleFightMatchMode::DevelopmentSubset,
            match_seed,
            participants,
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
                "Castle Fight 9.27 development matches support balanced 1v1, 2v2, or 3v3 rosters using authored slots 0/1/2 versus 6/7/8",
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
    mut config: CastleFightMatchConfig,
) -> Result<CastleFightResolvedMatch, CastleFightMatchSetupError> {
    validate_participants(&config.participants)?;
    config
        .participants
        .sort_unstable_by_key(|participant| participant.id);
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
        direct_buildings: content.playable_human_direct_building_kinds(),
    })
}

pub fn create_castle_fight_match(
    config: CastleFightMatchConfig,
    workers: usize,
) -> Result<CastleFightMatch, CastleFightMatchSetupError> {
    let resolved = resolve_castle_fight_match(config)?;
    let simulation_config = resolved.simulation_config.clone();
    let player_configs = resolved
        .match_config
        .participants
        .iter()
        .map(|participant| PlayerConfig {
            id: participant.id,
            team: participant.team,
        })
        .collect::<Vec<_>>();
    let mut simulation = Simulation::new_with_gameplay_bundle_and_players(
        simulation_config.clone(),
        workers,
        resolved.combat_rules.clone(),
        resolved.content.identity.into(),
        &player_configs,
    );
    let direct_rawcodes = resolved
        .direct_buildings
        .iter()
        .map(|kind| {
            kind.rawcode(resolved.content)
                .expect("resolved direct building must belong to content bundle")
        })
        .collect::<Vec<_>>();

    for participant in resolved.match_config.participants.iter().copied() {
        let builder = resolved
            .content
            .builder(participant.builder_race)
            .expect("selected builder race must belong to playable content bundle");
        let x = match participant.team.0 {
            0 => -BUILDER_START_X_WORLD,
            1 => BUILDER_START_X_WORLD,
            _ => unreachable!("participants validated before match construction"),
        };
        let y = match participant.id.0 {
            0 | 6 => 128,
            1 | 7 => 0,
            2 | 8 => -128,
            _ => unreachable!("participants validated against Castle Fight player slots"),
        };
        simulation.spawn_builder_for_player(
            participant.id,
            BuilderSpawn {
                team: participant.team,
                position: world_point(x, y),
                profile: builder.profile,
                configuration: builder.configuration_with_catalog(direct_rawcodes.clone()),
                repair_autocast_enabled: builder.repair_autocast_enabled_by_default,
            },
        );
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
    for team in [Team(0), Team(1)] {
        let owner = resolved
            .match_config
            .participants
            .iter()
            .filter(|participant| participant.team == team)
            .map(|participant| participant.id)
            .min()
            .expect("validated Castle Fight team must contain a player");
        let castle = simulation.spawn_building_for_player_with_properties(
            owner,
            passive_structure(team, castle_footprint(team.0), DEVELOPMENT_CASTLE_HEALTH),
            castle_properties,
        );
        simulation
            .register_team_objective(team, castle)
            .expect("authored main castle must be a valid team objective");
    }

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
    participants: &[CastleFightParticipantConfig],
) -> Result<(), CastleFightMatchSetupError> {
    if !matches!(participants.len(), 2 | 4 | 6) {
        return Err(CastleFightMatchSetupError::UnsupportedParticipants);
    }
    let mut ids = participants
        .iter()
        .map(|participant| participant.id)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    if ids.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(CastleFightMatchSetupError::UnsupportedParticipants);
    }
    let team_size = participants.len() / 2;
    let expected_ids =
        (0..team_size)
            .map(|slot| PlayerId(u8::try_from(slot).expect("Castle Fight team size fits u8")))
            .chain((0..team_size).map(|slot| {
                PlayerId(6 + u8::try_from(slot).expect("Castle Fight team size fits u8"))
            }))
            .collect::<Vec<_>>();
    if ids != expected_ids {
        return Err(CastleFightMatchSetupError::UnsupportedParticipants);
    }
    for participant in participants {
        let expected_team = if participant.id.0 <= 2 {
            Team(0)
        } else {
            Team(1)
        };
        if participant.team != expected_team {
            return Err(CastleFightMatchSetupError::UnsupportedParticipants);
        }
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
    let navigation_height = u16::try_from(navigation_max.y - navigation_min.y + 1)
        .expect("Castle Fight navigation height must fit a footprint");
    let side_dead_space_blockers = vec![
        BuildingFootprint::new(
            navigation_min.x,
            navigation_min.y,
            u16::try_from(left_build_region.min_x - navigation_min.x)
                .expect("left dead-space width must fit a footprint"),
            navigation_height,
        ),
        BuildingFootprint::new(
            right_build_region.max_x() + 1,
            navigation_min.y,
            u16::try_from(navigation_max.x - right_build_region.max_x())
                .expect("right dead-space width must fit a footprint"),
            navigation_height,
        ),
    ];
    let mut static_blockers = no_mans_land_blockers.clone();
    static_blockers.extend(side_dead_space_blockers.iter().copied());
    static_blockers.extend(original_wall_blockers());
    let mut air_static_blockers = no_mans_land_blockers;
    air_static_blockers.extend(side_dead_space_blockers);
    air_static_blockers.extend(original_air_pathing_blockers());

    let mut build_static_blockers = original_doodad_build_blockers();
    let castle_half_size = i32::from(CASTLE_PATHING_SIZE_CELLS) * NAV_CELL_WORLD / 2;
    for (inner_x, castle_front_x, castle_rear_x, moss_center_x, rear_x) in [
        (
            LEFT_BUILD_MAX_X_WORLD,
            -CASTLE_CENTER_X_WORLD + castle_half_size,
            -CASTLE_CENTER_X_WORLD - castle_half_size,
            -REAR_MOSS_CENTER_X_WORLD,
            LEFT_BUILD_MIN_X_WORLD,
        ),
        (
            RIGHT_BUILD_MIN_X_WORLD,
            CASTLE_CENTER_X_WORLD - castle_half_size,
            CASTLE_CENTER_X_WORLD + castle_half_size,
            REAR_MOSS_CENTER_X_WORLD,
            RIGHT_BUILD_MAX_X_WORLD,
        ),
    ] {
        let (path_min_x, path_max_x) = (inner_x.min(castle_front_x), inner_x.max(castle_front_x));
        build_static_blockers.push(world_rect_footprint(
            path_min_x,
            -ROAD_HALF_WIDTH_WORLD,
            path_max_x,
            ROAD_HALF_WIDTH_WORLD,
        ));
        let (castle_min_x, castle_max_x) = (
            castle_front_x.min(castle_rear_x),
            castle_front_x.max(castle_rear_x),
        );
        for (min_y, max_y) in [
            (-ROAD_HALF_WIDTH_WORLD, -castle_half_size),
            (castle_half_size, ROAD_HALF_WIDTH_WORLD),
        ] {
            build_static_blockers.push(world_rect_footprint(
                castle_min_x,
                min_y,
                castle_max_x,
                max_y,
            ));
        }
        let (rear_min_x, rear_max_x) = (castle_rear_x.min(rear_x), castle_rear_x.max(rear_x));
        for (min_y, max_y) in [
            (-ROAD_HALF_WIDTH_WORLD, -REAR_MOSS_HALF_WIDTH_WORLD),
            (REAR_MOSS_HALF_WIDTH_WORLD, ROAD_HALF_WIDTH_WORLD),
        ] {
            build_static_blockers.push(world_rect_footprint(rear_min_x, min_y, rear_max_x, max_y));
        }
        let moss_min_x = moss_center_x - REAR_MOSS_HALF_WIDTH_WORLD;
        let moss_max_x = moss_center_x + REAR_MOSS_HALF_WIDTH_WORLD;
        for (min_x, max_x) in [(rear_min_x, moss_min_x), (moss_max_x, rear_max_x)] {
            build_static_blockers.push(world_rect_footprint(
                min_x,
                -REAR_MOSS_HALF_WIDTH_WORLD,
                max_x,
                REAR_MOSS_HALF_WIDTH_WORLD,
            ));
        }
    }

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
        build_static_blockers,
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
    use crate::CastleFightProductionKind;

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
        let first = create_castle_fight_match(config.clone(), 1).unwrap();
        let second = create_castle_fight_match(config.clone(), 4).unwrap();
        assert_eq!(first.simulation.checksum(), second.simulation.checksum());
        assert_eq!(first.simulation.tick(), 0);
        assert_eq!(first.simulation.building_count(), 2);
        assert_eq!(first.content.identity, config.content_identity);
        assert_eq!(first.direct_buildings.len(), 6);
    }

    #[test]
    fn participant_rosters_follow_authored_wc3_slot_prefixes() {
        let invalid = CastleFightMatchConfig::development_subset_with_participants(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            0x4341_5354_4c45,
            vec![
                CastleFightParticipantConfig {
                    id: PlayerId(1),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(7),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
            ],
        );
        assert!(matches!(
            invalid,
            Err(CastleFightMatchSetupError::UnsupportedParticipants)
        ));

        let three_vs_three = CastleFightMatchConfig::development_subset_with_participants(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            0x4341_5354_4c45,
            vec![
                CastleFightParticipantConfig {
                    id: PlayerId(0),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(1),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(2),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(6),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(7),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(8),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
            ],
        )
        .unwrap();
        assert_eq!(
            three_vs_three
                .participants
                .iter()
                .map(|participant| participant.id)
                .collect::<Vec<_>>(),
            vec![
                PlayerId(0),
                PlayerId(1),
                PlayerId(2),
                PlayerId(6),
                PlayerId(7),
                PlayerId(8),
            ]
        );
    }

    #[test]
    fn two_vs_two_match_has_one_builder_and_economy_per_player() {
        let config = CastleFightMatchConfig::development_subset_with_participants(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            0x4341_5354_4c45,
            vec![
                CastleFightParticipantConfig {
                    id: PlayerId(0),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(1),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(6),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(7),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
            ],
        )
        .unwrap();
        let mut game = create_castle_fight_match(config, 1).unwrap();
        assert_eq!(game.simulation.players().len(), 4);
        assert_eq!(game.simulation.builders().len(), 4);
        for id in [PlayerId(0), PlayerId(1), PlayerId(6), PlayerId(7)] {
            assert!(game.simulation.builder_for_player(id).is_some());
            let resources = game.simulation.player_resources_for(id).unwrap();
            assert_eq!(resources.gold, 250);
            assert_eq!(resources.lumber, 125);
        }
        assert!(game.simulation.player_resources(Team(0)).is_none());
        assert!(game.simulation.player_resources(Team(1)).is_none());
        assert!(
            game.simulation
                .debug_grant_player_resources_for(PlayerId(0), 100, 50)
        );
        assert_eq!(
            game.simulation
                .player_resources_for(PlayerId(0))
                .unwrap()
                .gold,
            350
        );
        assert_eq!(
            game.simulation
                .player_resources_for(PlayerId(1))
                .unwrap()
                .gold,
            250
        );
        let castles = game.simulation.buildings();
        assert_eq!(castles.len(), 2);
        assert_eq!(castles[0].team, Team(0));
        assert_eq!(castles[0].owner, Some(PlayerId(0)));
        assert_eq!(castles[1].team, Team(1));
        assert_eq!(castles[1].owner, Some(PlayerId(6)));
    }

    #[test]
    fn allied_players_keep_independent_building_ownership_and_refunds() {
        let config = CastleFightMatchConfig::development_subset_with_participants(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            0x4341_5354_4c45,
            vec![
                CastleFightParticipantConfig {
                    id: PlayerId(0),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(1),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(6),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(7),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
            ],
        )
        .unwrap();
        let mut game = create_castle_fight_match(config, 1).unwrap();
        let builder = game
            .simulation
            .builder_for_player(PlayerId(0))
            .expect("player 0 builder");
        let barracks = game
            .content
            .production_building(CastleFightProductionKind::Barracks)
            .expect("Barracks in development content");
        let footprint = BuildingFootprint::new(-138, 16, 4, 4);
        game.simulation
            .order_builder_purchase_building_with_properties_as(
                PlayerId(0),
                builder.id,
                barracks.spawn(Team(0), footprint),
                barracks.gameplay_properties(),
            )
            .unwrap();
        assert_eq!(
            game.simulation
                .player_resources_for(PlayerId(0))
                .unwrap()
                .gold,
            150
        );
        assert_eq!(
            game.simulation
                .player_resources_for(PlayerId(1))
                .unwrap()
                .gold,
            250
        );

        let building = (0..64)
            .find_map(|_| {
                game.simulation.step();
                game.simulation.buildings().into_iter().find(|building| {
                    building.content.map(|content| content.rawcode) == Some(barracks.rawcode)
                })
            })
            .expect("builder should reach the legal Barracks site and begin construction");
        assert_eq!(building.owner, Some(PlayerId(0)));
        assert_eq!(building.team, Team(0));
        assert!(building.construction_complete_tick.is_some());

        game.simulation
            .cancel_building_construction_for_player(PlayerId(0), building.id)
            .unwrap();
        assert_eq!(
            game.simulation
                .player_resources_for(PlayerId(0))
                .unwrap()
                .gold,
            250
        );
        assert_eq!(
            game.simulation
                .player_resources_for(PlayerId(1))
                .unwrap()
                .gold,
            250
        );
    }

    #[test]
    fn production_preserves_the_owning_player_on_spawned_units() {
        let config = CastleFightMatchConfig::development_subset_with_participants(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            0x4341_5354_4c45,
            vec![
                CastleFightParticipantConfig {
                    id: PlayerId(0),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(1),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(6),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(7),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
            ],
        )
        .unwrap();
        let mut game = create_castle_fight_match(config, 1).unwrap();
        let barracks = game
            .content
            .production_building(CastleFightProductionKind::Barracks)
            .expect("Barracks in development content");
        let building = barracks.spawn(Team(0), BuildingFootprint::new(-138, 16, 4, 4));
        let initial_delay = building
            .production
            .expect("Barracks must produce units")
            .initial_delay_ticks;
        game.simulation.spawn_building_for_player_with_properties(
            PlayerId(1),
            building,
            barracks.gameplay_properties(),
        );

        for _ in 0..=initial_delay {
            game.simulation.step();
        }
        let produced = game
            .simulation
            .units()
            .into_iter()
            .find(|unit| unit.owner == PlayerId(1))
            .expect("player 1 Barracks should produce a player 1 unit");
        assert_eq!(produced.team, Team(0));
        assert_eq!(
            produced.content.map(|content| content.rawcode),
            Some(
                game.content
                    .unit(barracks.unit)
                    .expect("Barracks unit must belong to development content")
                    .rawcode
            )
        );
    }

    #[test]
    fn disconnected_owner_delegates_only_their_builder_to_connected_teammates() {
        let config = CastleFightMatchConfig::development_subset_with_participants(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            0x4341_5354_4c45,
            vec![
                CastleFightParticipantConfig {
                    id: PlayerId(0),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(1),
                    team: Team(0),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(6),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
                CastleFightParticipantConfig {
                    id: PlayerId(7),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                },
            ],
        )
        .unwrap();
        let mut game = create_castle_fight_match(config, 1).unwrap();
        let builder = game
            .simulation
            .builder_for_player(PlayerId(0))
            .expect("player 0 builder");
        let destination = SimPoint::new(
            builder.position.x + 64 * SUBUNITS_PER_WORLD_UNIT,
            builder.position.y,
        );

        assert_eq!(
            game.simulation
                .order_builder_move_as(PlayerId(1), builder.id, destination),
            Err(crate::BuilderCommandError::NotAuthorized)
        );
        let western_castle = game.simulation.team_objective(Team(0)).unwrap();
        assert!(
            !game
                .simulation
                .can_player_control_building(PlayerId(1), western_castle)
        );
        assert!(game.simulation.set_player_connection_status(
            PlayerId(0),
            crate::PlayerConnectionStatus::Disconnected
        ));
        assert_eq!(game.simulation.lifecycle(), crate::MatchLifecycle::Running);
        game.simulation
            .order_builder_move_as(PlayerId(1), builder.id, destination)
            .unwrap();
        assert!(
            !game
                .simulation
                .can_player_control_building(PlayerId(1), western_castle)
        );

        assert!(
            game.simulation.set_player_connection_status(
                PlayerId(0),
                crate::PlayerConnectionStatus::Connected
            )
        );
        assert_eq!(
            game.simulation
                .order_builder_move_as(PlayerId(1), builder.id, destination),
            Err(crate::BuilderCommandError::NotAuthorized)
        );

        assert!(game.simulation.set_player_connection_status(
            PlayerId(0),
            crate::PlayerConnectionStatus::Disconnected
        ));
        assert!(game.simulation.set_player_connection_status(
            PlayerId(1),
            crate::PlayerConnectionStatus::Disconnected
        ));
        assert_eq!(
            game.simulation.lifecycle(),
            crate::MatchLifecycle::PausedForDisconnect {
                disconnected_teams_mask: 1
            }
        );
        let paused_tick = game.simulation.tick();
        let paused_checksum = game.simulation.checksum();
        game.simulation.step();
        assert_eq!(game.simulation.tick(), paused_tick);
        assert_eq!(game.simulation.checksum(), paused_checksum);

        assert!(
            game.simulation.set_player_connection_status(
                PlayerId(1),
                crate::PlayerConnectionStatus::Connected
            )
        );
        assert_eq!(game.simulation.lifecycle(), crate::MatchLifecycle::Running);
        game.simulation.step();
        assert_eq!(game.simulation.tick(), paused_tick + 1);
    }

    #[test]
    fn objective_destruction_finishes_and_freezes_match_deterministically() {
        let config = CastleFightMatchConfig::development_subset(
            MapVersion::CASTLE_FIGHT_9_27,
            "r1",
            0x4341_5354_4c45,
        )
        .unwrap();
        let mut game = create_castle_fight_match(config.clone(), 1).unwrap();
        let eastern_castle = game.simulation.team_objective(Team(1)).unwrap();
        assert!(game.simulation.remove_building(eastern_castle));
        game.simulation.step();
        assert_eq!(
            game.simulation.lifecycle(),
            crate::MatchLifecycle::Finished {
                outcome: crate::MatchOutcome::Victory(Team(0)),
                finished_tick: 0,
            }
        );
        let final_tick = game.simulation.tick();
        let final_checksum = game.simulation.checksum();
        assert!(!game.simulation.set_player_connection_status(
            PlayerId(0),
            crate::PlayerConnectionStatus::Disconnected
        ));
        game.simulation.step();
        assert_eq!(game.simulation.tick(), final_tick);
        assert_eq!(game.simulation.checksum(), final_checksum);

        let mut draw = create_castle_fight_match(config, 1).unwrap();
        let western_castle = draw.simulation.team_objective(Team(0)).unwrap();
        let eastern_castle = draw.simulation.team_objective(Team(1)).unwrap();
        assert!(draw.simulation.remove_building(western_castle));
        assert!(draw.simulation.remove_building(eastern_castle));
        draw.simulation.step();
        assert_eq!(
            draw.simulation.lifecycle(),
            crate::MatchLifecycle::Finished {
                outcome: crate::MatchOutcome::Draw,
                finished_tick: 0,
            }
        );
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
        assert_eq!(
            resolved.simulation_config.team_build_regions[0],
            vec![left_build_region]
        );
        assert_eq!(
            resolved.simulation_config.team_build_regions[1],
            vec![right_build_region]
        );

        let left_dead_space = world_rect_footprint(
            PLAYABLE_MIN_X_WORLD,
            PLAYABLE_MIN_Y_WORLD,
            LEFT_BUILD_MIN_X_WORLD,
            PLAYABLE_MAX_Y_WORLD,
        );
        let right_dead_space = world_rect_footprint(
            RIGHT_BUILD_MAX_X_WORLD,
            PLAYABLE_MIN_Y_WORLD,
            PLAYABLE_MAX_X_WORLD,
            PLAYABLE_MAX_Y_WORLD,
        );
        for blocker in [left_dead_space, right_dead_space] {
            assert!(
                resolved
                    .simulation_config
                    .static_blockers
                    .contains(&blocker)
            );
            assert!(
                resolved
                    .simulation_config
                    .air_static_blockers
                    .contains(&blocker)
            );
        }

        let navigation_min_x = resolved.simulation_config.navigation_min.x
            * resolved.simulation_config.navigation_cell_size;
        let navigation_max_x = (resolved.simulation_config.navigation_max.x + 1)
            * resolved.simulation_config.navigation_cell_size;
        assert_eq!(
            navigation_min_x,
            PLAYABLE_MIN_X_WORLD * SUBUNITS_PER_WORLD_UNIT
        );
        assert_eq!(
            navigation_max_x,
            PLAYABLE_MAX_X_WORLD * SUBUNITS_PER_WORLD_UNIT
        );
        assert!(resolved.terrain.origin().x < navigation_min_x);
        assert!(resolved.terrain.max_point().x > navigation_max_x);
    }

    #[test]
    fn road_placement_blocks_seven_rows_but_keeps_rear_moss_buildable() {
        let config =
            CastleFightMatchConfig::development_subset(MapVersion::CASTLE_FIGHT_9_27, "r1", 42)
                .unwrap();
        let game = create_castle_fight_match(config, 1).unwrap();

        for (team, side) in [(Team(0), -1), (Team(1), 1)] {
            let footprint = |x: i32, y: i32| world_rect_footprint(x - 64, y - 64, x + 64, y + 64);
            let front_x = side * 4_000;
            let rear_x = side * REAR_MOSS_CENTER_X_WORLD;
            assert!(
                !game
                    .simulation
                    .can_place_building_for_team(team, footprint(front_x, 0))
            );
            assert!(
                !game
                    .simulation
                    .can_place_building_for_team(team, footprint(front_x, 384))
            );
            assert!(
                !game
                    .simulation
                    .can_place_building_for_team(team, footprint(front_x, -384))
            );
            assert!(
                game.simulation
                    .can_place_building_for_team(team, footprint(front_x, 512))
            );
            assert!(
                game.simulation
                    .can_place_building_for_team(team, footprint(front_x, -512))
            );
            assert!(
                game.simulation
                    .can_place_building_for_team(team, footprint(rear_x, 0))
            );
            assert!(
                game.simulation
                    .can_place_building_for_team(team, footprint(rear_x, 128))
            );
            assert!(
                game.simulation
                    .can_place_building_for_team(team, footprint(rear_x, -128))
            );
            for paved_x in [side * 5_440, side * 5_568, side * 5_952, side * 6_048] {
                assert!(
                    !game
                        .simulation
                        .can_place_building_for_team(team, footprint(paved_x, 0))
                );
            }
            assert!(
                !game.simulation.can_place_building_for_team(
                    team,
                    footprint(side * CASTLE_CENTER_X_WORLD, 384)
                )
            );
            assert!(
                !game
                    .simulation
                    .can_place_building_for_team(team, footprint(rear_x, 320))
            );
            assert!(
                !game
                    .simulation
                    .can_place_building_for_team(team, footprint(rear_x, -320))
            );
        }
    }
}
