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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct StarfallWaveProfile {
    pub ability: AbilityId,
    pub buff: u32,
    pub radius: i32,
    pub damage: i32,
    pub interval_ticks: u16,
    pub duration_ticks: u16,
    pub targets: crate::AttackTargetMask,
    pub building_damage_per_10k: u16,
    pub mana_cost: i32,
    pub cooldown_ticks: u16,
}
impl StarfallWaveProfile {
    pub(crate) fn canonical_words(self) -> [u64; 10] {
        [
            u64::from(self.ability.0),
            u64::from(self.buff),
            self.radius as u64,
            self.damage as u64,
            u64::from(self.interval_ticks),
            u64::from(self.duration_ticks),
            u64::from(self.targets.bits()),
            u64::from(self.building_damage_per_10k),
            self.mana_cost as u64,
            u64::from(self.cooldown_ticks),
        ]
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct StarfallProfile {
    pub map_version: MapVersion,
    pub trigger_targets: crate::AttackTargetMask,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub trigger_mechanical_only: bool,
    pub options: [StarfallWaveProfile; 2],
    pub selector_exclusions: [AbilityId; 2],
    pub selector_excluded_unit: u32,
    pub dummy_rawcode: u32,
    pub dummy_mana: i32,
    pub dummy_lifetime_ticks: u16,
    pub visual_duration_ticks: u16,
    pub vision_radius: i32,
}
impl StarfallProfile {
    pub(crate) fn canonical_words(self) -> impl Iterator<Item = u64> {
        [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.trigger_targets.bits()),
            self.trigger_invulnerable as u64,
            self.trigger_spell_immune as u64,
            self.trigger_mechanical_only as u64,
            u64::from(self.selector_excluded_unit),
            u64::from(self.dummy_rawcode),
            self.dummy_mana as u64,
            u64::from(self.dummy_lifetime_ticks),
            u64::from(self.visual_duration_ticks),
            self.vision_radius as u64,
        ]
        .into_iter()
        .chain(self.selector_exclusions.into_iter().map(|a| u64::from(a.0)))
        .chain(
            self.options
                .into_iter()
                .flat_map(StarfallWaveProfile::canonical_words),
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct StarfallState {
    pub map_version: MapVersion,
    pub source: crate::SimId,
    pub team: crate::Team,
    pub center: crate::SimPoint,
    pub profile: StarfallWaveProfile,
    pub mana_after_cast: i32,
    pub cooldown_until_tick: u64,
    pub expires_tick: u64,
    pub next_pulse_tick: u64,
}
impl StarfallState {
    pub(crate) fn canonical_words(&self) -> impl Iterator<Item = u64> {
        [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            self.source.0,
            u64::from(self.team.0),
            self.center.x as u64,
            self.center.y as u64,
            self.mana_after_cast as u64,
            self.cooldown_until_tick,
            self.expires_tick,
            self.next_pulse_tick,
        ]
        .into_iter()
        .chain(self.profile.canonical_words())
    }
}
pub(crate) fn starfall_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    serde_json::from_value(evidence(version)["starfall_spellcasting"].clone())
        .expect("retained Starfall projection")
}
pub(crate) fn starfall_source(version: MapVersion, source: NativeEffectSource) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && starfall_bindings(version)
            .iter()
            .any(|b| b.source == source)
}
pub(crate) fn starfall_bindings(version: MapVersion) -> Vec<ResolvedNativeEffectBinding> {
    let spell = starfall_spellcasting_for_version(version);
    let AbilityEffect::Starfall(p) = spell.ability.effect else {
        unreachable!()
    };
    std::iter::once((NativeEffectSourceKind::UnitAbility, spell.ability.id.0))
        .chain(
            p.options
                .into_iter()
                .map(|w| (NativeEffectSourceKind::AbilityEffect, w.ability.0)),
        )
        .map(|(kind, key)| ResolvedNativeEffectBinding {
            source: NativeEffectSource::new(kind, key),
            implementation: NativeEffectImplementationId::WarcraftStarfallV1,
        })
        .collect()
}
