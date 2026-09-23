use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::OnceLock,
};

use serde::Deserialize;

use crate::{
    components::{
        AbilityEffect, AttackDelivery, AttackProfile, AttackTargetMask, BuilderConfiguration,
        BuilderLocomotion, BuilderProfile, BuilderSpawn, BuildingFootprint,
        BuildingGameplayProperties, BuildingSpawn, CollisionRadius, ContentIdentity,
        CorpseDefinitionId, CorpseProfile, GameplayBundleIdentity, MovementClass, MovementProfile,
        PassiveUnitEffect, PassiveUnitEffects, ProductionProfile, ResolvedUnitDefinition,
        SpellcastingProfile, Team, TriggeredAttackEffect, UnitGameplayProperties, UnitTemplate,
    },
    damage::{ArmorProfile, ArmorType, DamageRules, DamageType},
    economy::{BuildingEconomyProfile, EconomyRules, RESOURCE_FIXED_SCALE},
    math::SUBUNITS_PER_WORLD_UNIT,
    native_effects::{
        NativeEffectImplementationId, NativeEffectResolveError, NativeEffectSource,
        NativeEffectSourceKind, native_unit_mechanics_for, resolve_native_effect_requirements,
    },
    version::MapVersion,
};

pub const CASTLE_FIGHT_SIMULATION_HZ: i32 = 30;
pub const CASTLE_FIGHT_DEFAULT_MAP_VERSION: MapVersion = MapVersion::CASTLE_FIGHT_9_27;
pub const CASTLE_FIGHT_CONTENT_REVISION_927: &str = "cf-native-dev-slice-r4";
const CASTLE_FIGHT_EXTRACTION_TREE_927_R1: &str = "8ea806dca331ff254995e94e6f0baf225a14bf10";
// The stock Warcraft Build command (`AHbu`) has no editable cast-range field; workers use the
// engine's 50-world-unit construction contact range, matching the stock Repair contact range.
const CASTLE_FIGHT_BUILDER_BUILD_RANGE_WORLD_UNITS: i32 = 50;
const CASTLE_FIGHT_BUILDER_REPAIR_RANGE_WORLD_UNITS: i32 = 50;
const CASTLE_FIGHT_BUILDER_REPAIR_AUTOCAST_RANGE_WORLD_UNITS: i32 = 500;
const CASTLE_FIGHT_BUILDER_BLINK_RANGE_WORLD_UNITS: i32 = 10_000;
const CASTLE_FIGHT_BUILDER_BLINK_BOUNDARY_INSET_WORLD_UNITS: i32 = 64;
const CASTLE_FIGHT_STANDARD_REPAIR_TIME_SECONDS: u16 = 70;
const CASTLE_FIGHT_BUILDER_REPAIR_TIME_RATIO_NUMERATOR: u16 = 3;
const CASTLE_FIGHT_BUILDER_REPAIR_TIME_RATIO_DENOMINATOR: u16 = 2;
const CASTLE_FIGHT_STARTING_GOLD: u32 = 250;
const CASTLE_FIGHT_STARTING_LUMBER: u32 = 125;
const CASTLE_FIGHT_STARTING_LEGENDARY_POINTS: u16 = 1;
const CASTLE_FIGHT_BASE_INCOME_GOLD: u64 = 5;
const CASTLE_FIGHT_INCOME_INTERVAL_SECONDS: u32 = 10;
const CASTLE_FIGHT_INCOME_TAX_BRACKET_GOLD: u64 = 25;
const PRODUCTION_SPAWN_SEARCH_RADIUS_CELLS: u16 = 12;
const BUILDINGS_927_TSV: &str =
    include_str!("../../../docs/original_map/extracted/resolved/buildings.tsv");
const BUILDING_UPGRADES_927_TSV: &str =
    include_str!("../../../docs/original_map/extracted/script/building-upgrades.tsv");
const BUILDING_INCOME_927_TSV: &str =
    include_str!("../../../docs/original_map/extracted/script/race-building-semantics.tsv");
const BUILDER_CATALOG_927_TSV: &str =
    include_str!("../../../docs/original_map/extracted/script/race-buildings.tsv");
const WAR3MAP_MISC_927: &str = include_str!("../../../docs/original_map/extracted/war3mapMisc.txt");
const UNITS_927_TSV: &str = include_str!("../../../docs/original_map/extracted/resolved/units.tsv");
const PROTECTED_UNIT_STATS_927_TSV: &str =
    include_str!("../../../docs/original_map/extracted/resolved/protected-unit-stats.tsv");
const PRODUCTION_UNIT_ATTACKS_927_TSV: &str =
    include_str!("../../../docs/original_map/extracted/resolved/production-unit-attacks.tsv");
const PRODUCTION_BUILDINGS_927_TSV: &str =
    include_str!("../../../docs/original_map/extracted/resolved/production-buildings.tsv");
const PRODUCTION_UNIT_CORPSES_927_TSV: &str =
    include_str!("../../../docs/original_map/extracted/resolved/production-unit-corpses.tsv");
const CATALOG_SOURCE_927_R1_JSON: &str =
    include_str!("../data/castle-fight/9.27/catalog-source-r1.json");
const CATALOG_SUPPLEMENT_927_R1_JSON: &str =
    include_str!("../data/castle-fight/9.27/catalog-supplement-r1.json");
const OBJECT_FIELDS_927_R1_SHA256: &str =
    "01040f03e4a625ac1d960a0a2d43d0732d64b797e9357f4f40e8c2c0d6d9710d";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedCastleFightMapVersion(pub MapVersion);

impl fmt::Display for UnsupportedCastleFightMapVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Castle Fight {} content is not available",
            self.0
        )
    }
}

impl std::error::Error for UnsupportedCastleFightMapVersion {}

pub const CASTLE_FIGHT_CONTENT_BUNDLE_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CastleFightUnitId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CastleFightBuildingId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CastleFightBuilderId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CastleFightAbilityId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CastleFightContentIdentity {
    pub schema_version: u32,
    pub gameplay_hash: u64,
}

impl From<CastleFightContentIdentity> for GameplayBundleIdentity {
    fn from(identity: CastleFightContentIdentity) -> Self {
        Self {
            schema_version: identity.schema_version,
            gameplay_hash: identity.gameplay_hash,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CastleFightContentAvailability {
    Unavailable,
    Archived,
    SupportedDevelopmentSubset,
    SupportedFull,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResolvedCastleFightBehavior {
    pub id: CastleFightAbilityId,
    pub source: NativeEffectSource,
    pub implementation: NativeEffectImplementationId,
}

#[derive(Debug)]
pub struct CastleFightContentBundle {
    pub map_version: MapVersion,
    pub revision: &'static str,
    pub availability: CastleFightContentAvailability,
    pub identity: CastleFightContentIdentity,
    pub command_card: CastleFightCommandCardLayout,
    pub economy: EconomyRules,
    pub damage_rules: DamageRules,
    pub main_castle_repair_time_ticks: u32,
    units: BTreeMap<CastleFightUnitId, CastleFightUnitDefinition>,
    production_buildings: BTreeMap<CastleFightBuildingId, CastleFightProductionDefinition>,
    towers: BTreeMap<CastleFightBuildingId, CastleFightTowerDefinition>,
    builders: BTreeMap<CastleFightBuilderId, CastleFightBuilderDefinition>,
    behaviors: Vec<ResolvedCastleFightBehavior>,
}

impl CastleFightContentBundle {
    #[must_use]
    pub fn unit(&self, kind: CastleFightUnitKind) -> Option<CastleFightUnitDefinition> {
        self.units.get(&kind.stable_id()).copied()
    }

    #[must_use]
    pub fn production_building(
        &self,
        kind: CastleFightProductionKind,
    ) -> Option<CastleFightProductionDefinition> {
        self.production_buildings.get(&kind.stable_id()).copied()
    }

    #[must_use]
    pub fn tower(&self, kind: CastleFightTowerKind) -> Option<CastleFightTowerDefinition> {
        self.towers.get(&kind.stable_id()).copied()
    }

    #[must_use]
    pub fn builder(&self, race: CastleFightBuilderRace) -> Option<&CastleFightBuilderDefinition> {
        self.builders.get(&race.stable_id())
    }

    #[must_use]
    pub fn behaviors(&self) -> &[ResolvedCastleFightBehavior] {
        &self.behaviors
    }

    pub fn unit_definitions(&self) -> impl Iterator<Item = CastleFightUnitDefinition> + '_ {
        self.units.values().copied()
    }

    pub fn builder_definitions(&self) -> impl Iterator<Item = &CastleFightBuilderDefinition> + '_ {
        self.builders.values()
    }

    pub fn production_building_definitions(
        &self,
    ) -> impl Iterator<Item = CastleFightProductionDefinition> + '_ {
        self.production_buildings.values().copied()
    }

    pub fn tower_definitions(&self) -> impl Iterator<Item = CastleFightTowerDefinition> + '_ {
        self.towers.values().copied()
    }

    #[must_use]
    pub fn building_kind(&self, id: CastleFightBuildingId) -> Option<CastleFightBuildingKind> {
        CastleFightProductionKind::ALL
            .into_iter()
            .find(|kind| kind.stable_id() == id && self.production_building(*kind).is_some())
            .map(CastleFightBuildingKind::Production)
            .or_else(|| {
                CastleFightTowerKind::ALL
                    .into_iter()
                    .find(|kind| kind.stable_id() == id && self.tower(*kind).is_some())
                    .map(CastleFightBuildingKind::Tower)
            })
    }

    #[must_use]
    pub fn building_kind_for_rawcode(&self, rawcode: u32) -> Option<CastleFightBuildingKind> {
        CastleFightProductionKind::ALL
            .into_iter()
            .find(|kind| {
                self.production_building(*kind)
                    .is_some_and(|definition| definition.rawcode == rawcode)
            })
            .map(CastleFightBuildingKind::Production)
            .or_else(|| {
                CastleFightTowerKind::ALL
                    .into_iter()
                    .find(|kind| {
                        self.tower(*kind)
                            .is_some_and(|definition| definition.rawcode == rawcode)
                    })
                    .map(CastleFightBuildingKind::Tower)
            })
    }

    #[must_use]
    pub fn content_identity_for_rawcode(&self, rawcode: u32) -> Option<ContentIdentity> {
        self.units
            .values()
            .find(|definition| definition.rawcode == rawcode)
            .map(|definition| ContentIdentity {
                rawcode,
                name: definition.name,
            })
            .or_else(|| {
                self.production_buildings
                    .values()
                    .find(|definition| definition.rawcode == rawcode)
                    .map(|definition| ContentIdentity {
                        rawcode,
                        name: definition.name,
                    })
            })
            .or_else(|| {
                self.towers
                    .values()
                    .find(|definition| definition.rawcode == rawcode)
                    .map(|definition| ContentIdentity {
                        rawcode,
                        name: definition.name,
                    })
            })
            .or_else(|| {
                self.builders
                    .values()
                    .find(|definition| definition.rawcode == rawcode)
                    .map(|definition| ContentIdentity {
                        rawcode,
                        name: definition.name,
                    })
            })
            .or_else(|| {
                (rawcode == u32::from_be_bytes(*b"hcas")).then_some(ContentIdentity {
                    rawcode,
                    name: "Main Castle",
                })
            })
    }

    #[must_use]
    pub fn direct_building_kinds(&self) -> Vec<CastleFightBuildingKind> {
        CastleFightProductionKind::ALL
            .into_iter()
            .filter(|kind| self.production_building(*kind).is_some())
            .filter(|kind| {
                kind.upgrade_from_for_version(self.map_version)
                    .expect("bundle map version must support its production definitions")
                    .is_none()
            })
            .map(CastleFightBuildingKind::Production)
            .chain(
                CastleFightTowerKind::ALL
                    .into_iter()
                    .filter(|kind| self.tower(*kind).is_some())
                    .map(CastleFightBuildingKind::Tower),
            )
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CastleFightContentError {
    UnsupportedRelease(MapVersion),
    UnsupportedContentRevision {
        map_version: MapVersion,
        content_revision: &'static str,
    },
    ArchivedOnly(MapVersion),
    NativeEffect(NativeEffectResolveError),
    MissingAbilityInventory(u32),
    MissingStableAbilityId(NativeEffectSource),
}

impl fmt::Display for CastleFightContentError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedRelease(version) => {
                write!(
                    formatter,
                    "Castle Fight {version} is not registered as runtime content"
                )
            }
            Self::UnsupportedContentRevision {
                map_version,
                content_revision,
            } => write!(
                formatter,
                "Castle Fight {map_version} content revision {content_revision} is not available in this build"
            ),
            Self::ArchivedOnly(version) => write!(
                formatter,
                "Castle Fight {version} is archived source evidence but has no supported runtime bundle"
            ),
            Self::NativeEffect(error) => error.fmt(formatter),
            Self::MissingAbilityInventory(rawcode) => write!(
                formatter,
                "playable object {rawcode:#010x} is missing its extracted ability inventory"
            ),
            Self::MissingStableAbilityId(source) => {
                write!(formatter, "{source} has no stable Castle Fight ability ID")
            }
        }
    }
}

impl std::error::Error for CastleFightContentError {}

impl From<NativeEffectResolveError> for CastleFightContentError {
    fn from(error: NativeEffectResolveError) -> Self {
        Self::NativeEffect(error)
    }
}

#[must_use]
pub const fn castle_fight_content_availability(
    version: MapVersion,
) -> CastleFightContentAvailability {
    if version.major == 9 && version.minor == 27 {
        CastleFightContentAvailability::SupportedDevelopmentSubset
    } else if version.major == 9 && version.minor == 32 {
        CastleFightContentAvailability::Archived
    } else {
        CastleFightContentAvailability::Unavailable
    }
}

pub fn castle_fight_content_bundle(
    version: MapVersion,
) -> Result<&'static CastleFightContentBundle, CastleFightContentError> {
    match castle_fight_content_availability(version) {
        CastleFightContentAvailability::SupportedDevelopmentSubset => {
            static BUNDLE_927: OnceLock<Result<CastleFightContentBundle, CastleFightContentError>> =
                OnceLock::new();
            match BUNDLE_927.get_or_init(build_content_bundle_927) {
                Ok(bundle) => Ok(bundle),
                Err(error) => Err(*error),
            }
        }
        CastleFightContentAvailability::Archived => {
            Err(CastleFightContentError::ArchivedOnly(version))
        }
        CastleFightContentAvailability::Unavailable => {
            Err(CastleFightContentError::UnsupportedRelease(version))
        }
        CastleFightContentAvailability::SupportedFull => {
            unreachable!("no full Castle Fight content bundle is registered yet")
        }
    }
}

pub fn castle_fight_content_bundle_for_revision(
    version: MapVersion,
    content_revision: &'static str,
) -> Result<&'static CastleFightContentBundle, CastleFightContentError> {
    if version == MapVersion::CASTLE_FIGHT_9_27
        && content_revision == CASTLE_FIGHT_CONTENT_REVISION_927
    {
        return castle_fight_content_bundle(version);
    }
    Err(CastleFightContentError::UnsupportedContentRevision {
        map_version: version,
        content_revision,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CastleFightBuildingKind {
    Production(CastleFightProductionKind),
    Tower(CastleFightTowerKind),
}

impl CastleFightBuildingKind {
    #[must_use]
    pub const fn stable_id(self) -> CastleFightBuildingId {
        match self {
            Self::Production(kind) => kind.stable_id(),
            Self::Tower(kind) => kind.stable_id(),
        }
    }

    #[must_use]
    pub fn rawcode(self, bundle: &CastleFightContentBundle) -> Option<u32> {
        match self {
            Self::Production(kind) => bundle
                .production_building(kind)
                .map(|definition| definition.rawcode),
            Self::Tower(kind) => bundle.tower(kind).map(|definition| definition.rawcode),
        }
    }

    #[must_use]
    pub fn name(self, bundle: &CastleFightContentBundle) -> Option<&'static str> {
        match self {
            Self::Production(kind) => bundle
                .production_building(kind)
                .map(|definition| definition.name),
            Self::Tower(kind) => bundle.tower(kind).map(|definition| definition.name),
        }
    }

    #[must_use]
    pub fn tooltips(
        self,
        bundle: &CastleFightContentBundle,
    ) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Production(kind) => bundle
                .production_building(kind)
                .map(|definition| (definition.basic_tooltip, definition.extended_tooltip)),
            Self::Tower(kind) => bundle
                .tower(kind)
                .map(|definition| (definition.basic_tooltip, definition.extended_tooltip)),
        }
    }

    #[must_use]
    pub fn footprint_size_cells(self, bundle: &CastleFightContentBundle) -> Option<u16> {
        match self {
            Self::Production(kind) => bundle
                .production_building(kind)
                .map(|definition| definition.footprint_size_cells),
            Self::Tower(kind) => bundle
                .tower(kind)
                .map(|definition| definition.footprint_size_cells),
        }
    }

    #[must_use]
    pub fn economy(self, bundle: &CastleFightContentBundle) -> Option<BuildingEconomyProfile> {
        match self {
            Self::Production(kind) => bundle
                .production_building(kind)
                .map(|definition| definition.economy),
            Self::Tower(kind) => bundle.tower(kind).map(|definition| definition.economy),
        }
    }

    #[must_use]
    pub fn hotkey(self, bundle: &CastleFightContentBundle) -> Option<char> {
        match self {
            Self::Production(kind) => bundle
                .production_building(kind)
                .map(|definition| definition.hotkey),
            Self::Tower(kind) => bundle.tower(kind).map(|definition| definition.hotkey),
        }
    }

    #[must_use]
    pub fn command_card_position(
        self,
        bundle: &CastleFightContentBundle,
    ) -> Option<CommandCardPosition> {
        match self {
            Self::Production(kind) => bundle
                .production_building(kind)
                .map(|definition| definition.command_card_position),
            Self::Tower(kind) => bundle
                .tower(kind)
                .map(|definition| definition.command_card_position),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandCardPosition {
    pub x: u8,
    pub y: u8,
}

impl CommandCardPosition {
    #[must_use]
    pub const fn new(x: u8, y: u8) -> Self {
        Self { x, y }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastleFightCommandCardLayout {
    pub move_command: CommandCardPosition,
    pub attack_command: CommandCardPosition,
    pub build_command: CommandCardPosition,
    pub build_hotkey: char,
    pub cancel_command: CommandCardPosition,
    pub repair_ability: CommandCardPosition,
    pub blink_ability: CommandCardPosition,
    pub blink_hotkey: char,
    pub map_version: MapVersion,
}

#[must_use]
pub fn castle_fight_command_card_layout() -> CastleFightCommandCardLayout {
    castle_fight_command_card_layout_for_version(CASTLE_FIGHT_DEFAULT_MAP_VERSION)
        .expect("default Castle Fight map version must remain available")
}

pub fn castle_fight_command_card_layout_for_version(
    version: MapVersion,
) -> Result<CastleFightCommandCardLayout, UnsupportedCastleFightMapVersion> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return Err(UnsupportedCastleFightMapVersion(version));
    }
    Ok(CastleFightCommandCardLayout {
        // The basic command positions come from Warcraft III's commandfunc data used by 9.27.
        move_command: CommandCardPosition::new(0, 0),
        attack_command: CommandCardPosition::new(3, 0),
        build_command: CommandCardPosition::new(0, 2),
        build_hotkey: 'B',
        cancel_command: CommandCardPosition::new(3, 2),
        // Castle Fight's 9.27 Repair and scripted live Blink object data.
        repair_ability: CommandCardPosition::new(1, 1),
        blink_ability: CommandCardPosition::new(1, 2),
        blink_hotkey: 'D',
        map_version: version,
    })
}

#[repr(u8)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CastleFightBuilderRace {
    Chaos = 0,
    Corrupted = 1,
    Critter = 2,
    Desert = 3,
    Elemental = 4,
    Elf = 5,
    Human = 6,
    Mechanical = 7,
    Naga = 8,
    Nature = 9,
    NightElf = 10,
    Northern = 11,
    Orc = 12,
    Pandaren = 13,
    Undead = 14,
}

impl CastleFightBuilderRace {
    #[must_use]
    pub const fn stable_id(self) -> CastleFightBuilderId {
        CastleFightBuilderId(match self {
            Self::Chaos => 0x3000_0001,
            Self::Corrupted => 0x3000_0002,
            Self::Critter => 0x3000_0003,
            Self::Desert => 0x3000_0004,
            Self::Elemental => 0x3000_0005,
            Self::Elf => 0x3000_0006,
            Self::Human => 0x3000_0007,
            Self::Mechanical => 0x3000_0008,
            Self::Naga => 0x3000_0009,
            Self::Nature => 0x3000_000a,
            Self::NightElf => 0x3000_000b,
            Self::Northern => 0x3000_000c,
            Self::Orc => 0x3000_000d,
            Self::Pandaren => 0x3000_000e,
            Self::Undead => 0x3000_000f,
        })
    }

    pub const ALL: [Self; 15] = [
        Self::Chaos,
        Self::Corrupted,
        Self::Critter,
        Self::Desert,
        Self::Elemental,
        Self::Elf,
        Self::Human,
        Self::Mechanical,
        Self::Naga,
        Self::Nature,
        Self::NightElf,
        Self::Northern,
        Self::Orc,
        Self::Pandaren,
        Self::Undead,
    ];

    pub const STANDARD: [Self; 14] = [
        Self::Chaos,
        Self::Corrupted,
        Self::Desert,
        Self::Elemental,
        Self::Elf,
        Self::Human,
        Self::Mechanical,
        Self::Naga,
        Self::Nature,
        Self::NightElf,
        Self::Northern,
        Self::Orc,
        Self::Pandaren,
        Self::Undead,
    ];

    #[must_use]
    pub fn definition(self) -> CastleFightBuilderDefinition {
        self.definition_for_version(CASTLE_FIGHT_DEFAULT_MAP_VERSION)
            .expect("default Castle Fight map version must remain available")
    }

    pub fn definition_for_version(
        self,
        version: MapVersion,
    ) -> Result<CastleFightBuilderDefinition, UnsupportedCastleFightMapVersion> {
        if version != MapVersion::CASTLE_FIGHT_9_27 {
            return Err(UnsupportedCastleFightMapVersion(version));
        }
        Ok(extracted_builder_definition_927(self))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CastleFightBuilderDefinition {
    pub race: CastleFightBuilderRace,
    pub race_index: u8,
    pub rawcode: u32,
    pub name: &'static str,
    pub campaign_only: bool,
    pub locomotion: BuilderLocomotion,
    pub repair_autocast_enabled_by_default: bool,
    pub profile: BuilderProfile,
    pub build_catalog: Vec<u32>,
    pub map_version: MapVersion,
}

impl CastleFightBuilderDefinition {
    #[must_use]
    pub fn configuration(&self) -> BuilderConfiguration {
        self.configuration_with_catalog(self.build_catalog.clone())
    }

    #[must_use]
    pub fn configuration_with_catalog(&self, build_catalog: Vec<u32>) -> BuilderConfiguration {
        BuilderConfiguration {
            appearance: ContentIdentity {
                rawcode: self.rawcode,
                name: self.name,
            },
            locomotion: self.locomotion,
            build_catalog,
        }
    }

    #[must_use]
    pub fn spawn(&self, team: Team, position: crate::math::SimPoint) -> BuilderSpawn {
        BuilderSpawn {
            team,
            position,
            profile: self.profile,
            configuration: self.configuration(),
            repair_autocast_enabled: self.repair_autocast_enabled_by_default,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CastleFightUnitKind {
    Footman,
    Defender,
    Ranger,
    Catapult,
    IceTrollShadowPriest,
    GryphonRider,
}

impl CastleFightUnitKind {
    #[must_use]
    pub const fn stable_id(self) -> CastleFightUnitId {
        CastleFightUnitId(match self {
            Self::Footman => 0x1000_0001,
            Self::Defender => 0x1000_0002,
            Self::Ranger => 0x1000_0003,
            Self::Catapult => 0x1000_0004,
            Self::IceTrollShadowPriest => 0x1000_0005,
            Self::GryphonRider => 0x1000_0006,
        })
    }

    pub const ALL: [Self; 6] = [
        Self::Footman,
        Self::Defender,
        Self::Ranger,
        Self::Catapult,
        Self::IceTrollShadowPriest,
        Self::GryphonRider,
    ];

    #[must_use]
    pub fn definition(self) -> CastleFightUnitDefinition {
        self.definition_for_version(CASTLE_FIGHT_DEFAULT_MAP_VERSION)
            .expect("default Castle Fight map version must remain available")
    }

    pub fn definition_for_version(
        self,
        version: MapVersion,
    ) -> Result<CastleFightUnitDefinition, UnsupportedCastleFightMapVersion> {
        if version != MapVersion::CASTLE_FIGHT_9_27 {
            return Err(UnsupportedCastleFightMapVersion(version));
        }
        let mut definition = self.definition_9_27();
        let abilities = extracted_content_927()
            .unit_abilities
            .get(&definition.rawcode)
            .unwrap_or_else(|| {
                panic!(
                    "unit {:#010x} is missing retained 9.27 ability inventory",
                    definition.rawcode
                )
            });
        let mechanics = native_unit_mechanics_for(version, definition.rawcode, abilities)
            .expect("supported Castle Fight version must have native-effect tuning");
        definition.passive_effects = mechanics.passive_effects;
        definition.spellcasting = mechanics.spellcasting;
        Ok(definition)
    }

    fn definition_9_27(self) -> CastleFightUnitDefinition {
        let (rawcode, name, expose_corpse) = match self {
            Self::Footman => (u32::from_be_bytes(*b"hfoo"), "Footman", true),
            Self::Defender => (u32::from_be_bytes(*b"h03A"), "Defender", true),
            Self::Ranger => (u32::from_be_bytes(*b"e003"), "Ranger", true),
            Self::Catapult => (u32::from_be_bytes(*b"o001"), "Catapult", false),
            Self::IceTrollShadowPriest => (
                u32::from_be_bytes(*b"n015"),
                "Ice Troll Shadow Priest",
                true,
            ),
            Self::GryphonRider => (u32::from_be_bytes(*b"h016"), "Gryphon Rider", false),
        };
        extracted_unit_definition_927(rawcode, name, expose_corpse)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastleFightUnitDefinition {
    pub rawcode: u32,
    pub name: &'static str,
    pub health: i32,
    pub health_regen_per_second_per_10k: u32,
    pub build_time_ticks: u32,
    pub repair_time_ticks: u32,
    pub armor: ArmorProfile,
    pub passive_effects: PassiveUnitEffects,
    pub spellcasting: Option<SpellcastingProfile>,
    pub damage_type: DamageType,
    pub attack_targets: AttackTargetMask,
    pub movement_class: MovementClass,
    pub mechanical: bool,
    pub collision_radius: CollisionRadius,
    pub corpse: Option<CorpseProfile>,
    pub attack: AttackProfile,
    pub movement: MovementProfile,
}

impl CastleFightUnitDefinition {
    #[must_use]
    pub const fn resolved(self) -> ResolvedUnitDefinition {
        ResolvedUnitDefinition {
            template: self.template(),
            properties: self.gameplay_properties(),
            spellcasting: self.spellcasting,
        }
    }

    #[must_use]
    pub const fn template(self) -> UnitTemplate {
        UnitTemplate {
            health: self.health,
            attack: self.attack,
            movement: self.movement,
        }
    }

    #[must_use]
    pub const fn gameplay_properties(self) -> UnitGameplayProperties {
        UnitGameplayProperties {
            content: Some(ContentIdentity {
                rawcode: self.rawcode,
                name: self.name,
            }),
            health_regen_per_second_per_10k: self.health_regen_per_second_per_10k,
            corpse: self.corpse,
            collision_radius: Some(self.collision_radius),
            movement_class: self.movement_class,
            mechanical: self.mechanical,
            build_time_ticks: Some(self.build_time_ticks),
            repair_time_ticks: Some(self.repair_time_ticks),
            attack_targets: self.attack_targets,
            damage_type: self.damage_type,
            armor: self.armor,
            passive_effects: self.passive_effects,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CastleFightProductionKind {
    Barracks,
    Stronghold,
    RangersHall,
    OrcishSiegeFactory,
    IceTrollHut,
    GryphonRock,
}

impl CastleFightProductionKind {
    #[must_use]
    pub const fn stable_id(self) -> CastleFightBuildingId {
        CastleFightBuildingId(match self {
            Self::Barracks => 0x2000_0001,
            Self::Stronghold => 0x2000_0002,
            Self::RangersHall => 0x2000_0003,
            Self::OrcishSiegeFactory => 0x2000_0004,
            Self::IceTrollHut => 0x2000_0005,
            Self::GryphonRock => 0x2000_0006,
        })
    }

    pub const ALL: [Self; 6] = [
        Self::Barracks,
        Self::Stronghold,
        Self::RangersHall,
        Self::OrcishSiegeFactory,
        Self::IceTrollHut,
        Self::GryphonRock,
    ];

    #[must_use]
    pub fn from_rawcode(rawcode: u32) -> Option<Self> {
        Self::from_rawcode_for_version(rawcode, CASTLE_FIGHT_DEFAULT_MAP_VERSION)
            .expect("default Castle Fight map version must remain available")
    }

    pub fn from_rawcode_for_version(
        rawcode: u32,
        version: MapVersion,
    ) -> Result<Option<Self>, UnsupportedCastleFightMapVersion> {
        if version != MapVersion::CASTLE_FIGHT_9_27 {
            return Err(UnsupportedCastleFightMapVersion(version));
        }
        Ok(match rawcode {
            value if value == u32::from_be_bytes(*b"h000") => Some(Self::Barracks),
            value if value == u32::from_be_bytes(*b"h039") => Some(Self::Stronghold),
            value if value == u32::from_be_bytes(*b"h03D") => Some(Self::RangersHall),
            value if value == u32::from_be_bytes(*b"h02I") => Some(Self::OrcishSiegeFactory),
            value if value == u32::from_be_bytes(*b"h03K") => Some(Self::IceTrollHut),
            value if value == u32::from_be_bytes(*b"h015") => Some(Self::GryphonRock),
            _ => None,
        })
    }

    #[must_use]
    pub fn upgrade_from(self) -> Option<Self> {
        self.upgrade_from_for_version(CASTLE_FIGHT_DEFAULT_MAP_VERSION)
            .expect("default Castle Fight map version must remain available")
    }

    pub fn upgrade_from_for_version(
        self,
        version: MapVersion,
    ) -> Result<Option<Self>, UnsupportedCastleFightMapVersion> {
        let definition = self.definition_for_version(version)?;
        let (precursor, _) = extracted_building_upgrade_links_927(definition.rawcode);
        match precursor {
            Some(rawcode) => Self::from_rawcode_for_version(rawcode, version),
            None => Ok(None),
        }
    }

    #[must_use]
    pub fn upgrade_targets(self) -> Vec<Self> {
        self.upgrade_targets_for_version(CASTLE_FIGHT_DEFAULT_MAP_VERSION)
            .expect("default Castle Fight map version must remain available")
    }

    pub fn upgrade_targets_for_version(
        self,
        version: MapVersion,
    ) -> Result<Vec<Self>, UnsupportedCastleFightMapVersion> {
        let definition = self.definition_for_version(version)?;
        let (_, targets) = extracted_building_upgrade_links_927(definition.rawcode);
        targets
            .into_iter()
            .map(|rawcode| Self::from_rawcode_for_version(rawcode, version))
            .filter_map(|result| match result {
                Ok(Some(kind)) => Some(Ok(kind)),
                Ok(None) => None,
                Err(error) => Some(Err(error)),
            })
            .collect()
    }

    #[must_use]
    pub fn definition(self) -> CastleFightProductionDefinition {
        self.definition_for_version(CASTLE_FIGHT_DEFAULT_MAP_VERSION)
            .expect("default Castle Fight map version must remain available")
    }

    pub fn definition_for_version(
        self,
        version: MapVersion,
    ) -> Result<CastleFightProductionDefinition, UnsupportedCastleFightMapVersion> {
        if version != MapVersion::CASTLE_FIGHT_9_27 {
            return Err(UnsupportedCastleFightMapVersion(version));
        }
        Ok(self.definition_9_27())
    }

    fn definition_9_27(self) -> CastleFightProductionDefinition {
        let (rawcode, unit) = match self {
            Self::Barracks => (u32::from_be_bytes(*b"h000"), CastleFightUnitKind::Footman),
            Self::Stronghold => (u32::from_be_bytes(*b"h039"), CastleFightUnitKind::Defender),
            Self::RangersHall => (u32::from_be_bytes(*b"h03D"), CastleFightUnitKind::Ranger),
            Self::OrcishSiegeFactory => {
                (u32::from_be_bytes(*b"h02I"), CastleFightUnitKind::Catapult)
            }
            Self::IceTrollHut => (
                u32::from_be_bytes(*b"h03K"),
                CastleFightUnitKind::IceTrollShadowPriest,
            ),
            Self::GryphonRock => (
                u32::from_be_bytes(*b"h015"),
                CastleFightUnitKind::GryphonRider,
            ),
        };
        production_definition(rawcode, unit)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastleFightProductionDefinition {
    pub rawcode: u32,
    pub name: &'static str,
    pub basic_tooltip: &'static str,
    pub extended_tooltip: &'static str,
    pub gold_cost: u16,
    pub lumber_cost: u16,
    pub economy: BuildingEconomyProfile,
    pub building_health: i32,
    pub construction_time_ticks: u32,
    pub repair_time_ticks: u32,
    pub armor: ArmorProfile,
    pub spawn_interval_ticks: u16,
    pub footprint_size_cells: u16,
    pub unit: CastleFightUnitKind,
    pub produced_unit: CastleFightUnitDefinition,
    pub train_command_position: CommandCardPosition,
    pub train_hotkey: char,
    pub train_basic_tooltip: &'static str,
    pub train_extended_tooltip: &'static str,
    pub command_card_position: CommandCardPosition,
    pub hotkey: char,
    pub map_version: MapVersion,
}

impl CastleFightProductionDefinition {
    #[must_use]
    pub fn spawn(self, team: Team, footprint: BuildingFootprint) -> BuildingSpawn {
        let unit = self.produced_unit.resolved();
        BuildingSpawn {
            team,
            footprint,
            health: self.building_health,
            production: Some(ProductionProfile {
                initial_delay_ticks: self.spawn_interval_ticks,
                interval_ticks: self.spawn_interval_ticks,
                search_radius_cells: PRODUCTION_SPAWN_SEARCH_RADIUS_CELLS,
                unit: unit.template,
            }),
            attack: None,
            spellcasting: None,
        }
    }

    #[must_use]
    pub fn gameplay_properties(self) -> BuildingGameplayProperties {
        let unit = self.produced_unit.resolved();
        BuildingGameplayProperties {
            content: Some(ContentIdentity {
                rawcode: self.rawcode,
                name: self.name,
            }),
            construction_time_ticks: Some(self.construction_time_ticks),
            repair_time_ticks: Some(self.repair_time_ticks),
            attack_targets: AttackTargetMask::ALL,
            damage_type: DamageType::Normal,
            armor: self.armor,
            economy: Some(self.economy),
            production_unit: unit.properties,
            production_spellcasting: unit.spellcasting,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CastleFightTowerKind {
    WatchTower,
    PoofTower,
}

impl CastleFightTowerKind {
    #[must_use]
    pub const fn stable_id(self) -> CastleFightBuildingId {
        CastleFightBuildingId(match self {
            Self::WatchTower => 0x2100_0001,
            Self::PoofTower => 0x2100_0002,
        })
    }

    pub const ALL: [Self; 2] = [Self::WatchTower, Self::PoofTower];

    #[must_use]
    pub fn from_rawcode(rawcode: u32) -> Option<Self> {
        Self::from_rawcode_for_version(rawcode, CASTLE_FIGHT_DEFAULT_MAP_VERSION)
            .expect("default Castle Fight map version must remain available")
    }

    pub fn from_rawcode_for_version(
        rawcode: u32,
        version: MapVersion,
    ) -> Result<Option<Self>, UnsupportedCastleFightMapVersion> {
        if version != MapVersion::CASTLE_FIGHT_9_27 {
            return Err(UnsupportedCastleFightMapVersion(version));
        }
        Ok(match rawcode {
            value if value == u32::from_be_bytes(*b"h006") => Some(Self::WatchTower),
            value if value == u32::from_be_bytes(*b"h07P") => Some(Self::PoofTower),
            _ => None,
        })
    }

    #[must_use]
    pub fn definition(self) -> CastleFightTowerDefinition {
        self.definition_for_version(CASTLE_FIGHT_DEFAULT_MAP_VERSION)
            .expect("default Castle Fight map version must remain available")
    }

    pub fn definition_for_version(
        self,
        version: MapVersion,
    ) -> Result<CastleFightTowerDefinition, UnsupportedCastleFightMapVersion> {
        if version != MapVersion::CASTLE_FIGHT_9_27 {
            return Err(UnsupportedCastleFightMapVersion(version));
        }
        Ok(self.definition_9_27())
    }

    fn definition_9_27(self) -> CastleFightTowerDefinition {
        let (rawcode, expected_name) = match self {
            Self::WatchTower => (u32::from_be_bytes(*b"h006"), "Watch Tower"),
            Self::PoofTower => (u32::from_be_bytes(*b"h07P"), "Poof Tower"),
        };
        extracted_tower_definition_927(rawcode, expected_name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastleFightTowerDefinition {
    pub rawcode: u32,
    pub name: &'static str,
    pub basic_tooltip: &'static str,
    pub extended_tooltip: &'static str,
    pub gold_cost: u16,
    pub lumber_cost: u16,
    pub economy: BuildingEconomyProfile,
    pub health: i32,
    pub construction_time_ticks: u32,
    pub repair_time_ticks: u32,
    pub armor: ArmorProfile,
    pub damage_type: DamageType,
    pub attack_targets: AttackTargetMask,
    pub footprint_size_cells: u16,
    pub command_card_position: CommandCardPosition,
    pub hotkey: char,
    pub attack: AttackProfile,
    pub map_version: MapVersion,
}

impl CastleFightTowerDefinition {
    #[must_use]
    pub const fn spawn(self, team: Team, footprint: BuildingFootprint) -> BuildingSpawn {
        BuildingSpawn {
            team,
            footprint,
            health: self.health,
            production: None,
            attack: Some(self.attack),
            spellcasting: None,
        }
    }

    #[must_use]
    pub const fn gameplay_properties(self) -> BuildingGameplayProperties {
        BuildingGameplayProperties {
            content: Some(ContentIdentity {
                rawcode: self.rawcode,
                name: self.name,
            }),
            construction_time_ticks: Some(self.construction_time_ticks),
            repair_time_ticks: Some(self.repair_time_ticks),
            attack_targets: self.attack_targets,
            damage_type: self.damage_type,
            armor: self.armor,
            economy: Some(self.economy),
            production_unit: UnitGameplayProperties {
                content: None,
                corpse: None,
                collision_radius: None,
                movement_class: MovementClass::Ground,
                mechanical: false,
                build_time_ticks: None,
                repair_time_ticks: None,
                attack_targets: AttackTargetMask::ALL,
                health_regen_per_second_per_10k: 0,
                damage_type: DamageType::Normal,
                armor: ArmorProfile::UNARMORED,
                passive_effects: PassiveUnitEffects::EMPTY,
            },
            production_spellcasting: None,
        }
    }
}

#[must_use]
pub fn castle_fight_builder_profile() -> BuilderProfile {
    castle_fight_builder_profile_for_version(CASTLE_FIGHT_DEFAULT_MAP_VERSION)
        .expect("default Castle Fight map version must remain available")
}

pub fn castle_fight_builder_profile_for_version(
    version: MapVersion,
) -> Result<BuilderProfile, UnsupportedCastleFightMapVersion> {
    Ok(CastleFightBuilderRace::Human
        .definition_for_version(version)?
        .profile)
}

fn builder_profile(speed_per_tick: i32) -> BuilderProfile {
    BuilderProfile {
        speed_per_tick,
        build_range: world(CASTLE_FIGHT_BUILDER_BUILD_RANGE_WORLD_UNITS),
        repair_range: world(CASTLE_FIGHT_BUILDER_REPAIR_RANGE_WORLD_UNITS),
        repair_autocast_range: world(CASTLE_FIGHT_BUILDER_REPAIR_AUTOCAST_RANGE_WORLD_UNITS),
        repair_time_ratio_numerator: CASTLE_FIGHT_BUILDER_REPAIR_TIME_RATIO_NUMERATOR,
        repair_time_ratio_denominator: CASTLE_FIGHT_BUILDER_REPAIR_TIME_RATIO_DENOMINATOR,
        // Real Castle Fight content supplies target-specific `urtm`; this fallback only covers
        // synthetic/native test buildings without object metadata.
        full_repair_duration_ticks: CASTLE_FIGHT_STANDARD_REPAIR_TIME_SECONDS
            * CASTLE_FIGHT_SIMULATION_HZ as u16
            * CASTLE_FIGHT_BUILDER_REPAIR_TIME_RATIO_NUMERATOR
            / CASTLE_FIGHT_BUILDER_REPAIR_TIME_RATIO_DENOMINATOR,
        blink_range: world(CASTLE_FIGHT_BUILDER_BLINK_RANGE_WORLD_UNITS),
        blink_boundary_inset: world(CASTLE_FIGHT_BUILDER_BLINK_BOUNDARY_INSET_WORLD_UNITS),
    }
}

fn build_content_bundle_927() -> Result<CastleFightContentBundle, CastleFightContentError> {
    let _ = extracted_content_927();
    let version = MapVersion::CASTLE_FIGHT_9_27;

    let units = CastleFightUnitKind::ALL
        .into_iter()
        .map(|kind| {
            kind.definition_for_version(version)
                .map(|definition| (kind.stable_id(), definition))
                .map_err(|_| CastleFightContentError::UnsupportedRelease(version))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let production_buildings = CastleFightProductionKind::ALL
        .into_iter()
        .map(|kind| {
            kind.definition_for_version(version)
                .map(|definition| (kind.stable_id(), definition))
                .map_err(|_| CastleFightContentError::UnsupportedRelease(version))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let towers = CastleFightTowerKind::ALL
        .into_iter()
        .map(|kind| {
            kind.definition_for_version(version)
                .map(|definition| (kind.stable_id(), definition))
                .map_err(|_| CastleFightContentError::UnsupportedRelease(version))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;
    let builders = CastleFightBuilderRace::ALL
        .into_iter()
        .map(|race| {
            race.definition_for_version(version)
                .map(|definition| (race.stable_id(), definition))
                .map_err(|_| CastleFightContentError::UnsupportedRelease(version))
        })
        .collect::<Result<BTreeMap<_, _>, _>>()?;

    let extracted = extracted_content_927();
    let mut root_set = BTreeSet::new();
    let playable_rawcodes = units
        .values()
        .map(|definition| definition.rawcode)
        .chain(
            production_buildings
                .values()
                .map(|definition| definition.rawcode),
        )
        .chain(towers.values().map(|definition| definition.rawcode));
    for rawcode in playable_rawcodes {
        let abilities = extracted
            .unit_abilities
            .get(&rawcode)
            .ok_or(CastleFightContentError::MissingAbilityInventory(rawcode))?;
        root_set.extend(
            abilities
                .iter()
                .copied()
                .map(|key| NativeEffectSource::new(NativeEffectSourceKind::UnitAbility, key)),
        );
    }
    let roots = root_set.into_iter().collect::<Vec<_>>();
    let mut behaviors = resolve_native_effect_requirements(version, &roots)?
        .into_iter()
        .map(|binding| {
            Ok(ResolvedCastleFightBehavior {
                id: stable_ability_id(binding.source)?,
                source: binding.source,
                implementation: binding.implementation,
            })
        })
        .collect::<Result<Vec<_>, CastleFightContentError>>()?;
    behaviors.sort_unstable_by_key(|behavior| behavior.id);

    let mut bundle = CastleFightContentBundle {
        map_version: version,
        revision: CASTLE_FIGHT_CONTENT_REVISION_927,
        availability: CastleFightContentAvailability::SupportedDevelopmentSubset,
        identity: CastleFightContentIdentity {
            schema_version: CASTLE_FIGHT_CONTENT_BUNDLE_SCHEMA_VERSION,
            gameplay_hash: 0,
        },
        command_card: castle_fight_command_card_layout_for_version(version)
            .map_err(|_| CastleFightContentError::UnsupportedRelease(version))?,
        economy: castle_fight_economy_rules_for_version(version)
            .map_err(|_| CastleFightContentError::UnsupportedRelease(version))?,
        damage_rules: castle_fight_damage_rules_for_version(version)
            .map_err(|_| CastleFightContentError::UnsupportedRelease(version))?,
        main_castle_repair_time_ticks: castle_fight_main_castle_repair_time_ticks_for_version(
            version,
        )
        .map_err(|_| CastleFightContentError::UnsupportedRelease(version))?,
        units,
        production_buildings,
        towers,
        builders,
        behaviors,
    };
    bundle.identity.gameplay_hash = canonical_content_bundle_hash(&bundle);
    Ok(bundle)
}

fn stable_ability_id(
    source: NativeEffectSource,
) -> Result<CastleFightAbilityId, CastleFightContentError> {
    let id = match (source.kind, source.key) {
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0CV") => {
            0x4000_0001
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03N") => {
            0x4000_0002
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A00U") => {
            0x4000_0003
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03G") => {
            0x4000_0004
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A05K") => {
            0x4000_0005
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A01B") => {
            0x4000_0006
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A049") => {
            0x4000_0007
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A05X") => {
            0x4000_0008
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A03W") => {
            0x4000_0009
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A02J") => {
            0x4000_000a
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03Z") => {
            0x4000_000b
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A09A") => {
            0x4000_000c
        }
        _ => return Err(CastleFightContentError::MissingStableAbilityId(source)),
    };
    Ok(CastleFightAbilityId(id))
}

fn canonical_content_bundle_hash(bundle: &CastleFightContentBundle) -> u64 {
    let mut hash = ContentHash64::new();
    hash.write_u64(0x4346_434f_4e54_0001);
    hash.write_u32(CASTLE_FIGHT_CONTENT_BUNDLE_SCHEMA_VERSION);
    hash.write_u16(bundle.map_version.major);
    hash.write_u16(bundle.map_version.minor);
    hash.write_bytes(bundle.revision.as_bytes());
    hash.write_i32(CASTLE_FIGHT_SIMULATION_HZ);
    hash_command_card(&mut hash, bundle.command_card);
    hash_economy_rules(&mut hash, bundle.economy);
    hash_damage_rules(&mut hash, bundle.damage_rules);
    hash.write_u32(bundle.main_castle_repair_time_ticks);

    hash.write_u64(bundle.units.len() as u64);
    for (id, definition) in &bundle.units {
        hash.write_u32(id.0);
        hash_unit_definition(&mut hash, *definition);
    }
    hash.write_u64(bundle.production_buildings.len() as u64);
    for (id, definition) in &bundle.production_buildings {
        hash.write_u32(id.0);
        hash_production_definition(&mut hash, *definition);
    }
    hash.write_u64(bundle.towers.len() as u64);
    for (id, definition) in &bundle.towers {
        hash.write_u32(id.0);
        hash_tower_definition(&mut hash, *definition);
    }
    hash.write_u64(bundle.builders.len() as u64);
    for (id, definition) in &bundle.builders {
        hash.write_u32(id.0);
        hash_builder_definition(&mut hash, definition);
    }
    hash.write_u64(bundle.behaviors.len() as u64);
    for behavior in &bundle.behaviors {
        hash.write_u32(behavior.id.0);
        hash.write_u8(behavior.source.kind.stable_tag());
        hash.write_u32(behavior.source.key);
        hash.write_u8(behavior.implementation.stable_tag());
    }
    hash.finish()
}

fn hash_command_card(hash: &mut ContentHash64, layout: CastleFightCommandCardLayout) {
    for position in [
        layout.move_command,
        layout.attack_command,
        layout.build_command,
        layout.cancel_command,
        layout.repair_ability,
        layout.blink_ability,
    ] {
        hash.write_u8(position.x);
        hash.write_u8(position.y);
    }
    hash.write_u32(layout.build_hotkey as u32);
    hash.write_u32(layout.blink_hotkey as u32);
}

fn hash_economy_rules(hash: &mut ContentHash64, rules: EconomyRules) {
    hash.write_u32(rules.starting_gold);
    hash.write_u32(rules.starting_lumber);
    hash.write_u16(rules.starting_legendary_points);
    hash.write_u64(rules.base_income_per_10k);
    hash.write_u32(rules.income_interval_ticks);
    hash.write_u64(rules.income_tax_bracket_per_10k);
}

fn hash_damage_rules(hash: &mut ContentHash64, rules: DamageRules) {
    use crate::damage::{ArmorType, DamageType};
    hash.write_u16(rules.armor_factor_per_10k());
    for damage_type in [
        DamageType::Normal,
        DamageType::Pierce,
        DamageType::Siege,
        DamageType::Magic,
        DamageType::Chaos,
        DamageType::Spells,
        DamageType::Hero,
    ] {
        for armor_type in [
            ArmorType::Small,
            ArmorType::Medium,
            ArmorType::Large,
            ArmorType::Fortified,
            ArmorType::Normal,
            ArmorType::Hero,
            ArmorType::Divine,
            ArmorType::Unarmored,
        ] {
            hash.write_u16(rules.bonus_per_10k(damage_type, armor_type));
        }
    }
}

fn hash_unit_definition(hash: &mut ContentHash64, definition: CastleFightUnitDefinition) {
    hash.write_u32(definition.rawcode);
    hash.write_i32(definition.health);
    hash.write_u32(definition.health_regen_per_second_per_10k);
    hash.write_u32(definition.build_time_ticks);
    hash.write_u32(definition.repair_time_ticks);
    hash.write_u8(definition.armor.armor_type.stable_tag());
    hash.write_i32(i32::from(definition.armor.armor_points));
    hash_passive_effects(hash, definition.passive_effects);
    hash_optional_spellcasting(hash, definition.spellcasting);
    hash.write_u8(definition.damage_type.stable_tag());
    hash.write_u8(definition.attack_targets.bits());
    hash.write_u8(match definition.movement_class {
        MovementClass::Ground => 0,
        MovementClass::Air => 1,
    });
    hash.write_u8(u8::from(definition.mechanical));
    hash.write_i32(definition.collision_radius.0);
    match definition.corpse {
        Some(corpse) => {
            hash.write_u8(1);
            hash.write_u32(corpse.definition.0);
            match corpse.lifetime_ticks {
                Some(ticks) => {
                    hash.write_u8(1);
                    hash.write_u32(ticks);
                }
                None => hash.write_u8(0),
            }
        }
        None => hash.write_u8(0),
    }
    hash_attack_profile(hash, definition.attack);
    hash.write_i32(definition.movement.speed_per_tick);
}

fn hash_production_definition(
    hash: &mut ContentHash64,
    definition: CastleFightProductionDefinition,
) {
    hash.write_u32(definition.rawcode);
    hash.write_u16(definition.gold_cost);
    hash.write_u16(definition.lumber_cost);
    hash_building_economy(hash, definition.economy);
    hash.write_i32(definition.building_health);
    hash.write_u32(definition.construction_time_ticks);
    hash.write_u32(definition.repair_time_ticks);
    hash.write_u8(definition.armor.armor_type.stable_tag());
    hash.write_i32(i32::from(definition.armor.armor_points));
    hash.write_u16(definition.spawn_interval_ticks);
    hash.write_u16(definition.footprint_size_cells);
    hash.write_u32(definition.unit.stable_id().0);
    hash_unit_definition(hash, definition.produced_unit);
    hash.write_u8(definition.train_command_position.x);
    hash.write_u8(definition.train_command_position.y);
    hash.write_u32(definition.train_hotkey as u32);
    hash.write_u8(definition.command_card_position.x);
    hash.write_u8(definition.command_card_position.y);
    hash.write_u32(definition.hotkey as u32);

    let kind = CastleFightProductionKind::from_rawcode_for_version(
        definition.rawcode,
        definition.map_version,
    )
    .expect("bundle production version is supported")
    .expect("bundle production rawcode is registered");
    match kind
        .upgrade_from_for_version(definition.map_version)
        .expect("bundle production version is supported")
    {
        Some(source) => {
            hash.write_u8(1);
            hash.write_u32(source.stable_id().0);
        }
        None => hash.write_u8(0),
    }
    let targets = kind
        .upgrade_targets_for_version(definition.map_version)
        .expect("bundle production version is supported");
    hash.write_u64(targets.len() as u64);
    for target in targets {
        hash.write_u32(target.stable_id().0);
    }
}

fn hash_tower_definition(hash: &mut ContentHash64, definition: CastleFightTowerDefinition) {
    hash.write_u32(definition.rawcode);
    hash.write_u16(definition.gold_cost);
    hash.write_u16(definition.lumber_cost);
    hash_building_economy(hash, definition.economy);
    hash.write_i32(definition.health);
    hash.write_u32(definition.construction_time_ticks);
    hash.write_u32(definition.repair_time_ticks);
    hash.write_u8(definition.armor.armor_type.stable_tag());
    hash.write_i32(i32::from(definition.armor.armor_points));
    hash.write_u8(definition.damage_type.stable_tag());
    hash.write_u8(definition.attack_targets.bits());
    hash.write_u16(definition.footprint_size_cells);
    hash.write_u8(definition.command_card_position.x);
    hash.write_u8(definition.command_card_position.y);
    hash.write_u32(definition.hotkey as u32);
    hash_attack_profile(hash, definition.attack);
}

fn hash_builder_definition(hash: &mut ContentHash64, definition: &CastleFightBuilderDefinition) {
    hash.write_u32(definition.rawcode);
    hash.write_u8(definition.race_index);
    hash.write_u8(u8::from(definition.campaign_only));
    hash.write_u8(match definition.locomotion {
        BuilderLocomotion::Foot => 0,
        BuilderLocomotion::Hover => 1,
    });
    hash.write_u8(u8::from(definition.repair_autocast_enabled_by_default));
    hash.write_i32(definition.profile.speed_per_tick);
    hash.write_i32(definition.profile.build_range);
    hash.write_i32(definition.profile.repair_range);
    hash.write_i32(definition.profile.repair_autocast_range);
    hash.write_u16(definition.profile.repair_time_ratio_numerator);
    hash.write_u16(definition.profile.repair_time_ratio_denominator);
    hash.write_u16(definition.profile.full_repair_duration_ticks);
    hash.write_i32(definition.profile.blink_range);
    hash.write_i32(definition.profile.blink_boundary_inset);
    hash.write_u64(definition.build_catalog.len() as u64);
    for rawcode in &definition.build_catalog {
        hash.write_u32(*rawcode);
    }
}

fn hash_building_economy(hash: &mut ContentHash64, economy: BuildingEconomyProfile) {
    hash.write_u32(economy.gold_cost);
    hash.write_u32(economy.lumber_cost);
    hash.write_u32(economy.lumber_refund);
    hash.write_u64(economy.income_per_10k);
}

fn hash_attack_profile(hash: &mut ContentHash64, attack: AttackProfile) {
    hash.write_u8(attack.delivery.stable_tag());
    match attack.delivery {
        AttackDelivery::Melee => {}
        AttackDelivery::RangedGuaranteedHit { speed_per_tick } => {
            hash.write_i32(speed_per_tick);
        }
        AttackDelivery::RangedBallistic {
            speed_per_tick,
            impact_radius,
        } => {
            hash.write_i32(speed_per_tick);
            hash.write_i32(impact_radius);
        }
        AttackDelivery::Bounce {
            speed_per_tick,
            bounce_range,
            max_bounces,
            damage_percent_per_bounce,
            allow_repeat_targets,
        } => {
            hash.write_i32(speed_per_tick);
            hash.write_i32(bounce_range);
            hash.write_u8(max_bounces);
            hash.write_u16(damage_percent_per_bounce);
            hash.write_u8(u8::from(allow_repeat_targets));
        }
    }
    hash.write_i32(attack.damage);
    hash.write_i32(attack.range);
    hash.write_i32(attack.acquisition_range);
    hash.write_u16(attack.cooldown_ticks);
}

fn hash_passive_effects(hash: &mut ContentHash64, effects: PassiveUnitEffects) {
    let effects = effects.iter().collect::<Vec<_>>();
    hash.write_u64(effects.len() as u64);
    for effect in effects {
        match effect {
            PassiveUnitEffect::Bash(profile) => {
                hash.write_u8(0);
                hash.write_u32(profile.ability.0);
                hash.write_u16(profile.chance_per_10k);
                hash.write_i32(profile.bonus_damage);
                hash.write_u16(profile.stun_duration_ticks);
                hash.write_u8(profile.targets.bits());
            }
            PassiveUnitEffect::Evasion(profile) => {
                hash.write_u8(1);
                hash.write_u32(profile.ability.0);
                hash.write_u16(profile.chance_per_10k);
            }
            PassiveUnitEffect::Defend(profile) => {
                hash.write_u8(2);
                hash.write_u32(profile.ability.0);
                hash.write_u16(profile.ranged_damage_taken_per_10k);
                hash.write_u16(profile.spell_damage_taken_per_10k);
                hash.write_u16(profile.deflect_chance_per_10k);
                hash.write_u16(profile.deflected_pierce_damage_taken_per_10k);
                hash.write_u16(profile.activation_delay_ticks);
            }
            PassiveUnitEffect::TriggeredSpellProc(profile) => {
                hash.write_u8(3);
                hash.write_u32(profile.ability.0);
                hash.write_u16(profile.chance_per_10k);
                hash.write_u8(profile.targets.bits());
                hash_triggered_effect(hash, profile.effect);
            }
            PassiveUnitEffect::BurningOil(profile) => {
                hash.write_u8(4);
                hash.write_u32(profile.ability.0);
                hash.write_i32(profile.radius);
                hash.write_i32(profile.full_damage);
                hash.write_u16(profile.full_interval_millis);
                hash.write_i32(profile.half_damage);
                hash.write_u16(profile.half_interval_millis);
                hash.write_u16(profile.full_duration_millis);
                hash.write_u16(profile.total_duration_millis);
                hash.write_u8(u8::from(profile.target_ground_units));
                hash.write_u8(u8::from(profile.target_buildings));
            }
        }
    }
}

fn hash_triggered_effect(hash: &mut ContentHash64, effect: TriggeredAttackEffect) {
    match effect {
        TriggeredAttackEffect::ChainLightning(profile) => {
            hash.write_u8(0);
            hash.write_u32(profile.ability.0);
            hash.write_i32(profile.initial_damage);
            hash.write_u8(profile.maximum_targets);
            hash.write_i32(profile.jump_radius);
            hash.write_u16(profile.damage_reduction_per_10k);
            hash.write_u8(profile.targets.bits());
        }
        TriggeredAttackEffect::EntanglingRoots(profile) => {
            hash.write_u8(1);
            hash.write_u32(profile.ability.0);
            hash.write_i32(profile.damage_per_second);
            hash.write_u16(profile.duration_ticks);
            hash.write_u8(profile.targets.bits());
        }
    }
}

fn hash_optional_spellcasting(hash: &mut ContentHash64, spellcasting: Option<SpellcastingProfile>) {
    let Some(spellcasting) = spellcasting else {
        hash.write_u8(0);
        return;
    };
    hash.write_u8(1);
    hash.write_i32(spellcasting.mana.maximum);
    hash.write_i32(spellcasting.mana.starting);
    hash.write_u32(spellcasting.mana.regen_per_tick_per_10k);
    hash.write_u32(spellcasting.ability.id.0);
    hash.write_i32(spellcasting.ability.mana_cost);
    hash.write_u16(spellcasting.ability.cooldown_ticks);
    hash.write_i32(spellcasting.ability.range);
    hash.write_u8(spellcasting.ability.target_policy.stable_tag());
    hash.write_u8(spellcasting.ability.effect.stable_tag());
    match spellcasting.ability.effect {
        AbilityEffect::Damage { amount } => hash.write_i32(amount),
        AbilityEffect::Stun { duration_ticks } => hash.write_u16(duration_ticks),
        AbilityEffect::ModifyMovementSpeedPercent {
            modifier,
            percent_delta,
            duration_ticks,
        } => {
            hash.write_u32(modifier.0);
            hash.write_i32(i32::from(percent_delta));
            hash.write_u16(duration_ticks);
        }
        AbilityEffect::AreaDamage { amount, radius } => {
            hash.write_i32(amount);
            hash.write_i32(radius);
        }
        AbilityEffect::FrostArmor {
            modifier,
            armor_bonus_per_100,
            armor_duration_ticks,
            slow_duration_ticks,
            movement_percent_delta,
            attack_speed_percent_delta,
        } => {
            hash.write_u32(modifier.0);
            hash.write_i32(i32::from(armor_bonus_per_100));
            hash.write_u16(armor_duration_ticks);
            hash.write_u16(slow_duration_ticks);
            hash.write_i32(i32::from(movement_percent_delta));
            hash.write_i32(i32::from(attack_speed_percent_delta));
        }
    }
}

struct ContentHash64(u64);

impl ContentHash64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;

    const fn new() -> Self {
        Self(Self::OFFSET)
    }

    fn write_bytes(&mut self, bytes: &[u8]) {
        self.write_u64(bytes.len() as u64);
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    fn write_u8(&mut self, value: u8) {
        self.write_bytes_raw(&[value]);
    }

    fn write_u16(&mut self, value: u16) {
        self.write_bytes_raw(&value.to_le_bytes());
    }

    fn write_u32(&mut self, value: u32) {
        self.write_bytes_raw(&value.to_le_bytes());
    }

    fn write_u64(&mut self, value: u64) {
        self.write_bytes_raw(&value.to_le_bytes());
    }

    fn write_i32(&mut self, value: i32) {
        self.write_bytes_raw(&value.to_le_bytes());
    }

    fn write_bytes_raw(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            self.0 ^= u64::from(byte);
            self.0 = self.0.wrapping_mul(Self::PRIME);
        }
    }

    const fn finish(self) -> u64 {
        self.0
    }
}

#[derive(Debug, Clone, Copy)]
struct ExtractedBuilding927 {
    name: &'static str,
    basic_tooltip: &'static str,
    extended_tooltip: &'static str,
    gold_cost: u16,
    lumber_cost: u16,
    construction_time_ticks: u32,
    health: i32,
    armor: ArmorProfile,
    footprint_size_cells: Option<u16>,
}

#[derive(Debug, Clone, Copy)]
struct ExtractedUnit927 {
    name: &'static str,
    basic_tooltip: &'static str,
    extended_tooltip: &'static str,
    build_time_ticks: u32,
    repair_time_ticks: Option<u32>,
    health_regen_per_second_per_10k: Option<i32>,
    movement_class: Option<MovementClass>,
    builder_locomotion: Option<BuilderLocomotion>,
    move_speed_per_tick: Option<i32>,
    mechanical: bool,
    collision_radius: CollisionRadius,
    acquisition_range: Option<i32>,
    projectile_speed_per_tick: Option<i32>,
    outer_splash_radius: Option<i32>,
    attack1_damage_type: Option<DamageType>,
    attack1_weapon_kind: Option<ExtractedWeaponKind927>,
    attack1_targets: Option<AttackTargetMask>,
}

#[derive(Debug, Clone, Copy)]
struct ExtractedProtectedStats927 {
    health: i32,
    armor: ArmorProfile,
    move_speed_per_tick: Option<i32>,
    attack1_damage: Option<i32>,
    attack1_cooldown_ticks: Option<u16>,
    attack1_range: Option<i32>,
}

#[derive(Debug, Clone, Copy)]
struct ExtractedAttack927 {
    damage_type: DamageType,
    weapon_kind: ExtractedWeaponKind927,
    targets: AttackTargetMask,
    damage: Option<i32>,
    cooldown_ticks: u16,
    range: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtractedWeaponKind927 {
    Melee,
    Missile,
    Artillery,
    Splash,
    Instant,
    Bounce,
    Line,
}

#[derive(Debug, Clone, Copy)]
struct ExtractedProduction927 {
    unit_rawcode: u32,
    spawn_interval_ticks: u16,
}

#[derive(Debug, Clone, Copy)]
struct ExtractedCorpse927 {
    does_decay: bool,
    lifetime_ticks: Option<u32>,
}

#[derive(Debug, Clone, Default)]
struct ExtractedUpgradeLinks927 {
    precursor: Option<u32>,
    targets: Vec<u32>,
}

#[derive(Debug, Clone, Copy)]
struct ExtractedIncomeSemantics927 {
    factor_per_1000: u16,
    is_siege: bool,
    precursor: Option<u32>,
}

#[derive(Debug, Clone)]
struct ExtractedBuilderCatalog927 {
    builder_name: &'static str,
    campaign_only: bool,
    direct_buildings: Vec<u32>,
}

#[derive(Debug, Deserialize)]
struct CatalogSourceManifest927 {
    schema_version: u32,
    map_version: String,
    release_revision: String,
    content_revision: String,
    extraction_git_tree: String,
    source_evidence_fnv64: u64,
}

#[derive(Debug, Deserialize)]
struct CatalogSupplement927 {
    schema_version: u32,
    map_version: String,
    release_revision: String,
    extraction_git_tree: String,
    source_object_fields_sha256: String,
    objects: Vec<CatalogSupplementObject927>,
}

#[derive(Debug, Deserialize)]
struct CatalogSupplementObject927 {
    rawcode: String,
    repair_time_seconds: u32,
    button_x: Option<u8>,
    button_y: Option<u8>,
    hotkey: Option<char>,
}

#[derive(Debug)]
struct ExtractedContent927 {
    buildings: BTreeMap<u32, ExtractedBuilding927>,
    units: BTreeMap<u32, ExtractedUnit927>,
    unit_abilities: BTreeMap<u32, Vec<u32>>,
    protected_stats: BTreeMap<u32, ExtractedProtectedStats927>,
    primary_attacks: BTreeMap<u32, ExtractedAttack927>,
    production: BTreeMap<u32, ExtractedProduction927>,
    corpses: BTreeMap<u32, ExtractedCorpse927>,
    repair_time_ticks: BTreeMap<u32, u32>,
    command_card_positions: BTreeMap<u32, CommandCardPosition>,
    building_hotkeys: BTreeMap<u32, char>,
    upgrades: BTreeMap<u32, ExtractedUpgradeLinks927>,
    income_semantics: BTreeMap<u32, ExtractedIncomeSemantics927>,
    income_per_10k: BTreeMap<u32, u64>,
    builder_catalogs: BTreeMap<(u8, u32), ExtractedBuilderCatalog927>,
    damage_rules: DamageRules,
}

impl ExtractedContent927 {
    fn load() -> Result<Self, String> {
        validate_catalog_source_927()?;
        let mut buildings = BTreeMap::new();
        for line in BUILDINGS_927_TSV.lines().skip(1) {
            let columns = line.split('\t').collect::<Vec<_>>();
            if columns.len() <= 13 {
                return Err("9.27 buildings.tsv row is missing required columns".to_owned());
            }
            let rawcode = parse_rawcode(columns[1]);
            let construction_seconds = columns[13]
                .parse::<u32>()
                .map_err(|_| format!("building {rawcode:#010x} has invalid construction time"))?;
            let footprint_width = parse_optional_u16_927(columns[15])?;
            let footprint_height = parse_optional_u16_927(columns[16])?;
            let footprint_size_cells = match (footprint_width, footprint_height) {
                (Some(width), Some(height)) if width == height => Some(width),
                (Some(_), Some(_)) => None,
                _ => None,
            };
            let row = ExtractedBuilding927 {
                name: columns[3],
                basic_tooltip: columns[4],
                extended_tooltip: columns[5],
                gold_cost: columns[11]
                    .parse::<u16>()
                    .map_err(|_| format!("building {rawcode:#010x} has invalid gold cost"))?,
                lumber_cost: columns[12]
                    .parse::<u16>()
                    .map_err(|_| format!("building {rawcode:#010x} has invalid lumber cost"))?,
                construction_time_ticks: construction_seconds
                    .checked_mul(CASTLE_FIGHT_SIMULATION_HZ as u32)
                    .ok_or_else(|| {
                        format!("building {rawcode:#010x} construction time overflowed")
                    })?,
                health: columns[6]
                    .parse::<i32>()
                    .map_err(|_| format!("building {rawcode:#010x} has invalid health"))?,
                armor: ArmorProfile::new(
                    parse_armor_type_927(columns[8])?,
                    columns[7]
                        .parse::<i16>()
                        .map_err(|_| format!("building {rawcode:#010x} has invalid armor"))?,
                ),
                footprint_size_cells,
            };
            if buildings.insert(rawcode, row).is_some() {
                return Err(format!("duplicate building row for {rawcode:#010x}"));
            }
        }

        let supplement = catalog_supplement_927()?;
        let mut repair_time_ticks = BTreeMap::<u32, u32>::new();
        let mut command_card_positions = BTreeMap::new();
        let mut building_hotkeys = BTreeMap::new();
        for object in supplement.objects {
            let rawcode = parse_rawcode(&object.rawcode);
            let repair_ticks = object
                .repair_time_seconds
                .checked_mul(CASTLE_FIGHT_SIMULATION_HZ as u32)
                .ok_or_else(|| format!("unit {rawcode:#010x} repair time overflowed"))?;
            if repair_time_ticks.insert(rawcode, repair_ticks).is_some() {
                return Err(format!(
                    "catalog supplement repeats repair time for {rawcode:#010x}"
                ));
            }
            // Some archived unit rows expose only one button coordinate. Preserve the evidence
            // in the generated supplement, but only promote a complete pair into the native
            // command-card index.
            if let (Some(x), Some(y)) = (object.button_x, object.button_y)
                && command_card_positions
                    .insert(rawcode, CommandCardPosition::new(x, y))
                    .is_some()
            {
                return Err(format!(
                    "catalog supplement repeats command-card position for {rawcode:#010x}"
                ));
            }
            if let Some(hotkey) = object.hotkey
                && building_hotkeys.insert(rawcode, hotkey).is_some()
            {
                return Err(format!(
                    "catalog supplement repeats hotkey for {rawcode:#010x}"
                ));
            }
        }

        let mut units = BTreeMap::new();
        let mut unit_abilities = BTreeMap::new();
        for line in UNITS_927_TSV.lines().skip(1) {
            let columns = line.split('\t').collect::<Vec<_>>();
            if columns.len() <= 45 {
                return Err("9.27 units.tsv row is missing required columns".to_owned());
            }
            let rawcode = parse_rawcode(columns[1]);
            let build_seconds = columns[8]
                .parse::<u32>()
                .map_err(|_| format!("unit {rawcode:#010x} has invalid build time"))?;
            let health_regen_per_second_per_10k =
                parse_optional_decimal_scaled_i32_927(columns[12], 10_000).map_err(|error| {
                    format!("unit {rawcode:#010x} has invalid health regeneration: {error}")
                })?;
            let collision_world = columns[21]
                .parse::<i32>()
                .map_err(|_| format!("unit {rawcode:#010x} has invalid collision radius"))?;
            let acquisition_range = parse_optional_i32_927(columns[22])?.map(world);
            let projectile_speed_per_tick =
                parse_optional_i32_927(columns[45])?.map(projectile_speed);
            let outer_splash_radius = parse_optional_i32_927(columns[41])?.map(world);
            let attack1_enabled = columns[26] == "True";
            let attack1_damage_type = if attack1_enabled && columns[27] != "unknown" {
                Some(parse_damage_type_927(columns[27])?)
            } else {
                None
            };
            let attack1_weapon_kind = attack1_enabled
                .then(|| parse_weapon_kind_927(columns[28]))
                .transpose()?;
            let attack1_targets = attack1_enabled.then(|| parse_attack_targets_927(columns[38]));
            let (movement_class, builder_locomotion) = match columns[19] {
                "fly" => (Some(MovementClass::Air), None),
                "foot" => (Some(MovementClass::Ground), Some(BuilderLocomotion::Foot)),
                "hover" => (Some(MovementClass::Ground), Some(BuilderLocomotion::Hover)),
                "horse" | "float" | "amph" => (Some(MovementClass::Ground), None),
                "" | "_" | "-" => (None, None),
                other => {
                    return Err(format!(
                        "unit {rawcode:#010x} has unknown movement type {other:?}"
                    ));
                }
            };
            let move_speed_per_tick =
                parse_optional_i32_927(columns[20])?.map(|speed| movement(speed).speed_per_tick);
            let mechanical = columns[24]
                .split(',')
                .any(|classification| classification.eq_ignore_ascii_case("mechanical"));
            let abilities = columns[23]
                .split(',')
                .map(str::trim)
                .filter(|ability| !ability.is_empty() && *ability != "-" && *ability != "_")
                .map(|ability| {
                    let bytes: [u8; 4] = ability.as_bytes().try_into().map_err(|_| {
                        format!("unit {rawcode:#010x} has invalid ability rawcode {ability:?}")
                    })?;
                    Ok(u32::from_be_bytes(bytes))
                })
                .collect::<Result<Vec<_>, String>>()?;
            let row = ExtractedUnit927 {
                name: columns[3],
                basic_tooltip: columns[4],
                extended_tooltip: columns[5],
                build_time_ticks: build_seconds
                    .checked_mul(CASTLE_FIGHT_SIMULATION_HZ as u32)
                    .ok_or_else(|| format!("unit {rawcode:#010x} build time overflowed"))?,
                repair_time_ticks: repair_time_ticks.get(&rawcode).copied(),
                health_regen_per_second_per_10k,
                movement_class,
                builder_locomotion,
                move_speed_per_tick,
                mechanical,
                collision_radius: CollisionRadius(world(collision_world)),
                acquisition_range,
                projectile_speed_per_tick,
                outer_splash_radius,
                attack1_damage_type,
                attack1_weapon_kind,
                attack1_targets,
            };
            if units.insert(rawcode, row).is_some() {
                return Err(format!("duplicate unit row for {rawcode:#010x}"));
            }
            let mut seen_abilities = BTreeSet::new();
            for &ability in &abilities {
                if !seen_abilities.insert(ability) {
                    return Err(format!(
                        "unit {rawcode:#010x} repeats ability rawcode {ability:#010x}"
                    ));
                }
            }
            unit_abilities.insert(rawcode, abilities);
        }

        let mut protected_stats = BTreeMap::new();
        for line in PROTECTED_UNIT_STATS_927_TSV.lines().skip(1) {
            let columns = line.split('\t').collect::<Vec<_>>();
            if columns.len() <= 19 {
                return Err("9.27 protected-unit-stats row is missing required columns".to_owned());
            }
            let rawcode = parse_rawcode(columns[0]);
            let health = columns[8]
                .parse::<i32>()
                .map_err(|_| format!("unit {rawcode:#010x} has invalid protected HP"))?;
            let armor_points = columns[12]
                .parse::<i16>()
                .map_err(|_| format!("unit {rawcode:#010x} has invalid protected armor"))?;
            let move_speed_per_tick =
                parse_optional_i32_927(columns[18])?.map(|speed| movement(speed).speed_per_tick);
            let attack1_damage = parse_optional_decimal_rounded_i32_927(columns[42])?;
            let attack1_cooldown_ticks = parse_optional_seconds_to_ticks_u16_927(columns[34])?;
            let attack1_range = parse_optional_i32_927(columns[38])?.map(world);
            let row = ExtractedProtectedStats927 {
                health,
                armor: ArmorProfile::new(parse_armor_type_927(columns[14])?, armor_points),
                move_speed_per_tick,
                attack1_damage,
                attack1_cooldown_ticks,
                attack1_range,
            };
            if protected_stats.insert(rawcode, row).is_some() {
                return Err(format!(
                    "duplicate protected unit stats for {rawcode:#010x}"
                ));
            }
        }

        let mut primary_attacks = BTreeMap::new();
        for line in PRODUCTION_UNIT_ATTACKS_927_TSV.lines().skip(1) {
            let columns = line.split('\t').collect::<Vec<_>>();
            if columns.len() <= 26 || columns[4] != "1" || columns[5] != "default" {
                continue;
            }
            let rawcode = parse_rawcode(columns[2]);
            let weapon_kind = parse_weapon_kind_927(columns[12])?;
            let row = ExtractedAttack927 {
                damage_type: parse_damage_type_927(columns[11])?,
                weapon_kind,
                targets: parse_attack_targets_927(columns[13]),
                damage: parse_optional_decimal_rounded_i32_927(columns[26])?,
                cooldown_ticks: parse_seconds_to_ticks_u16_927(columns[20])?,
                range: world(
                    columns[22]
                        .parse::<i32>()
                        .map_err(|_| format!("unit {rawcode:#010x} has invalid attack range"))?,
                ),
            };
            if primary_attacks.insert(rawcode, row).is_some() {
                return Err(format!("duplicate primary attack row for {rawcode:#010x}"));
            }
        }

        let production_header = PRODUCTION_BUILDINGS_927_TSV
            .lines()
            .next()
            .ok_or_else(|| "9.27 production-buildings.tsv is empty".to_owned())?;
        let production_building_rawcode =
            tsv_column_index_927(production_header, "building_rawcode")?;
        let production_kind = tsv_column_index_927(production_header, "building_kind")?;
        let production_unit_rawcode = tsv_column_index_927(production_header, "unit_rawcode")?;
        let production_spawn_time = tsv_column_index_927(production_header, "spawn_time")?;
        let production_required_max = [
            production_building_rawcode,
            production_kind,
            production_unit_rawcode,
            production_spawn_time,
        ]
        .into_iter()
        .max()
        .expect("production field list must not be empty");

        let mut production = BTreeMap::new();
        for line in PRODUCTION_BUILDINGS_927_TSV.lines().skip(1) {
            let columns = line.split('\t').collect::<Vec<_>>();
            if columns.len() <= production_required_max {
                return Err("9.27 production-buildings row is missing required columns".to_owned());
            }
            if columns[production_kind] != "production" {
                continue;
            }
            let building_rawcode = parse_rawcode(columns[production_building_rawcode]);
            let unit_rawcode = parse_rawcode(columns[production_unit_rawcode]);
            let spawn_interval_ticks =
                parse_seconds_to_ticks_u16_927(columns[production_spawn_time])?;
            let row = ExtractedProduction927 {
                unit_rawcode,
                spawn_interval_ticks,
            };
            if production.insert(building_rawcode, row).is_some() {
                return Err(format!(
                    "duplicate production-building row for {building_rawcode:#010x}"
                ));
            }
        }

        let mut corpses = BTreeMap::new();
        for line in PRODUCTION_UNIT_CORPSES_927_TSV.lines().skip(1) {
            let columns = line.split('\t').collect::<Vec<_>>();
            if columns.len() <= 13 {
                return Err(
                    "9.27 production-unit-corpses row is missing required columns".to_owned(),
                );
            }
            let rawcode = parse_rawcode(columns[2]);
            let does_decay = columns[9] == "1";
            let lifetime_ticks = if does_decay && !columns[13].is_empty() {
                Some(parse_seconds_to_ticks_u32_927(columns[13])?)
            } else {
                None
            };
            corpses.insert(
                rawcode,
                ExtractedCorpse927 {
                    does_decay,
                    lifetime_ticks,
                },
            );
        }

        let mut upgrades = BTreeMap::<u32, ExtractedUpgradeLinks927>::new();
        for line in BUILDING_UPGRADES_927_TSV.lines().skip(1) {
            let mut columns = line.split('\t');
            let source = parse_rawcode(
                columns
                    .next()
                    .ok_or_else(|| "building-upgrade row missing source rawcode".to_owned())?,
            );
            let _source_integer = columns.next();
            let _source_name = columns.next();
            let target = parse_rawcode(
                columns
                    .next()
                    .ok_or_else(|| "building-upgrade row missing target rawcode".to_owned())?,
            );
            upgrades.entry(source).or_default().targets.push(target);
            let target_links = upgrades.entry(target).or_default();
            if target_links.precursor.replace(source).is_some() {
                return Err(format!(
                    "building {target:#010x} has more than one extracted precursor"
                ));
            }
        }
        for links in upgrades.values_mut() {
            links.targets.sort_unstable();
            links.targets.dedup();
        }

        let income_header = BUILDING_INCOME_927_TSV
            .lines()
            .next()
            .ok_or_else(|| "9.27 race-building-semantics.tsv is empty".to_owned())?;
        let income_rawcode = tsv_column_index_927(income_header, "building_rawcode")?;
        let income_factor = tsv_column_index_927(income_header, "income_factor")?;
        let income_precursor = tsv_column_index_927(income_header, "precursor_rawcode")?;
        let income_is_siege = tsv_column_index_927(income_header, "is_siege")?;
        let income_required_max = [
            income_rawcode,
            income_factor,
            income_precursor,
            income_is_siege,
        ]
        .into_iter()
        .max()
        .expect("income field list must not be empty");

        let mut income_semantics = BTreeMap::new();
        for line in BUILDING_INCOME_927_TSV.lines().skip(1) {
            let columns = line.split('\t').collect::<Vec<_>>();
            if columns.len() <= income_required_max {
                return Err(
                    "9.27 race-building-semantics row is missing required columns".to_owned(),
                );
            }
            let rawcode = parse_rawcode(columns[income_rawcode]);
            let precursor = (!columns[income_precursor].is_empty())
                .then(|| parse_rawcode(columns[income_precursor]));
            let semantics = ExtractedIncomeSemantics927 {
                factor_per_1000: parse_decimal_per_1000(columns[income_factor]),
                is_siege: columns[income_is_siege] == "1",
                precursor,
            };
            if income_semantics.insert(rawcode, semantics).is_some() {
                return Err(format!("duplicate income semantics for {rawcode:#010x}"));
            }
        }

        let mut income_per_10k = BTreeMap::new();
        for rawcode in income_semantics.keys().copied().collect::<Vec<_>>() {
            calculate_income_927(
                rawcode,
                &buildings,
                &income_semantics,
                &mut income_per_10k,
                &mut BTreeSet::new(),
            )?;
        }

        #[derive(Debug)]
        struct BuilderCatalogWork {
            builder_name: &'static str,
            campaign_only: bool,
            next_order: usize,
            direct_buildings: Vec<u32>,
        }

        let mut builder_catalog_work = BTreeMap::<(u8, u32), BuilderCatalogWork>::new();
        for line in BUILDER_CATALOG_927_TSV.lines().skip(1) {
            let mut columns = line.split('\t');
            let race_index = columns
                .next()
                .ok_or_else(|| "race-buildings row missing race index".to_owned())?
                .parse::<u8>()
                .map_err(|_| "race-buildings race index must be numeric".to_owned())?;
            let _race_function = columns.next();
            let builder_rawcode = parse_rawcode(
                columns
                    .next()
                    .ok_or_else(|| "race-buildings row missing builder rawcode".to_owned())?,
            );
            let _builder_rawcode_integer = columns.next();
            let builder_name = columns
                .next()
                .ok_or_else(|| "race-buildings row missing builder name".to_owned())?;
            let campaign_only = match columns
                .next()
                .ok_or_else(|| "race-buildings row missing campaign flag".to_owned())?
            {
                "0" => false,
                "1" => true,
                value => return Err(format!("invalid builder campaign flag {value:?}")),
            };
            let building_order = columns
                .next()
                .ok_or_else(|| "race-buildings row missing building order".to_owned())?
                .parse::<usize>()
                .map_err(|_| "race-buildings building order must be numeric".to_owned())?;
            let building_rawcode = parse_rawcode(
                columns
                    .next()
                    .ok_or_else(|| "race-buildings row missing building rawcode".to_owned())?,
            );

            let work = builder_catalog_work
                .entry((race_index, builder_rawcode))
                .or_insert_with(|| BuilderCatalogWork {
                    builder_name,
                    campaign_only,
                    next_order: 0,
                    direct_buildings: Vec::new(),
                });
            if work.builder_name != builder_name || work.campaign_only != campaign_only {
                return Err(format!(
                    "builder metadata changed within race {race_index} catalog"
                ));
            }
            if work.next_order != building_order {
                return Err(format!(
                    "builder catalog order changed for race {race_index}: expected {}, got {building_order}",
                    work.next_order
                ));
            }
            work.next_order += 1;
            let is_upgrade_target = upgrades
                .get(&building_rawcode)
                .is_some_and(|links| links.precursor.is_some());
            if !is_upgrade_target {
                if work.direct_buildings.contains(&building_rawcode) {
                    return Err(format!(
                        "builder race {race_index} repeats building {building_rawcode:#010x}"
                    ));
                }
                work.direct_buildings.push(building_rawcode);
            }
        }

        let mut builder_catalogs = BTreeMap::new();
        for (key, work) in builder_catalog_work {
            if work.direct_buildings.is_empty() {
                return Err(format!("builder race {} has no direct buildings", key.0));
            }
            builder_catalogs.insert(
                key,
                ExtractedBuilderCatalog927 {
                    builder_name: work.builder_name,
                    campaign_only: work.campaign_only,
                    direct_buildings: work.direct_buildings,
                },
            );
        }

        let damage_rules = DamageRules::from_wc3_misc_text(WAR3MAP_MISC_927)
            .map_err(|error| format!("invalid 9.27 war3mapMisc.txt damage table: {error}"))?;

        Ok(Self {
            buildings,
            units,
            unit_abilities,
            protected_stats,
            primary_attacks,
            production,
            corpses,
            repair_time_ticks,
            command_card_positions,
            building_hotkeys,
            upgrades,
            income_semantics,
            income_per_10k,
            builder_catalogs,
            damage_rules,
        })
    }
}

fn catalog_supplement_927() -> Result<CatalogSupplement927, String> {
    let supplement: CatalogSupplement927 = serde_json::from_str(CATALOG_SUPPLEMENT_927_R1_JSON)
        .map_err(|error| format!("invalid 9.27 catalog supplement: {error}"))?;
    if supplement.schema_version != 1 {
        return Err(format!(
            "unsupported 9.27 catalog supplement schema {}",
            supplement.schema_version
        ));
    }
    if supplement.map_version != "9.27" || supplement.release_revision != "r1" {
        return Err(format!(
            "9.27 catalog supplement has unexpected release identity {}/{}",
            supplement.map_version, supplement.release_revision
        ));
    }
    if supplement.extraction_git_tree != CASTLE_FIGHT_EXTRACTION_TREE_927_R1 {
        return Err(format!(
            "9.27 catalog supplement declares extraction tree {}, expected {}",
            supplement.extraction_git_tree, CASTLE_FIGHT_EXTRACTION_TREE_927_R1
        ));
    }
    if supplement.source_object_fields_sha256 != OBJECT_FIELDS_927_R1_SHA256 {
        return Err(format!(
            "9.27 catalog supplement declares unexpected object-fields digest {}",
            supplement.source_object_fields_sha256
        ));
    }
    Ok(supplement)
}

fn validate_catalog_source_927() -> Result<(), String> {
    let manifest: CatalogSourceManifest927 = serde_json::from_str(CATALOG_SOURCE_927_R1_JSON)
        .map_err(|error| format!("invalid 9.27 catalog-source manifest: {error}"))?;
    if manifest.schema_version != 2 {
        return Err(format!(
            "unsupported 9.27 catalog-source schema {}",
            manifest.schema_version
        ));
    }
    if manifest.map_version != "9.27" || manifest.release_revision != "r1" {
        return Err(format!(
            "9.27 catalog-source manifest has unexpected release identity {}/{}",
            manifest.map_version, manifest.release_revision
        ));
    }
    if manifest.content_revision != CASTLE_FIGHT_CONTENT_REVISION_927 {
        return Err(format!(
            "9.27 catalog-source manifest declares content revision {:?}, expected {:?}",
            manifest.content_revision, CASTLE_FIGHT_CONTENT_REVISION_927
        ));
    }
    if manifest.extraction_git_tree != CASTLE_FIGHT_EXTRACTION_TREE_927_R1 {
        return Err(format!(
            "9.27 catalog-source manifest declares extraction tree {}, expected {}",
            manifest.extraction_git_tree, CASTLE_FIGHT_EXTRACTION_TREE_927_R1
        ));
    }

    let sources = [
        (
            "docs/original_map/extracted/resolved/buildings.tsv",
            BUILDINGS_927_TSV,
        ),
        (
            "docs/original_map/extracted/script/building-upgrades.tsv",
            BUILDING_UPGRADES_927_TSV,
        ),
        (
            "docs/original_map/extracted/script/race-building-semantics.tsv",
            BUILDING_INCOME_927_TSV,
        ),
        (
            "docs/original_map/extracted/script/race-buildings.tsv",
            BUILDER_CATALOG_927_TSV,
        ),
        (
            "docs/original_map/extracted/war3mapMisc.txt",
            WAR3MAP_MISC_927,
        ),
        (
            "docs/original_map/extracted/resolved/units.tsv",
            UNITS_927_TSV,
        ),
        (
            "docs/original_map/extracted/resolved/protected-unit-stats.tsv",
            PROTECTED_UNIT_STATS_927_TSV,
        ),
        (
            "docs/original_map/extracted/resolved/production-unit-attacks.tsv",
            PRODUCTION_UNIT_ATTACKS_927_TSV,
        ),
        (
            "docs/original_map/extracted/resolved/production-buildings.tsv",
            PRODUCTION_BUILDINGS_927_TSV,
        ),
        (
            "docs/original_map/extracted/resolved/production-unit-corpses.tsv",
            PRODUCTION_UNIT_CORPSES_927_TSV,
        ),
        (
            "crates/sim/data/castle-fight/9.27/catalog-supplement-r1.json",
            CATALOG_SUPPLEMENT_927_R1_JSON,
        ),
    ];
    let mut hash = ContentHash64::new();
    for (path, contents) in sources {
        hash.write_bytes(path.as_bytes());
        write_catalog_source_evidence_927(&mut hash, path, contents)?;
    }
    let actual = hash.finish();
    if actual != manifest.source_evidence_fnv64 {
        return Err(format!(
            "retained 9.27 catalog evidence drifted: expected {:#018x}, got {actual:#018x}; register a new content revision instead of silently changing r1",
            manifest.source_evidence_fnv64
        ));
    }
    Ok(())
}

fn write_catalog_source_evidence_927(
    hash: &mut ContentHash64,
    path: &str,
    contents: &str,
) -> Result<(), String> {
    let projection = match path {
        "docs/original_map/extracted/script/race-building-semantics.tsv" => {
            Some(project_tsv_fields_927(
                contents,
                &[
                    "building_rawcode",
                    "income_factor",
                    "precursor_rawcode",
                    "is_siege",
                ],
                None,
                "building_rawcode",
            )?)
        }
        "docs/original_map/extracted/resolved/production-buildings.tsv" => {
            Some(project_tsv_fields_927(
                contents,
                &["building_rawcode", "unit_rawcode", "spawn_time"],
                Some(("building_kind", "production")),
                "building_rawcode",
            )?)
        }
        _ => None,
    };
    if let Some(projection) = projection {
        hash.write_bytes(projection.as_bytes());
    } else {
        write_canonical_catalog_text(hash, contents);
    }
    Ok(())
}

fn project_tsv_fields_927(
    contents: &str,
    fields: &[&str],
    predicate: Option<(&str, &str)>,
    sort_field: &str,
) -> Result<String, String> {
    let mut lines = contents.lines();
    let header = lines
        .next()
        .ok_or_else(|| "runtime catalog evidence TSV is empty".to_owned())?;
    let field_indices = fields
        .iter()
        .map(|field| tsv_column_index_927(header, field))
        .collect::<Result<Vec<_>, _>>()?;
    let predicate_index = predicate
        .map(|(field, value)| tsv_column_index_927(header, field).map(|index| (index, value)))
        .transpose()?;
    let sort_index = fields
        .iter()
        .position(|field| *field == sort_field)
        .ok_or_else(|| format!("sort field {sort_field:?} is not projected"))?;

    let mut rows = Vec::new();
    for line in lines.filter(|line| !line.is_empty()) {
        let columns = line.split('\t').collect::<Vec<_>>();
        if let Some((index, expected)) = predicate_index
            && columns.get(index).copied() != Some(expected)
        {
            continue;
        }
        let row = field_indices
            .iter()
            .map(|index| {
                columns.get(*index).copied().ok_or_else(|| {
                    "runtime catalog evidence TSV row is missing a projected column".to_owned()
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        rows.push(row);
    }
    rows.sort_unstable_by(|left, right| left[sort_index].cmp(right[sort_index]));

    let mut projected = fields.join("\t");
    projected.push('\n');
    for row in rows {
        projected.push_str(&row.join("\t"));
        projected.push('\n');
    }
    Ok(projected)
}

fn tsv_column_index_927(header: &str, field: &str) -> Result<usize, String> {
    header
        .split('\t')
        .position(|column| column == field)
        .ok_or_else(|| format!("runtime catalog evidence TSV is missing column {field:?}"))
}

fn write_canonical_catalog_text(hash: &mut ContentHash64, contents: &str) {
    if contents
        .as_bytes()
        .windows(2)
        .any(|window| window == b"\r\n")
    {
        let normalized = contents.replace("\r\n", "\n");
        hash.write_bytes(normalized.as_bytes());
    } else {
        hash.write_bytes(contents.as_bytes());
    }
}

fn calculate_income_927(
    rawcode: u32,
    buildings: &BTreeMap<u32, ExtractedBuilding927>,
    semantics: &BTreeMap<u32, ExtractedIncomeSemantics927>,
    memo: &mut BTreeMap<u32, u64>,
    visiting: &mut BTreeSet<u32>,
) -> Result<u64, String> {
    if let Some(value) = memo.get(&rawcode).copied() {
        return Ok(value);
    }
    if !visiting.insert(rawcode) {
        return Err(format!(
            "building income precursor cycle reaches {rawcode:#010x}"
        ));
    }
    let row = buildings
        .get(&rawcode)
        .ok_or_else(|| format!("income semantics reference missing building {rawcode:#010x}"))?;
    let income = semantics
        .get(&rawcode)
        .ok_or_else(|| format!("building {rawcode:#010x} missing income semantics"))?;
    let own = u64::from(row.gold_cost) * u64::from(income.factor_per_1000);
    let inherited = match income.precursor {
        Some(precursor) => calculate_income_927(precursor, buildings, semantics, memo, visiting)?,
        None => 0,
    };
    visiting.remove(&rawcode);
    let total = own
        .checked_add(inherited)
        .ok_or_else(|| format!("building {rawcode:#010x} income overflowed"))?;
    memo.insert(rawcode, total);
    Ok(total)
}

fn extracted_content_927() -> &'static ExtractedContent927 {
    static CONTENT: OnceLock<ExtractedContent927> = OnceLock::new();
    CONTENT.get_or_init(|| {
        ExtractedContent927::load()
            .expect("committed Castle Fight 9.27 extraction must remain valid")
    })
}

fn extracted_builder_definition_927(race: CastleFightBuilderRace) -> CastleFightBuilderDefinition {
    let race_index = race as u8;
    let content = extracted_content_927();
    let mut matching = content
        .builder_catalogs
        .iter()
        .filter(|((candidate_race, _), _)| *candidate_race == race_index);
    let ((_, builder_rawcode), catalog) = matching
        .next()
        .unwrap_or_else(|| panic!("builder race {race_index} missing from retained 9.27 catalog"));
    assert!(
        matching.next().is_none(),
        "builder race {race_index} has more than one retained builder"
    );
    let unit = content.units.get(builder_rawcode).unwrap_or_else(|| {
        panic!("builder {builder_rawcode:#010x} missing retained 9.27 unit metadata")
    });
    assert_eq!(
        unit.name, catalog.builder_name,
        "builder name differs between retained 9.27 sources"
    );
    let locomotion = unit.builder_locomotion.unwrap_or_else(|| {
        panic!("builder {builder_rawcode:#010x} has unsupported retained locomotion")
    });
    let speed_per_tick = unit.move_speed_per_tick.unwrap_or_else(|| {
        panic!("builder {builder_rawcode:#010x} is missing retained movement speed")
    });

    CastleFightBuilderDefinition {
        race,
        race_index,
        rawcode: *builder_rawcode,
        name: unit.name,
        campaign_only: catalog.campaign_only,
        locomotion,
        // 9.27 `udaa` resolves to Repair for every standard race builder. Campaign-only
        // Critter Builder still has Repair in its ability list but leaves `udaa` blank.
        repair_autocast_enabled_by_default: !matches!(race, CastleFightBuilderRace::Critter),
        profile: builder_profile(speed_per_tick),
        build_catalog: catalog.direct_buildings.clone(),
        map_version: MapVersion::CASTLE_FIGHT_9_27,
    }
}

fn parse_rawcode(value: &str) -> u32 {
    let bytes: [u8; 4] = value
        .as_bytes()
        .try_into()
        .expect("rawcode must contain exactly four bytes");
    u32::from_be_bytes(bytes)
}

#[must_use]
pub fn castle_fight_main_castle_repair_time_ticks() -> u32 {
    castle_fight_main_castle_repair_time_ticks_for_version(CASTLE_FIGHT_DEFAULT_MAP_VERSION)
        .expect("default Castle Fight map version must remain available")
}

pub fn castle_fight_main_castle_repair_time_ticks_for_version(
    version: MapVersion,
) -> Result<u32, UnsupportedCastleFightMapVersion> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return Err(UnsupportedCastleFightMapVersion(version));
    }
    Ok(*extracted_content_927()
        .repair_time_ticks
        .get(&u32::from_be_bytes(*b"hcas"))
        .expect("main castle repair time must exist in the 9.27 catalog supplement"))
}

#[must_use]
pub fn castle_fight_damage_rules() -> DamageRules {
    castle_fight_damage_rules_for_version(CASTLE_FIGHT_DEFAULT_MAP_VERSION)
        .expect("default Castle Fight map version must remain available")
}

pub fn castle_fight_damage_rules_for_version(
    version: MapVersion,
) -> Result<DamageRules, UnsupportedCastleFightMapVersion> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return Err(UnsupportedCastleFightMapVersion(version));
    }
    Ok(extracted_content_927().damage_rules)
}

#[must_use]
pub fn castle_fight_economy_rules() -> EconomyRules {
    castle_fight_economy_rules_for_version(CASTLE_FIGHT_DEFAULT_MAP_VERSION)
        .expect("default Castle Fight map version must remain available")
}

pub fn castle_fight_economy_rules_for_version(
    version: MapVersion,
) -> Result<EconomyRules, UnsupportedCastleFightMapVersion> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return Err(UnsupportedCastleFightMapVersion(version));
    }
    Ok(EconomyRules {
        starting_gold: CASTLE_FIGHT_STARTING_GOLD,
        starting_lumber: CASTLE_FIGHT_STARTING_LUMBER,
        starting_legendary_points: CASTLE_FIGHT_STARTING_LEGENDARY_POINTS,
        base_income_per_10k: CASTLE_FIGHT_BASE_INCOME_GOLD * RESOURCE_FIXED_SCALE,
        income_interval_ticks: CASTLE_FIGHT_INCOME_INTERVAL_SECONDS
            * CASTLE_FIGHT_SIMULATION_HZ as u32,
        income_tax_bracket_per_10k: CASTLE_FIGHT_INCOME_TAX_BRACKET_GOLD * RESOURCE_FIXED_SCALE,
    })
}

fn extracted_building_economy_927(rawcode: u32) -> BuildingEconomyProfile {
    let (gold_cost, lumber_cost) = extracted_building_costs_927(rawcode);
    let (_, is_siege, _) = extracted_building_income_semantics_927(rawcode);
    let lumber_refund = if lumber_cost == 0 {
        if is_siege {
            u32::from(gold_cost) * 3 / 4
        } else {
            u32::from(gold_cost)
        }
    } else {
        0
    };
    BuildingEconomyProfile {
        gold_cost: u32::from(gold_cost),
        lumber_cost: u32::from(lumber_cost),
        lumber_refund,
        income_per_10k: extracted_building_income_per_10k_927(rawcode),
    }
}

fn extracted_building_costs_927(rawcode: u32) -> (u16, u16) {
    let row = extracted_content_927()
        .buildings
        .get(&rawcode)
        .unwrap_or_else(|| panic!("building {rawcode:#010x} missing from retained 9.27 table"));
    (row.gold_cost, row.lumber_cost)
}

#[cfg(test)]
fn extracted_building_tooltips_927(rawcode: u32) -> (&'static str, &'static str) {
    let row = extracted_content_927()
        .buildings
        .get(&rawcode)
        .unwrap_or_else(|| panic!("building {rawcode:#010x} missing from retained 9.27 table"));
    (row.basic_tooltip, row.extended_tooltip)
}

fn extracted_building_upgrade_links_927(rawcode: u32) -> (Option<u32>, Vec<u32>) {
    let links = extracted_content_927()
        .upgrades
        .get(&rawcode)
        .cloned()
        .unwrap_or_default();
    (links.precursor, links.targets)
}

fn extracted_building_income_per_10k_927(rawcode: u32) -> u64 {
    *extracted_content_927()
        .income_per_10k
        .get(&rawcode)
        .unwrap_or_else(|| panic!("building {rawcode:#010x} missing retained 9.27 income"))
}

fn extracted_building_income_semantics_927(rawcode: u32) -> (u16, bool, Option<u32>) {
    let row = extracted_content_927()
        .income_semantics
        .get(&rawcode)
        .unwrap_or_else(|| {
            panic!("building {rawcode:#010x} missing from retained 9.27 income semantics")
        });
    (row.factor_per_1000, row.is_siege, row.precursor)
}

fn parse_optional_u16_927(value: &str) -> Result<Option<u16>, String> {
    let value = value.trim();
    if value.is_empty() || value == "-" || value == "_" {
        return Ok(None);
    }
    value
        .parse::<u16>()
        .map(Some)
        .map_err(|_| format!("invalid unsigned integer {value:?}"))
}

fn parse_optional_i32_927(value: &str) -> Result<Option<i32>, String> {
    let value = value.trim();
    if value.is_empty()
        || value == "-"
        || value == "_"
        || value == "2147483647"
        || value == "2147483648"
    {
        return Ok(None);
    }
    value
        .parse::<i32>()
        .map(Some)
        .map_err(|_| format!("invalid integer {value:?}"))
}

fn parse_optional_decimal_scaled_i32_927(value: &str, scale: u32) -> Result<Option<i32>, String> {
    let value = value.trim();
    if value.is_empty() || value == "-" || value == "_" {
        return Ok(None);
    }
    let parsed = value
        .parse::<f64>()
        .map_err(|_| format!("invalid decimal {value:?}"))?;
    let scaled = parsed * f64::from(scale);
    if !scaled.is_finite() || scaled < f64::from(i32::MIN) || scaled > f64::from(i32::MAX) {
        return Err(format!("scaled decimal is outside i32 range: {value:?}"));
    }
    Ok(Some(scaled.round() as i32))
}

fn parse_optional_decimal_rounded_i32_927(value: &str) -> Result<Option<i32>, String> {
    let value = value.trim();
    if value.is_empty() || value == "-" || value == "_" {
        return Ok(None);
    }
    parse_decimal_rounded_i32_927(value).map(Some)
}

fn parse_decimal_rounded_i32_927(value: &str) -> Result<i32, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| format!("invalid decimal {value:?}"))?;
    if !parsed.is_finite() || parsed < f64::from(i32::MIN) || parsed > f64::from(i32::MAX) {
        return Err(format!("decimal is outside i32 range: {value:?}"));
    }
    Ok(parsed.round() as i32)
}

fn parse_optional_seconds_to_ticks_u16_927(value: &str) -> Result<Option<u16>, String> {
    let value = value.trim();
    if value.is_empty() || value == "-" || value == "_" {
        return Ok(None);
    }
    parse_seconds_to_ticks_u16_927(value).map(Some)
}

fn parse_seconds_to_ticks_u16_927(value: &str) -> Result<u16, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| format!("invalid duration {value:?}"))?;
    let ticks = (parsed * f64::from(CASTLE_FIGHT_SIMULATION_HZ)).round();
    if !ticks.is_finite() || ticks < 0.0 || ticks > f64::from(u16::MAX) {
        return Err(format!("duration is outside u16 tick range: {value:?}"));
    }
    Ok(ticks as u16)
}

fn parse_seconds_to_ticks_u32_927(value: &str) -> Result<u32, String> {
    let parsed = value
        .parse::<f64>()
        .map_err(|_| format!("invalid duration {value:?}"))?;
    let ticks = (parsed * f64::from(CASTLE_FIGHT_SIMULATION_HZ)).round();
    if !ticks.is_finite() || ticks < 0.0 || ticks > f64::from(u32::MAX) {
        return Err(format!("duration is outside u32 tick range: {value:?}"));
    }
    Ok(ticks as u32)
}

fn parse_armor_type_927(value: &str) -> Result<ArmorType, String> {
    match value {
        "small" => Ok(ArmorType::Small),
        "medium" => Ok(ArmorType::Medium),
        "large" => Ok(ArmorType::Large),
        "fort" | "fortified" => Ok(ArmorType::Fortified),
        "normal" => Ok(ArmorType::Normal),
        "hero" => Ok(ArmorType::Hero),
        "divine" => Ok(ArmorType::Divine),
        "none" | "unarmored" => Ok(ArmorType::Unarmored),
        other => Err(format!("unsupported armor type {other:?}")),
    }
}

fn parse_weapon_kind_927(value: &str) -> Result<ExtractedWeaponKind927, String> {
    match value {
        "normal" => Ok(ExtractedWeaponKind927::Melee),
        "missile" => Ok(ExtractedWeaponKind927::Missile),
        "artillery" => Ok(ExtractedWeaponKind927::Artillery),
        "msplash" => Ok(ExtractedWeaponKind927::Splash),
        "instant" => Ok(ExtractedWeaponKind927::Instant),
        "mbounce" => Ok(ExtractedWeaponKind927::Bounce),
        "mline" | "aline" => Ok(ExtractedWeaponKind927::Line),
        other => Err(format!("unknown extracted weapon type {other:?}")),
    }
}

fn parse_damage_type_927(value: &str) -> Result<DamageType, String> {
    match value {
        "normal" => Ok(DamageType::Normal),
        "pierce" => Ok(DamageType::Pierce),
        "siege" => Ok(DamageType::Siege),
        "magic" => Ok(DamageType::Magic),
        "chaos" => Ok(DamageType::Chaos),
        "spells" => Ok(DamageType::Spells),
        "hero" => Ok(DamageType::Hero),
        other => Err(format!("unsupported damage type {other:?}")),
    }
}

fn parse_attack_targets_927(value: &str) -> AttackTargetMask {
    let mut ground = false;
    let mut air = false;
    let mut buildings = false;
    for target in value.split(',') {
        match target {
            "ground" => ground = true,
            "air" => air = true,
            "structure" => buildings = true,
            _ => {}
        }
    }
    if !ground && !air && !buildings && !value.trim().is_empty() && value != "_" {
        AttackTargetMask::ALL
    } else {
        AttackTargetMask::from_capabilities(ground, air, buildings)
    }
}

fn parse_decimal_per_1000(value: &str) -> u16 {
    let (whole, fraction) = value.split_once('.').unwrap_or((value, ""));
    let whole = whole
        .parse::<u16>()
        .expect("income factor whole part must be numeric");
    let mut fractional = 0u16;
    let mut place = 100u16;
    for byte in fraction.bytes().take(3) {
        assert!(
            byte.is_ascii_digit(),
            "income factor fraction must be numeric"
        );
        fractional += u16::from(byte - b'0') * place;
        place /= 10;
    }
    whole * 1_000 + fractional
}

fn extracted_tower_definition_927(
    rawcode: u32,
    expected_name: &'static str,
) -> CastleFightTowerDefinition {
    let content = extracted_content_927();
    let building = content.buildings.get(&rawcode).unwrap_or_else(|| {
        panic!("tower {rawcode:#010x} missing from retained 9.27 building table")
    });
    assert_eq!(
        building.name, expected_name,
        "tower name changed in extraction"
    );
    let unit = content
        .units
        .get(&rawcode)
        .unwrap_or_else(|| panic!("tower {rawcode:#010x} missing retained 9.27 unit metadata"));
    let protected = content
        .protected_stats
        .get(&rawcode)
        .unwrap_or_else(|| panic!("tower {rawcode:#010x} missing retained 9.27 protected stats"));
    let damage_type = unit
        .attack1_damage_type
        .unwrap_or_else(|| panic!("tower {rawcode:#010x} missing attack damage type"));
    let targets = unit
        .attack1_targets
        .unwrap_or_else(|| panic!("tower {rawcode:#010x} missing attack target mask"));
    let weapon_kind = unit
        .attack1_weapon_kind
        .unwrap_or_else(|| panic!("tower {rawcode:#010x} missing attack weapon type"));
    let delivery = match weapon_kind {
        ExtractedWeaponKind927::Melee => AttackDelivery::Melee,
        ExtractedWeaponKind927::Missile => AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: unit
                .projectile_speed_per_tick
                .unwrap_or_else(|| panic!("tower {rawcode:#010x} is missing projectile speed")),
        },
        ExtractedWeaponKind927::Artillery | ExtractedWeaponKind927::Splash => {
            AttackDelivery::RangedBallistic {
                speed_per_tick: unit
                    .projectile_speed_per_tick
                    .unwrap_or_else(|| panic!("tower {rawcode:#010x} is missing projectile speed")),
                impact_radius: unit
                    .outer_splash_radius
                    .unwrap_or_else(|| panic!("tower {rawcode:#010x} is missing splash radius")),
            }
        }
        ExtractedWeaponKind927::Instant
        | ExtractedWeaponKind927::Bounce
        | ExtractedWeaponKind927::Line => panic!(
            "tower {rawcode:#010x} uses an extracted weapon primitive that is not implemented in the current native slice"
        ),
    };
    let attack = AttackProfile {
        delivery,
        damage: protected
            .attack1_damage
            .unwrap_or_else(|| panic!("tower {rawcode:#010x} is missing effective attack damage")),
        range: protected
            .attack1_range
            .unwrap_or_else(|| panic!("tower {rawcode:#010x} is missing effective attack range")),
        acquisition_range: unit
            .acquisition_range
            .unwrap_or_else(|| panic!("tower {rawcode:#010x} is missing acquisition range")),
        cooldown_ticks: protected.attack1_cooldown_ticks.unwrap_or_else(|| {
            panic!("tower {rawcode:#010x} is missing effective attack cooldown")
        }),
    };
    let command_card_position = *content
        .command_card_positions
        .get(&rawcode)
        .unwrap_or_else(|| panic!("tower {rawcode:#010x} missing command-card position"));
    let hotkey = *content
        .building_hotkeys
        .get(&rawcode)
        .unwrap_or_else(|| panic!("tower {rawcode:#010x} missing build hotkey"));

    CastleFightTowerDefinition {
        rawcode,
        name: building.name,
        basic_tooltip: building.basic_tooltip,
        extended_tooltip: building.extended_tooltip,
        gold_cost: building.gold_cost,
        lumber_cost: building.lumber_cost,
        economy: extracted_building_economy_927(rawcode),
        health: protected.health,
        construction_time_ticks: building.construction_time_ticks,
        repair_time_ticks: unit
            .repair_time_ticks
            .unwrap_or_else(|| panic!("tower {rawcode:#010x} is missing retained repair time")),
        armor: building.armor,
        damage_type,
        attack_targets: targets,
        footprint_size_cells: building.footprint_size_cells.unwrap_or_else(|| {
            panic!("tower {rawcode:#010x} has no square retained 9.27 footprint")
        }),
        command_card_position,
        hotkey,
        attack,
        map_version: MapVersion::CASTLE_FIGHT_9_27,
    }
}

fn extracted_unit_definition_927(
    rawcode: u32,
    expected_name: &'static str,
    expose_corpse: bool,
) -> CastleFightUnitDefinition {
    let content = extracted_content_927();
    let unit = content
        .units
        .get(&rawcode)
        .unwrap_or_else(|| panic!("unit {rawcode:#010x} missing from retained 9.27 unit table"));
    let protected = content.protected_stats.get(&rawcode).unwrap_or_else(|| {
        panic!("unit {rawcode:#010x} missing from retained 9.27 protected stats")
    });
    let attack = content.primary_attacks.get(&rawcode).unwrap_or_else(|| {
        panic!("unit {rawcode:#010x} missing from retained 9.27 primary attacks")
    });
    assert_eq!(unit.name, expected_name, "unit name changed in extraction");
    let acquisition_range = unit
        .acquisition_range
        .unwrap_or_else(|| panic!("unit {rawcode:#010x} is missing acquisition range"));
    let delivery = match attack.weapon_kind {
        ExtractedWeaponKind927::Melee => AttackDelivery::Melee,
        ExtractedWeaponKind927::Missile => AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: unit.projectile_speed_per_tick.unwrap_or_else(|| {
                panic!("missile unit {rawcode:#010x} is missing projectile speed")
            }),
        },
        ExtractedWeaponKind927::Artillery | ExtractedWeaponKind927::Splash => {
            AttackDelivery::RangedBallistic {
                speed_per_tick: unit.projectile_speed_per_tick.unwrap_or_else(|| {
                    panic!("ballistic unit {rawcode:#010x} is missing projectile speed")
                }),
                impact_radius: unit.outer_splash_radius.unwrap_or_else(|| {
                    panic!("ballistic unit {rawcode:#010x} is missing splash radius")
                }),
            }
        }
        ExtractedWeaponKind927::Instant
        | ExtractedWeaponKind927::Bounce
        | ExtractedWeaponKind927::Line => panic!(
            "unit {rawcode:#010x} uses an extracted weapon primitive that is not implemented in the current native slice"
        ),
    };
    let corpse = if expose_corpse {
        let corpse = content.corpses.get(&rawcode).unwrap_or_else(|| {
            panic!("unit {rawcode:#010x} missing retained 9.27 corpse metadata")
        });
        assert!(
            corpse.does_decay,
            "current corpse primitive only exposes decaying corpses"
        );
        Some(CorpseProfile {
            definition: CorpseDefinitionId(rawcode),
            lifetime_ticks: corpse.lifetime_ticks,
        })
    } else {
        None
    };

    CastleFightUnitDefinition {
        rawcode,
        name: unit.name,
        health: protected.health,
        health_regen_per_second_per_10k: u32::try_from(
            unit.health_regen_per_second_per_10k.unwrap_or_else(|| {
                panic!("unit {rawcode:#010x} is missing retained health regeneration")
            }),
        )
        .unwrap_or_else(|_| {
            panic!("unit {rawcode:#010x} has negative regeneration unsupported by this native unit primitive")
        }),
        build_time_ticks: unit.build_time_ticks,
        repair_time_ticks: unit
            .repair_time_ticks
            .unwrap_or_else(|| panic!("unit {rawcode:#010x} is missing retained repair time")),
        armor: protected.armor,
        passive_effects: PassiveUnitEffects::EMPTY,
        spellcasting: None,
        damage_type: attack.damage_type,
        attack_targets: attack.targets,
        movement_class: unit
            .movement_class
            .unwrap_or_else(|| panic!("unit {rawcode:#010x} is missing retained movement class")),
        mechanical: unit.mechanical,
        collision_radius: unit.collision_radius,
        corpse,
        attack: AttackProfile {
            delivery,
            damage: attack
                .damage
                .unwrap_or_else(|| panic!("unit {rawcode:#010x} is missing effective attack damage")),
            range: attack.range,
            acquisition_range,
            cooldown_ticks: attack.cooldown_ticks,
        },
        movement: MovementProfile {
            speed_per_tick: protected.move_speed_per_tick.unwrap_or_else(|| {
                panic!("unit {rawcode:#010x} is missing effective movement speed")
            }),
        },
    }
}

fn production_definition(
    rawcode: u32,
    unit: CastleFightUnitKind,
) -> CastleFightProductionDefinition {
    let content = extracted_content_927();
    let building = content
        .buildings
        .get(&rawcode)
        .unwrap_or_else(|| panic!("building {rawcode:#010x} missing from retained 9.27 table"));
    let building_health = content
        .protected_stats
        .get(&rawcode)
        .map_or(building.health, |stats| stats.health);
    let production = content.production.get(&rawcode).unwrap_or_else(|| {
        panic!("building {rawcode:#010x} missing retained 9.27 production metadata")
    });
    let produced_unit = unit
        .definition_for_version(MapVersion::CASTLE_FIGHT_9_27)
        .expect("9.27 production unit content must resolve");
    // Some produced units inherit the stock unit button position; the retained map object
    // contains no explicit override for those. The WC3 training card starts at (0, 0).
    let train_command_position = content
        .command_card_positions
        .get(&produced_unit.rawcode)
        .copied()
        .unwrap_or(CommandCardPosition::new(0, 0));
    let train_hotkey = *content
        .building_hotkeys
        .get(&produced_unit.rawcode)
        .unwrap_or_else(|| {
            panic!(
                "produced unit {:#010x} missing train hotkey",
                produced_unit.rawcode
            )
        });
    let train_tooltips = content
        .units
        .get(&produced_unit.rawcode)
        .expect("produced unit must exist in retained unit table");
    assert_eq!(
        production.unit_rawcode, produced_unit.rawcode,
        "production link changed in retained 9.27 extraction"
    );
    let building_unit = content
        .units
        .get(&rawcode)
        .unwrap_or_else(|| panic!("building {rawcode:#010x} missing retained 9.27 unit metadata"));
    let command_card_position = *content
        .command_card_positions
        .get(&rawcode)
        .unwrap_or_else(|| panic!("building {rawcode:#010x} missing command-card position"));
    let hotkey = *content
        .building_hotkeys
        .get(&rawcode)
        .unwrap_or_else(|| panic!("building {rawcode:#010x} missing build hotkey"));
    let economy = extracted_building_economy_927(rawcode);
    CastleFightProductionDefinition {
        rawcode,
        name: building.name,
        basic_tooltip: building.basic_tooltip,
        extended_tooltip: building.extended_tooltip,
        gold_cost: building.gold_cost,
        lumber_cost: building.lumber_cost,
        economy,
        building_health,
        construction_time_ticks: building.construction_time_ticks,
        repair_time_ticks: building_unit.repair_time_ticks.unwrap_or_else(|| {
            panic!("production building {rawcode:#010x} is missing retained repair time")
        }),
        armor: building.armor,
        spawn_interval_ticks: production.spawn_interval_ticks,
        footprint_size_cells: building.footprint_size_cells.unwrap_or_else(|| {
            panic!("production building {rawcode:#010x} has no square retained 9.27 footprint")
        }),
        unit,
        produced_unit,
        train_command_position,
        train_hotkey,
        train_basic_tooltip: train_tooltips.basic_tooltip,
        train_extended_tooltip: train_tooltips.extended_tooltip,
        command_card_position,
        hotkey,
        map_version: MapVersion::CASTLE_FIGHT_9_27,
    }
}

const fn movement(world_units_per_second: i32) -> MovementProfile {
    MovementProfile {
        speed_per_tick: world_units_per_second * SUBUNITS_PER_WORLD_UNIT
            / CASTLE_FIGHT_SIMULATION_HZ,
    }
}

const fn projectile_speed(world_units_per_second: i32) -> i32 {
    world_units_per_second * SUBUNITS_PER_WORLD_UNIT / CASTLE_FIGHT_SIMULATION_HZ
}

const fn world(world_units: i32) -> i32 {
    world_units * SUBUNITS_PER_WORLD_UNIT
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_extracted_char(value: &str) -> char {
        let value = value.trim_matches('"');
        let mut chars = value.chars();
        let value = chars.next().expect("extracted hotkey must not be empty");
        assert!(
            chars.next().is_none(),
            "extracted hotkey must be one character"
        );
        value
    }

    fn parse_extracted_u8(value: &str) -> u8 {
        // `object-fields.tsv` is written with CSV-style quote escaping even though it is
        // tab-delimited, so a JSON string like `"1"` appears in the raw file as `"""1"""`.
        // Coordinates are scalar integers, so normalize the TSV quoting before parsing.
        value
            .trim_matches('"')
            .parse::<u8>()
            .expect("extracted button coordinate must be numeric")
    }

    #[test]
    fn content_availability_distinguishes_archive_from_runtime_support() {
        assert_eq!(
            castle_fight_content_availability(MapVersion::CASTLE_FIGHT_9_27),
            CastleFightContentAvailability::SupportedDevelopmentSubset
        );
        assert_eq!(
            castle_fight_content_availability(MapVersion::CASTLE_FIGHT_9_32),
            CastleFightContentAvailability::Archived
        );
        assert_eq!(
            castle_fight_content_availability(MapVersion::new(9, 31)),
            CastleFightContentAvailability::Unavailable
        );
        assert!(matches!(
            castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_32),
            Err(CastleFightContentError::ArchivedOnly(
                MapVersion::CASTLE_FIGHT_9_32
            ))
        ));
        assert!(matches!(
            castle_fight_content_bundle_for_revision(
                MapVersion::CASTLE_FIGHT_9_27,
                "cf-native-dev-slice-r1"
            ),
            Err(CastleFightContentError::UnsupportedContentRevision {
                map_version: MapVersion::CASTLE_FIGHT_9_27,
                content_revision: "cf-native-dev-slice-r1"
            })
        ));
    }

    #[test]
    fn retained_catalog_hash_normalizes_checkout_line_endings() {
        let mut lf = ContentHash64::new();
        write_canonical_catalog_text(&mut lf, "a\nb\n");
        let mut crlf = ContentHash64::new();
        write_canonical_catalog_text(&mut crlf, "a\r\nb\r\n");
        assert_eq!(lf.finish(), crlf.finish());
    }

    #[test]
    fn current_content_bundle_freezes_the_development_slice() {
        let bundle = castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
        assert_eq!(bundle.revision, CASTLE_FIGHT_CONTENT_REVISION_927);
        assert_eq!(
            bundle.availability,
            CastleFightContentAvailability::SupportedDevelopmentSubset
        );
        assert_eq!(
            bundle.identity.schema_version,
            CASTLE_FIGHT_CONTENT_BUNDLE_SCHEMA_VERSION
        );
        assert_eq!(bundle.identity.gameplay_hash, 0xdff0_f42b_7c1d_cd53);
        assert_eq!(bundle.behaviors().len(), 12);
        assert!(
            bundle
                .behaviors()
                .windows(2)
                .all(|pair| pair[0].id < pair[1].id)
        );
        for kind in CastleFightUnitKind::ALL {
            assert_eq!(bundle.unit(kind), Some(kind.definition()));
        }
        for kind in CastleFightProductionKind::ALL {
            assert_eq!(bundle.production_building(kind), Some(kind.definition()));
        }
        for kind in CastleFightTowerKind::ALL {
            assert_eq!(bundle.tower(kind), Some(kind.definition()));
        }
        for race in CastleFightBuilderRace::ALL {
            assert_eq!(bundle.builder(race), Some(&race.definition()));
        }
    }

    #[test]
    fn playable_behavior_roots_come_from_extracted_ability_inventories() {
        let extracted = extracted_content_927();
        let bundle = castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
        let resolved_sources = bundle
            .behaviors()
            .iter()
            .map(|behavior| behavior.source)
            .collect::<BTreeSet<_>>();
        let playable_rawcodes = CastleFightUnitKind::ALL
            .into_iter()
            .map(|kind| bundle.unit(kind).expect("playable unit definition").rawcode)
            .chain(CastleFightProductionKind::ALL.into_iter().map(|kind| {
                bundle
                    .production_building(kind)
                    .expect("playable production definition")
                    .rawcode
            }))
            .chain(CastleFightTowerKind::ALL.into_iter().map(|kind| {
                bundle
                    .tower(kind)
                    .expect("playable tower definition")
                    .rawcode
            }));

        for rawcode in playable_rawcodes {
            for &ability in extracted
                .unit_abilities
                .get(&rawcode)
                .expect("playable object ability inventory")
            {
                assert!(
                    resolved_sources.contains(&NativeEffectSource::new(
                        NativeEffectSourceKind::UnitAbility,
                        ability,
                    )),
                    "playable object {rawcode:#010x} ability {ability:#010x} was not resolved"
                );
            }
        }
    }

    #[test]
    fn retained_extraction_indexes_content_beyond_the_playable_subset() {
        let extracted = extracted_content_927();
        let hunter_hall = u32::from_be_bytes(*b"h00S");
        assert!(extracted.buildings.contains_key(&hunter_hall));
        assert!(extracted.production.contains_key(&hunter_hall));
        assert!(
            CastleFightProductionKind::from_rawcode_for_version(
                hunter_hall,
                MapVersion::CASTLE_FIGHT_9_27
            )
            .unwrap()
            .is_none(),
            "extraction evidence must not imply executable support"
        );
        assert!(extracted.buildings.len() > CastleFightProductionKind::ALL.len());
    }

    #[test]
    fn production_and_direct_spawning_share_one_resolved_unit_definition() {
        let unit = CastleFightUnitKind::IceTrollShadowPriest
            .definition()
            .resolved();
        let building = CastleFightProductionKind::IceTrollHut.definition();
        let spawn = building.spawn(Team(0), BuildingFootprint::new(0, 0, 4, 4));
        let properties = building.gameplay_properties();

        assert_eq!(spawn.production.unwrap().unit, unit.template);
        assert_eq!(properties.production_unit, unit.properties);
        assert_eq!(properties.production_spellcasting, unit.spellcasting);
    }

    #[test]
    fn stable_content_ids_do_not_depend_on_enum_positions_or_rawcodes() {
        assert_eq!(
            CastleFightUnitKind::Footman.stable_id(),
            CastleFightUnitId(0x1000_0001)
        );
        assert_eq!(
            CastleFightUnitKind::Defender.stable_id(),
            CastleFightUnitId(0x1000_0002)
        );
        assert_eq!(
            CastleFightProductionKind::Barracks.stable_id(),
            CastleFightBuildingId(0x2000_0001)
        );
        assert_eq!(
            CastleFightTowerKind::WatchTower.stable_id(),
            CastleFightBuildingId(0x2100_0001)
        );
        assert_eq!(
            CastleFightBuilderRace::Human.stable_id(),
            CastleFightBuilderId(0x3000_0007)
        );
        assert_ne!(
            CastleFightUnitKind::Footman.stable_id().0,
            u32::from_be_bytes(*b"hfoo")
        );
    }

    #[test]
    fn economy_rules_match_extracted_927_defaults() {
        assert_eq!(
            castle_fight_economy_rules(),
            EconomyRules {
                starting_gold: 250,
                starting_lumber: 125,
                starting_legendary_points: 1,
                base_income_per_10k: 5 * RESOURCE_FIXED_SCALE,
                income_interval_ticks: 300,
                income_tax_bracket_per_10k: 25 * RESOURCE_FIXED_SCALE,
            }
        );
    }

    #[test]
    fn exposed_buildings_use_extracted_cost_lumber_and_income_rules() {
        let cases = [
            (
                CastleFightProductionKind::Barracks.definition().economy,
                BuildingEconomyProfile {
                    gold_cost: 100,
                    lumber_cost: 0,
                    lumber_refund: 100,
                    income_per_10k: 20_000,
                },
            ),
            (
                CastleFightProductionKind::RangersHall.definition().economy,
                BuildingEconomyProfile {
                    gold_cost: 200,
                    lumber_cost: 0,
                    lumber_refund: 200,
                    // Ranger's Hall inherits the 3 gold income from its 150g Hunter's Hall
                    // precursor and adds another 4 from its own 200g upgrade cost.
                    income_per_10k: 70_000,
                },
            ),
            (
                CastleFightProductionKind::OrcishSiegeFactory
                    .definition()
                    .economy,
                BuildingEconomyProfile {
                    gold_cost: 380,
                    lumber_cost: 0,
                    lumber_refund: 285,
                    income_per_10k: 68_400,
                },
            ),
            (
                CastleFightProductionKind::IceTrollHut.definition().economy,
                BuildingEconomyProfile {
                    gold_cost: 175,
                    lumber_cost: 0,
                    lumber_refund: 175,
                    income_per_10k: 35_000,
                },
            ),
            (
                CastleFightProductionKind::GryphonRock.definition().economy,
                BuildingEconomyProfile {
                    gold_cost: 250,
                    lumber_cost: 0,
                    lumber_refund: 250,
                    income_per_10k: 50_000,
                },
            ),
            (
                CastleFightTowerKind::WatchTower.definition().economy,
                BuildingEconomyProfile {
                    gold_cost: 150,
                    lumber_cost: 300,
                    lumber_refund: 0,
                    income_per_10k: 6_000,
                },
            ),
            (
                CastleFightTowerKind::PoofTower.definition().economy,
                BuildingEconomyProfile {
                    gold_cost: 230,
                    lumber_cost: 300,
                    lumber_refund: 0,
                    income_per_10k: 9_200,
                },
            ),
        ];
        for (actual, expected) in cases {
            assert_eq!(actual, expected);
        }
    }

    #[test]
    fn exposed_buildings_use_extracted_927_tooltips() {
        for kind in CastleFightProductionKind::ALL {
            let definition = kind.definition();
            assert_eq!(
                (definition.basic_tooltip, definition.extended_tooltip),
                extracted_building_tooltips_927(definition.rawcode),
            );
        }
        for kind in CastleFightTowerKind::ALL {
            let definition = kind.definition();
            assert_eq!(
                (definition.basic_tooltip, definition.extended_tooltip),
                extracted_building_tooltips_927(definition.rawcode),
            );
        }
    }

    #[test]
    fn exposed_buildings_use_extracted_927_construction_times() {
        for kind in CastleFightProductionKind::ALL {
            assert_eq!(
                kind.definition().construction_time_ticks,
                2 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                "current production building {kind:?} must preserve extracted 2-second build time"
            );
        }
        assert_eq!(
            CastleFightTowerKind::WatchTower
                .definition()
                .construction_time_ticks,
            20 * CASTLE_FIGHT_SIMULATION_HZ as u32
        );
        assert_eq!(
            CastleFightTowerKind::PoofTower
                .definition()
                .construction_time_ticks,
            20 * CASTLE_FIGHT_SIMULATION_HZ as u32
        );
    }

    #[test]
    fn builder_profile_uses_extracted_927_movement_and_repair_timing() {
        assert_eq!(
            castle_fight_builder_profile(),
            BuilderProfile {
                speed_per_tick: 550 * SUBUNITS_PER_WORLD_UNIT / CASTLE_FIGHT_SIMULATION_HZ,
                build_range: 50 * SUBUNITS_PER_WORLD_UNIT,
                repair_range: 50 * SUBUNITS_PER_WORLD_UNIT,
                repair_autocast_range: 500 * SUBUNITS_PER_WORLD_UNIT,
                repair_time_ratio_numerator: 3,
                repair_time_ratio_denominator: 2,
                full_repair_duration_ticks: 3_150,
                blink_range: 10_000 * SUBUNITS_PER_WORLD_UNIT,
                blink_boundary_inset: 64 * SUBUNITS_PER_WORLD_UNIT,
            }
        );
    }

    #[test]
    fn builder_race_definitions_match_extracted_927_units_and_catalogs() {
        let units = include_str!("../../../docs/original_map/extracted/resolved/units.tsv");
        let mut unit_lines = units.lines();
        let unit_header = unit_lines.next().expect("units.tsv header missing");
        let unit_columns = unit_header.split('\t').collect::<Vec<_>>();
        let rawcode_column = unit_columns
            .iter()
            .position(|column| *column == "rawcode")
            .unwrap();
        let name_column = unit_columns
            .iter()
            .position(|column| *column == "name")
            .unwrap();
        let move_type_column = unit_columns
            .iter()
            .position(|column| *column == "move_type")
            .unwrap();
        let move_speed_column = unit_columns
            .iter()
            .position(|column| *column == "move_speed")
            .unwrap();
        let unit_rows = unit_lines
            .map(|line| line.split('\t').collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let expected_catalog_sizes = [10, 10, 6, 10, 11, 10, 12, 10, 10, 10, 10, 11, 10, 10, 10];

        for (race, expected_catalog_size) in CastleFightBuilderRace::ALL
            .into_iter()
            .zip(expected_catalog_sizes)
        {
            let definition = race.definition();
            let source = unit_rows
                .iter()
                .find(|row| parse_rawcode(row[rawcode_column]) == definition.rawcode)
                .expect("builder rawcode must exist in resolved units");
            assert_eq!(source[name_column], definition.name);
            assert_eq!(
                source[move_type_column],
                match definition.locomotion {
                    BuilderLocomotion::Foot => "foot",
                    BuilderLocomotion::Hover => "hover",
                }
            );
            let source_speed = source[move_speed_column].parse::<i32>().unwrap();
            assert_eq!(
                definition.profile.speed_per_tick,
                source_speed * SUBUNITS_PER_WORLD_UNIT / CASTLE_FIGHT_SIMULATION_HZ
            );
            assert_eq!(definition.build_catalog.len(), expected_catalog_size);
        }
        assert!(!CastleFightBuilderRace::STANDARD.contains(&CastleFightBuilderRace::Critter));
        assert!(CastleFightBuilderRace::Critter.definition().campaign_only);
        for race in CastleFightBuilderRace::STANDARD {
            assert!(
                race.definition().repair_autocast_enabled_by_default,
                "standard 9.27 builder {race:?} must preserve extracted default-active Repair"
            );
        }
        assert!(
            !CastleFightBuilderRace::Critter
                .definition()
                .repair_autocast_enabled_by_default,
            "campaign Critter Builder has Repair but no extracted Default Active Ability"
        );
    }

    #[test]
    fn production_upgrade_links_are_versioned_and_match_extracted_927_edges() {
        assert_eq!(
            CastleFightProductionKind::Barracks.upgrade_targets(),
            vec![CastleFightProductionKind::Stronghold]
        );
        assert_eq!(
            CastleFightProductionKind::Stronghold.upgrade_from(),
            Some(CastleFightProductionKind::Barracks)
        );
        assert!(
            CastleFightProductionKind::Stronghold
                .upgrade_targets()
                .is_empty()
        );

        // Ranger's Hall is itself an upgrade in 9.27, but its Hunter's Hall precursor is not
        // implemented in the current native slice yet. The raw extracted relation remains
        // authoritative even though the typed lookup cannot expose an unimplemented kind.
        let ranger = CastleFightProductionKind::RangersHall.definition();
        let (ranger_precursor, ranger_targets) =
            extracted_building_upgrade_links_927(ranger.rawcode);
        assert_eq!(ranger_precursor, Some(u32::from_be_bytes(*b"h00S")));
        assert_eq!(ranger_targets, vec![u32::from_be_bytes(*b"h03E")]);
        assert_eq!(CastleFightProductionKind::RangersHall.upgrade_from(), None);
        assert!(
            CastleFightProductionKind::RangersHall
                .upgrade_targets()
                .is_empty()
        );

        let human = CastleFightBuilderRace::Human.definition();
        assert!(human.build_catalog.contains(&u32::from_be_bytes(*b"h000")));
        assert!(!human.build_catalog.contains(&u32::from_be_bytes(*b"h039")));
    }

    #[test]
    fn draft_builder_configuration_changes_menu_without_changing_appearance() {
        let human = CastleFightBuilderRace::Human.definition();
        let drafted = human.configuration_with_catalog(vec![
            u32::from_be_bytes(*b"h000"),
            u32::from_be_bytes(*b"h02I"),
        ]);
        assert_eq!(drafted.appearance.rawcode, human.rawcode);
        assert_eq!(drafted.locomotion, human.locomotion);
        assert_eq!(drafted.build_catalog.len(), 2);
        assert!(drafted.allows_building(u32::from_be_bytes(*b"h02I")));
        assert!(!drafted.allows_building(u32::from_be_bytes(*b"h006")));
    }

    #[test]
    fn imported_roster_has_extracted_damage_and_armor_classes() {
        let cases = [
            (
                CastleFightUnitKind::Footman,
                DamageType::Normal,
                ArmorProfile::new(ArmorType::Large, 4),
            ),
            (
                CastleFightUnitKind::Defender,
                DamageType::Normal,
                ArmorProfile::new(ArmorType::Large, 7),
            ),
            (
                CastleFightUnitKind::Ranger,
                DamageType::Pierce,
                ArmorProfile::new(ArmorType::Small, 3),
            ),
            (
                CastleFightUnitKind::Catapult,
                DamageType::Siege,
                ArmorProfile::new(ArmorType::Medium, 5),
            ),
            (
                CastleFightUnitKind::IceTrollShadowPriest,
                DamageType::Magic,
                ArmorProfile::new(ArmorType::Small, 1),
            ),
            (
                CastleFightUnitKind::GryphonRider,
                DamageType::Magic,
                ArmorProfile::new(ArmorType::Medium, 2),
            ),
        ];
        for (kind, damage_type, armor) in cases {
            let definition = kind.definition();
            assert_eq!(definition.damage_type, damage_type);
            assert_eq!(definition.armor, armor);
        }
    }

    #[test]
    fn imported_roster_keeps_extracted_build_times_and_mechanical_classification() {
        let units = include_str!("../../../docs/original_map/extracted/resolved/units.tsv");
        let mut lines = units.lines();
        let columns = lines
            .next()
            .expect("units.tsv header missing")
            .split('\t')
            .collect::<Vec<_>>();
        let rawcode_column = columns
            .iter()
            .position(|column| *column == "rawcode")
            .unwrap();
        let build_time_column = columns
            .iter()
            .position(|column| *column == "build_time")
            .unwrap();
        let classifications_column = columns
            .iter()
            .position(|column| *column == "classifications")
            .unwrap();
        let rows = lines
            .map(|line| line.split('\t').collect::<Vec<_>>())
            .collect::<Vec<_>>();

        for kind in CastleFightUnitKind::ALL {
            let definition = kind.definition();
            let row = rows
                .iter()
                .find(|row| parse_rawcode(row[rawcode_column]) == definition.rawcode)
                .expect("imported unit rawcode must exist in resolved units");
            let build_time_seconds = row[build_time_column]
                .parse::<u32>()
                .expect("imported unit build time must be integral seconds");
            assert_eq!(
                definition.build_time_ticks,
                build_time_seconds * CASTLE_FIGHT_SIMULATION_HZ as u32
            );
            let extracted_mechanical = row[classifications_column]
                .split(',')
                .any(|classification| classification == "mechanical");
            assert_eq!(definition.mechanical, extracted_mechanical);
        }
    }

    #[test]
    fn native_repair_times_match_extracted_urtm_values() {
        let object_fields =
            include_str!("../../../docs/original_map/extracted/resolved/object-fields.tsv");
        let mut lines = object_fields.lines();
        let columns = lines
            .next()
            .expect("object-fields.tsv header missing")
            .split('\t')
            .collect::<Vec<_>>();
        let category_column = columns
            .iter()
            .position(|column| *column == "category")
            .unwrap();
        let rawcode_column = columns
            .iter()
            .position(|column| *column == "rawcode")
            .unwrap();
        let field_id_column = columns
            .iter()
            .position(|column| *column == "field_id")
            .unwrap();
        let recovered_column = columns
            .iter()
            .position(|column| *column == "recovered_value_json")
            .unwrap();
        let rows = lines
            .map(|line| line.split('\t').collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let repair_ticks = |rawcode: u32| {
            let rawcode = rawcode.to_be_bytes();
            let rawcode = std::str::from_utf8(&rawcode).expect("rawcode must be ASCII");
            let row = rows
                .iter()
                .find(|row| {
                    row[category_column] == "units"
                        && row[rawcode_column] == rawcode
                        && row[field_id_column] == "urtm"
                })
                .unwrap_or_else(|| panic!("missing extracted urtm for {rawcode}"));
            let seconds = serde_json::from_str::<u32>(row[recovered_column])
                .expect("urtm recovered value must be integral seconds");
            seconds * CASTLE_FIGHT_SIMULATION_HZ as u32
        };

        for kind in CastleFightUnitKind::ALL {
            let definition = kind.definition();
            assert_eq!(
                definition.repair_time_ticks,
                repair_ticks(definition.rawcode)
            );
        }
        for kind in CastleFightProductionKind::ALL {
            let definition = kind.definition();
            assert_eq!(
                definition.repair_time_ticks,
                repair_ticks(definition.rawcode)
            );
        }
        for kind in CastleFightTowerKind::ALL {
            let definition = kind.definition();
            assert_eq!(
                definition.repair_time_ticks,
                repair_ticks(definition.rawcode)
            );
        }
        assert_eq!(
            castle_fight_main_castle_repair_time_ticks(),
            repair_ticks(u32::from_be_bytes(*b"hcas"))
        );
    }

    #[test]
    fn native_build_command_slots_match_extracted_unit_button_positions() {
        let object_fields =
            include_str!("../../../docs/original_map/extracted/resolved/object-fields.tsv");
        let mut lines = object_fields.lines();
        let columns = lines
            .next()
            .expect("object-fields.tsv header missing")
            .split('\t')
            .collect::<Vec<_>>();
        let category_column = columns
            .iter()
            .position(|column| *column == "category")
            .unwrap();
        let rawcode_column = columns
            .iter()
            .position(|column| *column == "rawcode")
            .unwrap();
        let field_id_column = columns
            .iter()
            .position(|column| *column == "field_id")
            .unwrap();
        let recovered_column = columns
            .iter()
            .position(|column| *column == "recovered_value_json")
            .unwrap();
        let rows = lines
            .map(|line| line.split('\t').collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let coordinate = |rawcode: u32, field: &str| {
            let rawcode = rawcode.to_be_bytes();
            let rawcode = std::str::from_utf8(&rawcode).expect("rawcode must be ASCII");
            let row = rows
                .iter()
                .find(|row| {
                    row[category_column] == "units"
                        && row[rawcode_column] == rawcode
                        && row[field_id_column] == field
                })
                .unwrap_or_else(|| panic!("missing extracted {field} for {rawcode}"));
            parse_extracted_u8(row[recovered_column])
        };
        let extracted_position = |rawcode: u32| {
            CommandCardPosition::new(coordinate(rawcode, "ubpx"), coordinate(rawcode, "ubpy"))
        };
        let extracted_hotkey = |rawcode: u32| {
            let rawcode = rawcode.to_be_bytes();
            let rawcode = std::str::from_utf8(&rawcode).expect("rawcode must be ASCII");
            let row = rows
                .iter()
                .find(|row| {
                    row[category_column] == "units"
                        && row[rawcode_column] == rawcode
                        && row[field_id_column] == "uhot"
                })
                .unwrap_or_else(|| panic!("missing extracted uhot for {rawcode}"));
            parse_extracted_char(row[recovered_column])
        };

        for kind in CastleFightProductionKind::ALL {
            let definition = kind.definition();
            assert_eq!(
                definition.command_card_position,
                extracted_position(definition.rawcode)
            );
            assert_eq!(definition.hotkey, extracted_hotkey(definition.rawcode));
        }
        for kind in CastleFightTowerKind::ALL {
            let definition = kind.definition();
            assert_eq!(
                definition.command_card_position,
                extracted_position(definition.rawcode)
            );
            assert_eq!(definition.hotkey, extracted_hotkey(definition.rawcode));
        }
    }

    #[test]
    fn command_card_layout_is_versioned_and_matches_extracted_builder_abilities() {
        let layout = castle_fight_command_card_layout_for_version(MapVersion::CASTLE_FIGHT_9_27)
            .expect("9.27 command card must be supported");
        assert_eq!(layout.map_version, MapVersion::CASTLE_FIGHT_9_27);
        assert_eq!(layout.build_hotkey, 'B');
        assert!(castle_fight_command_card_layout_for_version(MapVersion::new(9, 28)).is_err());
        assert!(castle_fight_damage_rules_for_version(MapVersion::new(9, 28)).is_err());
        assert!(
            castle_fight_main_castle_repair_time_ticks_for_version(MapVersion::new(9, 28)).is_err()
        );
        assert!(
            CastleFightTowerKind::WatchTower
                .definition_for_version(MapVersion::new(9, 28))
                .is_err()
        );

        let object_fields =
            include_str!("../../../docs/original_map/extracted/resolved/object-fields.tsv");
        let mut lines = object_fields.lines();
        let columns = lines
            .next()
            .expect("object-fields.tsv header missing")
            .split('\t')
            .collect::<Vec<_>>();
        let category_column = columns
            .iter()
            .position(|column| *column == "category")
            .unwrap();
        let rawcode_column = columns
            .iter()
            .position(|column| *column == "rawcode")
            .unwrap();
        let field_id_column = columns
            .iter()
            .position(|column| *column == "field_id")
            .unwrap();
        let recovered_column = columns
            .iter()
            .position(|column| *column == "recovered_value_json")
            .unwrap();
        let rows = lines
            .map(|line| line.split('\t').collect::<Vec<_>>())
            .collect::<Vec<_>>();
        let ability_position = |rawcode: &str| {
            let coordinate = |field: &str| {
                let row = rows
                    .iter()
                    .find(|row| {
                        row[category_column] == "abilities"
                            && row[rawcode_column] == rawcode
                            && row[field_id_column] == field
                    })
                    .unwrap_or_else(|| panic!("missing extracted {field} for {rawcode}"));
                parse_extracted_u8(row[recovered_column])
            };
            CommandCardPosition::new(coordinate("abpx"), coordinate("abpy"))
        };

        let ability_hotkey = |rawcode: &str| {
            let row = rows
                .iter()
                .find(|row| {
                    row[category_column] == "abilities"
                        && row[rawcode_column] == rawcode
                        && row[field_id_column] == "ahky"
                })
                .unwrap_or_else(|| panic!("missing extracted ahky for {rawcode}"));
            parse_extracted_char(row[recovered_column])
        };

        assert_eq!(layout.repair_ability, ability_position("Ahrp"));
        assert_eq!(layout.blink_ability, ability_position("A0-1"));
        assert_eq!(layout.blink_hotkey, ability_hotkey("A0-1"));
    }

    #[test]
    fn imported_towers_are_fortified_and_keep_extracted_attack_types() {
        let watch = CastleFightTowerKind::WatchTower.definition();
        assert_eq!(watch.damage_type, DamageType::Pierce);
        assert_eq!(watch.armor, ArmorProfile::new(ArmorType::Fortified, 5));
        let poof = CastleFightTowerKind::PoofTower.definition();
        assert_eq!(poof.damage_type, DamageType::Magic);
        assert_eq!(poof.armor, ArmorProfile::new(ArmorType::Fortified, 5));
    }
}
