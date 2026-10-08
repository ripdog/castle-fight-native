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
    pub affects_invulnerable: bool,
    pub radius: i32,
    pub regeneration_per_second_per_10k: u32,
    pub lifetime_ticks: u16,
    pub stand_ticks: u16,
}

impl BuildingAuraProfile {
    pub(crate) fn canonical_words(self) -> [u64; 14] {
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
            self.affects_invulnerable.into(),
            self.radius as u64,
            self.regeneration_per_second_per_10k.into(),
            self.lifetime_ticks.into(),
            self.stand_ticks.into(),
        ]
    }
}

pub(crate) fn healing_aura_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static PROFILE: OnceLock<SpellcastingProfile> = OnceLock::new();
    *PROFILE.get_or_init(|| {
        let data = evidence(version);
        let fields = &data["fields"];
        let parent = &fields["A0HT"];
        let child = &fields["AstB"];
        let building = &fields["h07Y"];
        let number = |row: &Value, key: &str| row[key].as_f64().unwrap();
        let ticks = |seconds: f64| (seconds * f64::from(CASTLE_FIGHT_SIMULATION_HZ)).ceil() as u16;
        let protected = |field| {
            data["protected"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["rawcode"] == "A0HT" && row["field"] == field)
                .unwrap()["runtime_value"]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap()
        };
        let control = &data["healing_control"];
        let parent_tokens: Vec<_> = parent["atar:1"].as_str().unwrap().split(',').collect();
        let tokens: Vec<_> = child["atar:1"].as_str().unwrap().split(',').collect();
        // The native aura has no child cast or linger timer. The script removes its carrier.
        for key in ["amcs:1", "acdn:1", "adur:1", "ahdu:1", "Oar2:1"] {
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
                id: AbilityId(u32::from_be_bytes(*b"A0HT")),
                mana_cost: protected("mana_cost") as i32,
                cooldown_ticks: ticks(protected("cooldown")),
                range: (number(parent, "aran:1") * f64::from(SUBUNITS_PER_WORLD_UNIT)).round()
                    as i32,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::BuildingAura(BuildingAuraProfile {
                    map_version: version,
                    ability: AbilityId(u32::from_be_bytes(*b"AstB")),
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
                    affects_invulnerable: tokens.contains(&"invu"),
                    radius: (number(child, "aare:1") * f64::from(SUBUNITS_PER_WORLD_UNIT)).round()
                        as i32,
                    regeneration_per_second_per_10k: (number(child, "Oar1:1") * 10_000.0).round()
                        as u32,
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
    })
}

pub(crate) fn aura_carrier_source_for_version(
    source: NativeEffectSource,
    version: MapVersion,
) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && match source.kind {
            NativeEffectSourceKind::UnitAbility => source.key == u32::from_be_bytes(*b"A0HT"),
            NativeEffectSourceKind::AbilityEffect => source.key == u32::from_be_bytes(*b"AstB"),
        }
}

pub(crate) fn aura_carrier_bindings_for_version(
    version: MapVersion,
) -> Vec<ResolvedNativeEffectBinding> {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    [
        (NativeEffectSourceKind::UnitAbility, *b"A0HT"),
        (NativeEffectSourceKind::AbilityEffect, *b"AstB"),
    ]
    .into_iter()
    .map(|(kind, code)| ResolvedNativeEffectBinding {
        source: NativeEffectSource::new(kind, u32::from_be_bytes(code)),
        implementation: NativeEffectImplementationId::WarcraftAuraCarrierV1,
    })
    .collect()
}
