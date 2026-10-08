use super::*;
use crate::{
    AttackTargetMask, NativeEffectImplementationId, NativeEffectSource, NativeEffectSourceKind,
    ResolvedNativeEffectBinding,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingStasisProfile {
    pub map_version: MapVersion,
    pub ability: AbilityId,
    pub ward_rawcode: u32,
    pub trigger_targets: AttackTargetMask,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub targets: AttackTargetMask,
    pub detection_radius: i32,
    pub detonation_radius: i32,
    pub activation_ticks: u16,
    pub detonation_ticks: u16,
    pub lifetime_ticks: u16,
    pub stun_ticks: u16,
    pub hero_stun_ticks: u16,
    pub stand_ticks: u16,
}

impl BuildingStasisProfile {
    pub(crate) fn canonical_words(self) -> [u64; 16] {
        [
            self.map_version.major.into(),
            self.map_version.minor.into(),
            self.ability.0.into(),
            self.ward_rawcode.into(),
            self.trigger_targets.bits().into(),
            self.trigger_invulnerable.into(),
            self.trigger_spell_immune.into(),
            self.targets.bits().into(),
            self.detection_radius as u64,
            self.detonation_radius as u64,
            self.activation_ticks.into(),
            self.detonation_ticks.into(),
            self.lifetime_ticks.into(),
            self.stun_ticks.into(),
            self.hero_stun_ticks.into(),
            self.stand_ticks.into(),
        ]
    }
}

/// Native technology caps are independent of the player's legendary-point allowance.
pub fn building_type_limit_for_version(version: MapVersion, rawcode: u32) -> Option<u16> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return None;
    }
    let bytes = rawcode.to_be_bytes();
    let code = std::str::from_utf8(&bytes).ok()?;
    evidence(version)["building_type_limits"][code]
        .as_u64()
        .map(|limit| u16::try_from(limit).unwrap())
}

pub(crate) fn stasis_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static PROFILE: OnceLock<SpellcastingProfile> = OnceLock::new();
    *PROFILE.get_or_init(|| {
        let data = evidence(version);
        let fields = &data["fields"];
        let parent = &fields["A032"];
        let child = &fields["Ast9"];
        let building = &fields["h07X"];
        let number = |row: &Value, key: &str| row[key].as_f64().unwrap();
        let ticks = |seconds: f64| (seconds * f64::from(CASTLE_FIGHT_SIMULATION_HZ)).ceil() as u16;
        let radius = |key| (number(child, key) * f64::from(SUBUNITS_PER_WORLD_UNIT)).round() as i32;
        let protected = |field| {
            data["protected"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["rawcode"] == "A032" && row["field"] == field)
                .unwrap()["runtime_value"]
                .as_str()
                .unwrap()
                .parse::<f64>()
                .unwrap()
        };
        let control = &data["stasis_control"];
        let parent_tokens: Vec<_> = parent["atar:1"].as_str().unwrap().split(',').collect();
        assert_eq!(number(child, "amcs:1"), 0.0);
        assert_eq!(number(child, "acdn:1"), 0.0);
        assert_eq!(control["order"], "stasistrap");
        SpellcastingProfile {
            mana: ManaProfile::per_second(
                number(building, "umpm:0") as i32,
                building["umpi:0"].as_f64().unwrap_or_default() as i32,
                (number(building, "umpr:0") * 10_000.0).round() as u32,
            ),
            ability: AutomaticAbilityProfile {
                id: AbilityId(u32::from_be_bytes(*b"A032")),
                mana_cost: protected("mana_cost") as i32,
                cooldown_ticks: ticks(protected("cooldown")),
                range: (number(parent, "aran:1") * f64::from(SUBUNITS_PER_WORLD_UNIT)).round()
                    as i32,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::BuildingStasis(BuildingStasisProfile {
                    map_version: version,
                    ability: AbilityId(u32::from_be_bytes(*b"Ast9")),
                    ward_rawcode: u32::from_be_bytes(
                        control["hidden_ward_rawcode"]
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
                    detection_radius: radius("Sta2:1"),
                    detonation_radius: radius("Sta3:1"),
                    activation_ticks: ticks(number(child, "Sta1:1")),
                    detonation_ticks: ticks(number(child, "Sta5:1")),
                    lifetime_ticks: ticks(number(child, "adur:1")),
                    stun_ticks: ticks(number(child, "Sta4:1")),
                    hero_stun_ticks: ticks(number(child, "ahdu:1")),
                    stand_ticks: ticks(control["stand_seconds"].as_str().unwrap().parse().unwrap()),
                }),
            },
        }
    })
}

pub(crate) fn stasis_source_for_version(source: NativeEffectSource, version: MapVersion) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && match source.kind {
            NativeEffectSourceKind::UnitAbility => source.key == u32::from_be_bytes(*b"A032"),
            NativeEffectSourceKind::AbilityEffect => source.key == u32::from_be_bytes(*b"Ast9"),
        }
}

pub(crate) fn stasis_bindings_for_version(version: MapVersion) -> Vec<ResolvedNativeEffectBinding> {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    [
        (NativeEffectSourceKind::UnitAbility, *b"A032"),
        (NativeEffectSourceKind::AbilityEffect, *b"Ast9"),
    ]
    .into_iter()
    .map(|(kind, code)| ResolvedNativeEffectBinding {
        source: NativeEffectSource::new(kind, u32::from_be_bytes(code)),
        implementation: NativeEffectImplementationId::WarcraftStasisTrapV1,
    })
    .collect()
}
