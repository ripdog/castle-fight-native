//! Version-scoped Golden Shrine script projection, independent of native spell bindings.
use crate::MapVersion;
use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
pub struct GoldenShrineDefinition {
    pub parameters: GoldenShrineParameters,
    pub resurrection_model: String,
    pub dormant_attack_abilities: Vec<u32>,
    pub building_health_regen_per_second_per_10k: u32,
}

#[derive(Debug, Deserialize)]
pub struct GoldenShrineParameters {
    pub golden_shrine_unit_id: u32,
    pub chance_percent_per_shrine: u32,
    pub maximum_effective_chance_percent: u32,
    pub chance_roll_min: u32,
    pub chance_roll_max: u32,
    pub revive_delay_seconds: u64,
    pub exclude_legendary_marker_ability_id: u32,
    pub exclude_summoned_unit_marker_ability_id: u32,
}

#[must_use]
pub fn golden_shrine_definition_for_version(
    version: MapVersion,
) -> Option<&'static GoldenShrineDefinition> {
    static DEFINITION: OnceLock<GoldenShrineDefinition> = OnceLock::new();
    (version == MapVersion::CASTLE_FIGHT_9_27).then(|| {
        DEFINITION.get_or_init(|| {
            serde_json::from_str(include_str!(
                "../data/castle-fight/9.27/shrine-system-r1.json"
            ))
            .expect("validated retained Golden Shrine projection")
        })
    })
}
