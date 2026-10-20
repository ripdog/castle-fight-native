use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::OnceLock,
};

use serde::Deserialize;

use crate::{
    components::{
        AbilityEffect, AbilityId, AbilityTargetPolicy, ActionTimingProfile,
        AdditionalAutomaticAbilityDefinitions, AreaDamageOrigin, AttackDelivery, AttackProfile,
        AttackTargetMask, AuraEffectProfile, AutomaticAbilityProfile, BuilderConfiguration,
        BuilderLocomotion, BuilderProfile, BuilderSpawn, BuildingFootprint,
        BuildingGameplayProperties, BuildingSpawn, BurningOilEffectProfile, CleaveEffectProfile,
        CollisionRadius, ContentIdentity, CorpseDefinitionId, CorpseProfile,
        GameplayBundleIdentity, ManaProfile, ModifierId, MovementClass, MovementProfile,
        PassiveUnitEffect, PassiveUnitEffects, ProductionProfile, ResolvedUnitDefinition,
        SecondaryAttackProfile, SpellResistanceEffectProfile, SpellcastingProfile,
        SplashFalloffProfile, Team, TriggeredAttackEffect, UnitClassifications,
        UnitGameplayProperties, UnitTemplate, compose_spellcasting_profiles,
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

mod roster;
pub use roster::{CastleFightProductionKind, CastleFightTowerKind, CastleFightUnitKind};

pub const CASTLE_FIGHT_SIMULATION_HZ: i32 = 30;
pub const CASTLE_FIGHT_DEFAULT_MAP_VERSION: MapVersion = MapVersion::CASTLE_FIGHT_9_27;
pub const CASTLE_FIGHT_CONTENT_REVISION_927: &str = "cf-native-dev-slice-r200";
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

pub const CASTLE_FIGHT_CONTENT_BUNDLE_SCHEMA_VERSION: u32 = 88;

// Version-scoped selection gate; remaining fidelity caveats live in docs/verification.
const ELVEN_RACE_PROMOTED_927: bool = true;

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
    pub main_castle_classifications: UnitClassifications,
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

    /// The basic and extended object-data tooltip pair for one authored unit rawcode.
    #[must_use]
    pub fn unit_tooltips_for_rawcode(&self, rawcode: u32) -> Option<(&'static str, &'static str)> {
        (self.map_version == MapVersion::CASTLE_FIGHT_9_27)
            .then(|| extracted_content_927().units.get(&rawcode))
            .flatten()
            .map(|unit| (unit.basic_tooltip, unit.extended_tooltip))
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
                map_version: self.map_version,
                rawcode,
                name: definition.name,
            })
            .or_else(|| {
                self.production_buildings
                    .values()
                    .find(|definition| definition.rawcode == rawcode)
                    .map(|definition| ContentIdentity {
                        map_version: self.map_version,
                        rawcode,
                        name: definition.name,
                    })
            })
            .or_else(|| {
                self.towers
                    .values()
                    .find(|definition| definition.rawcode == rawcode)
                    .map(|definition| ContentIdentity {
                        map_version: self.map_version,
                        rawcode,
                        name: definition.name,
                    })
            })
            .or_else(|| {
                self.builders
                    .values()
                    .find(|definition| definition.rawcode == rawcode)
                    .map(|definition| ContentIdentity {
                        map_version: self.map_version,
                        rawcode,
                        name: definition.name,
                    })
            })
            .or_else(|| {
                if self.map_version == MapVersion::CASTLE_FIGHT_9_27
                    && extracted_content_927()
                        .native_noncombat_summons
                        .contains(&rawcode)
                {
                    return Some(ContentIdentity {
                        map_version: self.map_version,
                        rawcode,
                        name: extracted_content_927().units[&rawcode].name,
                    });
                }
                (rawcode == u32::from_be_bytes(*b"hcas")).then_some(ContentIdentity {
                    map_version: self.map_version,
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

    /// Promoted direct placements intersected with the builder's extracted command list.
    /// Coverage of this menu alone does not establish full race playability.
    #[must_use]
    pub fn direct_building_kinds_for_race(
        &self,
        race: CastleFightBuilderRace,
    ) -> Vec<CastleFightBuildingKind> {
        let Some(builder) = self.builder(race) else {
            return Vec::new();
        };
        let direct = self.direct_building_kinds();
        builder
            .build_catalog
            .iter()
            .filter_map(|rawcode| self.building_kind_for_rawcode(*rawcode))
            .filter(|kind| direct.contains(kind))
            .collect()
    }

    /// Race availability is version-scoped and independent of object-data/menu coverage.
    #[must_use]
    pub fn supported_builder_races(&self) -> &'static [CastleFightBuilderRace] {
        match self.map_version {
            MapVersion::CASTLE_FIGHT_9_27 => {
                if ELVEN_RACE_PROMOTED_927 {
                    &[
                        CastleFightBuilderRace::Human,
                        CastleFightBuilderRace::Elf,
                        CastleFightBuilderRace::Northern,
                        CastleFightBuilderRace::Orc,
                        CastleFightBuilderRace::Elemental,
                        CastleFightBuilderRace::NightElf,
                        CastleFightBuilderRace::Naga,
                        CastleFightBuilderRace::Critter,
                        CastleFightBuilderRace::Chaos,
                        CastleFightBuilderRace::Corrupted,
                        CastleFightBuilderRace::Desert,
                        CastleFightBuilderRace::Pandaren,
                        CastleFightBuilderRace::Mechanical,
                    ]
                } else {
                    &[CastleFightBuilderRace::Human]
                }
            }
            _ => &[],
        }
    }

    #[must_use]
    pub fn supports_builder_race(&self, race: CastleFightBuilderRace) -> bool {
        self.supported_builder_races().contains(&race)
    }

    /// Resolve selection identifiers from the retained builder object (e.g. X00P),
    /// not from independently maintained race indices or display labels.
    #[must_use]
    pub fn builder_race_for_rawcode(&self, rawcode: u32) -> Option<CastleFightBuilderRace> {
        self.builder_definitions()
            .find(|builder| builder.rawcode == rawcode)
            .map(|builder| builder.race)
    }

    #[must_use]
    pub fn playable_human_direct_building_kinds(&self) -> Vec<CastleFightBuildingKind> {
        self.direct_building_kinds_for_race(CastleFightBuilderRace::Human)
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

    pub fn upgrade_targets_for_version(
        self,
        version: MapVersion,
    ) -> Result<Vec<Self>, UnsupportedCastleFightMapVersion> {
        match self {
            Self::Production(kind) => kind
                .upgrade_targets_for_version(version)
                .map(|targets| targets.into_iter().map(Self::Production).collect()),
            Self::Tower(kind) => kind
                .upgrade_targets_for_version(version)
                .map(|targets| targets.into_iter().map(Self::Tower).collect()),
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
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
    pub rescue_strike_ability: CommandCardPosition,
    pub blink_hotkey: char,
    pub rescue_strike_hotkey: char,
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
        rescue_strike_ability: CommandCardPosition::new(3, 2),
        blink_hotkey: 'D',
        rescue_strike_hotkey: 'V',
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
                map_version: self.map_version,
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

impl CastleFightUnitKind {
    pub fn from_rawcode_for_version(
        rawcode: u32,
        version: MapVersion,
    ) -> Result<Option<Self>, UnsupportedCastleFightMapVersion> {
        if version != MapVersion::CASTLE_FIGHT_9_27 {
            return Err(UnsupportedCastleFightMapVersion(version));
        }
        Ok(Self::from_retained_rawcode_9_27(rawcode))
    }

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
        let mut passive_effects = mechanics.passive_effects.iter().collect::<Vec<_>>();
        if let Some(profile) =
            crate::native_effects::lava_spawn_profile_for_version(version, definition.rawcode)
        {
            passive_effects.push(PassiveUnitEffect::LavaSpawn(profile));
        }
        let primary_splash = extracted_content_927()
            .units
            .get(&definition.rawcode)
            .and_then(|unit| unit.splash_falloff);
        // The splash passive rides on whichever weapon launches a splash projectile. A body whose
        // splash belongs to its secondary artillery must not also carry a primary splash profile.
        let secondary_splash = extracted_content_927()
            .secondary_artillery_weapons
            .get(&definition.rawcode)
            .map(CatalogSecondaryArtilleryWeapon927::splash_profile);
        assert!(
            primary_splash.is_none() || secondary_splash.is_none(),
            "unit {:#010x} has splash bands on both weapons",
            definition.rawcode
        );
        if let Some(profile) = primary_splash.or(secondary_splash) {
            passive_effects.push(PassiveUnitEffect::SplashFalloff(profile));
        }
        for effect in self.human_passive_effects_9_27() {
            if !passive_effects.contains(&effect) {
                passive_effects.push(effect);
            }
        }
        definition.passive_effects = PassiveUnitEffects::from_slice(&passive_effects);
        // A scripted ability can be shared by bodies with different mana pools; the body's
        // retained pool, not the pool of the recipe's source unit, applies.
        let mut native_primary = mechanics.spellcasting;
        if let Some((profile, mana)) = native_primary
            .as_mut()
            .zip(native_mana_profile_for_version(version, definition.rawcode))
        {
            profile.mana = mana;
        }
        let native_mana = native_primary.map(|profile| profile.mana);
        let native_additional = mechanics
            .additional_abilities
            .as_ref()
            .into_iter()
            .flat_map(|definitions| definitions.iter())
            .map(|ability| SpellcastingProfile {
                mana: native_mana
                    .expect("translated additional abilities have a primary mana profile"),
                ability,
            });
        let (spellcasting, additional_abilities) = compose_spellcasting_profiles(
            native_primary,
            self.human_spellcasting_9_27()
                .into_iter()
                .chain(native_additional),
        )
        .expect("promoted unit abilities must have distinct IDs and one shared mana pool");
        definition.spellcasting = spellcasting;
        definition.additional_abilities = additional_abilities;
        Ok(definition)
    }

    fn human_spellcasting_9_27(self) -> Option<SpellcastingProfile> {
        let (mana, ability) = match self {
            Self::Crusader => (
                ManaProfile {
                    maximum: 200,
                    starting: 150,
                    regen_per_tick_per_10k: 666,
                },
                AutomaticAbilityProfile {
                    id: AbilityId(u32::from_be_bytes(*b"A03K")),
                    mana_cost: 66,
                    cooldown_ticks: 7 * CASTLE_FIGHT_SIMULATION_HZ as u16,
                    range: world(300),
                    target_policy: AbilityTargetPolicy::WoundedFriendlyUnit,
                    effect: AbilityEffect::HolyAid {
                        native_buff: crate::native_effects::native_buff_for_version(
                            MapVersion::CASTLE_FIGHT_9_27,
                            u32::from_be_bytes(*b"A03M"),
                        ),
                        modifier: ModifierId(u32::from_be_bytes(*b"A03M")),
                        healing: 25,
                        armor_bonus_per_100: 600,
                        regeneration_per_second_per_10k: 160_000,
                        duration_ticks: 10 * CASTLE_FIGHT_SIMULATION_HZ as u16,
                        permanent_max_health_bonus: 0,
                        resurrection_count: 0,
                        resurrection_radius: 0,
                        resurrection_mana_cost: 0,
                        resurrection_cooldown_ticks: 0,
                        resurrection_delay_ticks: 0,
                    },
                },
            ),
            Self::Paladin => (
                ManaProfile {
                    maximum: 300,
                    starting: 30,
                    regen_per_tick_per_10k: 833,
                },
                AutomaticAbilityProfile {
                    id: AbilityId(u32::from_be_bytes(*b"A03K")),
                    mana_cost: 66,
                    cooldown_ticks: 7 * CASTLE_FIGHT_SIMULATION_HZ as u16,
                    range: world(300),
                    target_policy: AbilityTargetPolicy::WoundedFriendlyUnit,
                    effect: AbilityEffect::HolyAid {
                        native_buff: crate::native_effects::native_buff_for_version(
                            MapVersion::CASTLE_FIGHT_9_27,
                            u32::from_be_bytes(*b"A03I"),
                        ),
                        modifier: ModifierId(u32::from_be_bytes(*b"A03I")),
                        healing: 25,
                        armor_bonus_per_100: 900,
                        regeneration_per_second_per_10k: 240_000,
                        duration_ticks: 10 * CASTLE_FIGHT_SIMULATION_HZ as u16,
                        permanent_max_health_bonus: 100,
                        resurrection_count: 1,
                        resurrection_radius: world(900),
                        resurrection_mana_cost: 70,
                        resurrection_cooldown_ticks: 40 * CASTLE_FIGHT_SIMULATION_HZ as u16,
                        resurrection_delay_ticks: CASTLE_FIGHT_SIMULATION_HZ as u16,
                    },
                },
            ),
            Self::HolyWarrior => (
                ManaProfile {
                    maximum: 200,
                    starting: 100,
                    regen_per_tick_per_10k: 443,
                },
                AutomaticAbilityProfile {
                    id: AbilityId(u32::from_be_bytes(*b"A0I0")),
                    mana_cost: 50,
                    cooldown_ticks: 20 * CASTLE_FIGHT_SIMULATION_HZ as u16,
                    range: world(300),
                    target_policy: AbilityTargetPolicy::WoundedFriendlyUnit,
                    effect: AbilityEffect::Prayer {
                        native_buff: crate::native_effects::native_buff_for_version(
                            MapVersion::CASTLE_FIGHT_9_27,
                            u32::from_be_bytes(*b"A0I2"),
                        ),
                        modifier: ModifierId(u32::from_be_bytes(*b"A0I2")),
                        healing: 50,
                        mana_restored: 50,
                        armor_bonus_per_100: 300,
                        damage_bonus_per_10k: 2_000,
                        duration_ticks: 7 * CASTLE_FIGHT_SIMULATION_HZ as u16,
                        radius: world(450),
                        resurrection_count: 3,
                        resurrection_radius: world(500),
                    },
                },
            ),
            Self::Warlock => (
                ManaProfile {
                    maximum: 100,
                    starting: 100,
                    regen_per_tick_per_10k: 333,
                },
                AutomaticAbilityProfile {
                    id: AbilityId(u32::from_be_bytes(*b"A00K")),
                    mana_cost: 35,
                    cooldown_ticks: 12 * CASTLE_FIGHT_SIMULATION_HZ as u16,
                    range: world(90),
                    target_policy: AbilityTargetPolicy::RandomGroundEnemyUnit,
                    effect: AbilityEffect::AreaDamage {
                        amount: 270,
                        radius: world(370),
                        origin: AreaDamageOrigin::Target,
                    },
                },
            ),
            _ => return None,
        };
        Some(SpellcastingProfile { mana, ability })
    }

    fn human_passive_effects_9_27(self) -> Vec<PassiveUnitEffect> {
        let mut effects = Vec::new();
        match self {
            Self::Crusader | Self::Paladin => {
                effects.push(PassiveUnitEffect::Cleave(CleaveEffectProfile {
                    ability: AbilityId(u32::from_be_bytes(*b"A01F")),
                    radius: world(175),
                    damage_per_10k: 2_500,
                    targets: AttackTargetMask::GROUND_UNITS,
                }));
                if self == Self::Paladin {
                    effects.push(PassiveUnitEffect::Aura(AuraEffectProfile {
                        self_only: false,
                        melee_life_steal_per_10k: 0,
                        melee_damage_return_per_10k: 0,
                        damage_bonus_per_10k: 0,
                        affects_melee: true,
                        affects_ranged: true,
                        linger_ticks: 2,
                        ability: AbilityId(u32::from_be_bytes(*b"A03J")),
                        radius: world(400),
                        armor_bonus_per_100: 300,
                        mana_regeneration_per_second_per_10k: 0,
                        suspend_during_spell_cooldown: false,
                    }));
                }
            }
            Self::HolyWarrior => {
                effects.push(PassiveUnitEffect::SpellResistance(
                    SpellResistanceEffectProfile {
                        ability: AbilityId(u32::from_be_bytes(*b"A0AH")),
                        damage_taken_per_10k: 3_000,
                    },
                ));
                effects.push(PassiveUnitEffect::Aura(AuraEffectProfile {
                    self_only: false,
                    melee_life_steal_per_10k: 0,
                    melee_damage_return_per_10k: 0,
                    damage_bonus_per_10k: 0,
                    affects_melee: true,
                    affects_ranged: true,
                    linger_ticks: 2,
                    ability: AbilityId(u32::from_be_bytes(*b"A0HZ")),
                    radius: world(420),
                    armor_bonus_per_100: 900,
                    mana_regeneration_per_second_per_10k: 0,
                    suspend_during_spell_cooldown: false,
                }));
            }
            Self::Warlock => effects.push(PassiveUnitEffect::Aura(AuraEffectProfile {
                self_only: false,
                melee_life_steal_per_10k: 0,
                melee_damage_return_per_10k: 0,
                damage_bonus_per_10k: 0,
                affects_melee: true,
                affects_ranged: true,
                linger_ticks: 2,
                ability: AbilityId(u32::from_be_bytes(*b"A00F")),
                radius: 0,
                armor_bonus_per_100: 0,
                mana_regeneration_per_second_per_10k: 10_000,
                suspend_during_spell_cooldown: true,
            })),
            _ => {}
        }
        effects
    }

    fn definition_9_27(self) -> CastleFightUnitDefinition {
        let rawcode = self.rawcode_9_27();
        let content = extracted_content_927();
        let unit = &content.units[&rawcode];
        // Mechanical does not mean corpse-less: retained siege death types leave decaying
        // remains. Resurrection eligibility is checked separately against the source definition.
        let expose_corpse = content.corpses[&rawcode].does_decay;
        extracted_unit_definition_927(rawcode, unit.name, expose_corpse)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastleFightUnitDefinition {
    pub action_timing: ActionTimingProfile,
    pub map_version: MapVersion,
    pub rawcode: u32,
    pub name: &'static str,
    pub health: i32,
    pub health_regen_per_second_per_10k: u32,
    pub build_time_ticks: u32,
    pub repair_time_ticks: u32,
    pub armor: ArmorProfile,
    pub passive_effects: PassiveUnitEffects,
    pub spellcasting: Option<SpellcastingProfile>,
    pub additional_abilities: Option<AdditionalAutomaticAbilityDefinitions>,
    pub damage_type: DamageType,
    pub attack_targets: AttackTargetMask,
    pub secondary_attack: Option<SecondaryAttackProfile>,
    pub movement_class: MovementClass,
    pub mechanical: bool,
    pub classifications: UnitClassifications,
    pub collision_radius: CollisionRadius,
    pub corpse: Option<CorpseProfile>,
    pub attack: AttackProfile,
    pub movement: MovementProfile,
}

impl CastleFightUnitDefinition {
    /// All declared automatic effects, retaining the primary control identity first.
    pub fn automatic_abilities(&self) -> impl Iterator<Item = AutomaticAbilityProfile> + '_ {
        self.spellcasting
            .map(|profile| profile.ability)
            .into_iter()
            .chain(
                self.additional_abilities
                    .as_ref()
                    .into_iter()
                    .flat_map(|definitions| definitions.iter()),
            )
    }

    #[must_use]
    pub const fn resolved(self) -> ResolvedUnitDefinition {
        ResolvedUnitDefinition {
            template: self.template(),
            properties: self.gameplay_properties(),
            spellcasting: self.spellcasting,
            additional_abilities: self.additional_abilities,
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
            action_timing: self.action_timing,
            content: Some(ContentIdentity {
                map_version: self.map_version,
                rawcode: self.rawcode,
                name: self.name,
            }),
            health_regen_per_second_per_10k: self.health_regen_per_second_per_10k,
            corpse: self.corpse,
            collision_radius: Some(self.collision_radius),
            movement_class: self.movement_class,
            mechanical: self.mechanical,
            classifications: self.classifications,
            build_time_ticks: Some(self.build_time_ticks),
            repair_time_ticks: Some(self.repair_time_ticks),
            attack_targets: if let Some(secondary) = self.secondary_attack {
                self.attack_targets.union(secondary.targets)
            } else {
                self.attack_targets
            },
            secondary_attack: self.secondary_attack,
            damage_type: self.damage_type,
            armor: self.armor,
            passive_effects: self.passive_effects,
        }
    }
}

impl CastleFightProductionKind {
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
        Ok(Self::from_retained_rawcode_9_27(rawcode))
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
        let rawcode = self.rawcode_9_27();
        let content = extracted_content_927();
        let trained_rawcode = content.production[&rawcode].unit_rawcode;
        let produced_rawcode = content
            .production_handoffs
            .get(&trained_rawcode)
            .copied()
            .unwrap_or(trained_rawcode);
        let unit = CastleFightUnitKind::from_retained_rawcode_9_27(produced_rawcode)
            .expect("promoted production must reference a promoted unit");
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
    pub classifications: UnitClassifications,
    pub construction_time_ticks: u32,
    pub repair_time_ticks: u32,
    pub health_regen_per_second_per_10k: u32,
    pub armor: ArmorProfile,
    pub spawn_interval_ticks: u16,
    pub footprint_size_cells: u16,
    pub unit: CastleFightUnitKind,
    pub produced_unit: CastleFightUnitDefinition,
    pub train_command_position: CommandCardPosition,
    pub train_hotkey: Option<char>,
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
        assert_eq!(
            self.produced_unit.map_version, self.map_version,
            "production and its resolved child must retain the same definition version"
        );
        let unit = self.produced_unit.resolved();
        BuildingGameplayProperties {
            action_timing: ActionTimingProfile::default(),
            secondary_attack: None,
            passive_effects: PassiveUnitEffects::EMPTY,
            content: Some(ContentIdentity {
                map_version: self.map_version,
                rawcode: self.rawcode,
                name: self.name,
            }),
            construction_time_ticks: Some(self.construction_time_ticks),
            repair_time_ticks: Some(self.repair_time_ticks),
            health_regen_per_second_per_10k: self.health_regen_per_second_per_10k,
            classifications: self.classifications,
            attack_targets: AttackTargetMask::ALL,
            damage_type: DamageType::Normal,
            armor: self.armor,
            economy: Some(self.economy),
            production_unit: unit.properties,
            production_spellcasting: unit.spellcasting,
            production_additional_abilities: unit.additional_abilities,
        }
    }
}

impl CastleFightTowerKind {
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
        Ok(Self::from_retained_rawcode_9_27(rawcode))
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
        let rawcode = self.rawcode_9_27();
        let expected_name = extracted_content_927().buildings[&rawcode].name;
        let mut definition = extracted_tower_definition_927(rawcode, expected_name);
        definition.spellcasting = match self {
            Self::Eraser | Self::EraserLevelTwo => {
                Some(crate::building_mechanics::eraser_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                    rawcode,
                ))
            }
            Self::MagicRuin => Some(
                crate::building_mechanics::magic_ruin_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                ),
            ),
            Self::Volcano => Some(
                crate::building_mechanics::flame_strike_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                ),
            ),
            Self::ShrineOfDestruction => Some(
                crate::building_mechanics::attack_proxy_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                ),
            ),
            Self::TidalGuardian => crate::building_mechanics::tidal_spellcasting_for_version(
                MapVersion::CASTLE_FIGHT_9_27,
            ),
            Self::OracleOfSirens => {
                Some(crate::building_mechanics::oracle_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                ))
            }
            Self::ForgottenOne => Some(
                crate::building_mechanics::forgotten_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                ),
            ),
            Self::WellOfPain => Some(crate::building_mechanics::well_spellcasting_for_version(
                MapVersion::CASTLE_FIGHT_9_27,
            )),
            Self::AncientPyramide => {
                Some(crate::building_mechanics::pyramid_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                ))
            }
            Self::StarfallObelisk => Some(
                crate::building_mechanics::starfall_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                ),
            ),
            Self::ObeliskOfWilderness => Some(
                crate::building_mechanics::wilderness_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                ),
            ),
            Self::SilverGlade => Some(crate::building_mechanics::silver_spellcasting_for_version(
                MapVersion::CASTLE_FIGHT_9_27,
            )),
            Self::EnduranceTotem => Some(
                crate::building_mechanics::endurance_aura_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                ),
            ),
            Self::HealingTotem => Some(
                crate::building_mechanics::healing_aura_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                ),
            ),
            Self::StasisTotem => Some(crate::building_mechanics::stasis_spellcasting_for_version(
                MapVersion::CASTLE_FIGHT_9_27,
            )),
            Self::CeremonialTotem => Some(
                crate::building_mechanics::ceremonial_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                ),
            ),
            Self::SerpentRock => Some(crate::building_mechanics::serpent_spellcasting_for_version(
                MapVersion::CASTLE_FIGHT_9_27,
            )),
            Self::WorldFreezer => Some(
                crate::building_mechanics::world_freezer_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                ),
            ),
            Self::ChillingMushroom => Some(
                crate::building_mechanics::mushroom_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                ),
            ),
            Self::FrostLauncher | Self::GreaterFrostLauncher => Some(
                crate::building_mechanics::launcher_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                    rawcode,
                ),
            ),
            Self::SnowveilFountain => {
                Some(crate::building_mechanics::snow_spellcasting_for_version(
                    MapVersion::CASTLE_FIGHT_9_27,
                ))
            }
            Self::CityOfMagic => Some(crate::building_mechanics::city_spellcasting_for_version(
                MapVersion::CASTLE_FIGHT_9_27,
            )),
            Self::Artillery => {
                let speed_per_tick = match definition
                    .attack
                    .expect("Artillery retains its authored ballistic weapon")
                    .delivery
                {
                    AttackDelivery::RangedBallistic { speed_per_tick, .. } => speed_per_tick,
                    _ => panic!("9.27 Artillery must use a ballistic weapon"),
                };
                definition.attack = None;
                Some(SpellcastingProfile {
                    mana: ManaProfile {
                        maximum: 0,
                        starting: 0,
                        regen_per_tick_per_10k: 0,
                    },
                    ability: AutomaticAbilityProfile {
                        id: AbilityId(u32::from_be_bytes(*b"A02K")),
                        mana_cost: 0,
                        cooldown_ticks: 15 * CASTLE_FIGHT_SIMULATION_HZ as u16,
                        range: 0,
                        target_policy: AbilityTargetPolicy::RandomEnemyBasePoint,
                        effect: AbilityEffect::ArtilleryBombardment {
                            min_damage: 300,
                            max_damage: 400,
                            speed_per_tick,
                            splash: SplashFalloffProfile {
                                full_radius: world(60),
                                medium_radius: world(150),
                                outer_radius: world(320),
                                medium_damage_per_10k: 5_000,
                                outer_damage_per_10k: 2_000,
                                targets: AttackTargetMask::ALL,
                            },
                            burning_oil: BurningOilEffectProfile {
                                ability: AbilityId(u32::from_be_bytes(*b"A02K")),
                                radius: world(150),
                                full_damage: 5,
                                full_interval_millis: 250,
                                half_damage: 3,
                                half_interval_millis: 1_000,
                                full_duration_millis: 1_010,
                                total_duration_millis: 1_010,
                                target_ground_units: true,
                                target_buildings: true,
                            },
                        },
                    },
                })
            }
            Self::Gjallarhorn => Some(SpellcastingProfile {
                mana: ManaProfile {
                    maximum: 10,
                    starting: 0,
                    // Ceil 1 mana/sec into the 30 Hz fixed-point tick rate so the map's
                    // mana-gated 7-second cadence lands on the authored whole-second boundary.
                    regen_per_tick_per_10k: 334,
                },
                ability: AutomaticAbilityProfile {
                    id: AbilityId(u32::from_be_bytes(*b"A01K")),
                    mana_cost: 7,
                    cooldown_ticks: CASTLE_FIGHT_SIMULATION_HZ as u16,
                    range: world(500),
                    target_policy: AbilityTargetPolicy::AllFriendlyUnits,
                    effect: AbilityEffect::HolyFervour {
                        modifier: ModifierId(u32::from_be_bytes(*b"A016")),
                        radius: world(500),
                        duration_ticks: 60 * CASTLE_FIGHT_SIMULATION_HZ as u16,
                    },
                },
            }),
            Self::VesselOfPurity => Some(SpellcastingProfile {
                mana: ManaProfile {
                    maximum: 18,
                    starting: 0,
                    // As above, preserve the 15-second mana cadence while retaining the
                    // separately recovered 1-second WC3 cooldown.
                    regen_per_tick_per_10k: 334,
                },
                ability: AutomaticAbilityProfile {
                    id: AbilityId(u32::from_be_bytes(*b"A0HN")),
                    mana_cost: 15,
                    cooldown_ticks: CASTLE_FIGHT_SIMULATION_HZ as u16,
                    range: world(99_999),
                    target_policy: AbilityTargetPolicy::RandomCorpse,
                    effect: AbilityEffect::Purification {
                        damage: 150,
                        radius: world(300),
                        consume_radius: world(220),
                        reveal_radius: spell_reveal_for_version(
                            MapVersion::CASTLE_FIGHT_9_27,
                            AbilityId(u32::from_be_bytes(*b"A0HN")),
                        )
                        .expect("resolved Far Sight proxy")
                        .radius,
                        reveal_duration_ticks: u16::try_from(
                            spell_reveal_for_version(
                                MapVersion::CASTLE_FIGHT_9_27,
                                AbilityId(u32::from_be_bytes(*b"A0HN")),
                            )
                            .expect("resolved Far Sight proxy")
                            .duration_ticks,
                        )
                        .expect("Far Sight duration capacity"),
                    },
                },
            }),
            _ => definition.spellcasting,
        };
        definition
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastleFightTowerDefinition {
    pub action_timing: ActionTimingProfile,
    pub secondary_attack: Option<SecondaryAttackProfile>,
    pub passive_effects: PassiveUnitEffects,
    pub rawcode: u32,
    pub name: &'static str,
    pub basic_tooltip: &'static str,
    pub extended_tooltip: &'static str,
    pub gold_cost: u16,
    pub lumber_cost: u16,
    pub economy: BuildingEconomyProfile,
    pub health: i32,
    pub classifications: UnitClassifications,
    pub construction_time_ticks: u32,
    pub repair_time_ticks: u32,
    pub health_regen_per_second_per_10k: u32,
    pub armor: ArmorProfile,
    pub damage_type: DamageType,
    pub attack_targets: AttackTargetMask,
    pub footprint_size_cells: u16,
    pub command_card_position: CommandCardPosition,
    pub hotkey: char,
    pub attack: Option<AttackProfile>,
    pub spellcasting: Option<SpellcastingProfile>,
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
            attack: self.attack,
            spellcasting: self.spellcasting,
        }
    }

    #[must_use]
    pub const fn gameplay_properties(self) -> BuildingGameplayProperties {
        BuildingGameplayProperties {
            action_timing: self.action_timing,
            secondary_attack: self.secondary_attack,
            passive_effects: self.passive_effects,
            content: Some(ContentIdentity {
                map_version: self.map_version,
                rawcode: self.rawcode,
                name: self.name,
            }),
            construction_time_ticks: Some(self.construction_time_ticks),
            repair_time_ticks: Some(self.repair_time_ticks),
            health_regen_per_second_per_10k: self.health_regen_per_second_per_10k,
            classifications: self.classifications,
            attack_targets: if let Some(secondary) = self.secondary_attack {
                self.attack_targets.union(secondary.targets)
            } else {
                self.attack_targets
            },
            damage_type: self.damage_type,
            armor: self.armor,
            economy: Some(self.economy),
            production_unit: UnitGameplayProperties {
                action_timing: ActionTimingProfile {
                    primary_attack_ticks: 0,
                    primary_attack_point_ticks: 0,
                    secondary_attack_ticks: 0,
                    secondary_attack_point_ticks: 0,
                    cast_ticks: 0,
                    cast_point_ticks: 0,
                },
                content: None,
                corpse: None,
                collision_radius: None,
                movement_class: MovementClass::Ground,
                mechanical: false,
                classifications: UnitClassifications {
                    locust: false,
                    melee_attacker: false,
                    hero: false,
                    summoned: false,
                    resistant_skin: false,
                    neutral_hostile_creep_level: None,
                    ancient: false,
                    spell_immune: false,
                    combat_sapper: false,
                    invulnerable: false,
                    legendary: false,
                    summoned_marker: false,
                    illusion: false,
                    invisible: false,
                },
                build_time_ticks: None,
                repair_time_ticks: None,
                attack_targets: AttackTargetMask::ALL,
                secondary_attack: None,
                health_regen_per_second_per_10k: 0,
                damage_type: DamageType::Normal,
                armor: ArmorProfile::UNARMORED,
                passive_effects: PassiveUnitEffects::EMPTY,
            },
            production_spellcasting: None,
            production_additional_abilities: None,
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
        // Stock Warcraft order/waypoint capacity, exposed through this versioned profile.
        // https://classic.battle.net/war3/basics/specialcommands.shtml
        order_queue_capacity: 35,
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
    let shrine_system =
        crate::golden_shrine_definition_for_version(version).expect("registered shrine system");
    let mut dormant_shrine_roots = BTreeSet::new();
    let playable_rawcodes = units
        .values()
        .map(|definition| definition.rawcode)
        .chain(
            production_buildings
                .values()
                .map(|definition| definition.rawcode),
        )
        .chain(towers.values().map(|definition| definition.rawcode))
        .chain(
            extracted
                .production_handoffs
                .iter()
                .filter_map(|(proxy, body)| {
                    units
                        .values()
                        .any(|unit| unit.rawcode == *body)
                        .then_some(*proxy)
                }),
        );
    for rawcode in playable_rawcodes {
        root_set.extend(crate::native_effects::script_granted_ability_sources_for(
            version, rawcode,
        ));
        let abilities = extracted
            .unit_abilities
            .get(&rawcode)
            .ok_or(CastleFightContentError::MissingAbilityInventory(rawcode))?;
        for &key in abilities {
            let source = NativeEffectSource::new(NativeEffectSourceKind::UnitAbility, key);
            if rawcode == shrine_system.parameters.golden_shrine_unit_id
                && shrine_system.dormant_attack_abilities.contains(&key)
            {
                // Retain the inherited critical-strike inventory without pretending the utility
                // building has a weapon. Dedicated script projection owns this dormant dependency.
                debug_assert!(
                    towers
                        .values()
                        .find(|tower| tower.rawcode == rawcode)
                        .is_some_and(|tower| tower.attack.is_none())
                );
                dormant_shrine_roots.insert(source);
            } else {
                root_set.insert(source);
            }
        }
    }
    let carrier = crate::native_carriers::carrier_for_version(version)
        .map_err(|_| CastleFightContentError::UnsupportedRelease(version))?;
    if towers
        .values()
        .any(|definition| definition.rawcode == carrier.building_rawcode)
    {
        // A07W's retained script installs A000 on an independent persistent carrier.
        // Keep that actual effect in dependency closure, not just the building's marker.
        root_set.insert(NativeEffectSource::new(
            NativeEffectSourceKind::AbilityEffect,
            carrier.ability.0,
        ));
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
    for source in dormant_shrine_roots {
        if !behaviors.iter().any(|behavior| behavior.source == source) {
            behaviors.push(ResolvedCastleFightBehavior {
                id: stable_ability_id(source)?,
                source,
                implementation: NativeEffectImplementationId::WarcraftCriticalStrikeV1,
            });
        }
    }
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
        main_castle_classifications: extracted_content_927().units[&u32::from_be_bytes(*b"hcas")]
            .target_classifications,
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
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A09I") => {
            0x4000_023a
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A09J") => {
            0x4000_023b
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0A0") => {
            0x4000_023c
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A095") => {
            0x4000_023d
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A09Y") => {
            0x4000_023e
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A09Z") => {
            0x4000_023f
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A09F") => {
            0x4000_0240
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A099") => {
            0x4000_0241
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0A3") => {
            0x4000_0242
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0FZ") => {
            0x4000_0243
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A09D") => {
            0x4000_0244
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A09E") => {
            0x4000_0245
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A091") => {
            0x4000_0246
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A08Z") => {
            0x4000_0247
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A092") => {
            0x4000_0248
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A094") => {
            0x4000_0249
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A00J") => {
            0x4000_024a
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A09C") => {
            0x4000_024b
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A09B") => {
            0x4000_024c
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0GN") => {
            0x4000_022a
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0GS") => {
            0x4000_022b
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0GQ") => {
            0x4000_022c
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0HG") => {
            0x4000_022d
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0HF") => {
            0x4000_022e
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0H7") => {
            0x4000_022f
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0HC") => {
            0x4000_0230
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0HH") => {
            0x4000_0231
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0HA") => {
            0x4000_0232
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0H4") => {
            0x4000_0233
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0H9") => {
            0x4000_0234
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0GV") => {
            0x4000_0235
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0GZ") => {
            0x4000_0236
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0H2") => {
            0x4000_0237
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0H6") => {
            0x4000_0238
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0GW") => {
            0x4000_0239
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0G6") => {
            0x4000_020c
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0GK") => {
            0x4000_0229
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0GF") => {
            0x4000_0228
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0GE") => {
            0x4000_0227
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A0HW") => {
            0x4000_0226
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0HX") => {
            0x4000_0224
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0GR") => {
            0x4000_0223
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A093") => {
            0x4000_0222
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0GU") => {
            0x4000_0221
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0J7") => {
            0x4000_0220
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0GB") => {
            0x4000_021f
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0AZ") => {
            0x4000_021e
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0J6") => {
            0x4000_021d
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0HQ") => {
            0x4000_021c
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0G8") => {
            0x4000_021b
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0H3") => {
            0x4000_021a
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0H1") => {
            0x4000_0219
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A0GD") => {
            0x4000_0217
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0KB") => {
            0x4000_0218
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0G5") => {
            0x4000_0216
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0G9") => {
            0x4000_0214
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0G3") => {
            0x4000_0213
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0G7") => {
            0x4000_020d
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0GL") => {
            0x4000_020e
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0G2") => {
            0x4000_020f
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0HM") => {
            0x4000_0210
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0HL") => {
            0x4000_0211
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0GG") => {
            0x4000_0212
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A08H") => {
            0x4000_01c1
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A050") => {
            0x4000_01c0
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A04U") => {
            0x4000_01bf
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A057") => {
            0x4000_01be
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A02C") => {
            0x4000_01bd
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A04O") => {
            0x4000_01bb
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A04N") => {
            0x4000_01bc
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A04Y") => {
            0x4000_01ba
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A04X") => {
            0x4000_01b9
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A04W") => {
            0x4000_01b8
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A04V") => {
            0x4000_01b7
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A04T") => {
            0x4000_01b6
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A051") => {
            0x4000_01b5
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A053") => {
            0x4000_01b4
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A069") => {
            0x4000_01b3
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A04Z") => {
            0x4000_01b2
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A058") => {
            0x4000_01b1
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A05G") => {
            0x4000_01b0
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0B9") => {
            0x4000_01af
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A05F") => {
            0x4000_01ae
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"ANic") => {
            0x4000_01ad
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A0AB") => {
            0x4000_01ac
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0AC") => {
            0x4000_01ab
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A0AF") => {
            0x4000_01aa
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0AE") => {
            0x4000_01a9
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A04M") => {
            0x4000_01a8
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A055") => {
            0x4000_01a7
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A054") => {
            0x4000_01a6
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A067") => {
            0x4000_01a5
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A04Q") => {
            0x4000_01a4
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A04P") => {
            0x4000_01a3
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A029") => {
            0x4000_01a2
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A028") => {
            0x4000_01a1
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A01W") => {
            0x4000_01a0
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"ACsw") => {
            0x4000_019f
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A06U") => {
            0x4000_019e
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A06T") => {
            0x4000_019d
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A06S") => {
            0x4000_019c
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A0BL") => {
            0x4000_019b
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A06R") => {
            0x4000_019a
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A01J") => {
            0x4000_0197
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A01I") => {
            0x4000_0198
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A05H") => {
            0x4000_0199
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A01E") => {
            0x4000_0194
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0EU") => {
            0x4000_0195
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0FE") => {
            0x4000_0196
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A07L") => {
            0x4000_0193
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A07J") => {
            0x4000_0192
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A02B") => {
            0x4000_0191
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A02A") => {
            0x4000_0190
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A05J") => {
            0x4000_018f
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A01U") => {
            0x4000_018e
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A012") => {
            0x4000_018d
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A006") => {
            0x4000_018c
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A01R") => {
            0x4000_018b
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A01P") => {
            0x4000_018a
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A01G") => {
            0x4000_0189
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A03R") => {
            0x4000_0188
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A03S") => {
            0x4000_0187
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A05Z") => {
            0x4000_0185
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A03V") => {
            0x4000_0183
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A05W") => {
            0x4000_0182
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"Aloc") => {
            0x4000_0181
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"Avul") => {
            0x4000_0180
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A0FS") => {
            0x4000_017f
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0FU") => {
            0x4000_017e
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0FR") => {
            0x4000_017d
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0FQ") => {
            0x4000_017c
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0FP") => {
            0x4000_017b
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0AI") => {
            0x4000_017a
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0FO") => {
            0x4000_0179
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A05V") => {
            0x4000_0178
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0Z1") => {
            0x4000_0177
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A09X") => {
            0x4000_0175
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A00P") => {
            0x4000_0176
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A07V") => {
            0x4000_0172
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0HY") => {
            0x4000_0173
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A007") => {
            0x4000_0174
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A00O") => {
            0x4000_0170
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A00N") => {
            0x4000_0171
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A00M") => {
            0x4000_016e
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A00B") => {
            0x4000_016f
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A02Q") => {
            0x4000_016c
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A02R") => {
            0x4000_016d
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A02S") => {
            0x4000_016b
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A02T") => {
            0x4000_016a
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A02U") => {
            0x4000_0169
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A01C") => {
            0x4000_0167
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A068") => {
            0x4000_0166
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A02Y") => {
            0x4000_0165
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A02X") => {
            0x4000_0164
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A009") => {
            0x4000_0163
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A09W") => {
            0x4000_015f
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A06J") => {
            0x4000_0160
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A06K") => {
            0x4000_0161
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A07G") => {
            0x4000_0162
        }

        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A00C") => {
            0x4000_015e
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A025") => {
            0x4000_0157
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A061") => {
            0x4000_0159
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A02N") => {
            0x4000_015a
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A0H0") => {
            0x4000_015b
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A024") => {
            0x4000_015c
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A021") => {
            0x4000_015d
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A060") => {
            0x4000_0155
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A026") => {
            0x4000_0156
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A09H") => {
            0x4000_0153
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A023") => {
            0x4000_0154
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A07U") => {
            0x4000_0150
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A07T") => {
            0x4000_0151
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A08T") => {
            0x4000_0152
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A07Z") => {
            0x4000_014f
        }
        (NativeEffectSourceKind::UnitAbility, v) if v == u32::from_be_bytes(*b"A08E") => {
            0x4000_014b
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A08C") => {
            0x4000_014c
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A08D") => {
            0x4000_014d
        }
        (NativeEffectSourceKind::AbilityEffect, v) if v == u32::from_be_bytes(*b"A08F") => {
            0x4000_014e
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A08B") => {
            0x4000_0149
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A08A") => {
            0x4000_014a
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0CG") => {
            0x4000_0147
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A003") => {
            0x4000_0148
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A074") => {
            0x4000_0143
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A07D") => {
            0x4000_0144
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A07O") => {
            0x4000_0145
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A07Y") => {
            0x4000_0146
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A073") => {
            0x4000_0140
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A07B") => {
            0x4000_0141
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A07C") => {
            0x4000_0142
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A078") => {
            0x4000_013e
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A075") => {
            0x4000_013f
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A077") => {
            0x4000_013a
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A076") => {
            0x4000_013b
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03U") => {
            0x4000_013c
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A07M") => {
            0x4000_013d
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A07R") => {
            0x4000_0138
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A07Q") => {
            0x4000_0139
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A07P") => {
            0x4000_0136
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A07A") => {
            0x4000_0137
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03O") => {
            0x4000_0135
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A00T") => {
            0x4000_0134
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0ET") => {
            0x4000_0133
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0D9") => {
            0x4000_0131
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0EX") => {
            0x4000_0132
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0EV") => {
            0x4000_012f
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0DC") => {
            0x4000_0130
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0FG") => {
            0x4000_012d
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0D0") => {
            0x4000_012e
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0EI") => {
            0x4000_0128
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0D1") => {
            0x4000_0129
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0D2") => {
            0x4000_012a
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0E3") => {
            0x4000_012b
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0E4") => {
            0x4000_012c
        }

        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0DV") => {
            0x4000_0121
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0D7") => {
            0x4000_0122
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0F3") => {
            0x4000_0123
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0D6") => {
            0x4000_0125
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0F2") => {
            0x4000_0126
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0F4") => {
            0x4000_0127
        }

        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0EK") => {
            0x4000_011f
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0EL") => {
            0x4000_0120
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0CP") => {
            0x4000_011e
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0DW") => {
            0x4000_011c
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0CO") => {
            0x4000_011d
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0CI") => {
            0x4000_0118
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0F0") => {
            0x4000_0119
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0CK") => {
            0x4000_011a
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0F6") => {
            0x4000_011b
        }

        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0DX") => {
            0x4000_0112
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0CH") => {
            0x4000_0113
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0F7") => {
            0x4000_0114
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0CJ") => {
            0x4000_0115
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0F8") => {
            0x4000_0116
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0D5") => {
            0x4000_0117
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0E1") => {
            0x4000_0111
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0CM") => {
            0x4000_010c
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0CN") => {
            0x4000_010d
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0DY") => {
            0x4000_010e
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0DZ") => {
            0x4000_010f
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"Aeth") => {
            0x4000_0110
        }

        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0E0") => {
            0x4000_010b
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0FA") => {
            0x4000_0108
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0FJ") => {
            0x4000_0109
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0CU") => {
            0x4000_010a
        }

        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0CT") => {
            0x4000_0106
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0FI") => {
            0x4000_0107
        }

        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0CS") => {
            0x4000_0104
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0FH") => {
            0x4000_0105
        }

        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03Q") => {
            0x4000_0103
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0C5") => {
            0x4000_0099
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0EN") => {
            0x4000_0100
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0E6") => {
            0x4000_0101
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0CR") => {
            0x4000_0102
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0CQ") => {
            0x4000_0097
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0E5") => {
            0x4000_0098
        }

        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0DS") => {
            0x4000_0095
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0F5") => {
            0x4000_0096
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0HS") => {
            0x4000_0093
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"AstC") => {
            0x4000_0094
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0HT") => {
            0x4000_0091
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"AstB") => {
            0x4000_0092
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A032") => {
            0x4000_008f
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"Ast9") => {
            0x4000_0090
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A020") => {
            0x4000_008e
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A039") => {
            0x4000_0082
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A03B") => {
            0x4000_0083
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A03A") => {
            0x4000_0084
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A03D") => {
            0x4000_0085
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A08L") => {
            0x4000_0086
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A05I") => {
            0x4000_0087
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A06W") => {
            0x4000_0088
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A08M") => {
            0x4000_0089
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A06Y") => {
            0x4000_008a
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A08R") => {
            0x4000_008b
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A06X") => {
            0x4000_008c
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0F1") => {
            0x4000_008d
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A02Z") => {
            0x4000_007f
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A030") => {
            0x4000_0080
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A02O") => {
            0x4000_0081
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A08P") => {
            0x4000_007c
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A08O") => {
            0x4000_007d
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A08N") => {
            0x4000_007e
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A034") => {
            0x4000_007a
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A037") => {
            0x4000_007b
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A035") => {
            0x4000_0078
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A036") => {
            0x4000_0079
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A038") => {
            0x4000_0077
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0AG") => {
            0x4000_0073
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0FB") => {
            0x4000_0074
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0FC") => {
            0x4000_0075
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0HR") => {
            0x4000_0076
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A06I") => {
            0x4000_0071
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A06H") => {
            0x4000_0072
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A07K") => {
            0x4000_006f
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A02I") => {
            0x4000_0070
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0A6") => {
            0x4000_006e
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0GT") => {
            0x4000_006d
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A02M") => {
            0x4000_006c
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A02W") => {
            0x4000_006a
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A02P") => {
            0x4000_006b
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A02V") => {
            0x4000_0069
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A04I") => {
            0x4000_0062
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A081") => {
            0x4000_0063
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A080") => {
            0x4000_0064
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A084") => {
            0x4000_0065
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A082") => {
            0x4000_0066
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A083") => {
            0x4000_0067
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A04H") => {
            0x4000_0068
        }

        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A06P") => {
            0x4000_0060
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0AK") => {
            0x4000_0061
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A04L") => {
            0x4000_005e
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A04K") => {
            0x4000_005f
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A04J") => {
            0x4000_005c
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A04F") => {
            0x4000_005d
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A04B") => {
            0x4000_0058
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0HO") => {
            0x4000_005a
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"AM0{") => {
            0x4000_005b
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A04C") => {
            0x4000_0059
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A04A") => {
            0x4000_0052
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A044") => {
            0x4000_0053
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A043") => {
            0x4000_0054
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A046") => {
            0x4000_0055
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A045") => {
            0x4000_0056
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A042") => {
            0x4000_0057
        }

        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03Y") => {
            0x4000_0051
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A05A") => {
            0x4000_004e
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A05C") => {
            0x4000_004f
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A05B") => {
            0x4000_0050
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A059") => {
            0x4000_004d
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A047") => {
            0x4000_004c
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03C") => {
            0x4000_0049
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A048") => {
            0x4000_004a
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A040") => {
            0x4000_004b
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0FM") => {
            0x4000_0047
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0FL") => {
            0x4000_0048
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03X") => {
            0x4000_0046
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A04E") => {
            0x4000_0042
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A04D") => {
            0x4000_0043
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A05E") => {
            0x4000_0044
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A05D") => {
            0x4000_0045
        }

        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A00D") => {
            0x4000_0040
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A063") => {
            0x4000_0041
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A062") => {
            0x4000_003f
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A017") => {
            0x4000_020a
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A018") => {
            0x4000_020b
        }
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
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A02L") => {
            0x4000_000d
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A06F") => {
            0x4000_000e
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A06G") => {
            0x4000_000f
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A06L") => {
            0x4000_0010
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A07E") => {
            0x4000_0011
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0FK") => {
            0x4000_0012
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0EZ") => {
            0x4000_0013
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A070") => {
            0x4000_0014
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A06V") => {
            0x4000_0015
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A07H") => {
            0x4000_0016
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03M") => {
            0x4000_0017
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03I") => {
            0x4000_0018
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03H") => {
            0x4000_0019
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A01A") => {
            0x4000_001a
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A090") => {
            0x4000_001b
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A01F") => {
            0x4000_001c
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0AD") => {
            0x4000_001d
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0AH") => {
            0x4000_001e
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03J") => {
            0x4000_001f
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0HZ") => {
            0x4000_0020
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A00F") => {
            0x4000_0021
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03K") => {
            0x4000_0022
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0I0") => {
            0x4000_0023
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A00K") => {
            0x4000_0024
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A02K") => {
            0x4000_0025
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A01K") => {
            0x4000_0026
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0HN") => {
            0x4000_0027
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"AM05") => {
            0x4000_0028
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A06M") => {
            0x4000_0029
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A08I") => {
            0x4000_002a
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A08J") => {
            0x4000_002b
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A03P") => {
            0x4000_002c
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A00V") => {
            0x4000_002d
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0AL") => {
            0x4000_002e
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0AN") => {
            0x4000_002f
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A05Y") => {
            0x4000_0030
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A014") => {
            0x4000_0031
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A00W") => {
            0x4000_0032
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A08V") => {
            0x4000_0033
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A08U") => {
            0x4000_0034
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0A7") => {
            0x4000_0035
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0A8") => {
            0x4000_0036
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A00X") => {
            0x4000_0037
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A00Y") => {
            0x4000_0038
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A010") => {
            0x4000_0039
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A0A9") => {
            0x4000_003a
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A0AA") => {
            0x4000_003b
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A00Z") => {
            0x4000_003c
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A00L") => {
            0x4000_003d
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A013") => {
            0x4000_003e
        }
        // Dedicated shrine inventory range; no shared native-effect recipe/binding is needed for
        // a critical-strike ability on a utility building with both weapons disabled.
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A06A") => {
            0x4300_0001
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A015") => {
            0x4200_000b
        }
        (NativeEffectSourceKind::UnitAbility, value) if value == u32::from_be_bytes(*b"A07W") => {
            0x4200_000c
        }
        (NativeEffectSourceKind::AbilityEffect, value) if value == u32::from_be_bytes(*b"A000") => {
            0x4200_000d
        }
        _ => return Err(CastleFightContentError::MissingStableAbilityId(source)),
    };
    Ok(CastleFightAbilityId(id))
}

fn canonical_content_bundle_hash(bundle: &CastleFightContentBundle) -> u64 {
    let mut hash = ContentHash64::new();
    for r in catalog_supplement_927()
        .expect("validated catalog")
        .spell_reveals
    {
        hash.write_u32(parse_rawcode(&r.rawcode));
        hash.write_i32(r.radius_world);
        hash.write_u64(r.duration_ticks);
        hash.write_u8(u8::from(r.only_if_hidden));
        hash.write_u8(u8::from(r.detects_invisible));
    }
    for p in catalog_supplement_927()
        .expect("validated catalog")
        .sight_profiles
    {
        hash.write_u32(parse_rawcode(&p.rawcode));
        hash.write_i32(p.day_world);
        hash.write_i32(p.night_world);
    }
    hash.write_u64(0x4346_434f_4e54_0001);
    hash.write_u32(CASTLE_FIGHT_CONTENT_BUNDLE_SCHEMA_VERSION);
    hash.write_u16(bundle.map_version.major);
    hash.write_u16(bundle.map_version.minor);
    hash.write_bytes(bundle.revision.as_bytes());
    hash.write_i32(CASTLE_FIGHT_SIMULATION_HZ);
    // Includes source identities as well as tuning; no independent hand-authored tower values.
    hash.write_bytes(
        &crate::native_carriers::canonical_projection_for_version(bundle.map_version)
            .expect("registered native tower content version"),
    );
    hash.write_bytes(crate::building_mechanics::canonical_projection_for_version(
        bundle.map_version,
    ));
    hash.write_bytes(crate::native_effects::canonical_projection_for_version(
        bundle.map_version,
    ));
    for rule in crate::native_effects::unit_death_rules_for_version(bundle.map_version) {
        hash.write_u32(rule.rawcode);
        hash.write_u16(rule.heal_percent);
        hash.write_i32(rule.retaliation_damage);
    }
    for rule in crate::native_effects::unit_structure_damage_rules_for_version(bundle.map_version) {
        hash.write_u32(rule.rawcode);
        hash.write_u16(rule.factor_per_10k);
    }
    for rule in crate::native_effects::unit_flying_retarget_rules_for_version(bundle.map_version) {
        hash.write_u32(rule.rawcode);
        hash.write_u32(rule.throttle_ticks);
    }
    let mut death_spawns: Vec<_> =
        crate::native_effects::death_spawns_for_version(bundle.map_version).collect();
    death_spawns.sort_unstable_by_key(|spawn| spawn.ability);
    for spawn in death_spawns {
        hash.write_u32(spawn.ability.0);
        hash.write_u32(spawn.child_rawcode);
        hash.write_u8(spawn.count);
    }
    let air_bonuses = (bundle.map_version == MapVersion::CASTLE_FIGHT_9_27)
        .then(|| &extracted_content_927().air_attack_bonuses);
    hash.write_u64(air_bonuses.map_or(0, |profiles| profiles.len()) as u64);
    for (rawcode, bonus) in air_bonuses.into_iter().flat_map(|profiles| profiles.iter()) {
        hash.write_u32(*rawcode);
        hash.write_u16(*bonus);
    }
    hash_command_card(&mut hash, bundle.command_card);
    hash_economy_rules(&mut hash, bundle.economy);
    hash_damage_rules(&mut hash, bundle.damage_rules);
    hash.write_u32(bundle.main_castle_repair_time_ticks);
    hash_classifications(&mut hash, bundle.main_castle_classifications);
    if let Some(shrine) = crate::golden_shrine_definition_for_version(bundle.map_version) {
        let p = &shrine.parameters;
        hash.write_u32(p.golden_shrine_unit_id);
        hash.write_u32(p.chance_percent_per_shrine);
        hash.write_u32(p.maximum_effective_chance_percent);
        hash.write_u32(p.chance_roll_min);
        hash.write_u32(p.chance_roll_max);
        hash.write_u64(p.revive_delay_seconds);
        hash.write_u32(p.exclude_legendary_marker_ability_id);
        hash.write_u32(p.exclude_summoned_unit_marker_ability_id);
        hash.write_u32(shrine.building_health_regen_per_second_per_10k);
    }

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
    hash.write_u32(layout.rescue_strike_hotkey as u32);
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
    hash.write_u16(definition.map_version.major);
    hash.write_u16(definition.map_version.minor);
    hash.write_u32(definition.rawcode);
    let mana = native_mana_profile_for_version(definition.map_version, definition.rawcode);
    hash.write_u8(u8::from(mana.is_some()));
    if let Some(mana) = mana {
        hash.write_i32(mana.maximum);
        hash.write_i32(mana.starting);
        hash.write_u32(mana.regen_per_tick_per_10k);
    }
    hash.write_i32(definition.health);
    hash.write_u32(definition.health_regen_per_second_per_10k);
    hash.write_u16(definition.action_timing.primary_attack_ticks);
    hash.write_u16(definition.action_timing.primary_attack_point_ticks);
    hash.write_u16(definition.action_timing.secondary_attack_ticks);
    hash.write_u16(definition.action_timing.secondary_attack_point_ticks);
    hash.write_u16(definition.action_timing.cast_ticks);
    hash.write_u16(definition.action_timing.cast_point_ticks);
    hash.write_u32(definition.build_time_ticks);
    hash.write_u32(definition.repair_time_ticks);
    hash.write_u8(definition.armor.armor_type.stable_tag());
    hash.write_i32(i32::from(definition.armor.armor_points));
    hash_passive_effects(hash, definition.passive_effects);
    hash_optional_spellcasting(hash, definition.spellcasting);
    hash_additional_ability_definitions(hash, definition.additional_abilities);
    hash.write_u8(definition.damage_type.stable_tag());
    hash.write_u8(definition.attack_targets.bits());
    if let Some(secondary) = definition.secondary_attack {
        hash.write_u8(1);
        hash_attack_profile(hash, secondary.attack);
        hash.write_u8(secondary.primary_targets.bits());
        hash.write_u8(secondary.targets.bits());
        hash.write_u8(secondary.damage_type.stable_tag());
    } else {
        hash.write_u8(0);
    }
    hash.write_u8(match definition.movement_class {
        MovementClass::Ground => 0,
        MovementClass::Air => 1,
    });
    hash.write_u8(u8::from(definition.mechanical));
    hash_classifications(hash, definition.classifications);
    hash.write_i32(definition.collision_radius.0);
    match definition.corpse {
        Some(corpse) => {
            hash.write_u8(1);
            hash.write_u32(corpse.definition.0);
            hash.write_u32(corpse.decay_start_ticks);
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

fn hash_classifications(hash: &mut ContentHash64, flags: UnitClassifications) {
    hash.write_u8(u8::from(flags.locust));
    hash.write_u8(u8::from(flags.hero));
    hash.write_u8(u8::from(flags.resistant_skin));
    hash.write_u8(u8::from(flags.neutral_hostile_creep_level.is_some()));
    if let Some(level) = flags.neutral_hostile_creep_level {
        hash.write_u16(level);
    }
    hash.write_u8(u8::from(flags.ancient));
    hash.write_u8(u8::from(flags.summoned));
    hash.write_u8(u8::from(flags.spell_immune));
    hash.write_u8(u8::from(flags.combat_sapper));
    hash.write_u8(u8::from(flags.invulnerable));
    hash.write_u8(u8::from(flags.legendary));
    hash.write_u8(u8::from(flags.summoned_marker));
    hash.write_u8(u8::from(flags.illusion));
    hash.write_u8(u8::from(flags.invisible));
    hash.write_u8(u8::from(flags.melee_attacker));
}

fn hash_production_definition(
    hash: &mut ContentHash64,
    definition: CastleFightProductionDefinition,
) {
    hash.write_u16(definition.map_version.major);
    hash.write_u16(definition.map_version.minor);
    hash.write_u32(definition.rawcode);
    hash.write_u16(definition.gold_cost);
    hash.write_u16(definition.lumber_cost);
    hash_building_economy(hash, definition.economy);
    hash.write_i32(definition.building_health);
    hash_classifications(hash, definition.classifications);
    hash.write_u32(definition.construction_time_ticks);
    hash.write_u32(definition.repair_time_ticks);
    hash.write_u32(definition.health_regen_per_second_per_10k);
    hash.write_u8(definition.armor.armor_type.stable_tag());
    hash.write_i32(i32::from(definition.armor.armor_points));
    hash.write_u16(definition.spawn_interval_ticks);
    hash.write_u16(definition.footprint_size_cells);
    hash.write_u32(definition.unit.stable_id().0);
    hash_unit_definition(hash, definition.produced_unit);
    hash.write_u8(definition.train_command_position.x);
    hash.write_u8(definition.train_command_position.y);
    if let Some(hotkey) = definition.train_hotkey {
        hash.write_u8(1);
        hash.write_u32(hotkey as u32);
    } else {
        hash.write_u8(0);
    }
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
    hash_passive_effects(hash, definition.passive_effects);
    for ticks in [
        definition.action_timing.primary_attack_ticks,
        definition.action_timing.primary_attack_point_ticks,
        definition.action_timing.secondary_attack_ticks,
        definition.action_timing.secondary_attack_point_ticks,
        definition.action_timing.cast_ticks,
        definition.action_timing.cast_point_ticks,
    ] {
        hash.write_u16(ticks);
    }
    if let Some(secondary) = definition.secondary_attack {
        hash.write_u8(1);
        hash_attack_profile(hash, secondary.attack);
        hash.write_u8(secondary.primary_targets.bits());
        hash.write_u8(secondary.targets.bits());
        hash.write_u8(secondary.damage_type.stable_tag());
    } else {
        hash.write_u8(0);
    }
    hash.write_u16(definition.map_version.major);
    hash.write_u16(definition.map_version.minor);
    hash.write_u32(definition.rawcode);
    hash.write_u16(definition.gold_cost);
    hash.write_u16(definition.lumber_cost);
    hash_building_economy(hash, definition.economy);
    hash.write_i32(definition.health);
    hash_classifications(hash, definition.classifications);
    hash.write_u32(definition.construction_time_ticks);
    hash.write_u32(definition.repair_time_ticks);
    hash.write_u32(definition.health_regen_per_second_per_10k);
    hash.write_u8(definition.armor.armor_type.stable_tag());
    hash.write_i32(i32::from(definition.armor.armor_points));
    hash.write_u8(definition.damage_type.stable_tag());
    hash.write_u8(definition.attack_targets.bits());
    hash.write_u16(definition.footprint_size_cells);
    hash.write_u8(definition.command_card_position.x);
    hash.write_u8(definition.command_card_position.y);
    hash.write_u32(definition.hotkey as u32);
    if let Some(attack) = definition.attack {
        hash.write_u8(1);
        hash_attack_profile(hash, attack);
    } else {
        hash.write_u8(0);
    }
    hash_optional_spellcasting(hash, definition.spellcasting);
}

fn hash_builder_definition(hash: &mut ContentHash64, definition: &CastleFightBuilderDefinition) {
    hash.write_u16(definition.map_version.major);
    hash.write_u16(definition.map_version.minor);
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
    hash.write_u16(economy.legendary_points_cost);
    hash.write_u64(economy.income_per_10k);
}

fn hash_attack_profile(hash: &mut ContentHash64, attack: AttackProfile) {
    hash.write_u8(attack.delivery.stable_tag());
    match attack.delivery {
        AttackDelivery::Melee | AttackDelivery::RangedInstant => {}
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
        AttackDelivery::Line {
            speed_per_tick,
            minimum_range,
            spill_distance,
            spill_radius,
            damage_retention_per_10k,
            spill_targets,
        } => {
            hash.write_i32(speed_per_tick);
            hash.write_i32(minimum_range);
            hash.write_i32(spill_distance);
            hash.write_i32(spill_radius);
            hash.write_u16(damage_retention_per_10k);
            hash.write_u8(spill_targets.bits());
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
            PassiveUnitEffect::Poison(profile) => {
                hash.write_u8(15);
                hash.write_u32(profile.ability.0);
                hash.write_u32(profile.buff_rawcode);
                hash.write_i32(profile.damage_per_second);
                hash.write_i32(i32::from(profile.movement_percent_delta));
                hash.write_i32(i32::from(profile.attack_speed_percent_delta));
                hash.write_u16(profile.duration_ticks);
                hash.write_u16(profile.hero_duration_ticks);
                hash.write_u8(profile.targets.bits());
                hash.write_u8(u8::from(profile.stack_damage));
                hash.write_u8(u8::from(profile.can_kill));
                hash.write_u8(u8::from(profile.organic_only));
            }
            PassiveUnitEffect::DamageThreshold(profile) => {
                hash.write_u8(14);
                hash.write_u32(profile.ability.0);
                for threshold in profile.thresholds {
                    hash.write_u16(threshold.hp_ratio_below_per_10k);
                    hash.write_i32(threshold.attack_bonus);
                    for channel in threshold.vertex_rgba {
                        hash.write_u8(channel);
                    }
                    hash.write_u8(u8::from(threshold.adds_if_absent));
                }
            }
            PassiveUnitEffect::KillBerserk(profile) => {
                hash.write_u8(13);
                hash.write_u32(profile.ability.0);
                hash.write_u16(profile.duration_ticks);
                hash.write_i32(i32::from(profile.movement_percent_delta));
                hash.write_i32(i32::from(profile.attack_speed_percent_delta));
                hash.write_i32(i32::from(profile.damage_taken_bonus_per_10k));
            }
            PassiveUnitEffect::CriticalStrike(profile) => {
                hash.write_u8(5);
                hash.write_u32(profile.ability.0);
                hash.write_u16(profile.chance_per_10k);
                hash.write_u16(profile.damage_multiplier_per_10k);
                hash.write_u8(profile.targets.bits());
            }
            PassiveUnitEffect::SplashFalloff(profile) => {
                hash.write_u8(6);
                hash.write_i32(profile.full_radius);
                hash.write_i32(profile.medium_radius);
                hash.write_i32(profile.outer_radius);
                hash.write_u16(profile.medium_damage_per_10k);
                hash.write_u16(profile.outer_damage_per_10k);
                hash.write_u8(profile.targets.bits());
            }
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
                hash.write_u64(profile.hero_chance_per_10k.map_or(u64::MAX, u64::from));
                hash.write_u64(profile.summoned_chance_per_10k.map_or(u64::MAX, u64::from));
                hash.write_u8(profile.targets.bits());
                hash.write_u16(profile.cooldown_ticks);
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
            PassiveUnitEffect::Cleave(profile) => {
                hash.write_u8(7);
                hash.write_u32(profile.ability.0);
                hash.write_i32(profile.radius);
                hash.write_u16(profile.damage_per_10k);
                hash.write_u8(profile.targets.bits());
            }
            PassiveUnitEffect::SpikedShell(profile) => {
                hash.write_u8(25);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::LifeSteal(profile) => {
                hash.write_u8(33);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::DeathExplosion(profile) => {
                hash.write_u8(32);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::ManaAttackOrb(profile) => {
                hash.write_u8(31);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::Incinerate(profile) => {
                hash.write_u8(30);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::HostileArmorAura(profile) => {
                hash.write_u8(29);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::PermanentImmolation(profile) => {
                hash.write_u8(27);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::HardenedSkin(profile) => {
                hash.write_u8(26);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::Corruption(profile) => {
                hash.write_u8(24);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::SourceDamageDispatcher(ability) => {
                hash.write_u8(23);
                hash.write_u32(ability.0);
            }
            PassiveUnitEffect::HealthScaledAftershock(profile) => {
                hash.write_u8(22);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::EchoStep(profile) => {
                hash.write_u8(40);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::DamageRetaliation(profile) => {
                hash.write_u8(21);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::LavaSpawn(profile) => {
                hash.write_u8(20);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::MirrorImage(profile) => {
                hash.write_u8(19);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::DamageCharge(profile) => {
                hash.write_u8(18);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::ContactSelfDestruct(profile) => {
                hash.write_u8(44);
                for word in profile.canonical_words() {
                    hash.write_u64(word);
                }
            }
            PassiveUnitEffect::Sphere(profile) => {
                hash.write_u8(17);
                hash.write_u32(profile.ability.0);
                hash.write_u8(profile.attachment_count);
                hash.write_u16(profile.cooldown_ticks);
                hash.write_i32(profile.speed_per_tick);
            }
            PassiveUnitEffect::SpeedAura(profile) => {
                hash.write_u8(16);
                hash.write_u32(profile.ability.0);
                hash.write_u32(profile.buff_rawcode);
                hash.write_i32(profile.radius);
                hash.write_u8(profile.targets.bits());
                hash.write_u8(u8::from(profile.hostile));
                hash.write_u8(u8::from(profile.organic_only));
                hash.write_u8(u8::from(profile.affects_invulnerable));
                hash.write_i32(i32::from(profile.movement_percent_delta));
                hash.write_i32(i32::from(profile.attack_speed_percent_delta));
            }
            PassiveUnitEffect::Aura(profile) => {
                hash.write_u8(u8::from(profile.self_only));
                hash.write_u32(profile.melee_life_steal_per_10k);
                hash.write_u32(profile.melee_damage_return_per_10k);
                hash.write_i32(i32::from(profile.damage_bonus_per_10k));
                hash.write_u8(u8::from(profile.affects_melee));
                hash.write_u8(u8::from(profile.affects_ranged));
                hash.write_u16(profile.linger_ticks);
                hash.write_u8(8);
                hash.write_u32(profile.ability.0);
                hash.write_i32(profile.radius);
                hash.write_i32(i32::from(profile.armor_bonus_per_100));
                hash.write_u32(profile.mana_regeneration_per_second_per_10k);
                hash.write_u8(u8::from(profile.suspend_during_spell_cooldown));
            }
            PassiveUnitEffect::Pulverize(profile) => {
                hash.write_u8(12);
                hash.write_u32(profile.ability.0);
                hash.write_u16(profile.chance_per_10k);
                hash.write_i32(profile.damage);
                hash.write_i32(profile.full_radius);
                hash.write_i32(profile.half_radius);
                hash.write_u8(profile.targets.bits());
            }
            PassiveUnitEffect::FrostAttack(profile) => {
                hash.write_u8(11);
                hash.write_u32(profile.ability.0);
                hash.write_u16(profile.duration_ticks);
                hash.write_u16(profile.hero_duration_ticks);
                hash.write_i32(i32::from(profile.movement_percent_delta));
                hash.write_i32(i32::from(profile.attack_speed_percent_delta));
                hash.write_u8(profile.targets.bits());
            }
            PassiveUnitEffect::Feedback(profile) => {
                hash.write_u8(10);
                hash.write_u32(profile.ability.0);
                hash.write_i32(profile.maximum_mana_drained);
                hash.write_u16(profile.damage_per_mana_per_10k);
                hash.write_i32(profile.summoned_damage);
                hash.write_u8(profile.targets.bits());
            }
            PassiveUnitEffect::SpellResistance(profile) => {
                hash.write_u8(9);
                hash.write_u32(profile.ability.0);
                hash.write_u16(profile.damage_taken_per_10k);
            }
        }
    }
}

fn hash_triggered_effect(hash: &mut ContentHash64, effect: TriggeredAttackEffect) {
    match effect {
        TriggeredAttackEffect::NativeDebuff { parent, profile } => {
            hash.write_u8(4);
            hash.write_u32(parent.0);
            for word in profile.canonical_words() {
                hash.write_u64(word);
            }
        }
        TriggeredAttackEffect::ForkedLightning(profile) => {
            hash.write_u8(3);
            for word in profile.canonical_words() {
                hash.write_u64(word);
            }
        }
        TriggeredAttackEffect::ConeWave(profile) => {
            hash.write_u8(2);
            for word in profile.canonical_words() {
                hash.write_u64(word);
            }
        }
        TriggeredAttackEffect::ChainLightning(profile) => {
            hash.write_u8(0);
            hash.write_u32(profile.ability.0);
            hash.write_i32(profile.initial_damage);
            hash.write_u8(profile.maximum_targets);
            hash.write_i32(profile.jump_radius);
            hash.write_u16(profile.damage_reduction_per_10k);
            hash.write_u8(profile.targets.bits());
        }
        TriggeredAttackEffect::AcidBomb(profile) => {
            hash.write_u8(6);
            for word in profile.canonical_words() {
                hash.write_u64(word);
            }
        }
        TriggeredAttackEffect::Charm(profile) => {
            hash.write_u8(5);
            for word in profile.canonical_words() {
                hash.write_u64(word);
            }
        }
        TriggeredAttackEffect::Whirlwind(profile) => {
            hash.write_u8(7);
            for word in profile.canonical_words() {
                hash.write_u64(word);
            }
        }
        TriggeredAttackEffect::EntanglingRoots(profile) => {
            hash.write_u8(1);
            hash.write_u32(profile.ability.0);
            hash.write_i32(profile.damage_per_second);
            hash.write_u16(profile.duration_ticks);
            hash.write_u16(profile.hero_duration_ticks);
            hash.write_u8(u8::from(profile.nonhero_only));
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
    hash_automatic_ability_profile(hash, spellcasting.ability);
}

fn hash_additional_ability_definitions(
    hash: &mut ContentHash64,
    definitions: Option<AdditionalAutomaticAbilityDefinitions>,
) {
    let Some(definitions) = definitions else {
        hash.write_u8(0);
        return;
    };
    hash.write_u8(1);
    hash.write_u64(definitions.iter().count() as u64);
    for ability in definitions.iter() {
        hash_automatic_ability_profile(hash, ability);
    }
}

fn hash_ensnare_profile(hash: &mut ContentHash64, profile: crate::components::EnsnareProfile) {
    hash.write_u32(profile.ability.0);
    hash.write_u16(profile.duration_ticks);
    hash.write_u16(profile.hero_duration_ticks);
    hash.write_i32(profile.range);
    hash.write_i32(profile.speed_per_tick);
    hash.write_u16(profile.lowering_ticks);
    hash.write_i32(profile.air_height);
    hash.write_i32(profile.melee_range);
}

fn hash_native_bolt_profile(
    hash: &mut ContentHash64,
    profile: crate::components::NativeBoltProfile,
) {
    hash.write_u32(profile.ability.0);
    hash.write_i32(profile.damage);
    hash.write_u16(profile.stun_ticks);
    hash.write_u16(profile.hero_stun_ticks);
    hash.write_i32(profile.damage_per_second);
    hash.write_u16(profile.duration_ticks);
    hash.write_i32(profile.speed_per_tick);
    hash.write_u8(u8::from(profile.cleanse));
    hash.write_u8(profile.targets.bits());
}

fn hash_automatic_ability_profile(hash: &mut ContentHash64, ability: AutomaticAbilityProfile) {
    hash.write_u32(ability.id.0);
    hash.write_i32(ability.mana_cost);
    hash.write_u16(ability.cooldown_ticks);
    hash.write_i32(ability.range);
    hash.write_u8(ability.target_policy.stable_tag());
    hash.write_u8(ability.effect.stable_tag());
    let native_buff = match ability.effect {
        AbilityEffect::FaerieFire { native_buff, .. }
        | AbilityEffect::FrostArmor { native_buff, .. }
        | AbilityEffect::HolyAid { native_buff, .. }
        | AbilityEffect::Prayer { native_buff, .. } => native_buff,
        _ => None,
    };
    if let Some(buff) = native_buff {
        hash.write_u8(1);
        hash.write_u32(buff.rawcode);
        hash.write_u8(u8::from(buff.positive));
        hash.write_u8(u8::from(buff.stealable));
        hash.write_u8(u8::from(buff.organic_only));
    } else {
        hash.write_u8(0);
    }
    match ability.effect {
        AbilityEffect::NagaTidalWave(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::NagaCharm(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::NagaEarthquake(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::WellOfPain(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::ForgottenOne(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::Starfall(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::Wilderness(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::SilverGlade(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::Heal(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::SirenPurge(p) => p.visit_canonical_words(|word| hash.write_u64(word)),
        AbilityEffect::NativeDebuff(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::Curse(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::ImpaleFan(p) => {
            p.visit_canonical_words(|word| hash.write_u64(word));
        }
        AbilityEffect::ParasiteAreaBuff(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::ParasiteScript(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::BuildingScript(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::TravellingManaClap(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::NativeSummon(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::WindWalk(p) => p.visit_canonical_words(|word| hash.write_u64(word)),
        AbilityEffect::ScriptedRoots(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::BuildingAura(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::BuildingStasis(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::Ensnare {
            profile,
            search_radius,
            maximum_targets,
        } => {
            hash_ensnare_profile(hash, profile);
            hash.write_i32(search_radius);
            hash.write_u8(maximum_targets);
        }
        AbilityEffect::SpellSteal { recipient_radius } => hash.write_i32(recipient_radius),
        AbilityEffect::StatBuff {
            modifier,
            buff,
            armor_bonus_per_100,
            damage_bonus_per_10k,
            regeneration_per_second_per_10k,
            duration_ticks,
            hero_duration_ticks,
            autocast_range,
        } => {
            hash.write_u32(modifier.0);
            hash.write_u32(buff.rawcode);
            hash.write_u8(u8::from(buff.positive));
            hash.write_u8(u8::from(buff.stealable));
            hash.write_u8(u8::from(buff.organic_only));
            hash.write_i32(i32::from(armor_bonus_per_100));
            hash.write_i32(i32::from(damage_bonus_per_10k));
            hash.write_u32(regeneration_per_second_per_10k);
            hash.write_u16(duration_ticks);
            hash.write_u16(hero_duration_ticks);
            hash.write_i32(autocast_range);
        }
        AbilityEffect::MasterProjectiles(version) => {
            hash.write_u16(version.major);
            hash.write_u16(version.minor);
            for word in crate::native_effects::master_projectile_profile_for_version(version)
                .canonical_words()
            {
                hash.write_u64(word);
            }
        }
        AbilityEffect::BuildingEraser(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::BuildingMagicRuin(p) => {
            p.visit_canonical_words(|word| hash.write_u64(word));
        }
        AbilityEffect::BuildingFlameStrike(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::BuildingAttackProxy(p) => {
            p.visit_canonical_words(|word| hash.write_u64(word))
        }
        AbilityEffect::Hailstone(profile) => hash_hailstone_content(hash, profile),
        AbilityEffect::ManaBurnVisual => {}
        AbilityEffect::NativeWeatherVisual { duration_ticks } => hash.write_u16(duration_ticks),
        AbilityEffect::BuildingWeather(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::BuildingEnchantment(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::BuildingPermanentGrant(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::BuildingAcidBomb(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::WorldFreezer(p) => {
            for word in p.canonical_words() {
                hash.write_u64(word);
            }
        }
        AbilityEffect::BuildingBolt(p) => {
            hash.write_u16(p.map_version.major);
            hash.write_u16(p.map_version.minor);
            hash.write_u8(p.trigger_targets.bits());
            hash.write_u8(u8::from(p.trigger_invulnerable));
            hash.write_u8(u8::from(p.trigger_spell_immune));
            hash_native_bolt_profile(hash, p.bolt);
            hash.write_i32(p.child_range);
            hash.write_i32(p.vision_radius);
            hash.write_u16(p.vision_ticks);
        }
        AbilityEffect::Snowfall { map_version } => {
            hash.write_u16(map_version.major);
            hash.write_u16(map_version.minor);
        }
        AbilityEffect::Hex { profile } => {
            hash.write_u16(profile.map_version.major);
            hash.write_u16(profile.map_version.minor);
            hash.write_u16(profile.duration_ticks);
            hash.write_u16(profile.hero_duration_ticks);
            hash.write_u16(profile.initial_reengage_ticks);
            hash.write_u16(profile.defender_restore_ticks);
            hash.write_u16(profile.defender_resume_ticks);
            hash.write_u32(profile.defender_rawcode);
            for form in [profile.ground, profile.air] {
                hash.write_u32(form.rawcode);
                hash.write_i32(form.speed_per_tick);
                hash.write_i32(form.collision_radius);
                hash.write_i32(i32::from(form.armor.armor_points));
                hash.write_u8(form.armor.armor_type.stable_tag());
            }
        }
        AbilityEffect::FaerieFire {
            native_buff: _,
            modifier,
            armor_reduction_per_100,
            duration_ticks,
            hero_duration_ticks,
        } => {
            hash.write_u32(modifier.0);
            hash.write_i32(i32::from(armor_reduction_per_100));
            hash.write_u16(duration_ticks);
            hash.write_u16(hero_duration_ticks);
        }
        AbilityEffect::PhoenixFire(profile) => hash_native_bolt_profile(hash, profile),
        AbilityEffect::SolarStrike {
            profile,
            radius,
            maximum_targets,
        } => {
            hash_native_bolt_profile(hash, profile);
            hash.write_i32(radius);
            hash.write_u8(maximum_targets);
        }
        AbilityEffect::HealingWave(profile) => {
            hash.write_u32(profile.ability.0);
            hash.write_i32(profile.healing);
            hash.write_i32(profile.trigger_healing);
            hash.write_u8(profile.maximum_targets);
            hash.write_i32(profile.jump_radius);
            hash.write_u16(profile.retention_per_10k);
            hash.write_u16(profile.recovery_ticks);
        }
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
        AbilityEffect::FrostNova {
            ability,
            radius,
            primary_damage,
            area_damage,
            duration_ticks,
            hero_duration_ticks,
            movement_percent_delta,
            attack_speed_percent_delta,
            targets,
        } => {
            hash.write_u32(ability.0);
            hash.write_i32(radius);
            hash.write_i32(primary_damage);
            hash.write_i32(area_damage);
            hash.write_u16(duration_ticks);
            hash.write_u16(hero_duration_ticks);
            hash.write_i32(i32::from(movement_percent_delta));
            hash.write_i32(i32::from(attack_speed_percent_delta));
            hash.write_u8(targets.bits());
        }
        AbilityEffect::AreaDebuff {
            ability,
            radius,
            armor_delta_per_100,
            damage_delta_per_10k,
            duration_ticks,
            hero_duration_ticks,
            targets,
        } => {
            hash.write_u32(ability.0);
            hash.write_i32(radius);
            hash.write_i32(i32::from(armor_delta_per_100));
            hash.write_i32(i32::from(damage_delta_per_10k));
            hash.write_u16(duration_ticks);
            hash.write_u16(hero_duration_ticks);
            hash.write_u8(targets.bits());
        }
        AbilityEffect::AreaStun {
            ability,
            damage,
            radius,
            stun_ticks,
            hero_stun_ticks,
            targets,
        } => {
            hash.write_u32(ability.0);
            hash.write_i32(damage);
            hash.write_i32(radius);
            hash.write_u16(stun_ticks);
            hash.write_u16(hero_stun_ticks);
            hash.write_u8(targets.bits());
        }
        AbilityEffect::AreaDamage {
            amount,
            radius,
            origin,
        } => {
            hash.write_i32(amount);
            hash.write_i32(radius);
            hash.write_u8(match origin {
                AreaDamageOrigin::Caster => 0,
                AreaDamageOrigin::Target => 1,
            });
        }
        AbilityEffect::FanOfKnives {
            ability,
            damage,
            maximum_total_damage,
            maximum_targets,
            radius,
            targets,
            organic_only,
        } => {
            hash.write_u32(ability.0);
            hash.write_i32(damage);
            hash.write_i32(maximum_total_damage);
            hash.write_u16(maximum_targets);
            hash.write_i32(radius);
            hash.write_u8(targets.bits());
            hash.write_u8(u8::from(organic_only));
        }
        AbilityEffect::FrostArmor {
            native_buff: _,
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
        AbilityEffect::HolyAid {
            native_buff: _,
            modifier,
            healing,
            armor_bonus_per_100,
            regeneration_per_second_per_10k,
            duration_ticks,
            permanent_max_health_bonus,
            resurrection_count,
            resurrection_radius,
            resurrection_mana_cost,
            resurrection_cooldown_ticks,
            resurrection_delay_ticks,
        } => {
            hash.write_u32(modifier.0);
            hash.write_i32(healing);
            hash.write_i32(i32::from(armor_bonus_per_100));
            hash.write_u32(regeneration_per_second_per_10k);
            hash.write_u16(duration_ticks);
            hash.write_i32(permanent_max_health_bonus);
            hash.write_u8(resurrection_count);
            hash.write_i32(resurrection_radius);
            hash.write_i32(resurrection_mana_cost);
            hash.write_u16(resurrection_cooldown_ticks);
            hash.write_u16(resurrection_delay_ticks);
        }
        AbilityEffect::Prayer {
            native_buff: _,
            modifier,
            healing,
            mana_restored,
            armor_bonus_per_100,
            damage_bonus_per_10k,
            duration_ticks,
            radius,
            resurrection_count,
            resurrection_radius,
        } => {
            hash.write_u32(modifier.0);
            hash.write_i32(healing);
            hash.write_i32(mana_restored);
            hash.write_i32(i32::from(armor_bonus_per_100));
            hash.write_u16(damage_bonus_per_10k);
            hash.write_u16(duration_ticks);
            hash.write_i32(radius);
            hash.write_u8(resurrection_count);
            hash.write_i32(resurrection_radius);
        }
        AbilityEffect::HolyFervour {
            modifier,
            radius,
            duration_ticks,
        } => {
            hash.write_u32(modifier.0);
            hash.write_i32(radius);
            hash.write_u16(duration_ticks);
        }
        AbilityEffect::Purification {
            damage,
            radius,
            consume_radius,
            reveal_radius,
            reveal_duration_ticks,
        } => {
            hash.write_i32(damage);
            hash.write_i32(radius);
            hash.write_i32(consume_radius);
            hash.write_i32(reveal_radius);
            hash.write_u16(reveal_duration_ticks);
        }
        AbilityEffect::ArtilleryBombardment {
            min_damage,
            max_damage,
            speed_per_tick,
            splash,
            burning_oil,
        } => {
            hash.write_i32(min_damage);
            hash.write_i32(max_damage);
            hash.write_i32(speed_per_tick);
            hash.write_i32(splash.full_radius);
            hash.write_i32(splash.medium_radius);
            hash.write_i32(splash.outer_radius);
            hash.write_u16(splash.medium_damage_per_10k);
            hash.write_u16(splash.outer_damage_per_10k);
            hash.write_u8(splash.targets.bits());
            hash.write_u32(burning_oil.ability.0);
            hash.write_i32(burning_oil.radius);
            hash.write_i32(burning_oil.full_damage);
            hash.write_u16(burning_oil.full_interval_millis);
            hash.write_i32(burning_oil.half_damage);
            hash.write_u16(burning_oil.half_interval_millis);
            hash.write_u16(burning_oil.full_duration_millis);
            hash.write_u16(burning_oil.total_duration_millis);
            hash.write_u8(u8::from(burning_oil.target_ground_units));
            hash.write_u8(u8::from(burning_oil.target_buildings));
        }
    }
}

fn hash_hailstone_content(
    hash: &mut ContentHash64,
    p: crate::building_mechanics::HailstoneProfile,
) {
    hash.write_u16(p.map_version.major);
    hash.write_u16(p.map_version.minor);
    hash.write_u32(p.dummy_rawcode);
    hash.write_u32(p.ability.0);
    hash.write_i32(p.damage);
    hash.write_i32(p.speed_per_tick);
    hash.write_i32(p.range);
    hash.write_i32(p.full_radius);
    hash.write_u8(p.splash_targets.bits());
    hash.write_u8(p.trigger_targets.bits());
    hash.write_u8(u8::from(p.trigger_invulnerable));
    hash.write_u8(u8::from(p.trigger_spell_immune));
    hash.write_u8(p.freeze_targets.bits());
    hash.write_u16(p.freeze_ticks);
    hash.write_u16(p.hero_freeze_ticks);
    for code in p.excluded_rawcodes {
        hash.write_u32(code);
    }
    hash.write_u32(p.excluded_buff);
    hash.write_i32(p.vision_radius);
    hash.write_u16(p.vision_ticks);
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
    native_health: Option<i32>,
    native_armor: Option<ArmorProfile>,
    action_timing: ActionTimingProfile,
    mana_maximum: i32,
    native_mana: Option<ManaProfile>,
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
    target_classifications: UnitClassifications,
    sapper: bool,
    undead: bool,
    collision_radius: CollisionRadius,
    acquisition_range: Option<i32>,
    projectile_speed_per_tick: Option<i32>,
    outer_splash_radius: Option<i32>,
    splash_falloff: Option<SplashFalloffProfile>,
    attack1_damage_type: Option<DamageType>,
    attack1_damage: Option<i32>,
    attack1_cooldown_ticks: Option<u16>,
    attack1_range: Option<i32>,
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
    attack2_damage: Option<i32>,
    attack2_cooldown_ticks: Option<u16>,
    attack2_range: Option<i32>,
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
    decay_start_ticks: u32,
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
    simulation_hz: i32,
    map_version: String,
    release_revision: String,
    extraction_git_tree: String,
    source_object_fields_sha256: String,
    objects: Vec<CatalogSupplementObject927>,
    action_timings: Vec<CatalogActionTiming927>,
    building_secondary_attacks: Vec<CatalogBuildingSecondaryAttack927>,
    bounce_weapons: Vec<CatalogBounceWeapon927>,
    line_weapons: Vec<CatalogLineWeapon927>,
    secondary_artillery_weapons: Vec<CatalogSecondaryArtilleryWeapon927>,
    sight_profiles: Vec<CatalogSightProfile927>,
    fog_rules: CatalogFogRules927,
    spell_reveals: Vec<CatalogSpellReveal927>,
    production_handoffs: Vec<CatalogProductionHandoff927>,
    native_summon_bodies: Vec<CatalogSummonBodyCorpse927>,
    homing_splash_rawcodes: Vec<String>,
    air_attack_bonuses: Vec<CatalogAirAttackBonus927>,
}

#[derive(Debug, Deserialize)]
struct CatalogAirAttackBonus927 {
    rawcode: String,
    bonus_per_10k: u16,
}

pub(crate) fn native_air_attack_bonus_for_version(version: MapVersion, rawcode: u32) -> u16 {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return 0;
    }
    extracted_content_927()
        .air_attack_bonuses
        .get(&rawcode)
        .copied()
        .unwrap_or(0)
}

#[derive(Debug, Deserialize)]
struct CatalogSummonBodyCorpse927 {
    rawcode: String,
    does_decay: bool,
    decay_start_ticks: u32,
    lifetime_ticks: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct CatalogProductionHandoff927 {
    trained_rawcode: String,
    body_rawcode: String,
    body_death_ticks: u32,
}

#[derive(Debug, Deserialize)]
struct CatalogBuildingSecondaryAttack927 {
    rawcode: String,
    weapon_type: String,
    damage_type: String,
    targets: String,
    damage: String,
    range: String,
    cooldown: String,
    projectile_speed_world_per_second: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellRevealProfile {
    pub radius: i32,
    pub duration_ticks: u64,
    pub only_if_hidden: bool,
    pub detects_invisible: bool,
}

#[derive(Debug, Deserialize)]
struct CatalogSpellReveal927 {
    rawcode: String,
    radius_world: i32,
    duration_ticks: u64,
    only_if_hidden: bool,
    detects_invisible: bool,
}

pub(crate) fn spell_reveal_for_version(
    version: MapVersion,
    ability: AbilityId,
) -> Option<SpellRevealProfile> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return None;
    }
    static PROFILES: OnceLock<BTreeMap<u32, SpellRevealProfile>> = OnceLock::new();
    PROFILES
        .get_or_init(|| {
            catalog_supplement_927()
                .expect("validated spell vision projection")
                .spell_reveals
                .into_iter()
                .map(|r| {
                    (
                        parse_rawcode(&r.rawcode),
                        SpellRevealProfile {
                            radius: r.radius_world * SUBUNITS_PER_WORLD_UNIT,
                            duration_ticks: r.duration_ticks,
                            only_if_hidden: r.only_if_hidden,
                            detects_invisible: r.detects_invisible,
                        },
                    )
                })
                .collect()
        })
        .get(&ability.0)
        .copied()
}

#[derive(Debug, Deserialize)]
struct CatalogFogRules927 {
    initially_explored: bool,
    clock: crate::FogClock,
    attack_reveal_radius_world: i32,
    attack_reveal_duration_ticks: u64,
    permanent_rectangles_world: [Vec<[i32; 4]>; 2],
}

pub fn castle_fight_fog_rules_for_version(version: MapVersion) -> Option<crate::FogRules> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return None;
    }
    let rules = catalog_supplement_927()
        .expect("validated map fog projection")
        .fog_rules;
    Some(crate::FogRules {
        cell_size: 64 * SUBUNITS_PER_WORLD_UNIT,
        fallback_sight: crate::SightProfile::default(),
        initially_explored: rules.initially_explored,
        night: false,
        clock: Some(rules.clock),
        attack_reveal: Some(crate::FogAttackReveal {
            radius: rules.attack_reveal_radius_world * SUBUNITS_PER_WORLD_UNIT,
            duration_ticks: rules.attack_reveal_duration_ticks,
        }),
        permanent_rectangles: rules.permanent_rectangles_world.map(|rects| {
            rects
                .into_iter()
                .map(|r| {
                    (
                        crate::SimPoint::new(
                            r[0] * SUBUNITS_PER_WORLD_UNIT,
                            r[1] * SUBUNITS_PER_WORLD_UNIT,
                        ),
                        crate::SimPoint::new(
                            r[2] * SUBUNITS_PER_WORLD_UNIT,
                            r[3] * SUBUNITS_PER_WORLD_UNIT,
                        ),
                    )
                })
                .collect()
        }),
        sight_blockers: Vec::new(),
    })
}

#[derive(Debug, Deserialize)]
struct CatalogSightProfile927 {
    rawcode: String,
    day_world: i32,
    night_world: i32,
}

/// Resolved day/night sight, including inherited base fields, for this exact map version.
pub fn castle_fight_sight_for_version(
    version: MapVersion,
    rawcode: u32,
) -> Option<crate::fog::SightProfile> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return None;
    }
    static PROFILES: OnceLock<BTreeMap<u32, crate::fog::SightProfile>> = OnceLock::new();
    PROFILES
        .get_or_init(|| {
            catalog_supplement_927()
                .expect("validated sight projection")
                .sight_profiles
                .into_iter()
                .map(|p| {
                    (
                        parse_rawcode(&p.rawcode),
                        crate::fog::SightProfile {
                            day: p
                                .day_world
                                .checked_mul(SUBUNITS_PER_WORLD_UNIT)
                                .expect("day sight overflow"),
                            night: p
                                .night_world
                                .checked_mul(SUBUNITS_PER_WORLD_UNIT)
                                .expect("night sight overflow"),
                        },
                    )
                })
                .collect()
        })
        .get(&rawcode)
        .copied()
}

#[derive(Debug, Deserialize)]
struct CatalogActionTiming927 {
    rawcode: String,
    #[serde(flatten)]
    timing: ActionTimingProfile,
}

#[derive(Debug, Deserialize)]
struct CatalogSupplementObject927 {
    rawcode: String,
    repair_time_seconds: Option<u32>,
    button_x: Option<u8>,
    button_y: Option<u8>,
    hotkey: Option<char>,
    build_catalog: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct CatalogBounceWeapon927 {
    rawcode: String,
    maximum_targets: u8,
    damage_percent_per_bounce: u16,
    range_world: i32,
}

/// Secondary artillery weapon: its own projectile speed and splash bands, retained object fields
/// rather than the primary weapon's.
#[derive(Debug, Clone, Deserialize)]
struct CatalogSecondaryArtilleryWeapon927 {
    rawcode: String,
    projectile_speed_world_per_second: i32,
    full_world: i32,
    medium_world: i32,
    outer_world: i32,
    medium_damage_per_10k: u16,
    outer_damage_per_10k: u16,
    splash_targets: String,
}

impl CatalogSecondaryArtilleryWeapon927 {
    fn splash_profile(&self) -> SplashFalloffProfile {
        SplashFalloffProfile {
            full_radius: world(self.full_world),
            medium_radius: world(self.medium_world),
            outer_radius: world(self.outer_world),
            medium_damage_per_10k: self.medium_damage_per_10k,
            outer_damage_per_10k: self.outer_damage_per_10k,
            targets: parse_attack_targets_927(&self.splash_targets),
        }
    }
}

#[derive(Debug, Deserialize)]
struct CatalogLineWeapon927 {
    rawcode: String,
    minimum_range_world: i32,
    spill_distance_world: i32,
    spill_radius_world: i32,
    damage_retention_per_10k: u16,
    splash_targets: String,
}

#[derive(Debug)]
struct ExtractedContent927 {
    native_noncombat_summons: std::collections::BTreeSet<u32>,
    buildings: BTreeMap<u32, ExtractedBuilding927>,
    units: BTreeMap<u32, ExtractedUnit927>,
    unit_abilities: BTreeMap<u32, Vec<u32>>,
    protected_stats: BTreeMap<u32, ExtractedProtectedStats927>,
    primary_attacks: BTreeMap<u32, ExtractedAttack927>,
    homing_splash_rawcodes: BTreeSet<u32>,
    air_attack_bonuses: BTreeMap<u32, u16>,
    secondary_attacks: BTreeMap<u32, ExtractedAttack927>,
    building_secondary_attacks: BTreeMap<u32, CatalogBuildingSecondaryAttack927>,
    production: BTreeMap<u32, ExtractedProduction927>,
    production_handoffs: BTreeMap<u32, u32>,
    legendary_points_costs: BTreeMap<u32, u16>,
    corpses: BTreeMap<u32, ExtractedCorpse927>,
    repair_time_ticks: BTreeMap<u32, u32>,
    command_card_positions: BTreeMap<u32, CommandCardPosition>,
    /// Retained button coordinates exactly as authored; an absent component inherits the Warcraft
    /// III stock `Buttonpos` origin, which is only valid to resolve for buildings.
    authored_button_components: BTreeMap<u32, (Option<u8>, Option<u8>)>,
    building_hotkeys: BTreeMap<u32, char>,
    upgrades: BTreeMap<u32, ExtractedUpgradeLinks927>,
    income_semantics: BTreeMap<u32, ExtractedIncomeSemantics927>,
    income_per_10k: BTreeMap<u32, u64>,
    builder_catalogs: BTreeMap<(u8, u32), ExtractedBuilderCatalog927>,
    authored_builder_catalogs: BTreeMap<u32, Vec<u32>>,
    bounce_weapons: BTreeMap<u32, CatalogBounceWeapon927>,
    line_weapons: BTreeMap<u32, CatalogLineWeapon927>,
    secondary_artillery_weapons: BTreeMap<u32, CatalogSecondaryArtilleryWeapon927>,
    damage_rules: DamageRules,
}

impl ExtractedContent927 {
    /// Build-card position of a retained building. Map overrides record only coordinates that
    /// differ from the stock origin, so a missing component resolves to zero.
    fn building_command_card_position(&self, rawcode: u32) -> Option<CommandCardPosition> {
        self.command_card_positions
            .get(&rawcode)
            .copied()
            .or_else(|| {
                self.authored_button_components
                    .get(&rawcode)
                    .map(|&(x, y)| CommandCardPosition::new(x.unwrap_or(0), y.unwrap_or(0)))
            })
    }

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
        let handoffs = supplement.production_handoffs;
        let production_handoffs = handoffs
            .iter()
            .map(|entry| {
                (
                    parse_rawcode(&entry.trained_rawcode),
                    parse_rawcode(&entry.body_rawcode),
                )
            })
            .collect();
        let building_secondary_attacks = supplement
            .building_secondary_attacks
            .into_iter()
            .map(|weapon| (parse_rawcode(&weapon.rawcode), weapon))
            .collect();
        let action_timings: BTreeMap<_, _> = supplement
            .action_timings
            .into_iter()
            .map(|entry| (parse_rawcode(&entry.rawcode), entry.timing))
            .collect();
        let line_weapons = supplement
            .line_weapons
            .into_iter()
            .map(|weapon| (parse_rawcode(&weapon.rawcode), weapon))
            .collect();
        let secondary_artillery_weapons = supplement
            .secondary_artillery_weapons
            .into_iter()
            .map(|weapon| (parse_rawcode(&weapon.rawcode), weapon))
            .collect();
        let bounce_weapons = supplement
            .bounce_weapons
            .into_iter()
            .map(|weapon| (parse_rawcode(&weapon.rawcode), weapon))
            .collect();
        let native_summon_bodies = supplement.native_summon_bodies;
        let air_attack_bonuses = supplement
            .air_attack_bonuses
            .into_iter()
            .map(|bonus| (parse_rawcode(&bonus.rawcode), bonus.bonus_per_10k))
            .collect();
        let homing_splash_rawcodes = supplement
            .homing_splash_rawcodes
            .into_iter()
            .map(|rawcode| parse_rawcode(&rawcode))
            .collect();
        let mut repair_time_ticks = BTreeMap::<u32, u32>::new();
        let mut command_card_positions = BTreeMap::new();
        let mut authored_button_components = BTreeMap::new();
        let mut building_hotkeys = BTreeMap::new();
        let mut authored_builder_catalogs = BTreeMap::new();
        for object in supplement.objects {
            let rawcode = parse_rawcode(&object.rawcode);
            if let Some(catalog) = object.build_catalog {
                authored_builder_catalogs.insert(
                    rawcode,
                    catalog.iter().map(|code| parse_rawcode(code)).collect(),
                );
            }
            if let Some(seconds) = object.repair_time_seconds {
                let repair_ticks = seconds
                    .checked_mul(CASTLE_FIGHT_SIMULATION_HZ as u32)
                    .ok_or_else(|| format!("unit {rawcode:#010x} repair time overflowed"))?;
                if repair_time_ticks.insert(rawcode, repair_ticks).is_some() {
                    return Err(format!(
                        "catalog supplement repeats repair time for {rawcode:#010x}"
                    ));
                }
            }
            authored_button_components.insert(rawcode, (object.button_x, object.button_y));
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
            // The extraction marks a rate that neither the map nor the base object authors with a
            // dash; that is no regeneration, not an absent record (an empty column stays absent).
            let health_regen_per_second_per_10k = if columns[12].trim() == "-" {
                Some(0)
            } else {
                parse_optional_decimal_scaled_i32_927(columns[12], 10_000).map_err(|error| {
                    format!("unit {rawcode:#010x} has invalid health regeneration: {error}")
                })?
            };
            let collision_world = columns[21]
                .parse::<i32>()
                .map_err(|_| format!("unit {rawcode:#010x} has invalid collision radius"))?;
            let acquisition_range = parse_optional_i32_927(columns[22])?.map(world);
            let projectile_speed_per_tick = parse_optional_i32_927(columns[45])?
                .or_else(|| matches!(columns[28], "mbounce" | "msplash").then_some(0))
                .map(projectile_speed);
            let outer_splash_radius = parse_optional_i32_927(columns[41])?.map(world);
            let splash_falloff = match (
                parse_optional_i32_927(columns[39])?,
                parse_optional_i32_927(columns[40])?,
                parse_optional_i32_927(columns[41])?,
            ) {
                (Some(full), Some(medium), Some(outer)) if outer > 0 => {
                    let medium_factor = parse_optional_decimal_scaled_i32_927(columns[42], 10_000)?
                        .ok_or_else(|| {
                            format!("unit {rawcode:#010x} lacks medium splash factor")
                        })?;
                    let outer_factor = parse_optional_decimal_scaled_i32_927(columns[43], 10_000)?
                        .ok_or_else(|| format!("unit {rawcode:#010x} lacks outer splash factor"))?;
                    if !(0 <= full && full <= medium && medium <= outer)
                        || !(0..=10_000).contains(&medium_factor)
                        || !(0..=10_000).contains(&outer_factor)
                    {
                        return Err(format!("unit {rawcode:#010x} has invalid splash bands"));
                    }
                    Some(SplashFalloffProfile {
                        full_radius: world(full),
                        medium_radius: world(medium),
                        outer_radius: world(outer),
                        medium_damage_per_10k: u16::try_from(medium_factor)
                            .expect("validated splash factor"),
                        outer_damage_per_10k: u16::try_from(outer_factor)
                            .expect("validated splash factor"),
                        targets: parse_attack_targets_927(columns[44]),
                    })
                }
                _ => None,
            };
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
            let classifications = columns[24].split(',').map(str::trim).collect::<Vec<_>>();
            let mechanical = classifications
                .iter()
                .any(|classification| classification.eq_ignore_ascii_case("mechanical"));
            let sapper = classifications
                .iter()
                .any(|classification| classification.eq_ignore_ascii_case("sapper"));
            let undead = classifications
                .iter()
                .any(|classification| classification.eq_ignore_ascii_case("undead"));
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
            let mana_regen =
                parse_optional_decimal_scaled_i32_927(columns[16], 10_000)?.unwrap_or(0);
            // Damage Charge owns a separate signed native mana controller, including drain.
            // It must not also acquire a second ordinary ManaState pool.
            let charge_controls_mana = crate::native_effects::uses_damage_charge_mana_for_version(
                MapVersion::CASTLE_FIGHT_9_27,
                rawcode,
            );
            let row = ExtractedUnit927 {
                native_health: parse_optional_i32_927(columns[11])?,
                native_armor: parse_optional_i32_927(columns[17])?.map(|amount| ArmorProfile {
                    armor_type: parse_armor_type_927(columns[18]).expect("native armor type"),
                    armor_points: amount.try_into().expect("native armor amount"),
                }),
                mana_maximum: parse_optional_i32_927(columns[14])?.unwrap_or(0),
                native_mana: if !charge_controls_mana
                    && parse_optional_i32_927(columns[14])?.unwrap_or(0) > 0
                {
                    Some(ManaProfile::per_second(
                        parse_optional_i32_927(columns[14])?.unwrap_or(0),
                        parse_optional_i32_927(columns[15])?.unwrap_or(0),
                        u32::try_from(mana_regen).map_err(|_| {
                            format!("object {rawcode:#010x} has negative native mana regeneration")
                        })?,
                    ))
                } else {
                    None
                },
                action_timing: action_timings.get(&rawcode).copied().ok_or_else(|| {
                    format!("unit {rawcode:#010x} is missing native action timing")
                })?,
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
                target_classifications: UnitClassifications {
                    locust: abilities.contains(&u32::from_be_bytes(*b"Aloc")),
                    ancient: classifications
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case("ancient")),
                    resistant_skin: crate::native_effects::resistant_skin_for_version(
                        MapVersion::CASTLE_FIGHT_9_27,
                        &abilities,
                    ),
                    neutral_hostile_creep_level: None,
                    melee_attacker: attack1_enabled && columns[28] == "normal",
                    hero: columns[2]
                        .as_bytes()
                        .first()
                        .is_some_and(u8::is_ascii_uppercase),
                    summoned: classifications
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case("summoned")),
                    spell_immune: false,
                    combat_sapper: sapper,
                    invulnerable: columns[23].split(',').any(|ability| ability == "Avul"),
                    legendary: abilities.contains(
                        &crate::golden_shrine_definition_for_version(MapVersion::CASTLE_FIGHT_9_27)
                            .expect("9.27 shrine")
                            .parameters
                            .exclude_legendary_marker_ability_id,
                    ),
                    summoned_marker: abilities.contains(
                        &crate::golden_shrine_definition_for_version(MapVersion::CASTLE_FIGHT_9_27)
                            .expect("9.27 shrine")
                            .parameters
                            .exclude_summoned_unit_marker_ability_id,
                    ),
                    illusion: false,
                    invisible: false,
                },
                sapper,
                undead,
                collision_radius: CollisionRadius(world(collision_world)),
                acquisition_range,
                projectile_speed_per_tick,
                outer_splash_radius,
                splash_falloff,
                attack1_damage_type,
                attack1_damage: parse_optional_decimal_rounded_i32_927(columns[36])?,
                attack1_cooldown_ticks: parse_optional_seconds_to_ticks_u16_927(columns[30])?,
                attack1_range: parse_native_weapon_range_927(columns[29])?.map(world),
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
            let attack2_damage = parse_optional_decimal_rounded_i32_927(columns[66])?;
            let attack2_cooldown_ticks = parse_optional_seconds_to_ticks_u16_927(columns[58])?;
            let attack2_range = parse_optional_i32_927(columns[62])?.map(world);
            let row = ExtractedProtectedStats927 {
                health,
                armor: ArmorProfile::new(parse_armor_type_927(columns[14])?, armor_points),
                move_speed_per_tick,
                attack1_damage,
                attack1_cooldown_ticks,
                attack1_range,
                attack2_damage,
                attack2_cooldown_ticks,
                attack2_range,
            };
            if protected_stats.insert(rawcode, row).is_some() {
                return Err(format!(
                    "duplicate protected unit stats for {rawcode:#010x}"
                ));
            }
        }

        let mut primary_attacks = BTreeMap::new();
        let mut secondary_attacks = BTreeMap::new();
        for line in PRODUCTION_UNIT_ATTACKS_927_TSV.lines().skip(1) {
            let columns = line.split('\t').collect::<Vec<_>>();
            if columns.len() <= 26 || columns[5] != "default" {
                continue;
            }
            let rawcode = parse_rawcode(columns[2]);
            let Some(range_world) = parse_optional_i32_927(columns[22])? else {
                if columns[4] == "2" {
                    continue;
                }
                return Err(format!(
                    "unit {rawcode:#010x} has invalid primary attack range"
                ));
            };
            let weapon_kind = parse_weapon_kind_927(columns[12])?;
            let row = ExtractedAttack927 {
                damage_type: parse_damage_type_927(columns[11])?,
                weapon_kind,
                targets: parse_attack_targets_927(columns[13]),
                damage: parse_report_average_damage_927(&columns)?,
                cooldown_ticks: parse_seconds_to_ticks_u16_927(columns[20])?,
                range: world(range_world),
            };
            let selected = match columns[4] {
                "1" => &mut primary_attacks,
                "2" => &mut secondary_attacks,
                _ => continue,
            };
            if selected.insert(rawcode, row).is_some() {
                return Err(format!(
                    "duplicate attack {} row for {rawcode:#010x}",
                    columns[4]
                ));
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
        let production_food_used = tsv_column_index_927(production_header, "food_used")?;
        let production_is_legendary = tsv_column_index_927(production_header, "is_legendary")?;
        let production_required_max = [
            production_building_rawcode,
            production_kind,
            production_unit_rawcode,
            production_spawn_time,
            production_food_used,
            production_is_legendary,
        ]
        .into_iter()
        .max()
        .expect("production field list must not be empty");

        let mut production = BTreeMap::new();
        let mut legendary_points_costs = BTreeMap::new();
        for line in PRODUCTION_BUILDINGS_927_TSV.lines().skip(1) {
            let columns = line.split('\t').collect::<Vec<_>>();
            if columns.len() <= production_required_max {
                return Err("9.27 production-buildings row is missing required columns".to_owned());
            }
            let building_rawcode = parse_rawcode(columns[production_building_rawcode]);
            let legendary_points_cost = if columns[production_is_legendary] == "1" {
                columns[production_food_used]
                    .parse::<u16>()
                    .map_err(|error| error.to_string())?
            } else {
                0
            };
            legendary_points_costs.insert(building_rawcode, legendary_points_cost);
            if columns[production_kind] != "production" {
                continue;
            }
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
            let decay_start_ticks = parse_seconds_to_ticks_u32_927(columns[10])?;
            let lifetime_ticks = if does_decay && !columns[13].is_empty() {
                Some(parse_seconds_to_ticks_u32_927(columns[13])?)
            } else {
                None
            };
            corpses.insert(
                rawcode,
                ExtractedCorpse927 {
                    does_decay,
                    decay_start_ticks,
                    lifetime_ticks,
                },
            );
        }

        // The production report describes trained objects. A native summoner's actual body
        // consumes its own ordinary unit fields and protected weapon tuning instead.
        for handoff in handoffs {
            let rawcode = parse_rawcode(&handoff.body_rawcode);
            let unit = &units[&rawcode];
            let protected = &protected_stats[&rawcode];
            primary_attacks.insert(
                rawcode,
                ExtractedAttack927 {
                    damage_type: unit.attack1_damage_type.expect("handoff weapon type"),
                    weapon_kind: unit.attack1_weapon_kind.expect("handoff weapon delivery"),
                    targets: unit.attack1_targets.expect("handoff weapon mask"),
                    damage: protected.attack1_damage.or(unit.attack1_damage),
                    range: protected
                        .attack1_range
                        .or(unit.attack1_range)
                        .expect("handoff weapon range"),
                    cooldown_ticks: protected
                        .attack1_cooldown_ticks
                        .or(unit.attack1_cooldown_ticks)
                        .expect("handoff weapon cadence"),
                },
            );
            corpses.insert(
                rawcode,
                ExtractedCorpse927 {
                    does_decay: false,
                    decay_start_ticks: handoff.body_death_ticks,
                    lifetime_ticks: None,
                },
            );
        }

        // Native death-spawn children are outside the trained-unit report. Their own
        // retained unit fields/protected values supply weapons; the generated native
        // closure supplies corpse flags and versioned global decay defaults.
        let mut native_noncombat_summons = std::collections::BTreeSet::new();
        for body in native_summon_bodies {
            let rawcode = parse_rawcode(&body.rawcode);
            let unit = &units[&rawcode];
            let protected = protected_stats.get(&rawcode);
            if unit.attack1_damage_type.is_some() {
                primary_attacks
                    .entry(rawcode)
                    .or_insert_with(|| ExtractedAttack927 {
                        damage_type: unit.attack1_damage_type.expect("summon weapon type"),
                        weapon_kind: unit.attack1_weapon_kind.expect("summon weapon delivery"),
                        targets: unit.attack1_targets.expect("summon weapon mask"),
                        damage: protected
                            .and_then(|p| p.attack1_damage)
                            .or(unit.attack1_damage),
                        range: protected
                            .and_then(|p| p.attack1_range)
                            .or(unit.attack1_range)
                            .expect("summon weapon range"),
                        cooldown_ticks: protected
                            .and_then(|p| p.attack1_cooldown_ticks)
                            .or(unit.attack1_cooldown_ticks)
                            .expect("summon weapon cadence"),
                    });
            }
            if unit.attack1_damage_type.is_none() {
                native_noncombat_summons.insert(rawcode);
            }
            corpses.entry(rawcode).or_insert(ExtractedCorpse927 {
                does_decay: body.does_decay,
                decay_start_ticks: body.decay_start_ticks,
                lifetime_ticks: body.lifetime_ticks,
            });
        }

        // The default-combat report omits campaign units. Fill absent enabled native
        // weapons from the same retained unit metadata and protected overlays; never
        // replace a report row or infer a weapon for an unarmed object.
        for (&rawcode, unit) in &units {
            if let Some(attack) = native_primary_attack_927(unit, protected_stats.get(&rawcode)) {
                primary_attacks.entry(rawcode).or_insert(attack);
            }
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
            native_noncombat_summons,
            buildings,
            units,
            unit_abilities,
            protected_stats,
            primary_attacks,
            homing_splash_rawcodes,
            air_attack_bonuses,
            secondary_attacks,
            building_secondary_attacks,
            production,
            production_handoffs,
            legendary_points_costs,
            corpses,
            repair_time_ticks,
            command_card_positions,
            authored_button_components,
            building_hotkeys,
            upgrades,
            income_semantics,
            income_per_10k,
            builder_catalogs,
            authored_builder_catalogs,
            bounce_weapons,
            line_weapons,
            secondary_artillery_weapons,
            damage_rules,
        })
    }
}

fn catalog_supplement_927() -> Result<CatalogSupplement927, String> {
    let supplement: CatalogSupplement927 = serde_json::from_str(CATALOG_SUPPLEMENT_927_R1_JSON)
        .map_err(|error| format!("invalid 9.27 catalog supplement: {error}"))?;
    if supplement.schema_version != 11 {
        return Err(format!(
            "unsupported 9.27 catalog supplement schema {}",
            supplement.schema_version
        ));
    }
    if supplement.simulation_hz != CASTLE_FIGHT_SIMULATION_HZ {
        return Err(format!(
            "9.27 native action timing uses {} Hz, engine uses {} Hz",
            supplement.simulation_hz, CASTLE_FIGHT_SIMULATION_HZ
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
        // Object-authored build lists take precedence over race registration, which also
        // contains upgrades and perk-only verification entries. Neither list is hand-maintained.
        build_catalog: content
            .authored_builder_catalogs
            .get(builder_rawcode)
            .unwrap_or(&catalog.direct_buildings)
            .clone(),
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

pub(crate) fn vessel_corpse_qualifies_927(definition: CorpseDefinitionId) -> bool {
    extracted_content_927()
        .units
        .get(&definition.0)
        .is_some_and(|unit| unit.sapper && !unit.undead)
}

pub(crate) fn unit_has_ability_for_version(
    rawcode: u32,
    ability: AbilityId,
    version: MapVersion,
) -> bool {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    unit_has_ability_927(rawcode, ability)
}

pub(crate) fn unit_has_ability_927(rawcode: u32, ability: AbilityId) -> bool {
    extracted_content_927()
        .unit_abilities
        .get(&rawcode)
        .is_some_and(|abilities| abilities.contains(&ability.0))
}

pub(crate) fn delayed_resurrection_ability_for_version(
    version: MapVersion,
    parent: AbilityId,
) -> Option<AbilityId> {
    (version == MapVersion::CASTLE_FIGHT_9_27 && parent.0 == u32::from_be_bytes(*b"A03K"))
        .then_some(AbilityId(u32::from_be_bytes(*b"A03H")))
}

fn extracted_building_economy_927(rawcode: u32) -> BuildingEconomyProfile {
    let (gold_cost, lumber_cost) = extracted_building_costs_927(rawcode);
    if rawcode == u32::from_be_bytes(*b"h008") {
        return BuildingEconomyProfile {
            gold_cost: u32::from(gold_cost),
            lumber_cost: u32::from(lumber_cost),
            lumber_refund: 0,
            legendary_points_cost: 0,
            // Treasure Box multiplies the owner's aggregate raw income instead of contributing
            // a standalone income term.
            income_per_10k: 0,
        };
    }
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
        legendary_points_cost: extracted_content_927()
            .legendary_points_costs
            .get(&rawcode)
            .copied()
            .unwrap_or(0),
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

/// The report's average damage. A literal SLK dash for the base damage is the native integer
/// zero, which leaves the report's average blank; the weapon then deals its dice alone.
fn parse_report_average_damage_927(columns: &[&str]) -> Result<Option<i32>, String> {
    if let Some(damage) = parse_optional_decimal_rounded_i32_927(columns[26])? {
        return Ok(Some(damage));
    }
    if columns[14].trim() != "-" {
        return Ok(None);
    }
    let dice = columns[16].trim().parse::<f64>().ok();
    let sides = columns[18].trim().parse::<f64>().ok();
    Ok(dice
        .zip(sides)
        .map(|(dice, sides)| (dice * (sides + 1.0) / 2.0).round() as i32))
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

// A literal SLK dash is the native integer zero, unlike a missing catalog cell
// or the protection sentinels. Keep it for weapons enabled over an unarmed base.
fn parse_native_weapon_range_927(value: &str) -> Result<Option<i32>, String> {
    if value.trim() == "-" {
        Ok(Some(0))
    } else {
        parse_optional_i32_927(value)
    }
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
    let protected = content.protected_stats.get(&rawcode);
    let attack = unit.attack1_weapon_kind.map(|weapon_kind| {
    let delivery = match weapon_kind {
        ExtractedWeaponKind927::Melee => AttackDelivery::Melee,
        ExtractedWeaponKind927::Instant => AttackDelivery::RangedInstant,
        ExtractedWeaponKind927::Missile => AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: unit
                .projectile_speed_per_tick
                .unwrap_or_else(|| panic!("tower {rawcode:#010x} is missing projectile speed")),
        },
        ExtractedWeaponKind927::Splash if content.homing_splash_rawcodes.contains(&rawcode) => {
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: unit.projectile_speed_per_tick.expect("native homing splash speed"),
            }
        }
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
        ExtractedWeaponKind927::Line => {
            let line = content
                .line_weapons
                .get(&rawcode)
                .unwrap_or_else(|| panic!("tower {rawcode:#010x} needs retained line weapon evidence"));
            AttackDelivery::Line {
                speed_per_tick: unit
                    .projectile_speed_per_tick
                    .unwrap_or_else(|| panic!("tower {rawcode:#010x} is missing line missile speed")),
                minimum_range: world(line.minimum_range_world),
                spill_distance: world(line.spill_distance_world),
                spill_radius: world(line.spill_radius_world),
                damage_retention_per_10k: line.damage_retention_per_10k,
                spill_targets: parse_attack_targets_927(&line.splash_targets),
            }
        }
        ExtractedWeaponKind927::Bounce => panic!(
            "tower {rawcode:#010x} uses an extracted weapon primitive that is not implemented in the current native slice"
        ),
    };
    AttackProfile {
        delivery,
        damage: protected
            .and_then(|stats| stats.attack1_damage)
            .or(unit.attack1_damage)
            .unwrap_or_else(|| panic!("tower {rawcode:#010x} is missing effective attack damage")),
        range: protected
            .and_then(|stats| stats.attack1_range)
            .or(unit.attack1_range)
            .unwrap_or_else(|| panic!("tower {rawcode:#010x} is missing effective attack range")),
        acquisition_range: unit
            .acquisition_range
            .unwrap_or_else(|| panic!("tower {rawcode:#010x} is missing acquisition range")),
        cooldown_ticks: protected.and_then(|stats| stats.attack1_cooldown_ticks).or(unit.attack1_cooldown_ticks).unwrap_or_else(|| {
            panic!("tower {rawcode:#010x} is missing effective attack cooldown")
        }),
    }
    });
    let command_card_position = content
        .building_command_card_position(rawcode)
        .unwrap_or_else(|| panic!("tower {rawcode:#010x} missing command-card position"));
    let hotkey = *content
        .building_hotkeys
        .get(&rawcode)
        .unwrap_or_else(|| panic!("tower {rawcode:#010x} missing build hotkey"));

    let abilities = &content.unit_abilities[&rawcode];
    let mechanics = native_unit_mechanics_for(MapVersion::CASTLE_FIGHT_9_27, rawcode, abilities)
        .expect("tower inventory uses retained native tuning");
    let mut passive_effects = mechanics.passive_effects.iter().collect::<Vec<_>>();
    if let Some(profile) = unit.splash_falloff {
        passive_effects.push(PassiveUnitEffect::SplashFalloff(profile));
    }
    let secondary_attack = content
        .building_secondary_attacks
        .get(&rawcode)
        .map(|second| {
            let primary = attack.expect("secondary weapon requires a primary weapon");
            let delivery = match parse_weapon_kind_927(&second.weapon_type)
                .expect("building secondary weapon kind")
            {
                ExtractedWeaponKind927::Instant => AttackDelivery::RangedInstant,
                ExtractedWeaponKind927::Missile => AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: movement(
                        parse_optional_i32_927(&second.projectile_speed_world_per_second)
                            .expect("secondary missile speed field")
                            .expect("secondary missile speed"),
                    )
                    .speed_per_tick,
                },
                other => panic!("tower {rawcode:#010x} has unsupported secondary weapon {other:?}"),
            };
            SecondaryAttackProfile {
                primary_targets: unit.attack1_targets.expect("primary target mask"),
                attack: AttackProfile {
                    delivery,
                    damage: protected
                        .and_then(|stats| stats.attack2_damage)
                        .or(parse_optional_decimal_rounded_i32_927(&second.damage)
                            .expect("secondary damage field"))
                        .expect("secondary damage"),
                    range: protected
                        .and_then(|stats| stats.attack2_range)
                        .unwrap_or_else(|| {
                            world(
                                parse_optional_i32_927(&second.range)
                                    .expect("secondary range field")
                                    .expect("secondary range"),
                            )
                        }),
                    acquisition_range: primary.acquisition_range,
                    cooldown_ticks: protected
                        .and_then(|stats| stats.attack2_cooldown_ticks)
                        .unwrap_or_else(|| {
                            parse_seconds_to_ticks_u16_927(&second.cooldown)
                                .expect("secondary cooldown")
                        }),
                },
                targets: parse_attack_targets_927(&second.targets),
                damage_type: parse_damage_type_927(&second.damage_type)
                    .expect("secondary damage type"),
            }
        });
    CastleFightTowerDefinition {
        action_timing: unit.action_timing,
        secondary_attack,
        passive_effects: PassiveUnitEffects::from_slice(&passive_effects),
        rawcode,
        name: building.name,
        basic_tooltip: building.basic_tooltip,
        extended_tooltip: building.extended_tooltip,
        gold_cost: building.gold_cost,
        lumber_cost: building.lumber_cost,
        economy: extracted_building_economy_927(rawcode),
        health: protected.map_or(building.health, |stats| stats.health),
        classifications: unit.target_classifications,
        health_regen_per_second_per_10k: u32::try_from(
            unit.health_regen_per_second_per_10k.unwrap_or(0),
        )
        .expect("building health regeneration must be nonnegative"),
        construction_time_ticks: building.construction_time_ticks,
        repair_time_ticks: unit
            .repair_time_ticks
            .unwrap_or_else(|| panic!("tower {rawcode:#010x} is missing retained repair time")),
        armor: protected.map_or(building.armor, |stats| stats.armor),
        damage_type: unit.attack1_damage_type.unwrap_or(DamageType::Normal),
        attack_targets: unit.attack1_targets.unwrap_or(AttackTargetMask::ALL),
        footprint_size_cells: building.footprint_size_cells.unwrap_or_else(|| {
            panic!("tower {rawcode:#010x} has no square retained 9.27 footprint")
        }),
        command_card_position,
        hotkey,
        attack,
        spellcasting: mechanics.spellcasting,
        map_version: MapVersion::CASTLE_FIGHT_9_27,
    }
}

fn native_primary_attack_927(
    unit: &ExtractedUnit927,
    protected: Option<&ExtractedProtectedStats927>,
) -> Option<ExtractedAttack927> {
    Some(ExtractedAttack927 {
        damage_type: unit.attack1_damage_type?,
        weapon_kind: unit.attack1_weapon_kind?,
        targets: unit.attack1_targets?,
        damage: protected
            .and_then(|p| p.attack1_damage)
            .or(unit.attack1_damage),
        range: protected
            .and_then(|p| p.attack1_range)
            .or(unit.attack1_range)?,
        cooldown_ticks: protected
            .and_then(|p| p.attack1_cooldown_ticks)
            .or(unit.attack1_cooldown_ticks)?,
    })
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
    // Unit entities require an AttackProfile even when the native object has no weapon.
    // An empty target mask and zero damage make this engine carrier inert; none of
    // these placeholder fields are imported as a Warcraft weapon or tuning value.
    let inert = ExtractedAttack927 {
        damage_type: DamageType::Normal,
        weapon_kind: ExtractedWeaponKind927::Melee,
        targets: AttackTargetMask::from_capabilities(false, false, false),
        damage: Some(0),
        range: 0,
        cooldown_ticks: 1,
    };
    let attack = content.primary_attacks.get(&rawcode).unwrap_or_else(|| {
        assert!(
            unit.attack1_weapon_kind.is_none(),
            "armed object missing primary weapon"
        );
        &inert
    });
    assert_eq!(unit.name, expected_name, "unit name changed in extraction");
    let acquisition_range = unit.acquisition_range.unwrap_or_else(|| {
        assert!(
            unit.attack1_weapon_kind.is_none(),
            "armed object missing acquisition range"
        );
        0
    });
    let delivery = match attack.weapon_kind {
        ExtractedWeaponKind927::Melee => AttackDelivery::Melee,
        ExtractedWeaponKind927::Instant => AttackDelivery::RangedInstant,
        ExtractedWeaponKind927::Missile => AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: unit.projectile_speed_per_tick.unwrap_or_else(|| {
                panic!("missile unit {rawcode:#010x} is missing projectile speed")
            }),
        },
        ExtractedWeaponKind927::Splash if content.homing_splash_rawcodes.contains(&rawcode) => {
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: unit
                    .projectile_speed_per_tick
                    .expect("native homing splash speed"),
            }
        }
        ExtractedWeaponKind927::Artillery | ExtractedWeaponKind927::Splash => {
            AttackDelivery::RangedBallistic {
                speed_per_tick: unit.projectile_speed_per_tick.unwrap_or_else(|| {
                    panic!("ballistic unit {rawcode:#010x} is missing projectile speed")
                }),
                impact_radius: unit
                    .outer_splash_radius
                    .or_else(|| (attack.weapon_kind == ExtractedWeaponKind927::Splash).then_some(0))
                    .unwrap_or_else(|| {
                        panic!("ballistic unit {rawcode:#010x} is missing splash radius")
                    }),
            }
        }
        ExtractedWeaponKind927::Bounce => {
            let bounce = content
                .bounce_weapons
                .get(&rawcode)
                .expect("bounce unit needs retained weapon fields");
            AttackDelivery::Bounce {
                speed_per_tick: unit
                    .projectile_speed_per_tick
                    .expect("bounce missile speed"),
                bounce_range: world(bounce.range_world),
                max_bounces: bounce
                    .maximum_targets
                    .checked_sub(1)
                    .expect("bounce target count"),
                damage_percent_per_bounce: bounce.damage_percent_per_bounce,
                allow_repeat_targets: false,
            }
        }
        ExtractedWeaponKind927::Line => {
            let line = content
                .line_weapons
                .get(&rawcode)
                .expect("line weapon evidence");
            AttackDelivery::Line {
                speed_per_tick: unit.projectile_speed_per_tick.expect("line missile speed"),
                minimum_range: world(line.minimum_range_world),
                spill_distance: world(line.spill_distance_world),
                spill_radius: world(line.spill_radius_world),
                damage_retention_per_10k: line.damage_retention_per_10k,
                spill_targets: parse_attack_targets_927(&line.splash_targets),
            }
        }
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
            decay_start_ticks: corpse.decay_start_ticks,
            lifetime_ticks: corpse.lifetime_ticks,
        })
    } else {
        None
    };
    let secondary_attack = content.secondary_attacks.get(&rawcode).map(|second| {
        let delivery = match second.weapon_kind {
            ExtractedWeaponKind927::Instant => AttackDelivery::RangedInstant,
            ExtractedWeaponKind927::Missile => AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: unit
                    .projectile_speed_per_tick
                    .expect("secondary missile speed"),
            },
            ExtractedWeaponKind927::Artillery => {
                let weapon = content
                    .secondary_artillery_weapons
                    .get(&rawcode)
                    .unwrap_or_else(|| {
                        panic!("unit {rawcode:#010x} secondary artillery lacks retained splash")
                    });
                AttackDelivery::RangedBallistic {
                    speed_per_tick: projectile_speed(weapon.projectile_speed_world_per_second),
                    impact_radius: world(weapon.outer_world),
                }
            }
            other => panic!("unit {rawcode:#010x} has unsupported secondary weapon {other:?}"),
        };
        SecondaryAttackProfile {
            primary_targets: attack.targets,
            attack: AttackProfile {
                delivery,
                damage: protected
                    .attack2_damage
                    .or(second.damage)
                    .expect("secondary attack damage"),
                range: protected.attack2_range.unwrap_or(second.range),
                acquisition_range,
                cooldown_ticks: protected
                    .attack2_cooldown_ticks
                    .unwrap_or(second.cooldown_ticks),
            },
            targets: second.targets,
            damage_type: second.damage_type,
        }
    });

    CastleFightUnitDefinition {
        action_timing: unit.action_timing,
        map_version: MapVersion::CASTLE_FIGHT_9_27,
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
        additional_abilities: None,
        damage_type: attack.damage_type,
        attack_targets: attack.targets,
        secondary_attack,
        movement_class: unit
            .movement_class
            .unwrap_or_else(|| panic!("unit {rawcode:#010x} is missing retained movement class")),
        mechanical: unit.mechanical,
        classifications: unit.target_classifications,
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
        .get(&production.unit_rawcode)
        .copied()
        .unwrap_or(CommandCardPosition::new(0, 0));
    let train_hotkey = content
        .building_hotkeys
        .get(&production.unit_rawcode)
        .copied();
    let train_tooltips = content
        .units
        .get(&production.unit_rawcode)
        .expect("produced unit must exist in retained unit table");
    assert_eq!(
        content
            .production_handoffs
            .get(&production.unit_rawcode)
            .copied()
            .unwrap_or(production.unit_rawcode),
        produced_unit.rawcode,
        "production link changed in retained 9.27 extraction"
    );
    let building_unit = content
        .units
        .get(&rawcode)
        .unwrap_or_else(|| panic!("building {rawcode:#010x} missing retained 9.27 unit metadata"));
    let command_card_position = content
        .building_command_card_position(rawcode)
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
        classifications: building_unit.target_classifications,
        health_regen_per_second_per_10k: u32::try_from(
            building_unit.health_regen_per_second_per_10k.unwrap_or(0),
        )
        .expect("building health regeneration must be nonnegative"),
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

/// Native object mana remains available during construction and upgrades.
pub(crate) fn building_native_mana_maximum(version: MapVersion, rawcode: u32) -> Option<i32> {
    native_mana_maximum_for_version(version, rawcode)
}

pub(crate) fn native_mana_maximum_for_version(version: MapVersion, rawcode: u32) -> Option<i32> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return None;
    }
    extracted_content_927()
        .units
        .get(&rawcode)
        .map(|unit| unit.mana_maximum)
}

pub(crate) fn building_native_mana_profile(
    version: MapVersion,
    rawcode: u32,
) -> Option<ManaProfile> {
    native_mana_profile_for_version(version, rawcode)
}

pub(crate) fn native_mana_profile_for_version(
    version: MapVersion,
    rawcode: u32,
) -> Option<ManaProfile> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return None;
    }
    extracted_content_927()
        .units
        .get(&rawcode)
        .and_then(|unit| unit.native_mana)
}

pub(crate) fn building_has_production(version: MapVersion, rawcode: u32) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && extracted_content_927().production.contains_key(&rawcode)
}

/// Whether a native death-spawn body is one of the unarmed originals this primitive can build.
/// Armed vanilla bodies (the inert building Parasite parents' unmodified base) are not modeled.
pub(crate) fn is_native_noncombat_summon(version: MapVersion, rawcode: u32) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && extracted_content_927()
            .native_noncombat_summons
            .contains(&rawcode)
}

/// Original noncombat native summon bodies consume retained unit fields, without setupUnit.
pub(crate) fn native_noncombat_summon_for_version(
    rawcode: u32,
    version: MapVersion,
) -> ResolvedUnitDefinition {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    let content = extracted_content_927();
    assert!(content.native_noncombat_summons.contains(&rawcode));
    let unit = &content.units[&rawcode];
    assert!(unit.attack1_damage_type.is_none() && !unit.sapper);
    let corpse = content
        .corpses
        .get(&rawcode)
        .filter(|c| c.does_decay)
        .map(|c| CorpseProfile {
            definition: CorpseDefinitionId(rawcode),
            decay_start_ticks: c.decay_start_ticks,
            lifetime_ticks: c.lifetime_ticks,
        });
    ResolvedUnitDefinition {
        template: UnitTemplate {
            health: unit.native_health.expect("native body health"),
            // Inert engine weapon slot; the retained object has both weapons disabled.
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
            },
            movement: MovementProfile {
                speed_per_tick: unit.move_speed_per_tick.expect("native body movement"),
            },
        },
        properties: UnitGameplayProperties {
            content: Some(ContentIdentity {
                map_version: version,
                rawcode,
                name: unit.name,
            }),
            action_timing: unit.action_timing,
            armor: unit.native_armor.expect("native body armor"),
            health_regen_per_second_per_10k: unit
                .health_regen_per_second_per_10k
                .unwrap_or(0)
                .try_into()
                .unwrap(),
            classifications: UnitClassifications {
                summoned: true,
                ..unit.target_classifications
            },
            mechanical: unit.mechanical,
            movement_class: unit.movement_class.expect("native body movement class"),
            collision_radius: Some(unit.collision_radius),
            corpse,
            attack_targets: AttackTargetMask::from_capabilities(false, false, false),
            ..Default::default()
        },
        spellcasting: None,
        additional_abilities: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_report_weapons_use_native_fields_and_protected_overlays_without_arming_disabled_objects()
     {
        let content = extracted_content_927();
        let code = CastleFightUnitKind::Footman.definition().rawcode;
        let mut unit = content.units[&code];
        let mut p = content.protected_stats[&code];
        unit.attack1_damage = Some(3);
        unit.attack1_range = Some(17);
        unit.attack1_cooldown_ticks = Some(8);
        p.attack1_damage = Some(13);
        p.attack1_range = Some(29);
        p.attack1_cooldown_ticks = Some(5);
        let weapon = native_primary_attack_927(&unit, Some(&p)).unwrap();
        assert_eq!(
            (weapon.damage, weapon.range, weapon.cooldown_ticks),
            (Some(13), 29, 5)
        );
        let weapon = native_primary_attack_927(&unit, None).unwrap();
        assert_eq!(
            (weapon.damage, weapon.range, weapon.cooldown_ticks),
            (Some(3), 17, 8)
        );
        unit.attack1_range = parse_native_weapon_range_927("-").unwrap();
        assert_eq!(native_primary_attack_927(&unit, None).unwrap().range, 0);
        assert_eq!(parse_native_weapon_range_927("").unwrap(), None);
        assert_eq!(parse_native_weapon_range_927("2147483647").unwrap(), None);
        assert!(parse_native_weapon_range_927("not-a-number").is_err());
        unit.attack1_weapon_kind = None;
        assert!(native_primary_attack_927(&unit, Some(&p)).is_none());
    }

    #[test]
    fn training_hotkeys_preserve_authored_absence_across_the_complete_catalog() {
        let content = extracted_content_927();
        for kind in CastleFightProductionKind::ALL {
            let d = kind.definition();
            let trained = content.production[&d.rawcode].unit_rawcode;
            assert_eq!(
                d.train_hotkey,
                content.building_hotkeys.get(&trained).copied()
            );
        }
    }

    #[test]
    fn resolved_sight_covers_every_playable_source_and_is_version_scoped() {
        let version = MapVersion::CASTLE_FIGHT_9_27;
        let bundle = castle_fight_content_bundle(version).unwrap();
        let projection = catalog_supplement_927().unwrap();
        let sight: BTreeMap<_, _> = projection
            .sight_profiles
            .into_iter()
            .map(|p| {
                (
                    parse_rawcode(&p.rawcode),
                    crate::SightProfile {
                        day: p.day_world * SUBUNITS_PER_WORLD_UNIT,
                        night: p.night_world * SUBUNITS_PER_WORLD_UNIT,
                    },
                )
            })
            .collect();
        for rawcode in bundle
            .units
            .values()
            .map(|u| u.rawcode)
            .chain(
                bundle
                    .production_buildings
                    .values()
                    .flat_map(|b| [b.rawcode, b.produced_unit.rawcode]),
            )
            .chain(bundle.towers.values().map(|b| b.rawcode))
            .chain(bundle.builders.values().map(|b| b.rawcode))
            .chain([u32::from_be_bytes(*b"hcas")])
        {
            assert_eq!(
                castle_fight_sight_for_version(version, rawcode),
                sight.get(&rawcode).copied(),
                "{rawcode:08x}"
            );
            assert!(sight.contains_key(&rawcode));
        }
        let unsupported = MapVersion {
            major: u16::MAX,
            minor: u16::MAX,
        };
        assert!(
            castle_fight_sight_for_version(unsupported, u32::from_be_bytes(*b"hcas")).is_none()
        );
        assert!(castle_fight_fog_rules_for_version(unsupported).is_none());
        assert!(spell_reveal_for_version(unsupported, AbilityId(0)).is_none());
    }

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
    fn native_action_timing_is_source_owned_across_the_playable_catalog_and_hashed() {
        let supplement = catalog_supplement_927().unwrap();
        let timings: BTreeMap<_, _> = supplement
            .action_timings
            .into_iter()
            .map(|entry| (parse_rawcode(&entry.rawcode), entry.timing))
            .collect();
        let bundle = castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
        for unit in bundle.unit_definitions().chain(
            bundle
                .production_buildings
                .values()
                .map(|production| production.produced_unit),
        ) {
            assert_eq!(
                unit.action_timing, timings[&unit.rawcode],
                "{:08x}",
                unit.rawcode
            );
            assert_eq!(unit.gameplay_properties().action_timing, unit.action_timing);
            assert!(
                unit.action_timing.primary_attack_point_ticks
                    <= unit.action_timing.primary_attack_ticks
            );
            assert!(
                unit.action_timing.secondary_attack_point_ticks
                    <= unit.action_timing.secondary_attack_ticks
            );
            assert!(unit.action_timing.cast_point_ticks <= unit.action_timing.cast_ticks);
            let mut changed = unit;
            changed.action_timing.primary_attack_point_ticks += 1;
            let mut before = ContentHash64::new();
            let mut after = ContentHash64::new();
            hash_unit_definition(&mut before, unit);
            hash_unit_definition(&mut after, changed);
            assert_ne!(before.finish(), after.finish());
        }
        for tower in bundle.tower_definitions() {
            assert_eq!(tower.action_timing, timings[&tower.rawcode]);
            assert_eq!(
                tower.gameplay_properties().secondary_attack,
                tower.secondary_attack
            );
            assert_eq!(
                tower.secondary_attack.is_some(),
                extracted_content_927()
                    .building_secondary_attacks
                    .contains_key(&tower.rawcode)
            );
            if let Some(secondary) = tower.secondary_attack {
                assert_eq!(
                    tower.gameplay_properties().attack_targets,
                    tower.attack_targets.union(secondary.targets)
                );
                let mut changed = tower;
                changed.secondary_attack.as_mut().unwrap().attack.damage += 1;
                let mut before = ContentHash64::new();
                let mut after = ContentHash64::new();
                hash_tower_definition(&mut before, tower);
                hash_tower_definition(&mut after, changed);
                assert_ne!(before.finish(), after.finish());
            }
        }
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
    fn promoted_native_splash_weapons_keep_their_authored_homing_delivery() {
        let content = extracted_content_927();
        let mut homing = 0;
        for kind in CastleFightUnitKind::ALL {
            let definition = kind.definition();
            if !content
                .primary_attacks
                .get(&definition.rawcode)
                .is_some_and(|attack| attack.weapon_kind == ExtractedWeaponKind927::Splash)
            {
                continue;
            }
            if content.homing_splash_rawcodes.contains(&definition.rawcode) {
                assert!(matches!(
                    definition.attack.delivery,
                    AttackDelivery::RangedGuaranteedHit { .. }
                ));
                homing += 1;
            } else {
                assert!(matches!(
                    definition.attack.delivery,
                    AttackDelivery::RangedBallistic { .. }
                ));
            }
            assert_eq!(
                definition
                    .passive_effects
                    .iter()
                    .any(|p| matches!(p, PassiveUnitEffect::SplashFalloff(_))),
                content.units[&definition.rawcode].splash_falloff.is_some(),
            );
        }
        assert!(homing > 0);
    }

    #[test]
    fn promoted_line_weapons_and_production_consume_retained_projection() {
        let content = extracted_content_927();
        let mut promoted_lines = 0;
        for kind in CastleFightUnitKind::ALL {
            let definition = kind.definition();
            if !content
                .primary_attacks
                .get(&definition.rawcode)
                .is_some_and(|attack| attack.weapon_kind == ExtractedWeaponKind927::Line)
            {
                continue;
            }
            promoted_lines += 1;
            let evidence = &content.line_weapons[&definition.rawcode];
            assert_eq!(
                definition.attack.delivery,
                AttackDelivery::Line {
                    speed_per_tick: content.units[&definition.rawcode]
                        .projectile_speed_per_tick
                        .unwrap(),
                    minimum_range: world(evidence.minimum_range_world),
                    spill_distance: world(evidence.spill_distance_world),
                    spill_radius: world(evidence.spill_radius_world),
                    damage_retention_per_10k: evidence.damage_retention_per_10k,
                    spill_targets: parse_attack_targets_927(&evidence.splash_targets),
                }
            );
        }
        assert!(promoted_lines > 0);
        for kind in CastleFightProductionKind::ALL {
            let building = kind.definition();
            let spawn = building.spawn(Team(0), BuildingFootprint::new(0, 0, 4, 4));
            let produced = CastleFightUnitKind::from_retained_rawcode_9_27(
                content
                    .production_handoffs
                    .get(&content.production[&building.rawcode].unit_rawcode)
                    .copied()
                    .unwrap_or(content.production[&building.rawcode].unit_rawcode),
            )
            .unwrap()
            .definition()
            .resolved();
            let properties = building.gameplay_properties();
            assert_eq!(spawn.production.unwrap().unit, produced.template);
            assert_eq!(properties.production_unit, produced.properties);
            assert_eq!(properties.production_spellcasting, produced.spellcasting);
        }
    }

    #[test]
    fn every_registered_building_consumes_its_own_retained_classifications() {
        let bundle = castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
        let content = extracted_content_927();
        for definition in bundle.production_building_definitions() {
            assert_eq!(
                definition.classifications,
                content.units[&definition.rawcode].target_classifications
            );
            assert_eq!(
                definition.gameplay_properties().classifications,
                definition.classifications
            );
        }
        for definition in bundle.tower_definitions() {
            assert_eq!(
                definition.classifications,
                content.units[&definition.rawcode].target_classifications
            );
            assert_eq!(
                definition.gameplay_properties().classifications,
                definition.classifications
            );
        }
        assert_eq!(
            bundle.main_castle_classifications,
            content.units[&u32::from_be_bytes(*b"hcas")].target_classifications
        );
    }

    #[test]
    fn imported_corpse_creation_is_independent_of_mechanical_classification() {
        let content = extracted_content_927();
        let mut mechanical_remains = 0;
        for kind in CastleFightUnitKind::ALL {
            let definition = kind.definition();
            let evidence = content.corpses[&definition.rawcode];
            assert_eq!(definition.corpse.is_some(), evidence.does_decay);
            if let Some(profile) = definition.corpse {
                assert_eq!(profile.definition.0, definition.rawcode);
                assert_eq!(profile.decay_start_ticks, evidence.decay_start_ticks);
                assert_eq!(profile.lifetime_ticks, evidence.lifetime_ticks);
                mechanical_remains += usize::from(definition.mechanical);
            }
        }
        assert!(mechanical_remains > 0);
    }

    #[test]
    fn imported_corpse_decay_start_uses_wc3_death_time() {
        assert_eq!(
            CastleFightUnitKind::Warlock
                .definition()
                .corpse
                .expect("Warlock has a corpse")
                .decay_start_ticks,
            3 * CASTLE_FIGHT_SIMULATION_HZ as u32
        );
        assert_eq!(
            CastleFightUnitKind::HolyWarrior
                .definition()
                .corpse
                .expect("Holy Warrior has a corpse")
                .decay_start_ticks,
            153
        );
    }

    #[test]
    fn current_content_bundle_binds_the_requested_version_and_revision() {
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
        assert_ne!(bundle.identity.gameplay_hash, 0);
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
            }))
            .chain(
                extracted
                    .production_handoffs
                    .iter()
                    .filter_map(|(proxy, body)| {
                        bundle
                            .unit_definitions()
                            .any(|unit| unit.rawcode == *body)
                            .then_some(*proxy)
                    }),
            );

        for rawcode in playable_rawcodes {
            for source in crate::native_effects::script_granted_ability_sources_for(
                MapVersion::CASTLE_FIGHT_9_27,
                rawcode,
            ) {
                assert!(
                    resolved_sources.contains(&source),
                    "script-granted {source} was not resolved"
                );
            }
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
        let unpromoted = extracted
            .production
            .keys()
            .find(|&&rawcode| {
                CastleFightProductionKind::from_rawcode_for_version(
                    rawcode,
                    MapVersion::CASTLE_FIGHT_9_27,
                )
                .unwrap()
                .is_none()
            })
            .expect("retained production evidence extends beyond promoted races");
        assert!(extracted.buildings.contains_key(unpromoted));
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
                    legendary_points_cost: 0,
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
                    legendary_points_cost: 0,
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
                    legendary_points_cost: 0,
                    income_per_10k: 68_400,
                },
            ),
            (
                CastleFightProductionKind::IceTrollHut.definition().economy,
                BuildingEconomyProfile {
                    gold_cost: 175,
                    lumber_cost: 0,
                    lumber_refund: 175,
                    legendary_points_cost: 0,
                    income_per_10k: 35_000,
                },
            ),
            (
                CastleFightProductionKind::GryphonRock.definition().economy,
                BuildingEconomyProfile {
                    gold_cost: 250,
                    lumber_cost: 0,
                    lumber_refund: 250,
                    legendary_points_cost: 0,
                    income_per_10k: 50_000,
                },
            ),
            (
                CastleFightTowerKind::WatchTower.definition().economy,
                BuildingEconomyProfile {
                    gold_cost: 150,
                    lumber_cost: 300,
                    lumber_refund: 0,
                    legendary_points_cost: 0,
                    income_per_10k: 6_000,
                },
            ),
            (
                CastleFightTowerKind::PoofTower.definition().economy,
                BuildingEconomyProfile {
                    gold_cost: 230,
                    lumber_cost: 300,
                    lumber_refund: 0,
                    legendary_points_cost: 0,
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
    fn catalog_transfers_additional_definitions_and_hashes_their_parameters() {
        let extra = AutomaticAbilityProfile {
            id: AbilityId(1),
            mana_cost: 0,
            cooldown_ticks: 1,
            range: SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::RandomEnemyUnit,
            effect: AbilityEffect::Damage { amount: 1 },
        };
        let definitions =
            AdditionalAutomaticAbilityDefinitions::try_from_profiles([extra]).unwrap();
        for kind in CastleFightProductionKind::ALL {
            let mut production = kind.definition();
            if production.produced_unit.spellcasting.is_none() {
                production.produced_unit.spellcasting = Some(SpellcastingProfile {
                    mana: ManaProfile {
                        maximum: 10,
                        starting: 10,
                        regen_per_tick_per_10k: 0,
                    },
                    ability: AutomaticAbilityProfile {
                        id: AbilityId(2),
                        ..extra
                    },
                });
            }
            production.produced_unit.additional_abilities = Some(definitions);
            assert_eq!(
                production.produced_unit.resolved().additional_abilities,
                Some(definitions)
            );
            assert_eq!(
                production
                    .gameplay_properties()
                    .production_additional_abilities,
                Some(definitions)
            );
            let mut first = ContentHash64::new();
            hash_unit_definition(&mut first, production.produced_unit);
            production.produced_unit.additional_abilities = Some(
                AdditionalAutomaticAbilityDefinitions::try_from_profiles([
                    AutomaticAbilityProfile {
                        cooldown_ticks: 2,
                        ..extra
                    },
                ])
                .unwrap(),
            );
            let mut changed = ContentHash64::new();
            hash_unit_definition(&mut changed, production.produced_unit);
            assert_ne!(
                first.finish(),
                changed.finish(),
                "{kind:?}: future ability changes must affect content identity"
            );
        }
    }

    #[test]
    fn builder_profile_uses_extracted_927_movement_and_repair_timing() {
        assert_eq!(
            castle_fight_builder_profile(),
            BuilderProfile {
                speed_per_tick: 550 * SUBUNITS_PER_WORLD_UNIT / CASTLE_FIGHT_SIMULATION_HZ,
                order_queue_capacity: castle_fight_builder_profile().order_queue_capacity,
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
        for race in CastleFightBuilderRace::ALL {
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
            let content = extracted_content_927();
            let expected = content
                .authored_builder_catalogs
                .get(&definition.rawcode)
                .unwrap_or(
                    &content.builder_catalogs[&(race as u8, definition.rawcode)].direct_buildings,
                );
            assert_eq!(&definition.build_catalog, expected);
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

        for kind in CastleFightProductionKind::ALL {
            let (precursor, targets) =
                extracted_building_upgrade_links_927(kind.definition().rawcode);
            let promoted = |rawcode| {
                CastleFightProductionKind::from_rawcode_for_version(
                    rawcode,
                    MapVersion::CASTLE_FIGHT_9_27,
                )
                .unwrap()
            };
            assert_eq!(kind.upgrade_from(), precursor.and_then(promoted));
            assert_eq!(
                kind.upgrade_targets(),
                targets.into_iter().filter_map(promoted).collect::<Vec<_>>()
            );
        }

        let human = CastleFightBuilderRace::Human.definition();
        assert!(human.build_catalog.contains(&u32::from_be_bytes(*b"h000")));
        assert!(!human.build_catalog.contains(&u32::from_be_bytes(*b"h039")));

        assert_eq!(
            CastleFightTowerKind::TinyWatchTower
                .upgrade_targets_for_version(MapVersion::CASTLE_FIGHT_9_27)
                .unwrap(),
            vec![CastleFightTowerKind::TinyMultishotTower]
        );
        assert!(
            CastleFightTowerKind::TinyMultishotTower
                .upgrade_targets_for_version(MapVersion::CASTLE_FIGHT_9_27)
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn completed_races_resolve_every_source_building_and_upgrade() {
        let version = MapVersion::CASTLE_FIGHT_9_27;
        let bundle = castle_fight_content_bundle(version).unwrap();
        let source = extracted_content_927();
        for race in [
            CastleFightBuilderRace::Orc,
            CastleFightBuilderRace::Elemental,
            CastleFightBuilderRace::NightElf,
            CastleFightBuilderRace::Naga,
            CastleFightBuilderRace::Critter,
        ] {
            let builder = bundle.builder(race).unwrap();
            let registered =
                &source.builder_catalogs[&(race as u8, builder.rawcode)].direct_buildings;
            assert!(!registered.is_empty());
            for rawcode in registered {
                assert!(
                    bundle.building_kind_for_rawcode(*rawcode).is_some(),
                    "completed {race:?} omits source building {rawcode:#010x}"
                );
                let (_, upgrades) = extracted_building_upgrade_links_927(*rawcode);
                for upgrade in upgrades {
                    assert!(
                        bundle.building_kind_for_rawcode(upgrade).is_some(),
                        "completed {race:?} omits upgrade {upgrade:#010x}"
                    );
                }
            }
        }
    }

    #[test]
    fn human_direct_catalog_exposes_all_authored_927_build_slots() {
        let bundle = castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
        let direct = bundle.playable_human_direct_building_kinds();
        assert_eq!(direct.len(), 12);
        assert!(direct.contains(&CastleFightBuildingKind::Tower(
            CastleFightTowerKind::WatchTower
        )));
        assert!(direct.contains(&CastleFightBuildingKind::Tower(
            CastleFightTowerKind::TreasureBox
        )));
        assert!(direct.contains(&CastleFightBuildingKind::Tower(
            CastleFightTowerKind::HeroicShrine
        )));
        assert!(!direct.contains(&CastleFightBuildingKind::Tower(
            CastleFightTowerKind::TinyWatchTower
        )));
        assert!(!direct.contains(&CastleFightBuildingKind::Tower(
            CastleFightTowerKind::TinyMultishotTower
        )));
    }

    #[test]
    fn builder_selection_ids_and_direct_menus_are_source_owned_for_every_race() {
        let bundle = castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
        for builder in bundle.builder_definitions() {
            assert_eq!(
                bundle.builder_race_for_rawcode(builder.rawcode),
                Some(builder.race)
            );
            let expected = builder
                .build_catalog
                .iter()
                .filter_map(|rawcode| bundle.building_kind_for_rawcode(*rawcode))
                .filter(|kind| bundle.direct_building_kinds().contains(kind))
                .collect::<Vec<_>>();
            assert_eq!(
                bundle.direct_building_kinds_for_race(builder.race),
                expected
            );
        }
        assert_eq!(bundle.builder_race_for_rawcode(0), None);
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
                .any(|classification| classification.eq_ignore_ascii_case("mechanical"));
            assert_eq!(definition.mechanical, extracted_mechanical);
        }
    }

    #[test]
    fn all_promoted_building_regeneration_rates_consume_retained_unit_metadata() {
        let content = extracted_content_927();
        let properties = CastleFightProductionKind::ALL
            .iter()
            .map(|kind| kind.definition().gameplay_properties())
            .chain(
                CastleFightTowerKind::ALL
                    .iter()
                    .map(|kind| kind.definition().gameplay_properties()),
            );
        for properties in properties {
            let code = properties.content.unwrap().rawcode;
            let expected = content.units[&code]
                .health_regen_per_second_per_10k
                .unwrap_or(0);
            assert_eq!(
                properties.health_regen_per_second_per_10k,
                u32::try_from(expected).unwrap()
            );
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
            // Map overrides record only coordinates that differ from the stock origin.
            rows.iter()
                .find(|row| {
                    row[category_column] == "units"
                        && row[rawcode_column] == rawcode
                        && row[field_id_column] == field
                })
                .map_or(0, |row| parse_extracted_u8(row[recovered_column]))
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
        assert_eq!(layout.rescue_strike_hotkey, ability_hotkey("A005"));
    }

    #[test]
    fn promoted_towers_retain_native_body_and_ordinary_weapon_fields() {
        let source = extracted_content_927();
        for kind in CastleFightTowerKind::ALL {
            let definition = kind.definition();
            let code = definition.rawcode;
            let building = &source.buildings[&code];
            let unit = &source.units[&code];
            let protected = source.protected_stats.get(&code);
            assert_eq!(
                definition.health,
                protected.map_or(building.health, |p| p.health)
            );
            assert_eq!(
                definition.armor,
                protected.map_or(building.armor, |p| p.armor)
            );
            // Scripted bombardment replaces its native attack with the separately tested
            // artillery effect. Other towers retain their ordinary weapon unchanged.
            if definition.spellcasting.is_some_and(|p| {
                matches!(p.ability.effect, AbilityEffect::ArtilleryBombardment { .. })
            }) {
                assert!(definition.attack.is_none());
                continue;
            }
            let weapon = native_primary_attack_927(unit, protected);
            assert_eq!(
                definition.attack.is_some(),
                weapon.is_some(),
                "{}",
                definition.name
            );
            if let (Some(attack), Some(weapon)) = (definition.attack, weapon) {
                assert_eq!(
                    (Some(attack.damage), attack.range, attack.cooldown_ticks),
                    (weapon.damage, weapon.range, weapon.cooldown_ticks)
                );
                assert_eq!(definition.attack_targets, weapon.targets);
                assert_eq!(definition.damage_type, weapon.damage_type);
            }
        }
    }
}
