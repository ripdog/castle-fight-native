use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingMagicRuinProfile {
    pub map_version: MapVersion,
    pub ability: AbilityId,
    pub parent: crate::NativeParasiteProfile,
    pub trigger_targets: crate::AttackTargetMask,
    pub trigger_mechanical_only: bool,
    pub selector_exclusions: [AbilityId; 2],
    pub selector_excluded_rawcode: u32,
    pub outcome_count: u8,
    pub death_damage: [i32; 2],
    pub grants: [crate::NativeAbilityGrant; 2],
    pub mutation_rawcode: u32,
    pub dummy_rawcode: u32,
    pub dummy_flying: bool,
    pub control_dummy_ticks: [u16; 3],
    pub hex: HexEffectProfile,
    pub hex_ability: AbilityId,
    pub hex_range: i32,
    pub banish: crate::NativeDebuffProfile,
    pub banish_range: i32,
    pub bolt: crate::NativeBoltProfile,
    pub bolt_range: i32,
    pub vision_radius: i32,
    pub vision_ticks: u16,
}

impl BuildingMagicRuinProfile {
    pub(crate) fn visit_canonical_words(self, mut visit: impl FnMut(u64)) {
        for word in self.parent.canonical_words() {
            visit(word);
        }
        for word in [
            u64::from(self.ability.0),
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.trigger_targets.bits()),
            u64::from(self.trigger_mechanical_only),
            u64::from(self.selector_exclusions[0].0),
            u64::from(self.selector_exclusions[1].0),
            u64::from(self.selector_excluded_rawcode),
            u64::from(self.outcome_count),
            self.death_damage[0] as u64,
            self.death_damage[1] as u64,
            u64::from(self.mutation_rawcode),
            u64::from(self.dummy_rawcode),
            u64::from(self.dummy_flying),
            u64::from(self.control_dummy_ticks[0]),
            u64::from(self.control_dummy_ticks[1]),
            u64::from(self.control_dummy_ticks[2]),
            u64::from(self.hex.map_version.major),
            u64::from(self.hex.map_version.minor),
            u64::from(self.hex.duration_ticks),
            u64::from(self.hex.hero_duration_ticks),
            u64::from(self.hex.initial_reengage_ticks),
            u64::from(self.hex.defender_restore_ticks),
            u64::from(self.hex.defender_resume_ticks),
            u64::from(self.hex.defender_rawcode),
            u64::from(self.hex_ability.0),
            self.hex_range as u64,
            self.banish_range as u64,
            u64::from(self.bolt.ability.0),
            self.bolt.damage as u64,
            u64::from(self.bolt.stun_ticks),
            u64::from(self.bolt.hero_stun_ticks),
            self.bolt.damage_per_second as u64,
            u64::from(self.bolt.duration_ticks),
            self.bolt.speed_per_tick as u64,
            u64::from(self.bolt.cleanse),
            u64::from(self.bolt.targets.bits()),
            self.bolt_range as u64,
            self.vision_radius as u64,
            u64::from(self.vision_ticks),
        ] {
            visit(word);
        }
        for form in [self.hex.ground, self.hex.air] {
            for word in [
                u64::from(form.rawcode),
                form.speed_per_tick as u64,
                form.collision_radius as u64,
                form.armor.armor_points as u64,
                u64::from(form.armor.armor_type.stable_tag()),
            ] {
                visit(word);
            }
        }
        for grant in self.grants {
            for word in grant.canonical_words() {
                visit(word);
            }
        }
        for word in self.banish.canonical_words() {
            visit(word);
        }
    }
}

pub(crate) fn magic_ruin_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static PROFILE: OnceLock<SpellcastingProfile> = OnceLock::new();
    *PROFILE.get_or_init(|| {
        serde_json::from_value(evidence(version)["magic_ruin_spellcasting"].clone())
            .expect("retained Magic Ruin projection")
    })
}

pub(crate) fn magic_ruin_bindings(version: MapVersion) -> [crate::ResolvedNativeEffectBinding; 5] {
    let spell = magic_ruin_spellcasting_for_version(version);
    let AbilityEffect::BuildingMagicRuin(p) = spell.ability.effect else {
        unreachable!()
    };
    [
        (crate::NativeEffectSourceKind::UnitAbility, spell.ability.id),
        (
            crate::NativeEffectSourceKind::AbilityEffect,
            p.banish.ability,
        ),
        (crate::NativeEffectSourceKind::AbilityEffect, p.bolt.ability),
        (
            crate::NativeEffectSourceKind::AbilityEffect,
            p.grants[0].ability,
        ),
        (
            crate::NativeEffectSourceKind::AbilityEffect,
            p.grants[1].ability,
        ),
    ]
    .map(|(kind, ability)| crate::ResolvedNativeEffectBinding {
        source: crate::NativeEffectSource::new(kind, ability.0),
        implementation: crate::NativeEffectImplementationId::WarcraftMagicRuinV1,
    })
}

pub(crate) fn magic_ruin_source(version: MapVersion, source: crate::NativeEffectSource) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && magic_ruin_bindings(version)
            .iter()
            .any(|b| b.source == source)
}

#[must_use]
pub fn magic_ruin_target_attachment_for_version(version: MapVersion) -> &'static str {
    evidence(version)["magic_ruin_cast_visuals"][1]["attachment"]
        .as_str()
        .expect("source recipient attachment")
}
