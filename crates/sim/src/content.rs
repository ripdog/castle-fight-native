use std::fmt;

use crate::{
    components::{
        AttackDelivery, AttackProfile, AttackTargetMask, BuilderProfile, BuildingFootprint,
        BuildingGameplayProperties, BuildingSpawn, CollisionRadius, ContentIdentity,
        CorpseDefinitionId, CorpseProfile, MovementClass, MovementProfile, PassiveUnitEffects,
        ProductionProfile, SpellcastingProfile, Team, UnitGameplayProperties, UnitTemplate,
    },
    damage::{ArmorProfile, ArmorType, DamageRules, DamageType},
    math::SUBUNITS_PER_WORLD_UNIT,
    native_effects::native_unit_mechanics_for,
    version::MapVersion,
};

pub const CASTLE_FIGHT_SIMULATION_HZ: i32 = 30;
pub const CASTLE_FIGHT_DEFAULT_MAP_VERSION: MapVersion = MapVersion::CASTLE_FIGHT_9_27;
pub const CASTLE_FIGHT_BUILDING_FOOTPRINT_CELLS: u16 = 4;
const CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND: i32 = 550;
const CASTLE_FIGHT_BUILDER_REPAIR_RANGE_WORLD_UNITS: i32 = 50;
const CASTLE_FIGHT_STANDARD_BUILD_TIME_SECONDS: u16 = 2;
const CASTLE_FIGHT_BUILDER_REPAIR_TIME_RATIO_NUMERATOR: u16 = 3;
const CASTLE_FIGHT_BUILDER_REPAIR_TIME_RATIO_DENOMINATOR: u16 = 2;
const CASTLE_FIGHT_COLLISION_WORLD_UNITS: i32 = 16;
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CastleFightUnitKind {
    Footman,
    Ranger,
    Catapult,
    IceTrollShadowPriest,
    GryphonRider,
}

impl CastleFightUnitKind {
    pub const ALL: [Self; 5] = [
        Self::Footman,
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
                armor: ArmorProfile::new(ArmorType::Large, 4),
                passive_effects: PassiveUnitEffects::EMPTY,
                spellcasting: None,
                damage_type: DamageType::Normal,
                attack_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
                movement_class: MovementClass::Ground,
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
            Self::Ranger => CastleFightUnitDefinition {
                rawcode: u32::from_be_bytes(*b"e003"),
                name: "Ranger",
                health: 500,
                armor: ArmorProfile::new(ArmorType::Small, 3),
                passive_effects: PassiveUnitEffects::EMPTY,
                spellcasting: None,
                damage_type: DamageType::Pierce,
                attack_targets: AttackTargetMask::ALL,
                movement_class: MovementClass::Ground,
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
                armor: ArmorProfile::new(ArmorType::Medium, 5),
                passive_effects: PassiveUnitEffects::EMPTY,
                spellcasting: None,
                damage_type: DamageType::Siege,
                attack_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
                movement_class: MovementClass::Ground,
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
                armor: ArmorProfile::new(ArmorType::Small, 1),
                passive_effects: PassiveUnitEffects::EMPTY,
                spellcasting: None,
                damage_type: DamageType::Magic,
                attack_targets: AttackTargetMask::ALL,
                movement_class: MovementClass::Ground,
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
                armor: ArmorProfile::new(ArmorType::Medium, 2),
                passive_effects: PassiveUnitEffects::EMPTY,
                spellcasting: None,
                damage_type: DamageType::Magic,
                attack_targets: AttackTargetMask::ALL,
                movement_class: MovementClass::Air,
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
    pub armor: ArmorProfile,
    pub passive_effects: PassiveUnitEffects,
    pub spellcasting: Option<SpellcastingProfile>,
    pub damage_type: DamageType,
    pub attack_targets: AttackTargetMask,
    pub movement_class: MovementClass,
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
            corpse: self.corpse,
            collision_radius: Some(self.collision_radius),
            movement_class: self.movement_class,
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
    RangersHall,
    OrcishSiegeFactory,
    IceTrollHut,
    GryphonRock,
}

impl CastleFightProductionKind {
    pub const ALL: [Self; 5] = [
        Self::Barracks,
        Self::RangersHall,
        Self::OrcishSiegeFactory,
        Self::IceTrollHut,
        Self::GryphonRock,
    ];

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

    const fn definition_9_27(self) -> CastleFightProductionDefinition {
        match self {
            Self::Barracks => production_definition(
                u32::from_be_bytes(*b"h000"),
                "Barracks",
                100,
                1_200,
                20,
                CastleFightUnitKind::Footman,
            ),
            Self::RangersHall => production_definition(
                u32::from_be_bytes(*b"h03D"),
                "Ranger's Hall",
                200,
                1_400,
                32,
                CastleFightUnitKind::Ranger,
            ),
            Self::OrcishSiegeFactory => production_definition(
                u32::from_be_bytes(*b"h02I"),
                "Orcish Siege Factory",
                380,
                1_400,
                34,
                CastleFightUnitKind::Catapult,
            ),
            Self::IceTrollHut => production_definition(
                u32::from_be_bytes(*b"h03K"),
                "Ice Troll Hut",
                175,
                1_200,
                23,
                CastleFightUnitKind::IceTrollShadowPriest,
            ),
            Self::GryphonRock => production_definition(
                u32::from_be_bytes(*b"h015"),
                "Gryphon Rock",
                250,
                1_300,
                27,
                CastleFightUnitKind::GryphonRider,
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastleFightProductionDefinition {
    pub rawcode: u32,
    pub name: &'static str,
    pub gold_cost: u16,
    pub building_health: i32,
    pub armor: ArmorProfile,
    pub spawn_interval_ticks: u16,
    pub footprint_size_cells: u16,
    pub unit: CastleFightUnitKind,
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
            attack_targets: AttackTargetMask::ALL,
            damage_type: DamageType::Normal,
            armor: self.armor,
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
    pub const fn definition(self) -> CastleFightTowerDefinition {
        match self {
            Self::WatchTower => CastleFightTowerDefinition {
                rawcode: u32::from_be_bytes(*b"h006"),
                name: "Watch Tower",
                gold_cost: 150,
                health: 1_500,
                armor: ArmorProfile::new(ArmorType::Fortified, 5),
                damage_type: DamageType::Pierce,
                attack_targets: AttackTargetMask::AIR_AND_GROUND,
                footprint_size_cells: CASTLE_FIGHT_BUILDING_FOOTPRINT_CELLS,
                attack: AttackProfile {
                    delivery: AttackDelivery::RangedGuaranteedHit {
                        speed_per_tick: projectile_speed(1_800),
                    },
                    damage: 45,
                    range: world(950),
                    acquisition_range: world(1_000),
                    cooldown_ticks: 15,
                },
            },
            Self::PoofTower => CastleFightTowerDefinition {
                rawcode: u32::from_be_bytes(*b"h07P"),
                name: "Poof Tower",
                gold_cost: 230,
                health: 1_250,
                armor: ArmorProfile::new(ArmorType::Fortified, 5),
                damage_type: DamageType::Magic,
                attack_targets: AttackTargetMask::ALL,
                footprint_size_cells: CASTLE_FIGHT_BUILDING_FOOTPRINT_CELLS,
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
            },
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastleFightTowerDefinition {
    pub rawcode: u32,
    pub name: &'static str,
    pub gold_cost: u16,
    pub health: i32,
    pub armor: ArmorProfile,
    pub damage_type: DamageType,
    pub attack_targets: AttackTargetMask,
    pub footprint_size_cells: u16,
    pub attack: AttackProfile,
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
            attack_targets: self.attack_targets,
            damage_type: self.damage_type,
            armor: self.armor,
            production_unit: UnitGameplayProperties {
                content: None,
                corpse: None,
                collision_radius: None,
                movement_class: MovementClass::Ground,
                attack_targets: AttackTargetMask::ALL,
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
    Ok(BuilderProfile {
        speed_per_tick: CASTLE_FIGHT_BUILDER_MOVE_SPEED_WORLD_UNITS_PER_SECOND
            * SUBUNITS_PER_WORLD_UNIT
            / CASTLE_FIGHT_SIMULATION_HZ,
        repair_range: world(CASTLE_FIGHT_BUILDER_REPAIR_RANGE_WORLD_UNITS),
        full_repair_duration_ticks: CASTLE_FIGHT_STANDARD_BUILD_TIME_SECONDS
            * CASTLE_FIGHT_SIMULATION_HZ as u16
            * CASTLE_FIGHT_BUILDER_REPAIR_TIME_RATIO_NUMERATOR
            / CASTLE_FIGHT_BUILDER_REPAIR_TIME_RATIO_DENOMINATOR,
    })
}

#[must_use]
pub fn castle_fight_damage_rules() -> DamageRules {
    DamageRules::from_wc3_misc_text(include_str!(
        "../../../docs/original_map/extracted/war3mapMisc.txt"
    ))
    .expect("committed Castle Fight war3mapMisc.txt damage table must remain valid")
}

const fn production_definition(
    rawcode: u32,
    name: &'static str,
    gold_cost: u16,
    building_health: i32,
    spawn_seconds: u16,
    unit: CastleFightUnitKind,
) -> CastleFightProductionDefinition {
    CastleFightProductionDefinition {
        rawcode,
        name,
        gold_cost,
        building_health,
        armor: ArmorProfile::new(ArmorType::Fortified, 5),
        spawn_interval_ticks: spawn_seconds * CASTLE_FIGHT_SIMULATION_HZ as u16,
        footprint_size_cells: CASTLE_FIGHT_BUILDING_FOOTPRINT_CELLS,
        unit,
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

    #[test]
    fn builder_profile_uses_extracted_927_movement_and_repair_timing() {
        assert_eq!(
            castle_fight_builder_profile(),
            BuilderProfile {
                speed_per_tick: 550 * SUBUNITS_PER_WORLD_UNIT / CASTLE_FIGHT_SIMULATION_HZ,
                repair_range: 50 * SUBUNITS_PER_WORLD_UNIT,
                full_repair_duration_ticks: 90,
            }
        );
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
    fn imported_towers_are_fortified_and_keep_extracted_attack_types() {
        let watch = CastleFightTowerKind::WatchTower.definition();
        assert_eq!(watch.damage_type, DamageType::Pierce);
        assert_eq!(watch.armor, ArmorProfile::new(ArmorType::Fortified, 5));
        let poof = CastleFightTowerKind::PoofTower.definition();
        assert_eq!(poof.damage_type, DamageType::Magic);
        assert_eq!(poof.armor, ArmorProfile::new(ArmorType::Fortified, 5));
    }
}
