//! The Power Plant: three building auras, a periodic disable-cleanse sweep and the production
//! augmentation of units trained by an aura-covered building.
use super::*;
use crate::{
    NativeEffectImplementationId, NativeEffectSource, NativeEffectSourceKind,
    ResolvedNativeEffectBinding,
};

/// One native aura the plant carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct PowerPlantAura {
    pub ability: AbilityId,
    pub buff: u32,
    pub radius: i32,
    #[serde(default)]
    pub bonus_per_100: i16,
    #[serde(default)]
    pub regeneration_per_second_per_10k: u32,
    #[serde(default)]
    pub damage_per_10k: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct PowerPlantCleanse {
    pub radius: i32,
    pub interval_ticks: u32,
    pub buffs: [u32; 2],
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct PowerPlantSpawnProfile {
    /// A building carrying this buff when it trains a unit augments that unit.
    pub requires_buff: u32,
    pub max_life_bonus: i32,
    pub life_ability: AbilityId,
    pub armor_ability: AbilityId,
    pub armor_bonus_per_100: i16,
    pub damage_ability: AbilityId,
    pub damage_per_10k: i16,
    pub spell_resist_ability: AbilityId,
    pub spell_damage_taken_per_10k: u16,
    /// Bodies already carrying any of these spell-defense abilities gain no extra resistance.
    pub spell_resist_exclusions: Vec<AbilityId>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub(crate) struct PowerPlantProfile {
    pub source_rawcode: u32,
    pub armor: PowerPlantAura,
    pub mana: PowerPlantAura,
    pub drums: PowerPlantAura,
    pub cleanse: PowerPlantCleanse,
    pub spawn: PowerPlantSpawnProfile,
}

pub(crate) fn power_plant_for_version(version: MapVersion) -> Option<&'static PowerPlantProfile> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return None;
    }
    static PROFILE: OnceLock<PowerPlantProfile> = OnceLock::new();
    Some(PROFILE.get_or_init(|| {
        serde_json::from_value(evidence(version)["power_plant_profile"].clone())
            .expect("generated Power Plant evidence")
    }))
}

fn abilities(profile: &PowerPlantProfile) -> impl Iterator<Item = NativeEffectSource> + '_ {
    [profile.armor.ability, profile.mana.ability, profile.drums.ability]
        .into_iter()
        .map(|ability| NativeEffectSource::new(NativeEffectSourceKind::UnitAbility, ability.0))
        .chain(
            [
                profile.spawn.life_ability,
                profile.spawn.armor_ability,
                profile.spawn.damage_ability,
                profile.spawn.spell_resist_ability,
            ]
            .into_iter()
            .map(|ability| {
                NativeEffectSource::new(NativeEffectSourceKind::AbilityEffect, ability.0)
            }),
        )
}

/// Script-granted abilities: the plant's three auras, and the four augmentation abilities.
pub(crate) fn power_plant_granted_sources(
    version: MapVersion,
    rawcode: u32,
) -> impl Iterator<Item = NativeEffectSource> {
    power_plant_for_version(version)
        .filter(|profile| profile.source_rawcode == rawcode)
        .into_iter()
        .flat_map(abilities)
}

pub(crate) fn power_plant_bindings(version: MapVersion) -> Vec<ResolvedNativeEffectBinding> {
    power_plant_for_version(version)
        .into_iter()
        .flat_map(abilities)
        .map(|source| ResolvedNativeEffectBinding {
            source,
            implementation: NativeEffectImplementationId::WarcraftPowerPlantV1,
        })
        .collect()
}

pub(crate) fn power_plant_source(version: MapVersion, source: NativeEffectSource) -> bool {
    power_plant_for_version(version).is_some_and(|profile| abilities(profile).any(|s| s == source))
}
