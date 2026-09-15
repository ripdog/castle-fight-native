use std::fmt;

use crate::{
    components::{
        AttackDelivery, AttackProfile, AttackTargetMask, BuilderConfiguration, BuilderLocomotion,
        BuilderProfile, BuilderSpawn, BuildingFootprint, BuildingGameplayProperties, BuildingSpawn,
        CollisionRadius, ContentIdentity, CorpseDefinitionId, CorpseProfile, MovementClass,
        MovementProfile, PassiveUnitEffects, ProductionProfile, SpellcastingProfile, Team,
        UnitGameplayProperties, UnitTemplate,
    },
    damage::{ArmorProfile, ArmorType, DamageRules, DamageType},
    economy::{BuildingEconomyProfile, EconomyRules, RESOURCE_FIXED_SCALE},
    math::SUBUNITS_PER_WORLD_UNIT,
    native_effects::native_unit_mechanics_for,
    version::MapVersion,
};

pub const CASTLE_FIGHT_SIMULATION_HZ: i32 = 30;
pub const CASTLE_FIGHT_DEFAULT_MAP_VERSION: MapVersion = MapVersion::CASTLE_FIGHT_9_27;
const CASTLE_FIGHT_BUILDING_FOOTPRINT_CELLS_927: u16 = 4;
const CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND: i32 = 550;
const CASTLE_FIGHT_CRITTER_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND: i32 = 190;
// The stock Warcraft Build command (`AHbu`) has no editable cast-range field; workers use the
// engine's 50-world-unit construction contact range, matching the stock Repair contact range.
const CASTLE_FIGHT_BUILDER_BUILD_RANGE_WORLD_UNITS: i32 = 50;
const CASTLE_FIGHT_BUILDER_REPAIR_RANGE_WORLD_UNITS: i32 = 50;
const CASTLE_FIGHT_BUILDER_REPAIR_AUTOCAST_RANGE_WORLD_UNITS: i32 = 500;
const CASTLE_FIGHT_BUILDER_BLINK_RANGE_WORLD_UNITS: i32 = 10_000;
const CASTLE_FIGHT_BUILDER_BLINK_BOUNDARY_INSET_WORLD_UNITS: i32 = 64;
const CASTLE_FIGHT_STANDARD_REPAIR_TIME_SECONDS: u16 = 70;
const CASTLE_FIGHT_MAIN_CASTLE_REPAIR_TIME_TICKS_927: u32 = 700 * CASTLE_FIGHT_SIMULATION_HZ as u32;
const CASTLE_FIGHT_BUILDER_REPAIR_TIME_RATIO_NUMERATOR: u16 = 3;
const CASTLE_FIGHT_BUILDER_REPAIR_TIME_RATIO_DENOMINATOR: u16 = 2;
const CASTLE_FIGHT_COLLISION_WORLD_UNITS: i32 = 16;
const CASTLE_FIGHT_STARTING_GOLD: u32 = 250;
const CASTLE_FIGHT_STARTING_LUMBER: u32 = 125;
const CASTLE_FIGHT_STARTING_LEGENDARY_POINTS: u16 = 1;
const CASTLE_FIGHT_BASE_INCOME_GOLD: u64 = 5;
const CASTLE_FIGHT_INCOME_INTERVAL_SECONDS: u32 = 10;
const CASTLE_FIGHT_INCOME_TAX_BRACKET_GOLD: u64 = 25;
const PRODUCTION_SPAWN_SEARCH_RADIUS_CELLS: u16 = 12;

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
        let race_index = self as u8;
        let metadata = BUILDER_RACE_METADATA_927[usize::from(race_index)];
        Ok(CastleFightBuilderDefinition {
            race: self,
            race_index,
            rawcode: metadata.rawcode,
            name: metadata.name,
            campaign_only: metadata.campaign_only,
            locomotion: metadata.locomotion,
            // 9.27 `udaa` resolves to Repair for every standard race builder. Campaign-only
            // Critter Builder still has Repair in its ability list but leaves `udaa` blank.
            repair_autocast_enabled_by_default: !matches!(self, Self::Critter),
            profile: builder_profile(metadata.move_speed_world_units_per_second),
            build_catalog: extracted_builder_catalog(
                race_index,
                metadata.rawcode,
                metadata.name,
                metadata.campaign_only,
            ),
            map_version: version,
        })
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

#[derive(Debug, Clone, Copy)]
struct BuilderRaceMetadata {
    rawcode: u32,
    name: &'static str,
    campaign_only: bool,
    locomotion: BuilderLocomotion,
    move_speed_world_units_per_second: i32,
}

const BUILDER_RACE_METADATA_927: [BuilderRaceMetadata; 15] = [
    builder_race_metadata(
        *b"X00O",
        "Chaos Builder",
        false,
        BuilderLocomotion::Foot,
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
    builder_race_metadata(
        *b"X006",
        "Corrupted Builder",
        false,
        BuilderLocomotion::Hover,
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
    builder_race_metadata(
        *b"X0Z0",
        "Critter Builder",
        true,
        BuilderLocomotion::Foot,
        CASTLE_FIGHT_CRITTER_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
    builder_race_metadata(
        *b"X078",
        "Desert Builder",
        false,
        BuilderLocomotion::Foot,
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
    builder_race_metadata(
        *b"X051",
        "Elemental Builder",
        false,
        BuilderLocomotion::Foot,
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
    builder_race_metadata(
        *b"X00P",
        "Elf Builder",
        false,
        BuilderLocomotion::Foot,
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
    builder_race_metadata(
        *b"X00C",
        "Human Builder",
        false,
        BuilderLocomotion::Foot,
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
    builder_race_metadata(
        *b"X06P",
        "Mechanical Builder",
        false,
        BuilderLocomotion::Foot,
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
    builder_race_metadata(
        *b"X00E",
        "Naga Builder",
        false,
        BuilderLocomotion::Foot,
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
    builder_race_metadata(
        *b"X01A",
        "Nature Builder",
        false,
        BuilderLocomotion::Foot,
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
    builder_race_metadata(
        *b"X089",
        "Night Elf Builder",
        false,
        BuilderLocomotion::Foot,
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
    builder_race_metadata(
        *b"X017",
        "Northern Builder",
        false,
        BuilderLocomotion::Foot,
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
    builder_race_metadata(
        *b"X019",
        "Orc Builder",
        false,
        BuilderLocomotion::Foot,
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
    builder_race_metadata(
        *b"X07P",
        "Pandaren Builder",
        false,
        BuilderLocomotion::Foot,
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
    builder_race_metadata(
        *b"X018",
        "Undead Builder",
        false,
        BuilderLocomotion::Foot,
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ),
];

const fn builder_race_metadata(
    rawcode: [u8; 4],
    name: &'static str,
    campaign_only: bool,
    locomotion: BuilderLocomotion,
    move_speed_world_units_per_second: i32,
) -> BuilderRaceMetadata {
    BuilderRaceMetadata {
        rawcode: u32::from_be_bytes(rawcode),
        name,
        campaign_only,
        locomotion,
        move_speed_world_units_per_second,
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
        let mechanics = native_unit_mechanics_for(version, definition.rawcode)
            .expect("supported Castle Fight version must have native-effect tuning");
        definition.passive_effects = mechanics.passive_effects;
        definition.spellcasting = mechanics.spellcasting;
        Ok(definition)
    }

    const fn definition_9_27(self) -> CastleFightUnitDefinition {
        match self {
            Self::Footman => CastleFightUnitDefinition {
                rawcode: u32::from_be_bytes(*b"hfoo"),
                name: "Footman",
                health: 250,
                health_regen_per_second_per_10k: 2_500,
                build_time_ticks: 20 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                repair_time_ticks: 20 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                armor: ArmorProfile::new(ArmorType::Large, 4),
                passive_effects: PassiveUnitEffects::EMPTY,
                spellcasting: None,
                damage_type: DamageType::Normal,
                attack_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
                movement_class: MovementClass::Ground,
                mechanical: false,
                collision_radius: collision_radius(),
                corpse: Some(CorpseProfile {
                    definition: CorpseDefinitionId(u32::from_be_bytes(*b"hfoo")),
                    // Extracted total death + flesh + bone decay is 30.04 s.
                    lifetime_ticks: Some(901),
                }),
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    // Extracted 25-26; the fixed-damage sim uses nearest integer average.
                    damage: 26,
                    range: world(90),
                    acquisition_range: world(800),
                    cooldown_ticks: 41,
                },
                movement: movement(270),
            },
            Self::Defender => CastleFightUnitDefinition {
                rawcode: u32::from_be_bytes(*b"h03A"),
                name: "Defender",
                health: 575,
                health_regen_per_second_per_10k: 17_500,
                build_time_ticks: 28 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                repair_time_ticks: 20 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                armor: ArmorProfile::new(ArmorType::Large, 7),
                passive_effects: PassiveUnitEffects::EMPTY,
                spellcasting: None,
                damage_type: DamageType::Normal,
                attack_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
                movement_class: MovementClass::Ground,
                mechanical: false,
                collision_radius: collision_radius(),
                corpse: Some(CorpseProfile {
                    definition: CorpseDefinitionId(u32::from_be_bytes(*b"h03A")),
                    lifetime_ticks: Some(901),
                }),
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    // Protected runtime stats are 44 base + 1d16 = 45-60 (52.5 average).
                    damage: 53,
                    range: world(90),
                    acquisition_range: world(800),
                    cooldown_ticks: 41,
                },
                movement: movement(250),
            },
            Self::Ranger => CastleFightUnitDefinition {
                rawcode: u32::from_be_bytes(*b"e003"),
                name: "Ranger",
                health: 500,
                health_regen_per_second_per_10k: 5_000,
                build_time_ticks: 32 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                repair_time_ticks: 20 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                armor: ArmorProfile::new(ArmorType::Small, 3),
                passive_effects: PassiveUnitEffects::EMPTY,
                spellcasting: None,
                damage_type: DamageType::Pierce,
                attack_targets: AttackTargetMask::ALL,
                movement_class: MovementClass::Ground,
                mechanical: false,
                collision_radius: collision_radius(),
                corpse: Some(CorpseProfile {
                    definition: CorpseDefinitionId(u32::from_be_bytes(*b"e003")),
                    lifetime_ticks: Some(900),
                }),
                attack: AttackProfile {
                    delivery: AttackDelivery::RangedGuaranteedHit {
                        speed_per_tick: projectile_speed(1_000),
                    },
                    damage: 65,
                    range: world(425),
                    acquisition_range: world(800),
                    cooldown_ticks: 31,
                },
                movement: movement(300),
            },
            Self::Catapult => CastleFightUnitDefinition {
                rawcode: u32::from_be_bytes(*b"o001"),
                name: "Catapult",
                health: 475,
                health_regen_per_second_per_10k: 5_000,
                build_time_ticks: 34 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                repair_time_ticks: 36 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                armor: ArmorProfile::new(ArmorType::Medium, 5),
                passive_effects: PassiveUnitEffects::EMPTY,
                spellcasting: None,
                damage_type: DamageType::Siege,
                attack_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
                movement_class: MovementClass::Ground,
                mechanical: true,
                collision_radius: collision_radius(),
                corpse: None,
                attack: AttackProfile {
                    delivery: AttackDelivery::RangedBallistic {
                        speed_per_tick: projectile_speed(900),
                        // Extracted WC3 tiers are 60/110/160 at 100%/70%/35%.
                        // The current projectile primitive retains the real outer radius until
                        // tiered splash damage is represented.
                        impact_radius: world(160),
                    },
                    damage: 135,
                    range: world(1_000),
                    acquisition_range: world(1_200),
                    cooldown_ticks: 150,
                },
                movement: movement(220),
            },
            Self::IceTrollShadowPriest => CastleFightUnitDefinition {
                rawcode: u32::from_be_bytes(*b"n015"),
                name: "Ice Troll Shadow Priest",
                health: 350,
                health_regen_per_second_per_10k: 10_000,
                build_time_ticks: 23 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                repair_time_ticks: 25 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                armor: ArmorProfile::new(ArmorType::Small, 1),
                passive_effects: PassiveUnitEffects::EMPTY,
                spellcasting: None,
                damage_type: DamageType::Magic,
                attack_targets: AttackTargetMask::ALL,
                movement_class: MovementClass::Ground,
                mechanical: false,
                collision_radius: collision_radius(),
                corpse: Some(CorpseProfile {
                    definition: CorpseDefinitionId(u32::from_be_bytes(*b"n015")),
                    lifetime_ticks: Some(900),
                }),
                attack: AttackProfile {
                    delivery: AttackDelivery::RangedGuaranteedHit {
                        speed_per_tick: projectile_speed(1_200),
                    },
                    damage: 50,
                    range: world(350),
                    acquisition_range: world(800),
                    cooldown_ticks: 54,
                },
                movement: movement(270),
            },
            Self::GryphonRider => CastleFightUnitDefinition {
                rawcode: u32::from_be_bytes(*b"h016"),
                name: "Gryphon Rider",
                health: 500,
                health_regen_per_second_per_10k: 10_000,
                build_time_ticks: 27 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                repair_time_ticks: 45 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                armor: ArmorProfile::new(ArmorType::Medium, 2),
                passive_effects: PassiveUnitEffects::EMPTY,
                spellcasting: None,
                damage_type: DamageType::Magic,
                attack_targets: AttackTargetMask::ALL,
                movement_class: MovementClass::Air,
                mechanical: false,
                collision_radius: CollisionRadius(world(8)),
                corpse: None,
                attack: AttackProfile {
                    delivery: AttackDelivery::RangedGuaranteedHit {
                        speed_per_tick: projectile_speed(1_100),
                    },
                    // The primary extracted attack is 45-50 magic. Gryphon Rider also has a
                    // second pierce attack; multiple simultaneous attack profiles remain a
                    // separate combat-model extension.
                    damage: 48,
                    range: world(450),
                    acquisition_range: world(800),
                    cooldown_ticks: 60,
                },
                movement: movement(320),
            },
        }
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
        match self {
            Self::Barracks => production_definition(
                u32::from_be_bytes(*b"h000"),
                "Barracks",
                100,
                1_200,
                20,
                CastleFightUnitKind::Footman,
                CommandCardPosition::new(0, 0),
            ),
            Self::Stronghold => production_definition(
                u32::from_be_bytes(*b"h039"),
                "Stronghold",
                175,
                1_300,
                28,
                CastleFightUnitKind::Defender,
                CommandCardPosition::new(3, 0),
            ),
            Self::RangersHall => production_definition(
                u32::from_be_bytes(*b"h03D"),
                "Ranger's Hall",
                200,
                1_400,
                32,
                CastleFightUnitKind::Ranger,
                CommandCardPosition::new(3, 0),
            ),
            Self::OrcishSiegeFactory => production_definition(
                u32::from_be_bytes(*b"h02I"),
                "Orcish Siege Factory",
                380,
                1_400,
                34,
                CastleFightUnitKind::Catapult,
                CommandCardPosition::new(1, 1),
            ),
            Self::IceTrollHut => production_definition(
                u32::from_be_bytes(*b"h03K"),
                "Ice Troll Hut",
                175,
                1_200,
                23,
                CastleFightUnitKind::IceTrollShadowPriest,
                CommandCardPosition::new(2, 0),
            ),
            Self::GryphonRock => production_definition(
                u32::from_be_bytes(*b"h015"),
                "Gryphon Rock",
                250,
                1_300,
                27,
                CastleFightUnitKind::GryphonRider,
                CommandCardPosition::new(3, 0),
            ),
        }
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
    pub command_card_position: CommandCardPosition,
    pub map_version: MapVersion,
}

impl CastleFightProductionDefinition {
    #[must_use]
    pub fn spawn(self, team: Team, footprint: BuildingFootprint) -> BuildingSpawn {
        BuildingSpawn {
            team,
            footprint,
            health: self.building_health,
            production: Some(ProductionProfile {
                initial_delay_ticks: self.spawn_interval_ticks,
                interval_ticks: self.spawn_interval_ticks,
                search_radius_cells: PRODUCTION_SPAWN_SEARCH_RADIUS_CELLS,
                unit: self
                    .unit
                    .definition_for_version(self.map_version)
                    .expect("production definition map version must have matching unit content")
                    .template(),
            }),
            attack: None,
            spellcasting: None,
        }
    }

    #[must_use]
    pub fn gameplay_properties(self) -> BuildingGameplayProperties {
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
            production_unit: self
                .unit
                .definition_for_version(self.map_version)
                .expect("production definition map version must have matching unit content")
                .gameplay_properties(),
            production_spellcasting: self
                .unit
                .definition_for_version(self.map_version)
                .expect("production definition map version must have matching unit content")
                .spellcasting,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CastleFightTowerKind {
    WatchTower,
    PoofTower,
}

impl CastleFightTowerKind {
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
        match self {
            Self::WatchTower => {
                let (basic_tooltip, extended_tooltip) =
                    extracted_building_tooltips_927(u32::from_be_bytes(*b"h006"));
                CastleFightTowerDefinition {
                    rawcode: u32::from_be_bytes(*b"h006"),
                    name: "Watch Tower",
                    basic_tooltip,
                    extended_tooltip,
                    gold_cost: 150,
                    lumber_cost: 300,
                    economy: verified_building_economy_927(u32::from_be_bytes(*b"h006"), 150, 300),
                    health: 1_500,
                    construction_time_ticks: extracted_building_construction_time_ticks_927(
                        u32::from_be_bytes(*b"h006"),
                    ),
                    repair_time_ticks: 110 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                    armor: ArmorProfile::new(ArmorType::Fortified, 5),
                    damage_type: DamageType::Pierce,
                    attack_targets: AttackTargetMask::AIR_AND_GROUND,
                    footprint_size_cells: CASTLE_FIGHT_BUILDING_FOOTPRINT_CELLS_927,
                    command_card_position: CommandCardPosition::new(0, 2),
                    attack: AttackProfile {
                        delivery: AttackDelivery::RangedGuaranteedHit {
                            speed_per_tick: projectile_speed(1_800),
                        },
                        damage: 45,
                        range: world(950),
                        acquisition_range: world(1_000),
                        cooldown_ticks: 15,
                    },
                    map_version: MapVersion::CASTLE_FIGHT_9_27,
                }
            }
            Self::PoofTower => {
                let (basic_tooltip, extended_tooltip) =
                    extracted_building_tooltips_927(u32::from_be_bytes(*b"h07P"));
                CastleFightTowerDefinition {
                    rawcode: u32::from_be_bytes(*b"h07P"),
                    name: "Poof Tower",
                    basic_tooltip,
                    extended_tooltip,
                    gold_cost: 230,
                    lumber_cost: 300,
                    economy: verified_building_economy_927(u32::from_be_bytes(*b"h07P"), 230, 300),
                    health: 1_250,
                    construction_time_ticks: extracted_building_construction_time_ticks_927(
                        u32::from_be_bytes(*b"h07P"),
                    ),
                    repair_time_ticks: 110 * CASTLE_FIGHT_SIMULATION_HZ as u32,
                    armor: ArmorProfile::new(ArmorType::Fortified, 5),
                    damage_type: DamageType::Magic,
                    attack_targets: AttackTargetMask::ALL,
                    footprint_size_cells: CASTLE_FIGHT_BUILDING_FOOTPRINT_CELLS_927,
                    command_card_position: CommandCardPosition::new(0, 2),
                    attack: AttackProfile {
                        delivery: AttackDelivery::RangedBallistic {
                            speed_per_tick: projectile_speed(900),
                            // Extracted WC3 tiers are 175/200/250 at 100%/75%/45%.
                            impact_radius: world(250),
                        },
                        damage: 164,
                        range: world(800),
                        acquisition_range: world(1_000),
                        cooldown_ticks: 95,
                    },
                    map_version: MapVersion::CASTLE_FIGHT_9_27,
                }
            }
        }
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
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return Err(UnsupportedCastleFightMapVersion(version));
    }
    Ok(builder_profile(
        CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND,
    ))
}

fn builder_profile(move_speed_world_units_per_second: i32) -> BuilderProfile {
    BuilderProfile {
        speed_per_tick: move_speed_world_units_per_second * SUBUNITS_PER_WORLD_UNIT
            / CASTLE_FIGHT_SIMULATION_HZ,
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

fn extracted_builder_catalog(
    race_index: u8,
    builder_rawcode: u32,
    builder_name: &str,
    campaign_only: bool,
) -> Vec<u32> {
    let mut catalog = Vec::new();
    let mut expected_order = 0usize;
    for line in include_str!("../../../docs/original_map/extracted/script/race-buildings.tsv")
        .lines()
        .skip(1)
    {
        let mut columns = line.split('\t');
        let row_race_index = columns
            .next()
            .expect("race-buildings row missing race index")
            .parse::<u8>()
            .expect("race-buildings race index must be numeric");
        let _race_function = columns
            .next()
            .expect("race-buildings row missing race function");
        let row_builder_rawcode = parse_rawcode(
            columns
                .next()
                .expect("race-buildings row missing builder rawcode"),
        );
        let _builder_rawcode_integer = columns
            .next()
            .expect("race-buildings row missing builder rawcode integer");
        let row_builder_name = columns
            .next()
            .expect("race-buildings row missing builder name");
        let row_campaign_only = columns
            .next()
            .expect("race-buildings row missing campaign flag");
        let building_order = columns
            .next()
            .expect("race-buildings row missing building order")
            .parse::<usize>()
            .expect("race-buildings building order must be numeric");
        let building_rawcode = parse_rawcode(
            columns
                .next()
                .expect("race-buildings row missing building rawcode"),
        );

        if row_race_index != race_index || row_builder_rawcode != builder_rawcode {
            continue;
        }
        assert_eq!(
            row_builder_name, builder_name,
            "builder name changed in extraction"
        );
        assert_eq!(
            row_campaign_only,
            if campaign_only { "1" } else { "0" },
            "builder campaign-only flag changed in extraction"
        );
        assert_eq!(
            building_order, expected_order,
            "builder catalog order changed for race {race_index}"
        );
        assert!(
            !catalog.contains(&building_rawcode),
            "builder race {race_index} repeats building rawcode {building_rawcode:#010x}"
        );
        expected_order += 1;
        // Upgrade targets are reached from the precursor building's command card rather than
        // placed directly by the builder. Keep the builder catalog authoritative for direct
        // placement so UI/network callers cannot bypass that rule.
        if extracted_building_upgrade_links_927(building_rawcode)
            .0
            .is_none()
        {
            catalog.push(building_rawcode);
        }
    }
    assert!(
        !catalog.is_empty(),
        "builder race {race_index} has no extracted buildings"
    );
    catalog
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
    Ok(CASTLE_FIGHT_MAIN_CASTLE_REPAIR_TIME_TICKS_927)
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
    Ok(DamageRules::from_wc3_misc_text(include_str!(
        "../../../docs/original_map/extracted/war3mapMisc.txt"
    ))
    .expect("committed Castle Fight war3mapMisc.txt damage table must remain valid"))
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

fn verified_building_economy_927(
    rawcode: u32,
    expected_gold_cost: u16,
    expected_lumber_cost: u16,
) -> BuildingEconomyProfile {
    let economy = extracted_building_economy_927(rawcode);
    assert_eq!(
        economy.gold_cost,
        u32::from(expected_gold_cost),
        "building gold cost changed in extracted 9.27 data"
    );
    assert_eq!(
        economy.lumber_cost,
        u32::from(expected_lumber_cost),
        "building lumber cost changed in extracted 9.27 data"
    );
    economy
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
    for line in include_str!("../../../docs/original_map/extracted/resolved/buildings.tsv")
        .lines()
        .skip(1)
    {
        let columns = line.split('\t').collect::<Vec<_>>();
        if columns.get(1).copied().map(parse_rawcode) != Some(rawcode) {
            continue;
        }
        return (
            columns[11]
                .parse::<u16>()
                .expect("building gold cost must be an integer"),
            columns[12]
                .parse::<u16>()
                .expect("building lumber cost must be an integer"),
        );
    }
    panic!("building {rawcode:#010x} missing from extracted 9.27 building table")
}

fn extracted_building_tooltips_927(rawcode: u32) -> (&'static str, &'static str) {
    for line in include_str!("../../../docs/original_map/extracted/resolved/buildings.tsv")
        .lines()
        .skip(1)
    {
        let mut columns = line.split('\t');
        let _table = columns.next();
        let Some(candidate_rawcode) = columns.next() else {
            continue;
        };
        if parse_rawcode(candidate_rawcode) != rawcode {
            continue;
        }
        let _base_rawcode = columns.next();
        let _name = columns.next();
        let basic = columns
            .next()
            .expect("building tooltip row must contain basic tooltip");
        let extended = columns
            .next()
            .expect("building tooltip row must contain extended tooltip");
        return (basic, extended);
    }
    panic!("building {rawcode:#010x} missing from extracted 9.27 building table")
}

fn extracted_building_construction_time_ticks_927(rawcode: u32) -> u32 {
    for line in include_str!("../../../docs/original_map/extracted/resolved/buildings.tsv")
        .lines()
        .skip(1)
    {
        let columns = line.split('\t').collect::<Vec<_>>();
        if columns.get(1).copied().map(parse_rawcode) != Some(rawcode) {
            continue;
        }
        let seconds = columns[13]
            .parse::<u32>()
            .expect("building construction time must be an integer");
        return seconds
            .checked_mul(CASTLE_FIGHT_SIMULATION_HZ as u32)
            .expect("building construction time tick overflow");
    }
    panic!("building {rawcode:#010x} missing from extracted 9.27 building table")
}

fn extracted_building_upgrade_links_927(rawcode: u32) -> (Option<u32>, Vec<u32>) {
    let mut precursor = None;
    let mut targets = Vec::new();
    for line in include_str!("../../../docs/original_map/extracted/script/building-upgrades.tsv")
        .lines()
        .skip(1)
    {
        let mut columns = line.split('\t');
        let source = parse_rawcode(
            columns
                .next()
                .expect("building-upgrade row missing source rawcode"),
        );
        let _source_integer = columns.next();
        let _source_name = columns.next();
        let target = parse_rawcode(
            columns
                .next()
                .expect("building-upgrade row missing target rawcode"),
        );
        if source == rawcode {
            targets.push(target);
        }
        if target == rawcode {
            assert!(
                precursor.replace(source).is_none(),
                "building {rawcode:#010x} has more than one extracted precursor"
            );
        }
    }
    targets.sort_unstable();
    (precursor, targets)
}

fn extracted_building_income_per_10k_927(rawcode: u32) -> u64 {
    let (factor_per_1000, _, precursor) = extracted_building_income_semantics_927(rawcode);
    let (gold_cost, _) = extracted_building_costs_927(rawcode);
    let own_income = u64::from(gold_cost) * u64::from(factor_per_1000);
    own_income
        + precursor.map_or(0, |precursor| {
            extracted_building_income_per_10k_927(precursor)
        })
}

fn extracted_building_income_semantics_927(rawcode: u32) -> (u16, bool, Option<u32>) {
    for line in
        include_str!("../../../docs/original_map/extracted/script/race-building-semantics.tsv")
            .lines()
            .skip(1)
    {
        let columns = line.split('\t').collect::<Vec<_>>();
        if columns.first().copied().map(parse_rawcode) != Some(rawcode) {
            continue;
        }
        let precursor = columns
            .get(5)
            .copied()
            .filter(|value| !value.is_empty())
            .map(parse_rawcode);
        return (
            parse_decimal_per_1000(columns[4]),
            columns[11] == "1",
            precursor,
        );
    }
    panic!("building {rawcode:#010x} missing from extracted 9.27 income semantics")
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

fn production_definition(
    rawcode: u32,
    name: &'static str,
    gold_cost: u16,
    building_health: i32,
    spawn_seconds: u16,
    unit: CastleFightUnitKind,
    command_card_position: CommandCardPosition,
) -> CastleFightProductionDefinition {
    let economy = verified_building_economy_927(rawcode, gold_cost, 0);
    let (basic_tooltip, extended_tooltip) = extracted_building_tooltips_927(rawcode);
    CastleFightProductionDefinition {
        rawcode,
        name,
        basic_tooltip,
        extended_tooltip,
        gold_cost,
        lumber_cost: 0,
        economy,
        building_health,
        construction_time_ticks: extracted_building_construction_time_ticks_927(rawcode),
        repair_time_ticks: 70 * CASTLE_FIGHT_SIMULATION_HZ as u32,
        armor: ArmorProfile::new(ArmorType::Fortified, 5),
        spawn_interval_ticks: spawn_seconds * CASTLE_FIGHT_SIMULATION_HZ as u16,
        footprint_size_cells: CASTLE_FIGHT_BUILDING_FOOTPRINT_CELLS_927,
        unit,
        command_card_position,
        map_version: MapVersion::CASTLE_FIGHT_9_27,
    }
}

const fn collision_radius() -> CollisionRadius {
    CollisionRadius(world(CASTLE_FIGHT_COLLISION_WORLD_UNITS))
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

        for kind in CastleFightProductionKind::ALL {
            let definition = kind.definition();
            assert_eq!(
                definition.command_card_position,
                extracted_position(definition.rawcode)
            );
        }
        for kind in CastleFightTowerKind::ALL {
            let definition = kind.definition();
            assert_eq!(
                definition.command_card_position,
                extracted_position(definition.rawcode)
            );
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
