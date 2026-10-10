use super::*;
use crate::{
    NativeBuffIdentity, NativeEffectImplementationId, NativeEffectSource, NativeEffectSourceKind,
    ResolvedNativeEffectBinding,
};
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct SilverGladeProfile {
    pub map_version: MapVersion,
    pub trigger_targets: crate::AttackTargetMask,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub trigger_mechanical_only: bool,
    pub effect: AbilityId,
    pub excluded: AbilityId,
    pub radius: i32,
    pub health: i32,
    pub mana: i32,
    pub armor_per_100: i16,
    pub buff: NativeBuffIdentity,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
}
impl SilverGladeProfile {
    pub(crate) fn canonical_words(self) -> [u64; 18] {
        [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.trigger_targets.bits()),
            self.trigger_invulnerable as u64,
            self.trigger_spell_immune as u64,
            self.trigger_mechanical_only as u64,
            u64::from(self.effect.0),
            u64::from(self.excluded.0),
            self.radius as u64,
            self.health as u64,
            self.mana as u64,
            self.armor_per_100 as u64,
            u64::from(self.buff.rawcode),
            self.buff.positive as u64,
            self.buff.stealable as u64,
            self.buff.organic_only as u64,
            u64::from(self.duration_ticks),
            u64::from(self.hero_duration_ticks),
        ]
    }
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct SilverGladeState {
    pub profile: SilverGladeProfile,
    pub charges: [u32; 2],
}
impl SilverGladeState {
    pub(crate) fn canonical_words(&self) -> impl Iterator<Item = u64> {
        self.profile
            .canonical_words()
            .into_iter()
            .chain(self.charges.into_iter().map(u64::from))
    }
}
pub(crate) fn silver_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    serde_json::from_value(evidence(version)["silver_spellcasting"].clone())
        .expect("retained Silver Glade projection")
}
pub(crate) fn silver_source(version: MapVersion, source: NativeEffectSource) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && silver_bindings(version).iter().any(|b| b.source == source)
}
pub(crate) fn silver_bindings(version: MapVersion) -> Vec<ResolvedNativeEffectBinding> {
    let spell = silver_spellcasting_for_version(version);
    let AbilityEffect::SilverGlade(profile) = spell.ability.effect else {
        unreachable!()
    };
    [
        (NativeEffectSourceKind::UnitAbility, spell.ability.id.0),
        (NativeEffectSourceKind::AbilityEffect, profile.effect.0),
    ]
    .into_iter()
    .map(|(kind, key)| ResolvedNativeEffectBinding {
        source: NativeEffectSource::new(kind, key),
        implementation: NativeEffectImplementationId::WarcraftSilverGladeV1,
    })
    .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct WildernessProfile {
    pub map_version: MapVersion,
    pub trigger_targets: crate::AttackTargetMask,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub trigger_mechanical_only: bool,
    pub grants: [crate::NativeAbilityGrant; 3],
    pub exclusions: [AbilityId; 4],
    pub locust: AbilityId,
}
impl WildernessProfile {
    pub(crate) fn canonical_words(self) -> impl Iterator<Item = u64> {
        [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.trigger_targets.bits()),
            self.trigger_invulnerable as u64,
            self.trigger_spell_immune as u64,
            self.trigger_mechanical_only as u64,
            u64::from(self.locust.0),
        ]
        .into_iter()
        .chain(self.exclusions.into_iter().map(|a| u64::from(a.0)))
        .chain(
            self.grants
                .into_iter()
                .flat_map(crate::NativeAbilityGrant::canonical_words),
        )
    }
}
pub(crate) fn wilderness_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    serde_json::from_value(evidence(version)["wilderness_spellcasting"].clone())
        .expect("retained Wilderness projection")
}
pub(crate) fn wilderness_bindings(version: MapVersion) -> Vec<ResolvedNativeEffectBinding> {
    let spell = wilderness_spellcasting_for_version(version);
    let AbilityEffect::Wilderness(p) = spell.ability.effect else {
        unreachable!()
    };
    std::iter::once((NativeEffectSourceKind::UnitAbility, spell.ability.id.0))
        .chain(
            p.grants
                .into_iter()
                .map(|g| (NativeEffectSourceKind::AbilityEffect, g.ability.0)),
        )
        .map(|(kind, key)| ResolvedNativeEffectBinding {
            source: NativeEffectSource::new(kind, key),
            implementation: NativeEffectImplementationId::WarcraftWildernessV1,
        })
        .collect()
}
pub(crate) fn wilderness_source(version: MapVersion, source: NativeEffectSource) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && wilderness_bindings(version)
            .iter()
            .any(|b| b.source == source)
}
