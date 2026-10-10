//! A retained construction grant activates a native source-damage callback.
use super::*;
use crate::{
    NativeBoltProfile, NativeEffectSource, NativeEffectSourceKind, ResolvedNativeEffectBinding,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BuildingDamageProcProfile {
    pub source_rawcode: u32,
    pub helper: AbilityId,
    pub dispatcher: AbilityId,
    pub chance_per_10k: u16,
    pub range: i32,
    pub dummy_lifetime_ticks: u16,
    pub child_cooldown_ticks: u16,
    pub bolt: NativeBoltProfile,
}
impl BuildingDamageProcProfile {
    pub(crate) fn canonical_words(self) -> [u64; 16] {
        [
            u64::from(self.source_rawcode),
            u64::from(self.helper.0),
            u64::from(self.dispatcher.0),
            u64::from(self.chance_per_10k),
            self.range as u64,
            u64::from(self.dummy_lifetime_ticks),
            u64::from(self.child_cooldown_ticks),
            u64::from(self.bolt.ability.0),
            self.bolt.damage as u64,
            u64::from(self.bolt.stun_ticks),
            u64::from(self.bolt.hero_stun_ticks),
            self.bolt.damage_per_second as u64,
            u64::from(self.bolt.duration_ticks),
            self.bolt.speed_per_tick as u64,
            u64::from(self.bolt.cleanse),
            u64::from(self.bolt.targets.bits()),
        ]
    }
}

pub(crate) fn damage_proc_for_version(
    version: MapVersion,
    rawcode: u32,
) -> Option<BuildingDamageProcProfile> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return None;
    }
    static PROFILE: OnceLock<BuildingDamageProcProfile> = OnceLock::new();
    let profile = *PROFILE.get_or_init(|| {
        serde_json::from_value(evidence(version)["guard_damage_proc"].clone())
            .expect("generated building damage callback")
    });
    (rawcode == profile.source_rawcode).then_some(profile)
}
pub(crate) fn damage_proc_granted_sources(
    version: MapVersion,
    rawcode: u32,
) -> impl Iterator<Item = NativeEffectSource> {
    damage_proc_for_version(version, rawcode)
        .into_iter()
        .flat_map(|p| {
            [
                NativeEffectSource::new(NativeEffectSourceKind::UnitAbility, p.dispatcher.0),
                NativeEffectSource::new(NativeEffectSourceKind::UnitAbility, p.helper.0),
                NativeEffectSource::new(NativeEffectSourceKind::AbilityEffect, p.bolt.ability.0),
            ]
        })
}
pub(crate) fn damage_proc_bindings(version: MapVersion) -> [ResolvedNativeEffectBinding; 2] {
    let rawcode = evidence(version)["guard_damage_proc"]["source_rawcode"]
        .as_u64()
        .unwrap() as u32;
    let p = damage_proc_for_version(version, rawcode).unwrap();
    [
        NativeEffectSource::new(NativeEffectSourceKind::UnitAbility, p.helper.0),
        NativeEffectSource::new(NativeEffectSourceKind::AbilityEffect, p.bolt.ability.0),
    ]
    .map(|source| ResolvedNativeEffectBinding {
        source,
        implementation: crate::NativeEffectImplementationId::WarcraftBuildingDamageProcV1,
    })
}
pub(crate) fn damage_proc_source(version: MapVersion, source: NativeEffectSource) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && damage_proc_bindings(version)
            .iter()
            .any(|b| b.source == source)
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct BuildingDamageProcState {
    pub map_version: MapVersion,
    pub source: crate::SimId,
    pub team: crate::Team,
    pub origin: crate::SimPoint,
    pub profile: BuildingDamageProcProfile,
    pub event_sequence: u64,
}
impl BuildingDamageProcState {
    pub(crate) fn canonical_words(&self) -> impl Iterator<Item = u64> {
        [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            self.source.0,
            u64::from(self.team.0),
            self.origin.x as u64,
            self.origin.y as u64,
            self.event_sequence,
        ]
        .into_iter()
        .chain(self.profile.canonical_words())
    }
}
