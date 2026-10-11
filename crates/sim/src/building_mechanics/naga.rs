use super::*;
use crate::{
    NativeEffectImplementationId, NativeEffectSource, NativeEffectSourceKind,
    ResolvedNativeEffectBinding,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NagaEarthquakeProfile {
    pub parent: crate::NativeParasiteProfile,
    pub map_version: MapVersion,
    pub trigger_targets: crate::AttackTargetMask,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub trigger_mechanical_only: bool,
    pub selector_exclusions: [AbilityId; 2],
    pub child: AbilityId,
    pub buff: u32,
    pub radius: i32,
    pub damage: i32,
    pub movement_percent_delta: i16,
    pub attack_speed_percent_delta: i16,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
    pub targets: crate::AttackTargetMask,
}
impl NagaEarthquakeProfile {
    pub(crate) fn canonical_words(self) -> impl Iterator<Item = u64> {
        self.parent.canonical_words().into_iter().chain([
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.trigger_targets.bits()),
            self.trigger_invulnerable as u64,
            self.trigger_spell_immune as u64,
            self.trigger_mechanical_only as u64,
            u64::from(self.selector_exclusions[0].0),
            u64::from(self.selector_exclusions[1].0),
            u64::from(self.child.0),
            u64::from(self.buff),
            self.radius as u64,
            self.damage as u64,
            self.movement_percent_delta as u64,
            self.attack_speed_percent_delta as u64,
            u64::from(self.duration_ticks),
            u64::from(self.hero_duration_ticks),
            u64::from(self.targets.bits()),
        ])
    }
}
pub(crate) fn pyramid_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    serde_json::from_value(evidence(version)["pyramid_spellcasting"].clone())
        .expect("retained Pyramide projection")
}
pub(crate) fn pyramid_bindings(version: MapVersion) -> Vec<ResolvedNativeEffectBinding> {
    let spell = pyramid_spellcasting_for_version(version);
    let AbilityEffect::NagaEarthquake(p) = spell.ability.effect else {
        unreachable!()
    };
    [
        (NativeEffectSourceKind::UnitAbility, spell.ability.id.0),
        (NativeEffectSourceKind::AbilityEffect, p.child.0),
    ]
    .into_iter()
    .map(|(kind, key)| ResolvedNativeEffectBinding {
        source: NativeEffectSource::new(kind, key),
        implementation: NativeEffectImplementationId::WarcraftNagaEarthquakeV1,
    })
    .collect()
}
pub(crate) fn pyramid_source(version: MapVersion, source: NativeEffectSource) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && pyramid_bindings(version).iter().any(|b| b.source == source)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct NagaCharmProfile {
    pub parent: crate::NativeParasiteProfile,
    pub map_version: MapVersion,
    pub trigger_targets: crate::AttackTargetMask,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub trigger_mechanical_only: bool,
    pub selector_exclusions: [AbilityId; 2],
    pub selector_excluded_rawcode: u32,
    pub child: AbilityId,
    pub child_range: i32,
    pub targets: crate::AttackTargetMask,
    pub maximum_creep_level: u16,
    pub ownership_rejection: AbilityId,
    pub order_block: AbilityId,
    pub defender_rawcode: u32,
    pub defender_delay_ticks: u16,
    pub vision_radius: i32,
    pub vision_ticks: u16,
}
impl NagaCharmProfile {
    pub(crate) fn canonical_words(self) -> impl Iterator<Item = u64> {
        self.parent.canonical_words().into_iter().chain([
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.trigger_targets.bits()),
            self.trigger_invulnerable as u64,
            self.trigger_spell_immune as u64,
            self.trigger_mechanical_only as u64,
            u64::from(self.selector_exclusions[0].0),
            u64::from(self.selector_exclusions[1].0),
            u64::from(self.selector_excluded_rawcode),
            u64::from(self.child.0),
            self.child_range as u64,
            u64::from(self.targets.bits()),
            u64::from(self.maximum_creep_level),
            u64::from(self.ownership_rejection.0),
            u64::from(self.order_block.0),
            u64::from(self.defender_rawcode),
            u64::from(self.defender_delay_ticks),
            self.vision_radius as u64,
            u64::from(self.vision_ticks),
        ])
    }
}
pub(crate) fn oracle_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    serde_json::from_value(evidence(version)["oracle_spellcasting"].clone())
        .expect("retained Oracle projection")
}
pub(crate) fn oracle_bindings(version: MapVersion) -> Vec<ResolvedNativeEffectBinding> {
    let spell = oracle_spellcasting_for_version(version);
    let AbilityEffect::NagaCharm(p) = spell.ability.effect else {
        unreachable!()
    };
    [
        (NativeEffectSourceKind::UnitAbility, spell.ability.id.0),
        (NativeEffectSourceKind::AbilityEffect, p.child.0),
    ]
    .into_iter()
    .map(|(kind, key)| ResolvedNativeEffectBinding {
        source: NativeEffectSource::new(kind, key),
        implementation: NativeEffectImplementationId::WarcraftNagaCharmV1,
    })
    .collect()
}
pub(crate) fn oracle_source(version: MapVersion, source: NativeEffectSource) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && oracle_bindings(version).iter().any(|b| b.source == source)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NativeBuildingAuraProfile {
    pub map_version: MapVersion,
    pub source_rawcode: u32,
    pub inventory: AbilityId,
    pub armor_ability: AbilityId,
    pub armor_buff: u32,
    pub armor_radius: i32,
    pub armor_bonus_per_100: i16,
    pub armor_targets: crate::AttackTargetMask,
    pub armor_invulnerable: bool,
    pub regeneration_ability: AbilityId,
    pub regeneration_radius: i32,
    pub regeneration_per_second_per_10k: u32,
    pub regeneration_targets: crate::AttackTargetMask,
    pub regeneration_invulnerable: bool,
}
pub(crate) fn coral_auras_for_version(version: MapVersion) -> Option<NativeBuildingAuraProfile> {
    static PROFILE: std::sync::OnceLock<NativeBuildingAuraProfile> = std::sync::OnceLock::new();
    (version == MapVersion::CASTLE_FIGHT_9_27).then(|| {
        *PROFILE.get_or_init(|| {
            serde_json::from_value(evidence(version)["coral_auras"].clone())
                .expect("retained Coral auras")
        })
    })
}
pub(crate) fn coral_bindings(version: MapVersion) -> Vec<ResolvedNativeEffectBinding> {
    let Some(p) = coral_auras_for_version(version) else {
        return Vec::new();
    };
    [p.inventory, p.armor_ability, p.regeneration_ability]
        .into_iter()
        .map(|a| ResolvedNativeEffectBinding {
            source: NativeEffectSource::new(NativeEffectSourceKind::UnitAbility, a.0),
            implementation: NativeEffectImplementationId::WarcraftNativeBuildingAurasV1,
        })
        .collect()
}
pub(crate) fn coral_source(version: MapVersion, source: NativeEffectSource) -> bool {
    coral_bindings(version).iter().any(|b| b.source == source)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct NagaTidalWaveProfile {
    pub map_version: MapVersion,
    pub source_rawcode: u32,
    pub parent: crate::NativeParasiteProfile,
    pub trigger_targets: crate::AttackTargetMask,
    pub trigger_nonhero: bool,
    pub trigger_sapper: bool,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub selector_exclusions: [AbilityId; 2],
    pub child_range: i32,
    pub vision_radius: i32,
    pub vision_ticks: u16,
    pub wave: crate::ConeWaveProfile,
    pub weapon_sapper: bool,
    pub weapon_vulnerable: bool,
}
impl NagaTidalWaveProfile {
    pub(crate) fn canonical_words(self) -> impl Iterator<Item = u64> {
        self.parent
            .canonical_words()
            .into_iter()
            .chain(self.wave.canonical_words())
            .chain([
                u64::from(self.map_version.major),
                u64::from(self.map_version.minor),
                u64::from(self.source_rawcode),
                u64::from(self.trigger_targets.bits()),
                self.trigger_nonhero as u64,
                self.trigger_sapper as u64,
                self.trigger_invulnerable as u64,
                self.trigger_spell_immune as u64,
                u64::from(self.selector_exclusions[0].0),
                u64::from(self.selector_exclusions[1].0),
                self.child_range as u64,
                self.vision_radius as u64,
                u64::from(self.vision_ticks),
                self.weapon_sapper as u64,
                self.weapon_vulnerable as u64,
            ])
    }
}
pub(crate) fn tidal_spellcasting_for_version(version: MapVersion) -> Option<SpellcastingProfile> {
    static PROFILE: std::sync::OnceLock<SpellcastingProfile> = std::sync::OnceLock::new();
    (version == MapVersion::CASTLE_FIGHT_9_27).then(|| {
        *PROFILE.get_or_init(|| {
            serde_json::from_value(evidence(version)["tidal_spellcasting"].clone())
                .expect("retained Tidal projection")
        })
    })
}
pub(crate) fn tidal_bindings(version: MapVersion) -> Vec<ResolvedNativeEffectBinding> {
    let Some(spell) = tidal_spellcasting_for_version(version) else {
        return Vec::new();
    };
    let AbilityEffect::NagaTidalWave(p) = spell.ability.effect else {
        unreachable!()
    };
    [
        (NativeEffectSourceKind::UnitAbility, spell.ability.id.0),
        (NativeEffectSourceKind::AbilityEffect, p.wave.ability.0),
    ]
    .into_iter()
    .map(|(kind, key)| ResolvedNativeEffectBinding {
        source: NativeEffectSource::new(kind, key),
        implementation: NativeEffectImplementationId::WarcraftNagaTidalWaveV1,
    })
    .collect()
}
pub(crate) fn tidal_source(version: MapVersion, source: NativeEffectSource) -> bool {
    tidal_bindings(version).iter().any(|b| b.source == source)
}

impl std::hash::Hash for NagaTidalWaveProfile {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for word in self.canonical_words() {
            std::hash::Hash::hash(&word, state);
        }
    }
}
