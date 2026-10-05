//! Versioned, reproducible retained-source profiles for native tower weapons/carriers.
use std::{collections::BTreeMap, sync::OnceLock};

#[cfg(test)]
mod projection_tests;

use bevy_ecs::prelude::Component;
use serde::{Deserialize, Serialize};

use crate::{
    components::{AbilityId, AttackTargetMask, PlayerId, SimId, Team},
    content::{CASTLE_FIGHT_SIMULATION_HZ, UnsupportedCastleFightMapVersion},
    damage::DamageType,
    math::{SUBUNITS_PER_WORLD_UNIT, SimPoint},
    version::MapVersion,
};

#[derive(Debug, Clone, Copy)]
pub struct NativeBarrageProfile {
    pub ability: AbilityId,
    /// Raw Efk3 field, not a literal total-target count (native Aroc has an offset).
    pub maximum_targets: u16,
    pub damage_per_target: i32,
    pub maximum_total_damage: i32,
    pub range: i32,
    pub targets: AttackTargetMask,
    pub speed_per_tick: i32,
}

impl NativeBarrageProfile {
    /// Aroc's positive Efk3 mode launches Efk3 + 1 additional missiles. Efk2 is
    /// a target cap only for the special low-count mode; it is not a damage budget.
    /// See docs/verification/elven-towers.md for native research and its version limits.
    #[must_use]
    pub fn additional_targets(self) -> usize {
        if self.maximum_targets == 0 {
            // Efk3=0 switches to Efk2-as-additional-target-count (zero means unlimited).
            if self.maximum_total_damage == 0 {
                usize::MAX
            } else {
                self.maximum_total_damage.max(0) as usize
            }
        } else if self.maximum_targets == 1 && self.maximum_total_damage == 2 {
            1
        } else {
            usize::from(self.maximum_targets) + 1
        }
    }
}

#[derive(Debug, Clone)]
pub struct NativeCarrierProfile {
    pub building_rawcode: u32,
    pub ability: AbilityId,
    pub buff: AbilityId,
    pub initial_damage: i32,
    pub cooldown_ticks: u64,
    pub range: i32,
    pub targets: AttackTargetMask,
    pub speed_per_tick: i32,
    pub buff_duration_ticks: u64,
    pub removed_persistent_abilities: Vec<AbilityId>,
    /// Retained buff object identities, distinct from ability-granted permanent modifiers.
    pub native_buff_ids: Vec<u32>,
    pub holy_health_bonus: i32,
    pub arcane_regeneration_per_second_per_10k: u32,
    pub obelisk_regeneration_per_second_per_10k: u32,
}

#[derive(Deserialize, Serialize)]
struct Projection {
    fields: BTreeMap<String, BTreeMap<String, serde_json::Value>>,
    cleanse: Cleanse,
    native_buff_ids: Vec<u32>,
    /// Retain release/source identity as part of the normalized content commitment.
    #[serde(flatten)]
    metadata: BTreeMap<String, serde_json::Value>,
}
#[derive(Deserialize, Serialize)]
struct Cleanse {
    building_unit_id: u32,
    carrier_effect_ability_id: u32,
    removed_persistent_ability_ids: Vec<u32>,
    #[serde(flatten)]
    metadata: BTreeMap<String, serde_json::Value>,
}

impl Projection {
    fn number(&self, object: &str, field: &str) -> f64 {
        let value = &self.fields[object][field];
        value
            .as_f64()
            .unwrap_or_else(|| value.as_str().unwrap().parse().unwrap())
    }
    fn targets(&self, object: &str) -> AttackTargetMask {
        hostile_native_targets(self.fields[object]["atar"].as_str().unwrap())
            .expect("native target qualifiers must be supported explicitly")
    }
}

/// This implementation supports hostile relation plus explicit physical target classes only.
/// Do not silently erase qualifiers (hero, organic, vulnerability, etc.) from future evidence.
fn hostile_native_targets(mask: &str) -> Result<AttackTargetMask, &'static str> {
    let mut seen = 0_u8;
    for token in mask.split(',').map(str::trim) {
        let bit = match token {
            "ground" => 1,
            "air" => 2,
            "structure" => 4,
            "enemy" | "enemies" => 8,
            _ => return Err("unsupported hostile native target qualifier"),
        };
        if seen & bit != 0 {
            return Err("duplicate hostile native target qualifier");
        }
        seen |= bit;
    }
    if seen & 8 == 0 || seen & 7 == 0 {
        return Err("hostile native targeting requires enemy relation and a physical class");
    }
    Ok(AttackTargetMask::from_capabilities(
        seen & 1 != 0,
        seen & 2 != 0,
        seen & 4 != 0,
    ))
}

fn projection(
    version: MapVersion,
) -> Result<&'static Projection, UnsupportedCastleFightMapVersion> {
    if version != MapVersion::CASTLE_FIGHT_9_27 {
        return Err(UnsupportedCastleFightMapVersion(version));
    }
    static SOURCE: OnceLock<Projection> = OnceLock::new();
    Ok(SOURCE.get_or_init(|| {
        serde_json::from_str(include_str!(
            "../data/castle-fight/9.27/native-carriers.json"
        ))
        .expect("generated native carrier projection")
    }))
}

/// Serialization is over parsed, ordered fields, never the artifact's raw formatting.
pub(crate) fn canonical_projection_for_version(
    version: MapVersion,
) -> Result<Vec<u8>, UnsupportedCastleFightMapVersion> {
    Ok(serde_json::to_vec(projection(version)?).expect("serializable retained native projection"))
}

pub fn barrage_for_version(
    version: MapVersion,
) -> Result<NativeBarrageProfile, UnsupportedCastleFightMapVersion> {
    let p = projection(version)?;
    Ok(NativeBarrageProfile {
        ability: AbilityId(u32::from_be_bytes(*b"A015")),
        maximum_targets: p.number("A015", "Efk3") as u16,
        damage_per_target: p.number("A015", "Efk1") as i32,
        maximum_total_damage: p.number("A015", "Efk2") as i32,
        range: (p.number("A015", "aare") * f64::from(SUBUNITS_PER_WORLD_UNIT)) as i32,
        targets: p.targets("A015"),
        speed_per_tick: (p.number("A015", "amsp") * f64::from(SUBUNITS_PER_WORLD_UNIT)
            / f64::from(CASTLE_FIGHT_SIMULATION_HZ))
        .round() as i32,
    })
}

pub fn carrier_for_version(
    version: MapVersion,
) -> Result<&'static NativeCarrierProfile, UnsupportedCastleFightMapVersion> {
    let p = projection(version)?;
    static PROFILE: OnceLock<NativeCarrierProfile> = OnceLock::new();
    Ok(PROFILE.get_or_init(|| NativeCarrierProfile {
        building_rawcode: p.cleanse.building_unit_id,
        ability: AbilityId(p.cleanse.carrier_effect_ability_id),
        buff: AbilityId(u32::from_be_bytes(
            p.fields["A000"]["abuf"]
                .as_str()
                .unwrap()
                .as_bytes()
                .try_into()
                .unwrap(),
        )),
        initial_damage: p.number("A000", "pxf1") as i32,
        cooldown_ticks: (p.number("A000", "acdn") * f64::from(CASTLE_FIGHT_SIMULATION_HZ)).ceil()
            as u64,
        range: (p.number("A000", "aare") * f64::from(SUBUNITS_PER_WORLD_UNIT)) as i32,
        targets: p.targets("A000"),
        speed_per_tick: (p.number("A000", "amsp") * f64::from(SUBUNITS_PER_WORLD_UNIT)
            / f64::from(CASTLE_FIGHT_SIMULATION_HZ))
        .round() as i32,
        buff_duration_ticks: (p.number("A000", "adur") * f64::from(CASTLE_FIGHT_SIMULATION_HZ))
            .ceil() as u64,
        removed_persistent_abilities: p
            .cleanse
            .removed_persistent_ability_ids
            .iter()
            .copied()
            .map(AbilityId)
            .collect(),
        native_buff_ids: p.native_buff_ids.clone(),
        holy_health_bonus: p.number("A03D", "Ilif") as i32,
        arcane_regeneration_per_second_per_10k: (p.number("h014", "uhpr") * 10_000.0) as u32,
        obelisk_regeneration_per_second_per_10k: (p.number("h005", "uhpr") * 10_000.0) as u32,
    }))
}

/// Carrier ownership and independent projectiles share an isolated canonical entity family.
/// Removing the carrier stops new launches, not missiles already flying. Their native damage
/// survives source removal, but the script's source-has-A000 cleanse predicate no longer does.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) enum NativeCarrierState {
    Carrier {
        building: SimId,
        owner: Option<PlayerId>,
        team: Team,
        position: SimPoint,
        map_version: MapVersion,
        ready_tick: u64,
        sequence: u64,
    },
    Bolt(NativeCarrierBolt),
    Regeneration {
        building: SimId,
        map_version: MapVersion,
        per_second_per_10k: u32,
        remainder: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NativeCarrierBolt {
    pub source: SimId,
    pub visual_source: SimId,
    pub source_team: Team,
    pub target: SimId,
    pub ability: AbilityId,
    pub map_version: MapVersion,
    pub damage: i32,
    /// None for spell damage; Barrage retains the ordinary weapon attack type.
    pub attack_damage_type: Option<DamageType>,
    pub launch_position: SimPoint,
    pub launch_tick: u64,
    /// Live native homing position; launch identity remains immutable for replay/presentation.
    pub position: SimPoint,
    pub position_tick: u64,
    pub impact_tick: u64,
}
