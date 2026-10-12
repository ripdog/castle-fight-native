use super::*;
use crate::{
    AttackTargetMask, NativeEffectImplementationId, NativeEffectSource, NativeEffectSourceKind,
    ResolvedNativeEffectBinding,
};

/// A noninteractive, fixed-point dummy carrying a native aura until script removal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingAuraProfile {
    pub map_version: MapVersion,
    pub ability: AbilityId,
    pub buff_rawcode: u32,
    pub trigger_targets: AttackTargetMask,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub targets: AttackTargetMask,
    pub organic_only: bool,
    pub nonsapper_only: bool,
    pub affects_invulnerable: bool,
    pub radius: i32,
    pub regeneration_per_second_per_10k: u32,
    pub movement_percent_delta: i16,
    pub attack_speed_percent_delta: i16,
    pub lifetime_ticks: u16,
    pub stand_ticks: u16,
}

impl BuildingAuraProfile {
    pub(crate) fn canonical_words(self) -> [u64; 17] {
        [
            self.map_version.major.into(),
            self.map_version.minor.into(),
            self.ability.0.into(),
            self.buff_rawcode.into(),
            self.trigger_targets.bits().into(),
            self.trigger_invulnerable.into(),
            self.trigger_spell_immune.into(),
            self.targets.bits().into(),
            self.organic_only.into(),
            self.nonsapper_only.into(),
            self.affects_invulnerable.into(),
            self.radius as u64,
            self.regeneration_per_second_per_10k.into(),
            self.movement_percent_delta as u64,
            self.attack_speed_percent_delta as u64,
            self.lifetime_ticks.into(),
            self.stand_ticks.into(),
        ]
    }
}

pub(crate) fn healing_aura_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static PROFILE: OnceLock<SpellcastingProfile> = OnceLock::new();
    *PROFILE.get_or_init(|| aura_spellcasting(version, "healing"))
}

pub(crate) fn endurance_aura_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static PROFILE: OnceLock<SpellcastingProfile> = OnceLock::new();
    *PROFILE.get_or_init(|| aura_spellcasting(version, "endurance"))
}

fn aura_spellcasting(version: MapVersion, name: &str) -> SpellcastingProfile {
    let data = evidence(version);
    let row = &data[name];
    let parent_code = row["ability_rawcode"].as_str().unwrap();
    let building_code = row["building_rawcode"].as_str().unwrap();
    let parameters: Value = serde_json::from_str(row["parameters_json"].as_str().unwrap()).unwrap();
    let child_code = (parameters["effect_ability_id"].as_u64().unwrap() as u32).to_be_bytes();
    let child_code = std::str::from_utf8(&child_code).unwrap();
    let fields = &data["fields"];
    let parent = &fields[parent_code];
    let child = &fields[child_code];
    let building = &fields[building_code];
    let number = |row: &Value, key: &str| row[key].as_f64().unwrap();
    let ticks = |seconds: f64| (seconds * f64::from(CASTLE_FIGHT_SIMULATION_HZ)).ceil() as u16;
    let protected = |field| {
        data["protected"]
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["rawcode"] == parent_code && row["field"] == field)
            .unwrap()["runtime_value"]
            .as_str()
            .unwrap()
            .parse::<f64>()
            .unwrap()
    };
    let control = &data[format!("{name}_control")];
    let parent_tokens: Vec<_> = parent["atar:1"].as_str().unwrap().split(',').collect();
    let tokens: Vec<_> = child["atar:1"].as_str().unwrap().split(',').collect();
    // The native aura has no child cast or linger timer. The script removes its carrier.
    for key in ["amcs:1", "acdn:1", "adur:1", "ahdu:1"] {
        assert_eq!(number(child, key), 0.0);
    }
    let carrier = &fields[control["carrier_rawcode"].as_str().unwrap()];
    assert_eq!(carrier["umdl:0"], "no_model.mdl");
    assert_eq!(carrier["umvs:0"], 0);
    assert_eq!(carrier["ucol:0"], 0);
    assert_eq!(carrier["usid:0"], 0);
    assert_eq!(carrier["usin:0"], 0);
    let abilities: Vec<_> = carrier["uabi:0"].as_str().unwrap().split(',').collect();
    assert!(abilities.contains(&"Avul") && abilities.contains(&"Aloc"));
    SpellcastingProfile {
        mana: ManaProfile::per_second(
            number(building, "umpm:0") as i32,
            building["umpi:0"].as_f64().unwrap_or_default() as i32,
            (number(building, "umpr:0") * 10_000.0).round() as u32,
        ),
        ability: AutomaticAbilityProfile {
            id: AbilityId(u32::from_be_bytes(
                parent_code.as_bytes().try_into().unwrap(),
            )),
            mana_cost: protected("mana_cost") as i32,
            cooldown_ticks: ticks(protected("cooldown")),
            range: (number(parent, "aran:1") * f64::from(SUBUNITS_PER_WORLD_UNIT)).round() as i32,
            target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
            effect: AbilityEffect::BuildingAura(BuildingAuraProfile {
                map_version: version,
                ability: AbilityId(u32::from_be_bytes(
                    child_code.as_bytes().try_into().unwrap(),
                )),
                buff_rawcode: u32::from_be_bytes(
                    child["abuf:1"]
                        .as_str()
                        .unwrap()
                        .as_bytes()
                        .try_into()
                        .unwrap(),
                ),
                trigger_targets: source_mask(parent["atar:1"].as_str().unwrap()),
                trigger_invulnerable: parent_tokens.contains(&"invulnerable"),
                trigger_spell_immune: parent_tokens.contains(&"magicimmune"),
                targets: source_mask(child["atar:1"].as_str().unwrap()),
                organic_only: tokens.contains(&"organic"),
                nonsapper_only: tokens.contains(&"nonsapper"),
                affects_invulnerable: tokens.contains(&"invu") || !tokens.contains(&"vuln"),
                radius: (number(child, "aare:1") * f64::from(SUBUNITS_PER_WORLD_UNIT)).round()
                    as i32,
                regeneration_per_second_per_10k: if name == "healing" {
                    assert_eq!(number(child, "Oar2:1"), 0.0);
                    (number(child, "Oar1:1") * 10_000.0).round() as u32
                } else {
                    0
                },
                movement_percent_delta: if name == "endurance" {
                    (number(child, "Oae1:1") * 100.0).round() as i16
                } else {
                    0
                },
                attack_speed_percent_delta: if name == "endurance" {
                    (number(child, "Oae2:1") * 100.0).round() as i16
                } else {
                    0
                },
                lifetime_ticks: ticks(
                    control["lifetime_seconds"]
                        .as_str()
                        .unwrap()
                        .parse()
                        .unwrap(),
                ),
                stand_ticks: ticks(control["stand_seconds"].as_str().unwrap().parse().unwrap()),
            }),
        },
    }
}

pub(crate) fn aura_carrier_source_for_version(
    source: NativeEffectSource,
    version: MapVersion,
) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && aura_pairs()
            .iter()
            .any(|(parent, child)| match source.kind {
                NativeEffectSourceKind::UnitAbility => source.key == u32::from_be_bytes(*parent),
                NativeEffectSourceKind::AbilityEffect => source.key == u32::from_be_bytes(*child),
            })
}

fn aura_pairs() -> [([u8; 4], [u8; 4]); 2] {
    [(*b"A0HT", *b"AstB"), (*b"A0HS", *b"AstC")]
}

pub(crate) fn aura_carrier_bindings_for_version(
    version: MapVersion,
    roots: &[NativeEffectSource],
) -> Vec<ResolvedNativeEffectBinding> {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    aura_pairs()
        .into_iter()
        .filter(|(parent, child)| {
            roots.iter().any(|source| match source.kind {
                NativeEffectSourceKind::UnitAbility => source.key == u32::from_be_bytes(*parent),
                NativeEffectSourceKind::AbilityEffect => source.key == u32::from_be_bytes(*child),
            })
        })
        .flat_map(|(parent, child)| {
            [
                (NativeEffectSourceKind::UnitAbility, parent),
                (NativeEffectSourceKind::AbilityEffect, child),
            ]
        })
        .map(|(kind, code)| ResolvedNativeEffectBinding {
            source: NativeEffectSource::new(kind, u32::from_be_bytes(code)),
            implementation: NativeEffectImplementationId::WarcraftAuraCarrierV1,
        })
        .collect()
}
