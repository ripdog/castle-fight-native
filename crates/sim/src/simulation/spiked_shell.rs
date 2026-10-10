//! Native ANth/AUts: ordinary melee packets, independently reduced and returned after armor.
use super::*;
use crate::components::MAX_PASSIVE_UNIT_EFFECTS;

pub(super) struct ShellDamage {
    pub incoming: i32,
    pub returned: [i32; MAX_PASSIVE_UNIT_EFFECTS],
    pub count: usize,
}

pub(super) fn damage(unit: &UnitSnapshot, damage: i32, melee: bool) -> ShellDamage {
    let mut result = ShellDamage {
        incoming: damage,
        returned: [0; MAX_PASSIVE_UNIT_EFFECTS],
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
            result.returned[result.count] = returned;
            result.count += 1;
        }
    }
    result
}

fn scaled(damage: i32, factor: u32) -> i32 {
    i32::try_from((i64::from(damage) * i64::from(factor) + 5_000) / 10_000)
        .expect("native shell damage exceeds i32")
}
