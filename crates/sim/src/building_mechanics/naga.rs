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
