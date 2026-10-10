use super::*;

#[derive(Debug, serde::Deserialize)]
pub(crate) struct DeathHealDefinition {
    pub source_rawcode: u32,
    pub marker: u32,
    pub heal_per_building: i32,
    pub maximum_heal: i32,
    pub radius: i32,
    pub other_type_per_10k: u16,
    pub production_rawcodes: Vec<u32>,
    pub model_path: String,
    pub attachment: String,
}

pub fn death_heal_visual_for_version(version: MapVersion) -> Option<(&'static str, &'static str)> {
    let p = death_heal_for_version(version)?;
    Some((&p.model_path, &p.attachment))
}

pub(crate) fn death_heal_for_version(version: MapVersion) -> Option<&'static DeathHealDefinition> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return None;
    }
    static DEFINITION: OnceLock<DeathHealDefinition> = OnceLock::new();
    Some(DEFINITION.get_or_init(|| {
        serde_json::from_value(evidence(version)["linker_death_heal"].clone())
            .expect("generated native death healing")
    }))
}
