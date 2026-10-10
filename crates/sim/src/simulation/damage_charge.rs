//! Script damage-to-mana controller and native contact self-destruct autocasting.
use super::*;
use crate::components::{DamageChargeState, SelfDestructProfile};

const MANA_DENOMINATOR: i64 = crate::CASTLE_FIGHT_SIMULATION_HZ as i64 * 10_000;

pub(super) fn initial_state(effects: PassiveUnitEffects) -> Option<DamageChargeState> {
    effects.iter().find_map(|effect| match effect {
        PassiveUnitEffect::DamageCharge(profile) => Some(DamageChargeState {
            profile,
            mana_per_10k_hz: i64::from(profile.mana_starting) * MANA_DENOMINATOR,
            armed: false,
            detonation_tick: None,
        }),
        _ => None,
    })
}

pub(super) fn mana_current(state: DamageChargeState) -> i32 {
    i32::try_from(state.mana_per_10k_hz / MANA_DENOMINATOR).expect("bounded native charge mana")
}

pub(super) fn restore_mana(state: &mut DamageChargeState, amount: i32) {
    state.mana_per_10k_hz = (state.mana_per_10k_hz + i64::from(amount) * MANA_DENOMINATOR)
        .clamp(0, i64::from(state.profile.mana_maximum) * MANA_DENOMINATOR);
}

pub(super) fn advance_regeneration(state: &mut DamageChargeState, bonus_per_second_per_10k: u32) {
    state.mana_per_10k_hz = (state.mana_per_10k_hz
        + i64::from(state.profile.mana_regen_per_second_per_10k)
        + i64::from(bonus_per_second_per_10k))
    .clamp(0, i64::from(state.profile.mana_maximum) * MANA_DENOMINATOR);
}

pub(super) fn observe_damage_state(
    state: &mut Option<DamageChargeState>,
    health: i32,
    damage: i32,
) -> bool {
    let Some(state) = state.as_mut().filter(|state| !state.armed && health > 0) else {
        return false;
    };
    let next = state.mana_per_10k_hz + i64::from(damage) * MANA_DENOMINATOR;
    if next >= i64::from(state.profile.threshold) * MANA_DENOMINATOR {
        // Threshold branch removes A0C5, grants Asds and issues selfdestructon.
        // It deliberately does not write the new mana value or explode immediately.
        state.armed = true;
        true
    } else {
        state.mana_per_10k_hz =
            next.clamp(0, i64::from(state.profile.mana_maximum) * MANA_DENOMINATOR);
        false
    }
}

pub(super) fn autocast_target(
    source: &UnitSnapshot,
    units: &[UnitSnapshot],
    buildings: &[BuildingSnapshot],
    cell_size: i32,
    tick: u64,
) -> Option<(SimId, u64, i32)> {
    let profile = source.status.damage_charge?.profile.explosion;
    let units = units
        .iter()
        .filter(|target| {
            target.health > 0
                && target.team != source.team
                && !target.classifications.invulnerable
                && profile
                    .targets
                    .can_target_unit(target.target_movement_class())
                && target.visible_to(source.team, tick)
        })
        .map(|target| {
            (
                target.id,
                source.position.distance_sq(target.position),
                source.collision_radius + target.collision_radius,
            )
        });
    let buildings = buildings
        .iter()
        .filter(|target| {
            target.health > 0
                && target.team != source.team
                && !target.classifications.invulnerable
                && profile.targets.can_target_buildings()
                && target.visible_teams & (1 << source.team.0) != 0
        })
        .map(|target| {
            (
                target.id,
                point_to_footprint_distance_sq(source.position, target.footprint, cell_size),
                source.collision_radius,
            )
        });
    units
        .chain(buildings)
        .filter(|(_, distance, _)| *distance <= square_i32(source.attack.acquisition_range))
        .min_by_key(|(id, distance, _)| (*distance, *id))
}

fn explosion_damage(profile: SelfDestructProfile, distance: u64) -> Option<i32> {
    if distance <= square_i32(profile.full_radius) {
        Some(profile.full_damage)
    } else if distance <= square_i32(profile.partial_radius) {
        Some(profile.partial_damage)
    } else {
        None
    }
}

impl Simulation {
    pub(super) fn resolve_damage_charge(
        &mut self,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        for index in 0..units.len() {
            let source = units[index];
            let Some(mut charge) = source.status.damage_charge.filter(|state| state.armed) else {
                continue;
            };
            if source.health <= 0 {
                continue;
            }
            if source.abilities_disabled
                || source.orders_suspended
                || self.next_tick < source.status.stunned_until_tick
            {
                units[index]
                    .status
                    .damage_charge
                    .as_mut()
                    .unwrap()
                    .detonation_tick = None;
                continue;
            }
            if charge.detonation_tick.is_none() {
                if source.status.is_performing_action(self.next_tick) {
                    continue;
                }
                let Some((_, distance, contact)) = autocast_target(
                    &source,
                    units,
                    buildings,
                    self.config.navigation_cell_size,
                    self.next_tick,
                ) else {
                    continue;
                };
                // Native cast range zero means contact, independently of explosion radius.
                if distance > square_i32(contact) {
                    continue;
                }
                charge.detonation_tick =
                    Some(self.next_tick + u64::from(source.action_timing.cast_point_ticks));
                units[index].status.pending_attack = None;
                units[index].status.begin_action_animation(
                    ActionAnimationKind::Cast,
                    self.next_tick,
                    source.action_timing.cast_ticks,
                );
                units[index].status.damage_charge = Some(charge);
            }
            if self.next_tick < charge.detonation_tick.unwrap() {
                continue;
            }
            let profile = charge.profile.explosion;
            self.last_ability_casts.push(AbilityCastEvent {
                source: source.id,
                ability: profile.ability,
                target: AbilityCastTarget::Point(source.position),
                target_position: Some(source.position),
                effect: AbilityEffect::AreaDamage {
                    amount: profile.full_damage,
                    radius: profile.partial_radius,
                    origin: AreaDamageOrigin::Caster,
                },
            });
            for i in 0..units.len() {
                let target = &mut units[i];
                if target.health <= 0
                    || target.team == source.team
                    || target.classifications.invulnerable
                    || !profile
                        .targets
                        .can_target_unit(target.target_movement_class())
                {
                    continue;
                }
                let Some(amount) =
                    explosion_damage(profile, source.position.distance_sq(target.position))
                else {
                    continue;
                };
                let damage = self
                    .combat_rules
                    .damage_rules
                    .apply_physical_ability(amount, effective_armor_points_per_100(target));
                let damage = scale_damage_per_10k(
                    native_kill_effects::damage_after_native_incoming(
                        target,
                        damage,
                        self.next_tick,
                    ),
                    target.snow_damage_taken_per_10k,
                );
                damage_thresholds::observe_damage_event(target, target.health, damage);
                let after = target.health.saturating_sub(damage);
                death_healing::commit_health(units, i, after);
            }
            if !self.debug_buildings_invulnerable {
                for target in buildings.iter_mut() {
                    if target.health <= 0
                        || target.team == source.team
                        || target.classifications.invulnerable
                        || !profile.targets.can_target_buildings()
                    {
                        continue;
                    }
                    let Some(amount) = explosion_damage(
                        profile,
                        point_to_footprint_distance_sq(
                            source.position,
                            target.footprint,
                            self.config.navigation_cell_size,
                        ),
                    ) else {
                        continue;
                    };
                    let amount = scale_damage_per_10k(amount, profile.building_damage_per_10k);
                    let damage = self
                        .combat_rules
                        .damage_rules
                        .apply_physical_ability(amount, i32::from(target.armor.armor_points) * 100);
                    let damage = scale_damage_per_10k(damage, target.snow_damage_taken_per_10k);
                    target.health = target.health.saturating_sub(damage);
                }
            }
            // Asds kills its caster; its death flag controls corpse appearance, not a second AoE.
            death_healing::refresh_support(units, buildings, None);
            // Native caster suicide is a kill without a damage-event killer; fJ requires one.
            units[index].health = 0;
        }
    }
}
