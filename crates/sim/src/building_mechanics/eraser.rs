use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingEraserProfile {
    pub map_version: MapVersion,
    pub ability: AbilityId,
    pub parent: crate::NativeParasiteProfile,
    pub trigger_targets: crate::AttackTargetMask,
    pub trigger_mechanical_only: bool,
    pub selector_exclusions: [AbilityId; 2],
    pub selector_excluded_rawcode: u32,
    pub excluded_ability: AbilityId,
    pub shield_ability: AbilityId,
    pub shield_minimum_level: u8,
    pub damage: i32,
}
impl BuildingEraserProfile {
    pub(crate) fn canonical_words(self) -> impl Iterator<Item = u64> {
        self.parent.canonical_words().into_iter().chain([
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.ability.0),
            u64::from(self.trigger_targets.bits()),
            u64::from(self.trigger_mechanical_only),
            u64::from(self.selector_exclusions[0].0),
            u64::from(self.selector_exclusions[1].0),
            u64::from(self.selector_excluded_rawcode),
            u64::from(self.excluded_ability.0),
            u64::from(self.shield_ability.0),
            u64::from(self.shield_minimum_level),
            self.damage as u64,
        ])
    }
}
pub(crate) fn eraser_spellcasting_for_version(
    version: MapVersion,
    building: u32,
) -> SpellcastingProfile {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static CASTS: OnceLock<std::collections::BTreeMap<String, SpellcastingProfile>> =
        OnceLock::new();
    let casts = CASTS.get_or_init(|| {
        serde_json::from_value(evidence(version)["eraser_spellcasting"].clone())
            .expect("retained Eraser tiers")
    });
    casts[std::str::from_utf8(&building.to_be_bytes()).unwrap()]
}
pub(crate) fn eraser_binding(version: MapVersion) -> crate::ResolvedNativeEffectBinding {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static BINDING: OnceLock<crate::ResolvedNativeEffectBinding> = OnceLock::new();
    *BINDING.get_or_init(|| {
        let casts = evidence(version)["eraser_spellcasting"]
            .as_object()
            .unwrap();
        let spell: SpellcastingProfile =
            serde_json::from_value(casts.values().next().unwrap().clone()).unwrap();
        crate::ResolvedNativeEffectBinding {
            source: crate::NativeEffectSource::new(
                crate::NativeEffectSourceKind::UnitAbility,
                spell.ability.id.0,
            ),
            implementation: crate::NativeEffectImplementationId::WarcraftEraserV1,
        }
    })
}
pub(crate) fn eraser_source(version: MapVersion, source: crate::NativeEffectSource) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27 && eraser_binding(version).source == source
}
#[must_use]
pub fn eraser_target_attachment_for_version(version: MapVersion) -> &'static str {
    evidence(version)["eraser_cast_visuals"][1]["attachment"]
        .as_str()
        .unwrap()
}
