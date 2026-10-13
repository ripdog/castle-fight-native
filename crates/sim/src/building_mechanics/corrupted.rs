use super::*;
use crate::{
    NativeEffectImplementationId, NativeEffectSource, NativeEffectSourceKind,
    ResolvedNativeEffectBinding,
};

/// Well of Pain: a native Parasite building trigger followed by an independent scripted strike
/// that prefers wounded enemies and raises a friendly body from a lethal hit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct WellOfPainProfile {
    pub parent: crate::NativeParasiteProfile,
    pub map_version: MapVersion,
    pub trigger_targets: crate::AttackTargetMask,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub trigger_mechanical_only: bool,
    pub selector_exclusions: [AbilityId; 2],
    pub selector_excluded_rawcode: u32,
    pub wounded_exclusion: AbilityId,
    pub damage: i32,
    pub ghost_rawcode: u32,
}

impl WellOfPainProfile {
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
            u64::from(self.wounded_exclusion.0),
            self.damage as u64,
            u64::from(self.ghost_rawcode),
        ])
    }
}

pub(crate) fn well_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    serde_json::from_value(evidence(version)["well_spellcasting"].clone())
        .expect("retained Well of Pain projection")
}

pub(crate) fn well_bindings(version: MapVersion) -> Vec<ResolvedNativeEffectBinding> {
    let spell = well_spellcasting_for_version(version);
    vec![ResolvedNativeEffectBinding {
        source: NativeEffectSource::new(NativeEffectSourceKind::UnitAbility, spell.ability.id.0),
        implementation: NativeEffectImplementationId::WarcraftWellOfPainV1,
    }]
}

pub(crate) fn well_source(version: MapVersion, source: NativeEffectSource) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && well_bindings(version).iter().any(|b| b.source == source)
}
