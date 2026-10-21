//! A retained construction grant activates a native source-damage callback.
use super::*;
use crate::{
    ChainLightningEffectProfile, NativeBoltProfile, NativeEffectSource, NativeEffectSourceKind,
    ResolvedNativeEffectBinding,
};

fn granted_by_default() -> bool {
    true
}

/// A script callback on a building's landed weapon damage: each landed event rolls a chance and,
/// on success, a dummy at the building casts the native child at the damaged unit. The child is a
/// native Storm-Bolt-like bolt or a Chain Lightning; exactly one is present.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BuildingDamageProcProfile {
    pub source_rawcode: u32,
    pub helper: AbilityId,
    pub dispatcher: AbilityId,
    /// The dispatcher and helper are added at construction rather than listed on the object.
    #[serde(default = "granted_by_default")]
    pub granted_at_construction: bool,
    pub chance_per_10k: u16,
    pub range: i32,
    pub dummy_lifetime_ticks: u16,
    pub child_cooldown_ticks: u16,
    #[serde(default)]
    pub bolt: Option<NativeBoltProfile>,
    #[serde(default)]
    pub chain: Option<ChainLightningEffectProfile>,
}
impl BuildingDamageProcProfile {
    /// The native child ability the dummy casts.
    pub(crate) fn child_ability(&self) -> AbilityId {
        match (&self.bolt, &self.chain) {
            (Some(bolt), None) => bolt.ability,
            (None, Some(chain)) => chain.ability,
            _ => panic!("a building damage callback has exactly one native child"),
        }
    }

    pub(crate) fn canonical_words(self) -> Vec<u64> {
        let mut words = vec![
            u64::from(self.source_rawcode),
            u64::from(self.helper.0),
            u64::from(self.dispatcher.0),
            u64::from(self.chance_per_10k),
            self.range as u64,
            u64::from(self.dummy_lifetime_ticks),
            u64::from(self.child_cooldown_ticks),
            u64::from(self.granted_at_construction),
        ];
        match (self.bolt, self.chain) {
            (Some(bolt), None) => words.extend([
                1,
                u64::from(bolt.ability.0),
                bolt.damage as u64,
                u64::from(bolt.stun_ticks),
                u64::from(bolt.hero_stun_ticks),
                bolt.damage_per_second as u64,
                u64::from(bolt.duration_ticks),
                bolt.speed_per_tick as u64,
                u64::from(bolt.cleanse),
                u64::from(bolt.targets.bits()),
            ]),
            (None, Some(chain)) => words.extend([
                2,
                u64::from(chain.ability.0),
                chain.initial_damage as u64,
                u64::from(chain.maximum_targets),
                chain.jump_radius as u64,
                u64::from(chain.damage_reduction_per_10k),
                u64::from(chain.targets.bits()),
            ]),
            _ => panic!("a building damage callback has exactly one native child"),
        }
        words
    }
}

/// Every retained building damage callback, from the generated evidence.
fn damage_procs(version: MapVersion) -> &'static [BuildingDamageProcProfile] {
    static PROFILES: OnceLock<Vec<BuildingDamageProcProfile>> = OnceLock::new();
    PROFILES.get_or_init(|| {
        ["guard_damage_proc", "energy_burst_damage_proc"]
            .into_iter()
            .map(|key| {
                serde_json::from_value(evidence(version)[key].clone())
                    .expect("generated building damage callback")
            })
            .collect()
    })
}

pub(crate) fn damage_proc_for_version(
    version: MapVersion,
    rawcode: u32,
) -> Option<BuildingDamageProcProfile> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return None;
    }
    damage_procs(version)
        .iter()
        .find(|profile| profile.source_rawcode == rawcode)
        .copied()
}
/// Sources the source object does not list itself: the child always, and the dispatcher and helper
/// when construction grants them.
pub(crate) fn damage_proc_granted_sources(
    version: MapVersion,
    rawcode: u32,
) -> impl Iterator<Item = NativeEffectSource> {
    damage_proc_for_version(version, rawcode)
        .into_iter()
        .flat_map(|p| {
            let child = NativeEffectSource::new(NativeEffectSourceKind::AbilityEffect, p.child_ability().0);
            p.granted_at_construction
                .then_some([
                    NativeEffectSource::new(NativeEffectSourceKind::UnitAbility, p.dispatcher.0),
                    NativeEffectSource::new(NativeEffectSourceKind::UnitAbility, p.helper.0),
                ])
                .into_iter()
                .flatten()
                .chain(std::iter::once(child))
        })
}
pub(crate) fn damage_proc_bindings(version: MapVersion) -> Vec<ResolvedNativeEffectBinding> {
    damage_procs(version)
        .iter()
        .flat_map(|p| {
            p.granted_at_construction
                .then_some(NativeEffectSource::new(
                    NativeEffectSourceKind::UnitAbility,
                    p.helper.0,
                ))
                .into_iter()
                .chain(std::iter::once(NativeEffectSource::new(
                    NativeEffectSourceKind::AbilityEffect,
                    p.child_ability().0,
                )))
        })
        .map(|source| ResolvedNativeEffectBinding {
            source,
            implementation: crate::NativeEffectImplementationId::WarcraftBuildingDamageProcV1,
        })
        .collect()
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
