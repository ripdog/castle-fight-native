//! Shared damage-listener branch that orders a damaged unit to attack a flying damage source.
use super::*;

impl Simulation {
    /// The listener runs for every damage event whose recipient is a registered unit type. It
    /// stamps its per-unit throttle before issuing the order, so an order the target cannot
    /// accept still consumes the interval. The order replaces any current target and persists
    /// like a direct personal-attacker engagement until that target is no longer retainable.
    pub(super) fn observe_flying_attacker_retarget(
        victim: usize,
        attacker: usize,
        completed_tick: u64,
        units: &mut [UnitSnapshot],
        health: &[i32],
    ) {
        if health[victim] <= 0
            || health[attacker] <= 0
            || units[victim].team == units[attacker].team
            || units[attacker].target_movement_class() != MovementClass::Air
        {
            return;
        }
        let Some(version) = units[victim].map_version else {
            return;
        };
        let Some(rule) = crate::native_effects::unit_flying_retarget_rule_for_version(
            version,
            units[victim].rawcode,
        ) else {
            return;
        };
        let last = units[victim].status.flying_retarget_tick.unwrap_or(0);
        if completed_tick <= last.saturating_add(u64::from(rule.throttle_ticks)) {
            return;
        }
        units[victim].status.flying_retarget_tick = Some(completed_tick);
        units[victim].target = Some(units[attacker].id);
        units[victim].direct_retaliation_lock = true;
        units[victim].ally_defense_lock = false;
    }
}
