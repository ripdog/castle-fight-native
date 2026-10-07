//! Version-scoped building mechanics, independently projected from retained evidence.
use crate::{
    AbilityEffect, AbilityId, AbilityTargetPolicy, AutomaticAbilityProfile, ManaProfile,
    SpellcastingProfile,
    content::CASTLE_FIGHT_SIMULATION_HZ,
    damage::{ArmorProfile, ArmorType},
    math::SUBUNITS_PER_WORLD_UNIT,
    version::MapVersion,
};
use serde_json::Value;
use std::sync::OnceLock;

fn evidence(version: MapVersion) -> &'static Value {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../data/castle-fight/9.27/building-mechanics-r1.json"
        ))
        .expect("generated building evidence")
    })
}

pub(crate) fn canonical_projection_for_version(version: MapVersion) -> &'static [u8] {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    include_bytes!("../data/castle-fight/9.27/building-mechanics-r1.json")
}

pub(crate) struct SnowveilProfile {
    pub rawcode: u32,
    pub manual_ability: AbilityId,
    pub snow_terrain_rawcode: u32,
    pub tile_size: i32,
    pub origin: crate::SimPoint,
    pub battlefield_bounds: [i32; 4],
    pub width: i32,
    pub height: i32,
    pub damage_taken_per_10k: u16,
    pub explosion_damage: i32,
    pub explosion_radius: i32,
    pub manual_cooldown_ticks: u16,
}

pub(crate) fn snowveil_for_version(version: MapVersion) -> &'static SnowveilProfile {
    static PROFILE: OnceLock<SnowveilProfile> = OnceLock::new();
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    PROFILE.get_or_init(|| {
        let data = evidence(version);
        let p: Value =
            serde_json::from_str(data["snow"]["parameters_json"].as_str().unwrap()).unwrap();
        let world = |value: &Value| {
            value.as_str().unwrap().parse::<i32>().unwrap() * SUBUNITS_PER_WORLD_UNIT
        };
        let grid = &data["terrain_grid"];
        SnowveilProfile {
            rawcode: u32::from_be_bytes(*b"h07W"),
            manual_ability: AbilityId(p["manual_explosion_ability_id"].as_u64().unwrap() as u32),
            snow_terrain_rawcode: u32::from_be_bytes(
                grid["snow_rawcode"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
                    .try_into()
                    .unwrap(),
            ),
            tile_size: p["tile_spacing_world_units"].as_i64().unwrap() as i32
                * SUBUNITS_PER_WORLD_UNIT,
            battlefield_bounds: std::array::from_fn(|i| {
                grid["battlefield_bounds"][i].as_i64().unwrap() as i32 * SUBUNITS_PER_WORLD_UNIT
            }),
            origin: crate::SimPoint::new(
                grid["offset"]["x"].as_i64().unwrap() as i32 * SUBUNITS_PER_WORLD_UNIT,
                grid["offset"]["y"].as_i64().unwrap() as i32 * SUBUNITS_PER_WORLD_UNIT,
            ),
            width: grid["width"].as_i64().unwrap() as i32,
            height: grid["height"].as_i64().unwrap() as i32,
            damage_taken_per_10k: (p["incoming_damage_multiplier"]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap()
                * 10_000.0) as u16,
            explosion_damage: p["manual_explosion_damage"]
                .as_str()
                .unwrap()
                .parse()
                .unwrap(),
            explosion_radius: world(&p["manual_explosion_radius"]),
            manual_cooldown_ticks: ticks(&data["snow_manual_cooldowns"]["normal"]),
        }
    })
}

pub(crate) fn snow_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    let data = evidence(version);
    let fields = &data["fields"]["h07W"];
    let protected = |field: &str| {
        data["protected"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["rawcode"] == "A0HO" && row["field"] == field)
            .unwrap()["runtime_value"]
            .clone()
    };
    SpellcastingProfile {
        mana: ManaProfile::per_second(
            fields["umpm:0"].as_i64().unwrap() as i32,
            0,
            (fields["umpr:0"].as_f64().unwrap() * 10_000.0) as u32,
        ),
        ability: AutomaticAbilityProfile {
            id: AbilityId(u32::from_be_bytes(*b"A0HO")),
            mana_cost: protected("mana_cost").as_str().unwrap().parse().unwrap(),
            cooldown_ticks: ticks(&protected("cooldown")),
            range: 0,
            target_policy: AbilityTargetPolicy::RandomEnemyUnitGlobal,
            effect: AbilityEffect::Snowfall {
                map_version: version,
            },
        },
    }
}

pub(crate) fn snow_bindings_for_version(
    version: MapVersion,
) -> [crate::ResolvedNativeEffectBinding; 2] {
    [*b"A0HO", *b"AM0{"].map(|code| crate::ResolvedNativeEffectBinding {
        source: crate::NativeEffectSource::new(
            crate::NativeEffectSourceKind::UnitAbility,
            u32::from_be_bytes(code),
        ),
        implementation: {
            assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
            crate::NativeEffectImplementationId::WarcraftSnowveilV1
        },
    })
}

pub(crate) fn snow_source_for_version(
    source: crate::NativeEffectSource,
    version: MapVersion,
) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && snow_bindings_for_version(version)
            .iter()
            .any(|binding| binding.source == source)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HexFormProfile {
    pub rawcode: u32,
    pub speed_per_tick: i32,
    pub collision_radius: i32,
    pub armor: ArmorProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct HexEffectProfile {
    pub map_version: MapVersion,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
    pub ground: HexFormProfile,
    pub air: HexFormProfile,
    pub initial_reengage_ticks: u16,
    pub defender_restore_ticks: u16,
    pub defender_resume_ticks: u16,
    pub defender_rawcode: u32,
}

fn ticks(seconds: &Value) -> u16 {
    let seconds: f64 = seconds
        .as_str()
        .expect("source seconds string")
        .parse()
        .expect("seconds");
    // Exclusive expiry: never shorten fractional native durations to an earlier tick.
    (seconds * CASTLE_FIGHT_SIMULATION_HZ as f64).ceil() as u16
}

pub fn city_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static PROFILE: OnceLock<SpellcastingProfile> = OnceLock::new();
    *PROFILE.get_or_init(|| build_city_spellcasting(version))
}

fn build_city_spellcasting(version: MapVersion) -> SpellcastingProfile {
    let data = evidence(version);
    let fields = &data["fields"]["h00Z"];
    let effect_objects: Value =
        serde_json::from_str(data["city"]["effect_objects_json"].as_str().unwrap()).unwrap();
    let effect = &effect_objects[0]["ability_level1"];
    let params: Value =
        serde_json::from_str(data["city"]["parameters_json"].as_str().unwrap()).unwrap();
    let control = &params["control_reengage"];
    let mut cost = None;
    let mut cooldown = None;
    for row in data["protected"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["rawcode"].as_str() == Some("A017"))
    {
        match row["field"].as_str().unwrap() {
            "mana_cost" => cost = Some(row["runtime_value"].as_str().unwrap().parse().unwrap()),
            "cooldown" => cooldown = Some(ticks(&row["runtime_value"])),
            _ => {}
        }
    }
    let form = |code: &str| {
        let row = &data["forms"][code];
        let number = |key: &str| row[key].as_str().unwrap().parse::<i32>().unwrap();
        HexFormProfile {
            rawcode: u32::from_be_bytes(code.as_bytes().try_into().unwrap()),
            speed_per_tick: number("move_speed") * SUBUNITS_PER_WORLD_UNIT
                / CASTLE_FIGHT_SIMULATION_HZ,
            collision_radius: number("collision") * SUBUNITS_PER_WORLD_UNIT,
            armor: ArmorProfile {
                armor_points: number("armor") as i16,
                armor_type: match row["armor_type"].as_str().unwrap() {
                    "medium" => ArmorType::Medium,
                    _ => panic!("unsupported extracted critter armor"),
                },
            },
        }
    };
    SpellcastingProfile {
        mana: ManaProfile::per_second(
            fields["umpm:0"].as_i64().unwrap() as i32,
            fields["umpi:0"].as_i64().unwrap_or(0) as i32,
            (fields["umpr:0"].as_f64().unwrap() * 10_000.0) as u32,
        ),
        ability: AutomaticAbilityProfile {
            id: AbilityId(u32::from_be_bytes(*b"A017")),
            mana_cost: cost.unwrap(),
            cooldown_ticks: cooldown.unwrap(),
            range: 0,
            target_policy: AbilityTargetPolicy::RandomEnemyUnitGlobal,
            effect: AbilityEffect::Hex {
                profile: HexEffectProfile {
                    map_version: version,
                    duration_ticks: ticks(&effect["duration_normal"]),
                    hero_duration_ticks: ticks(&effect["duration_hero"]),
                    ground: form("n00F"),
                    air: form("n00G"),
                    initial_reengage_ticks: ticks(&control["initial_reengage_delay_seconds"]),
                    defender_restore_ticks: ticks(
                        &control["defender_defend_restore_delay_seconds_after_initial_reengage"],
                    ),
                    defender_resume_ticks: ticks(
                        &control["defender_attack_resume_delay_seconds_after_defend"],
                    ),
                    defender_rawcode: control["defender_unit_id"].as_u64().unwrap() as u32,
                },
            },
        },
    }
}

pub(crate) fn hex_bindings_for_version(
    version: MapVersion,
) -> [crate::ResolvedNativeEffectBinding; 2] {
    let data = evidence(version);
    let params: Value =
        serde_json::from_str(data["city"]["parameters_json"].as_str().unwrap()).unwrap();
    let parent = data["city"]["ability_rawcode"].as_str().unwrap();
    [
        crate::NativeEffectSource::new(
            crate::NativeEffectSourceKind::UnitAbility,
            u32::from_be_bytes(parent.as_bytes().try_into().unwrap()),
        ),
        crate::NativeEffectSource::new(
            crate::NativeEffectSourceKind::AbilityEffect,
            params["effect_ability_id"].as_u64().unwrap() as u32,
        ),
    ]
    .map(|source| crate::ResolvedNativeEffectBinding {
        source,
        implementation: crate::NativeEffectImplementationId::WarcraftBuildingHexV1,
    })
}

pub(crate) fn hex_source_for_version(
    source: crate::NativeEffectSource,
    version: MapVersion,
) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && hex_bindings_for_version(version)
            .iter()
            .any(|binding| binding.source == source)
}

/// Hidden source markers are not inferred from armor, legendary names or tooltips.
pub(crate) fn markers_for_version(rawcode: u32, version: MapVersion) -> (bool, bool) {
    let bytes = rawcode.to_be_bytes();
    let code = std::str::from_utf8(&bytes).unwrap_or("");
    let markers = &evidence(version)["target_markers"][code];
    let has = |marker| {
        markers
            .as_array()
            .is_some_and(|a| a.iter().any(|v| v.as_str() == Some(marker)))
    };
    (has("A070"), has("A08H") || has("Avul") || code == "h06B")
}

pub(crate) fn shield_armor_for_version(level: u8, version: MapVersion) -> i16 {
    if level == 0 {
        return 0;
    }
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static ARMOR: OnceLock<[i16; 2]> = OnceLock::new();
    ARMOR.get_or_init(|| {
        let params: Value = serde_json::from_str(
            evidence(version)["shield"]["parameters_json"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        std::array::from_fn(|index| {
            params["active_shield_armor_bonus_by_level"][index]
                .as_i64()
                .unwrap() as i16
        })
    })[usize::from(level - 1)]
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OverheatExplosion {
    pub full_damage: i32,
    pub full_radius: i32,
    pub partial_damage: i32,
    pub partial_radius: i32,
}

pub(crate) struct OverheatShieldProfile {
    pub rawcode: u32,
    pub chance_percent: u8,
    pub initial_level: u8,
    pub maximum_level: u8,
    pub attack_speed_ability: AbilityId,
    pub attack_speed_percent: [i16; 5],
    pub explosions: [OverheatExplosion; 5],
}

pub(crate) fn overheat_shield_for_version(version: MapVersion) -> &'static OverheatShieldProfile {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static PROFILE: OnceLock<OverheatShieldProfile> = OnceLock::new();
    PROFILE.get_or_init(|| {
        let data = evidence(version);
        let params: Value =
            serde_json::from_str(data["shield"]["parameters_json"].as_str().unwrap()).unwrap();
        OverheatShieldProfile {
            rawcode: params["goblin_shredder_unit_id"].as_u64().unwrap() as u32,
            chance_percent: params["goblin_shredder_block_chance_percent"]
                .as_u64()
                .unwrap() as u8,
            initial_level: params["goblin_shredder_overheat_initial_level"]
                .as_u64()
                .unwrap() as u8,
            maximum_level: params["goblin_shredder_overheat_max_level"]
                .as_u64()
                .unwrap() as u8,
            attack_speed_ability: AbilityId(
                params["goblin_shredder_overheat_attack_speed_ability_id"]
                    .as_u64()
                    .unwrap() as u32,
            ),
            attack_speed_percent: std::array::from_fn(|i| {
                (params["goblin_shredder_overheat_attack_speed_fraction_by_level"][i]
                    .as_f64()
                    .unwrap()
                    * 100.0) as i16
            }),
            explosions: std::array::from_fn(|i| {
                let fields: Value = serde_json::from_str(
                    data["overheat_explosions"][i]["data_fields_labeled_json"]
                        .as_str()
                        .unwrap(),
                )
                .unwrap();
                let number = |key: &str| fields[key].as_i64().unwrap() as i32;
                OverheatExplosion {
                    full_damage: number("Full Damage Amount"),
                    full_radius: number("Full Damage Radius") * SUBUNITS_PER_WORLD_UNIT,
                    partial_damage: number("Partial Damage Amount"),
                    partial_radius: number("Partial Damage Radius") * SUBUNITS_PER_WORLD_UNIT,
                }
            }),
        }
    })
}

/// Warcraft source model/effect identities for presentation, not gameplay tuning.
pub fn building_effect_art_for_version(
    rawcode: u32,
    field: &str,
    version: MapVersion,
) -> Option<&'static str> {
    let bytes = rawcode.to_be_bytes();
    let code = std::str::from_utf8(&bytes).ok()?;
    evidence(version)["fields"][code][field]
        .as_str()
        .filter(|value| !value.is_empty() && *value != "_")
}

pub fn snowveil_manual_ability_for_version(version: MapVersion) -> AbilityId {
    snowveil_for_version(version).manual_ability
}

pub(crate) fn snow_trigger_eligible(
    point: crate::SimPoint,
    sapper: bool,
    version: MapVersion,
) -> bool {
    let [min_x, min_y, max_x, max_y] = snowveil_for_version(version).battlefield_bounds;
    sapper && point.x >= min_x && point.x <= max_x && point.y >= min_y && point.y <= max_y
}
