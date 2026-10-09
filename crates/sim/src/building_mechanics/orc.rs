use super::*;
use crate::{AcidBombProfile, AttackTargetMask};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingPermanentGrantProfile {
    pub map_version: MapVersion,
    pub trigger_targets: AttackTargetMask,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub retry_marker: AbilityId,
    pub always: [crate::NativeAbilityGrant; 4],
    pub options: [[crate::NativeAbilityGrant; 2]; 3],
}

impl BuildingPermanentGrantProfile {
    pub(crate) fn canonical_words(self) -> impl Iterator<Item = u64> {
        [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.trigger_targets.bits()),
            u64::from(self.trigger_invulnerable),
            u64::from(self.trigger_spell_immune),
            u64::from(self.retry_marker.0),
        ]
        .into_iter()
        .chain(
            self.always
                .into_iter()
                .chain(self.options.into_iter().flatten())
                .flat_map(crate::NativeAbilityGrant::canonical_words),
        )
    }
}

pub(crate) fn ceremonial_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static PROFILE: OnceLock<SpellcastingProfile> = OnceLock::new();
    *PROFILE.get_or_init(|| {
        use crate::{NativeAbilityGrant, NativeAbilityGrantEffect as Effect};
        let data = evidence(version);
        let params: Value =
            serde_json::from_str(data["ceremonial"]["parameters_json"].as_str().unwrap()).unwrap();
        let number = |fields: &Value, key: &str| fields[key].as_f64().unwrap();
        let grant = |id: &Value| {
            let ability = AbilityId(id.as_u64().unwrap() as u32);
            let bytes = ability.0.to_be_bytes();
            let code = std::str::from_utf8(&bytes).unwrap();
            let fields = &data["fields"][code];
            let effect = match code {
                "A03B" => Effect::AttackSpeed((number(fields, "Isx1:1") * 100.0).round() as i16),
                "A03A" => Effect::AttackDamage(number(fields, "Iatt:1") as i32),
                "A03D" => Effect::MaximumLife(number(fields, "Ilif:1") as i32),
                "A08L" => Effect::Armor((number(fields, "Idef:1") * 100.0).round() as i16),
                "A05I" => Effect::CriticalStrike {
                    chance_per_10k: (number(fields, "Ocr1:1") * 100.0).round() as u16,
                    multiplier_per_10k: (number(fields, "Ocr2:1") * 10_000.0).round() as u16,
                    targets: source_mask(fields["atar:1"].as_str().unwrap()),
                },
                "A08M" => Effect::Evasion {
                    chance_per_10k: (number(fields, "Eev1:1") * 10_000.0).round() as u16,
                },
                "A08R" => {
                    assert_eq!(number(fields, "Nde3:1"), 0.0);
                    assert_eq!(number(fields, "Nde4:1"), 0.0);
                    Effect::Demolish {
                        chance_per_10k: (number(fields, "Nde1:1") * 100.0).round() as u16,
                        multiplier_per_10k: (number(fields, "Nde2:1") * 10_000.0).round() as u16,
                        targets: source_mask(fields["atar:1"].as_str().unwrap()),
                    }
                }
                "A06W" | "A06X" | "A06Y" => {
                    assert_eq!(number(fields, "Oae1:1"), 0.0);
                    assert_eq!(number(fields, "Oae2:1"), 0.0);
                    assert_eq!(number(fields, "aare:1"), 0.0);
                    assert_eq!(fields["atar:1"], "self");
                    Effect::AuraMarker
                }
                _ => panic!("unhandled native ability grant {code}"),
            };
            NativeAbilityGrant { ability, effect }
        };
        let always = std::array::from_fn(|index| grant(&params["always_ability_ids"][index]));
        let options = std::array::from_fn(|option| {
            std::array::from_fn(|index| grant(&params["option_ability_ids"][option][index]))
        });
        let parent = &data["fields"]["A039"];
        let building = &data["fields"]["h02R"];
        let protected = |field| {
            data["protected"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["rawcode"] == "A039" && row["field"] == field)
                .unwrap()["runtime_value"]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap()
        };
        let tokens: Vec<_> = parent["atar:1"].as_str().unwrap().split(',').collect();
        SpellcastingProfile {
            mana: ManaProfile::per_second(
                number(building, "umpm:0") as i32,
                building["umpi:0"].as_f64().unwrap_or_default() as i32,
                (number(building, "umpr:0") * 10_000.0).round() as u32,
            ),
            ability: AutomaticAbilityProfile {
                id: AbilityId(u32::from_be_bytes(*b"A039")),
                mana_cost: protected("mana_cost") as i32,
                cooldown_ticks: (protected("cooldown") * f64::from(CASTLE_FIGHT_SIMULATION_HZ))
                    .ceil() as u16,
                range: (number(parent, "aran:1") * f64::from(SUBUNITS_PER_WORLD_UNIT)).round()
                    as i32,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::BuildingPermanentGrant(BuildingPermanentGrantProfile {
                    map_version: version,
                    trigger_targets: source_mask(parent["atar:1"].as_str().unwrap()),
                    trigger_invulnerable: tokens.contains(&"invulnerable"),
                    trigger_spell_immune: tokens.contains(&"magicimmune"),
                    retry_marker: AbilityId(
                        data["ceremonial_retry_marker"].as_u64().unwrap() as u32
                    ),
                    always,
                    options,
                }),
            },
        }
    })
}

pub(crate) fn ceremonial_bindings_for_version(
    version: MapVersion,
) -> Vec<crate::ResolvedNativeEffectBinding> {
    let AbilityEffect::BuildingPermanentGrant(profile) =
        ceremonial_spellcasting_for_version(version).ability.effect
    else {
        unreachable!()
    };
    std::iter::once((
        crate::NativeEffectSourceKind::UnitAbility,
        u32::from_be_bytes(*b"A039"),
    ))
    .chain(
        profile
            .always
            .into_iter()
            .chain(profile.options.into_iter().flatten())
            .map(|grant| {
                (
                    crate::NativeEffectSourceKind::AbilityEffect,
                    grant.ability.0,
                )
            }),
    )
    .map(|(kind, key)| crate::ResolvedNativeEffectBinding {
        source: crate::NativeEffectSource::new(kind, key),
        implementation: crate::NativeEffectImplementationId::WarcraftPermanentAbilityGrantsV1,
    })
    .collect()
}

pub(crate) fn ceremonial_source_for_version(
    source: crate::NativeEffectSource,
    version: MapVersion,
) -> bool {
    use crate::NativeEffectSourceKind::{AbilityEffect, UnitAbility};
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return false;
    }
    match source.kind {
        UnitAbility => source.key == u32::from_be_bytes(*b"A039"),
        AbilityEffect => [
            *b"A03B", *b"A03A", *b"A03D", *b"A08L", *b"A05I", *b"A06W", *b"A08M", *b"A06Y",
            *b"A08R", *b"A06X",
        ]
        .into_iter()
        .any(|code| source.key == u32::from_be_bytes(code)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingAcidBombProfile {
    pub map_version: MapVersion,
    pub trigger_targets: AttackTargetMask,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub child: AcidBombProfile,
    pub vision_radius: i32,
    pub vision_ticks: u16,
    pub attack_delay_ticks: u16,
    pub attack_animation_ticks: u16,
}

impl BuildingAcidBombProfile {
    pub(crate) fn canonical_words(self) -> impl Iterator<Item = u64> {
        self.child.canonical_words().into_iter().chain([
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.trigger_targets.bits()),
            u64::from(self.trigger_invulnerable),
            u64::from(self.trigger_spell_immune),
            self.vision_radius as u64,
            u64::from(self.vision_ticks),
            u64::from(self.attack_delay_ticks),
            u64::from(self.attack_animation_ticks),
        ])
    }
}

pub(crate) fn serpent_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    let data = evidence(version);
    let building = &data["fields"]["h02N"];
    let parent = &data["fields"]["A02Z"];
    let child = &data["fields"]["A030"];
    let number = |fields: &Value, key: &str| fields[key].as_f64().unwrap();
    let world = |fields: &Value, key: &str| {
        (number(fields, key) * f64::from(SUBUNITS_PER_WORLD_UNIT)).round() as i32
    };
    let native_ticks = |fields: &Value, key: &str| {
        (number(fields, key) * f64::from(CASTLE_FIGHT_SIMULATION_HZ)).ceil() as u16
    };
    let protected = |code: &str, field: &str| {
        data["protected"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["rawcode"] == code && row["field"] == field)
            .unwrap()["runtime_value"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap()
    };
    assert_eq!(protected("A030", "mana_cost"), 0.0);
    assert_eq!(protected("A030", "cooldown"), 0.0);
    assert_eq!(child["atar:1"], "air,enemies,ground,neutral,organic");
    let tokens: Vec<_> = parent["atar:1"].as_str().unwrap().split(',').collect();
    SpellcastingProfile {
        mana: ManaProfile::per_second(
            number(building, "umpm:0") as i32,
            building
                .get("umpi:0")
                .and_then(Value::as_f64)
                .unwrap_or_default() as i32,
            (number(building, "umpr:0") * 10_000.0).round() as u32,
        ),
        ability: AutomaticAbilityProfile {
            id: AbilityId(u32::from_be_bytes(*b"A02Z")),
            mana_cost: protected("A02Z", "mana_cost") as i32,
            cooldown_ticks: (protected("A02Z", "cooldown") * f64::from(CASTLE_FIGHT_SIMULATION_HZ))
                .ceil() as u16,
            range: world(parent, "aran:1"),
            target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
            effect: AbilityEffect::BuildingAcidBomb(BuildingAcidBombProfile {
                map_version: version,
                trigger_targets: source_mask(parent["atar:1"].as_str().unwrap()),
                trigger_invulnerable: tokens.contains(&"invulnerable"),
                trigger_spell_immune: tokens.contains(&"magicimmune"),
                child: AcidBombProfile {
                    ability: AbilityId(u32::from_be_bytes(*b"A030")),
                    range: world(child, "aran:1"),
                    radius: world(child, "aare:1"),
                    speed_per_tick: world(child, "amsp:0") / CASTLE_FIGHT_SIMULATION_HZ,
                    homing: number(child, "amho:0") != 0.0,
                    armor_penalty_per_100: (number(child, "Nab3:1") * 100.0).round() as i16,
                    movement_percent_delta: (number(child, "Nab1:1") * 100.0).round() as i16,
                    attack_speed_percent_delta: (number(child, "Nab2:1") * 100.0).round() as i16,
                    primary_damage: number(child, "Nab4:1") as i32,
                    secondary_damage: number(child, "Nab5:1") as i32,
                    interval_ticks: native_ticks(child, "Nab6:1"),
                    duration_ticks: native_ticks(child, "adur:1"),
                    hero_duration_ticks: native_ticks(child, "ahdu:1"),
                    targets: source_mask(child["atar:1"].as_str().unwrap()),
                },
                vision_radius: (data["target_vision"]["radius"]
                    .as_str()
                    .unwrap()
                    .parse::<f64>()
                    .unwrap()
                    * f64::from(SUBUNITS_PER_WORLD_UNIT))
                .round() as i32,
                vision_ticks: ticks(&data["target_vision"]["duration"]),
                attack_delay_ticks: ticks(&data["serpent_animation_delays"]["attack"]),
                attack_animation_ticks: ticks(&data["serpent_animation_delays"]["stand"]),
            }),
        },
    }
}

pub(crate) fn serpent_bindings_for_version(
    version: MapVersion,
) -> [crate::ResolvedNativeEffectBinding; 2] {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    [
        (crate::NativeEffectSourceKind::UnitAbility, *b"A02Z"),
        (crate::NativeEffectSourceKind::AbilityEffect, *b"A030"),
    ]
    .map(|(kind, code)| crate::ResolvedNativeEffectBinding {
        source: crate::NativeEffectSource::new(kind, u32::from_be_bytes(code)),
        implementation: crate::NativeEffectImplementationId::WarcraftAcidBombV1,
    })
}

pub(crate) fn serpent_source_for_version(
    source: crate::NativeEffectSource,
    version: MapVersion,
) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && serpent_bindings_for_version(version)
            .iter()
            .any(|binding| binding.source == source)
}
