//! Native ANth/AUts and AEah: ordinary melee packets returned independently after armor.
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

pub(super) fn damage(unit: &UnitSnapshot, damage: i32, melee: bool) -> ShellDamage {
    let mut result = ShellDamage {
        incoming: damage,
        returned: [ReturnedDamage::default(); MAX_RETURN_PACKETS],
        count: 0,
    };
    if !melee || damage <= 0 {
        return result;
    }
    for effect in unit.passive_effects.iter() {
        let PassiveUnitEffect::SpikedShell(profile) = effect else {
            continue;
        };
        result.incoming = scaled(result.incoming, profile.received_damage_per_10k);
        let returned = scaled(damage, profile.returned_damage_per_10k);
        if returned > 0 {
            result.returned[result.count] = ReturnedDamage {
                damage: returned,
                thorns: false,
            };
            result.count += 1;
        }
    }
    for modifier in &unit.status.armor_modifiers[..usize::from(unit.status.armor_modifier_count)] {
        let returned = scaled(damage, modifier.melee_damage_return_per_10k);
        if returned > 0 {
            result.returned[result.count] = ReturnedDamage {
                damage: returned,
                thorns: true,
            };
            result.count += 1;
        }
    }
    result
}

fn scaled(damage: i32, factor: u32) -> i32 {
    i32::try_from((i64::from(damage) * i64::from(factor) + 5_000) / 10_000)
        .expect("native shell damage exceeds i32")
}
