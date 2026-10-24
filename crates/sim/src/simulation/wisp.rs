//! Ancient of Wonders: released forest wisps and the death blast they leave behind.
use super::*;
use crate::{
    CastleFightUnitKind,
    components::{
        BuildingChild, DeathBurstState, DeathSlowField, NativeAction, WispPoolState,
        WispReleaseProfile, WispReleaseState,
    },
};

impl Simulation {
    /// `A0BS` cast closure: six wisps follow, one per period, for the casting player.
    pub(super) fn cast_wisp_release(
        &mut self,
        source: AbilitySourceSnapshot,
        profile: WispReleaseProfile,
        units: &[UnitSnapshot],
    ) -> usize {
        let Some(owner) = self
            .world
            .get::<Owner>(self.source_entity(source, units))
            .map(|owner| owner.0)
        else {
            return 0;
        };
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::WispRelease(WispReleaseState {
                owner,
                team: source.team,
                profile,
                iteration: 0,
                // doPeriodically fires its first callback one period after the cast.
                next_tick: self.next_tick + u64::from(profile.period_ticks),
            }),
        ));
        1
    }

    /// `iD`: completing the source building (re)starts the shared repeating refill timer.
    pub(super) fn note_building_completed(&mut self, building: BuildingSpawn) {
        let Some(AbilityEffect::BuildingScript(script)) =
            building.spellcasting.map(|profile| profile.ability.effect)
        else {
            return;
        };
        let BuildingChild::WispRelease(profile) = script.child else {
            return;
        };
        let next_refill_tick = self.next_tick + u64::from(profile.refill_ticks);
        let mut pools = self.world.query::<&mut NativeAction>();
        for mut action in pools.iter_mut(&mut self.world) {
            // `TimerStart` on the running timer restarts its period.
            if let NativeAction::WispPool(pool) = &mut *action {
                pool.next_refill_tick = next_refill_tick;
                return;
            }
        }
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::WispPool(WispPoolState {
                profile,
                occupied: 0,
                next_refill_tick,
            }),
        ));
    }

    /// `MC`: tops up the first empty slot of each pool group (the walk skips a group's first
    /// slot after filling the previous group, exactly as the script does).
    pub(super) fn advance_wisp_pool(&mut self, entity: Entity, mut state: WispPoolState) {
        let profile = state.profile;
        let mut changed = false;
        while self.next_tick >= state.next_refill_tick {
            state.occupied = refill_pool(state.occupied, profile);
            state.next_refill_tick += u64::from(profile.refill_ticks);
            changed = true;
        }
        if changed && let Some(mut action) = self.world.get_mut::<NativeAction>(entity) {
            *action = NativeAction::WispPool(state);
        }
    }

    /// `sA`: the first occupied slot at or after the group's first slot, which it empties.
    fn take_pool_wisp(&mut self, group: u8) -> Option<(u8, WispReleaseProfile)> {
        let mut pools = self.world.query::<&mut NativeAction>();
        for mut action in pools.iter_mut(&mut self.world) {
            let NativeAction::WispPool(pool) = &mut *action else {
                continue;
            };
            let profile = pool.profile;
            let (slot, occupied) = take_pool_slot(pool.occupied, group, profile)?;
            pool.occupied = occupied;
            return Some((slot, profile));
        }
        None
    }

    pub(super) fn advance_wisp_release(&mut self, entity: Entity, mut state: WispReleaseState) {
        if self.next_tick < state.next_tick {
            return;
        }
        let profile = state.profile;
        // The pool group of the owner's team side, alternating between its two corners. An
        // exhausted pool leaves the callback with no unit, so its natives do nothing.
        let group = 2 * state.team.0 + state.iteration % 2;
        if let Some((slot, pool)) = self.take_pool_wisp(group) {
            let [_, _, max_x, max_y] =
                crate::building_mechanics::snowveil_for_version(profile.map_version)
                    .battlefield_bounds;
            let side =
                |negative: bool, magnitude: i32| if negative { -magnitude } else { magnitude };
            let position = SimPoint {
                x: side(slot < pool.west_slots, pool.x_limit.min(max_x)),
                y: side(
                    slot % pool.y_period < pool.y_south_below,
                    max_y - pool.y_inset,
                ),
            };
            let mut definition = CastleFightUnitKind::from_rawcode_for_version(
                profile.wisp_rawcode,
                profile.map_version,
            )
            .unwrap()
            .expect("promoted wisp")
            .definition_for_version(profile.map_version)
            .unwrap()
            .resolved();
            // The release adds the combat-body type to the pooled neutral wisp.
            definition.properties.classifications.combat_sapper = true;
            let child = self.spawn_resolved_unit_unchecked(
                Some(state.owner),
                UnitSpawn::from_template(state.team, position, definition.template),
                definition,
            );
            let id = self.allocate_id();
            self.world.spawn((
                id,
                NativeAction::TimedLife {
                    child,
                    expires_tick: self.next_tick + u64::from(profile.life_ticks),
                },
            ));
        }
        state.iteration += 1;
        state.next_tick += u64::from(profile.period_ticks);
        if state.iteration >= profile.releases {
            self.world.despawn(entity);
        } else if let Some(mut action) = self.world.get_mut::<NativeAction>(entity) {
            *action = NativeAction::WispRelease(state);
        }
    }

    /// `fJ`: a dying combat body whose rule carries a death blast leaves one at its position.
    pub(super) fn observe_death_bursts(
        &mut self,
        units: &[UnitSnapshot],
        health: &[i32],
        positions: &[SimPoint],
    ) {
        for (i, unit) in units.iter().enumerate() {
            if health[i] > 0 || unit.removed_without_death || !unit.classifications.combat_sapper {
                continue;
            }
            let (Some(rule), Some(map_version)) = (
                unit.death_rule.and_then(|rule| rule.death_burst),
                unit.map_version,
            ) else {
                continue;
            };
            let id = self.allocate_id();
            self.world.spawn((
                id,
                NativeAction::DeathBurst(DeathBurstState {
                    map_version,
                    source: unit.id,
                    team: unit.team,
                    center: positions[i],
                    rule,
                    struck: false,
                    expires_tick: self.next_tick + u64::from(rule.slow.duration_ticks),
                }),
            ));
        }
    }

    pub(super) fn advance_death_burst(
        &mut self,
        entity: Entity,
        mut state: DeathBurstState,
        units: &mut [UnitSnapshot],
    ) {
        if !state.struck {
            state.struck = true;
            let rule = state.rule;
            let radius_sq = square_i32(rule.radius);
            for index in 0..units.len() {
                let unit = &units[index];
                if unit.team == state.team
                    || unit.health <= 0
                    || !unit.classifications.combat_sapper
                    || unit.classifications.locust
                    || unit.classifications.invulnerable
                    || unit.classifications.spell_immune
                    || unit.rawcode == rule.excluded_type
                    || crate::content::unit_has_ability_for_version(
                        unit.rawcode,
                        rule.excluded_ability,
                        state.map_version,
                    )
                    || state.center.distance_sq(unit.position) > radius_sq
                {
                    continue;
                }
                self.apply_source_ability_to_unit(
                    state.source,
                    state.team,
                    index,
                    AbilityEffect::Damage {
                        amount: rule.damage,
                    },
                    units,
                );
            }
        }
        if self.next_tick >= state.expires_tick {
            self.world.despawn(entity);
            return;
        }
        self.apply_slow_field(state.center, state.team, state.rule.slow, units);
        if let Some(mut action) = self.world.get_mut::<NativeAction>(entity) {
            *action = NativeAction::DeathBurst(state);
        }
    }

    /// `Aasl`: hostile combat bodies inside the radius are slowed for as long as they stay in it.
    pub(super) fn apply_slow_field(
        &self,
        center: SimPoint,
        team: Team,
        slow: DeathSlowField,
        units: &mut [UnitSnapshot],
    ) {
        apply_slow_aura(
            self.next_tick,
            center,
            team,
            (
                slow.ability,
                slow.buff,
                slow.radius,
                slow.movement_percent_delta,
                slow.attack_speed_percent_delta,
                slow.targets,
                slow.affects_invulnerable,
            ),
            units,
        );
    }
}

/// Shared hostile slow aura: refreshed every tick, so it ends as soon as a body leaves.
pub(super) fn apply_slow_aura(
    tick: u64,
    center: SimPoint,
    team: Team,
    (ability, buff, radius, movement, attack_speed, targets, affects_invulnerable): (
        AbilityId,
        u32,
        i32,
        i16,
        i16,
        AttackTargetMask,
        bool,
    ),
    units: &mut [UnitSnapshot],
) {
    let expiry = tick + 1;
    let id = ModifierId(ability.0);
    for unit in units.iter_mut() {
        if unit.team == team
            || unit.health <= 0
            || unit.classifications.locust
            || !unit.classifications.combat_sapper
            || (unit.classifications.invulnerable && !affects_invulnerable)
            || !targets.can_target_unit(unit.target_movement_class())
            || center.distance_sq(unit.position) > square_i32(radius)
        {
            continue;
        }
        apply_timed_armor_modifier(
            &mut unit.status,
            TimedArmorModifier {
                undispellable: false,
                aura_membership: true,
                id,
                native_buff: Some(crate::NativeBuffIdentity {
                    rawcode: buff,
                    positive: false,
                    stealable: false,
                    organic_only: false,
                }),
                expires_tick: expiry,
                ..Default::default()
            },
        );
        apply_timed_movement_modifier(&mut unit.status, id, movement, expiry);
        apply_timed_attack_speed_modifier(&mut unit.status, id, attack_speed, expiry);
    }
}

/// `MC`: fills the first empty slot of each group. After a fill the walk jumps to the next
/// group's first slot and then steps once more, so that group's first slot is not examined.
fn refill_pool(mut occupied: u32, profile: WispReleaseProfile) -> u32 {
    let mut slot = 0;
    while slot < profile.pool_slots {
        if occupied & (1 << slot) == 0 {
            occupied |= 1 << slot;
            slot = (slot / profile.slots_per_group + 1) * profile.slots_per_group;
        }
        slot += 1;
    }
    occupied
}

/// `sA`: the first occupied slot at or after the group's first slot (later groups included).
fn take_pool_slot(occupied: u32, group: u8, profile: WispReleaseProfile) -> Option<(u8, u32)> {
    let slot = (group.checked_mul(profile.slots_per_group)?..profile.pool_slots)
        .find(|slot| occupied & (1 << slot) != 0)?;
    Some((slot, occupied & !(1 << slot)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile() -> WispReleaseProfile {
        WispReleaseProfile {
            map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
            wisp_rawcode: 0,
            releases: 6,
            period_ticks: 9,
            life_ticks: 1,
            x_limit: 0,
            y_inset: 0,
            pool_slots: 20,
            slots_per_group: 5,
            west_slots: 10,
            y_period: 10,
            y_south_below: 5,
            refill_ticks: 120,
        }
    }

    fn slots(mask: u32) -> Vec<u8> {
        (0..20).filter(|slot| mask & (1 << slot) != 0).collect()
    }

    #[test]
    fn refill_walk_skips_the_first_slot_of_groups_after_a_fill() {
        let first = refill_pool(0, profile());
        assert_eq!(slots(first), [0, 5 + 1, 10 + 1, 15 + 1]);
        let second = refill_pool(first, profile());
        assert_eq!(slots(second), [0, 1, 6, 7, 11, 12, 16, 17]);
        // A full pool is a fixed point.
        assert_eq!(refill_pool(u32::MAX >> 12, profile()), u32::MAX >> 12);
    }

    #[test]
    fn taking_spills_into_later_groups_but_never_earlier_ones() {
        let occupied = (1 << 3) | (1 << 12);
        assert_eq!(take_pool_slot(occupied, 1, profile()), Some((12, 1 << 3)));
        assert_eq!(take_pool_slot(occupied, 0, profile()), Some((3, 1 << 12)));
        assert_eq!(take_pool_slot(1 << 3, 3, profile()), None);
    }
}
