use super::*;
use crate::components::{DeathExplosionState, NativeAction};

fn amount(profile: crate::DeathExplosionProfile, distance: u64) -> Option<i32> {
    if distance <= square_i32(profile.full_radius) {
        Some(profile.full_damage)
    } else if distance <= square_i32(profile.partial_radius) {
        Some(profile.partial_damage)
    } else {
        None
    }
}
impl Simulation {
    pub(super) fn observe_native_death_explosions(
        &mut self,
        units: &[UnitSnapshot],
        health: &[i32],
        positions: &[SimPoint],
    ) {
        for (i, unit) in units.iter().enumerate() {
            if health[i] > 0 || unit.removed_without_death || unit.abilities_disabled {
                continue;
            }
            for effect in unit.effective_passive_effects() {
                let PassiveUnitEffect::DeathExplosion(profile) = effect else {
                    continue;
                };
                let id = self.allocate_id();
                self.world.spawn((
                    id,
                    NativeAction::DeathExplosion(DeathExplosionState {
                        source: unit.id,
                        owner: unit.owner,
                        team: unit.team,
                        profile,
                        position: positions[i],
                        due_tick: self.next_tick + u64::from(profile.delay_ticks),
                    }),
                ));
            }
        }
    }
    pub(super) fn advance_native_death_explosion(
        &mut self,
        entity: Entity,
        state: DeathExplosionState,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        if self.next_tick < state.due_tick {
            return;
        }
        let mut health = units.iter().map(|u| u.health).collect::<Vec<_>>();
        for i in 0..units.len() {
            let target = units[i];
            if health[i] <= 0
                || target.team == state.team
                || target.classifications.locust
                || target.classifications.invulnerable
                || target
                    .status
                    .impale
                    .is_some_and(|p| self.next_tick < p.landed_tick)
                || !state
                    .profile
                    .targets
                    .can_target_unit(target.target_movement_class())
            {
                continue;
            }
            let Some(amount) = amount(state.profile, state.position.distance_sq(target.position))
            else {
                continue;
            };
            // Native death AOE admits spell immunity. Its spell attack class still consumes
            // the version-owned armor-type table and incoming modifiers, independently.
            let damage = self
                .combat_rules
                .damage_rules
                .apply_spell(amount, target.armor.armor_type);
            let damage = spell_damage_after_defend(target, damage, self.next_tick);
            source_damage_runtime::dispatch(
                state.source,
                i,
                damage,
                source_damage_runtime::SourceDamageState {
                    units,
                    health: &mut health,
                    rules: self.combat_rules.damage_rules,
                    tick: self.next_tick,
                },
                &self.retired_damage_sources,
            );
            damage_thresholds::observe_damage_event(&mut units[i], health[i], damage);
            let after = health[i]
                .checked_sub(damage)
                .expect("native death explosion health overflow");
            death_healing::commit_split_health_from(
                units,
                &mut health,
                None,
                i,
                after,
                Some(state.source),
            );
        }
        for (unit, health) in units.iter_mut().zip(health) {
            unit.health = health;
        }
        if state.profile.targets.can_target_buildings() && !self.debug_buildings_invulnerable {
            for target in buildings.iter_mut() {
                if target.health <= 0
                    || target.team == state.team
                    || target.classifications.invulnerable
                {
                    continue;
                }
                let distance = point_to_footprint_distance_sq(
                    state.position,
                    target.footprint,
                    self.config.navigation_cell_size,
                );
                let Some(amount) = amount(state.profile, distance) else {
                    continue;
                };
                let damage = self
                    .combat_rules
                    .damage_rules
                    .apply_spell(amount, target.armor.armor_type);
                target.health = target
                    .health
                    .checked_sub(scale_damage_per_10k(
                        damage,
                        target.snow_damage_taken_per_10k,
                    ))
                    .expect("native building death explosion health overflow");
            }
        }
        death_healing::refresh_support(units, buildings, None);
        // Native Amnx has no cast event or separately authored ability model. Presentation
        // follows the source body's normal death sequence and its own event objects.
        self.world.despawn(entity);
    }
}
