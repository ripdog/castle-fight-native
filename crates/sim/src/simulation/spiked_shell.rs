//! Native ANth/AUts and AEah: ordinary melee packets returned independently of armor.
//!
//! The engine's phase-1 damage listeners (`Game.dll` `0x6f5b2820`, `0x6f4ec790`) read the record
//! before the armor-point reduction, so the returned amount is a fraction of the unreduced hit;
//! the returned packet is Defensive/Plant kind and takes no armor points either.
use super::*;
use crate::components::{MAX_PASSIVE_UNIT_EFFECTS, MAX_TIMED_ARMOR_MODIFIERS};

const MAX_RETURN_PACKETS: usize = MAX_PASSIVE_UNIT_EFFECTS + MAX_TIMED_ARMOR_MODIFIERS;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct ReturnedDamage {
    pub damage: i32,
    pub thorns: bool,
}

pub(super) struct ShellDamage {
    pub incoming: i32,
    pub returned: [ReturnedDamage; MAX_RETURN_PACKETS],
    pub count: usize,
}

/// `unreduced` is the hit before armor; `damage` is the hit after armor.
pub(super) fn damage(unit: &UnitSnapshot, unreduced: i32, damage: i32, melee: bool) -> ShellDamage {
    let mut result = ShellDamage {
        incoming: damage,
        returned: [ReturnedDamage::default(); MAX_RETURN_PACKETS],
        count: 0,
    };
    if !melee || unreduced <= 0 {
        return result;
    }
    for effect in unit.passive_effects.iter() {
        let PassiveUnitEffect::SpikedShell(profile) = effect else {
            continue;
        };
        result.incoming = scaled(result.incoming, profile.received_damage_per_10k);
        if profile.returned_damage_per_10k > 0 {
            result.returned[result.count] = ReturnedDamage {
                damage: returned_amount(unreduced, profile.returned_damage_per_10k),
                thorns: false,
            };
            result.count += 1;
        }
    }
    for modifier in &unit.status.armor_modifiers[..usize::from(unit.status.armor_modifier_count)] {
        if modifier.melee_damage_return_per_10k > 0 {
            result.returned[result.count] = ReturnedDamage {
                damage: returned_amount(unreduced, modifier.melee_damage_return_per_10k),
                thorns: true,
            };
            result.count += 1;
        }
    }
    result
}

/// The native listeners clamp the returned amount to at least one point of damage.
fn returned_amount(unreduced: i32, factor: u32) -> i32 {
    scaled(unreduced, factor).max(1)
}

fn scaled(damage: i32, factor: u32) -> i32 {
    i32::try_from((i64::from(damage) * i64::from(factor) + 5_000) / 10_000)
        .expect("native shell damage exceeds i32")
}
