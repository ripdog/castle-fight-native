use super::*;
use crate::components::{
    HealingWaveProfile, HealingWaveState, NativeAction, NativeBoltProfile, NativeBoltState,
    TimedMovementModifier,
};

// Warcraft's native Healing Wave bounce interval is 250 ms. Derive each deadline
// from the original cast, using ceiling at 30 Hz, rather than accumulating rounded hops.
fn healing_wave_due_tick(start: u64, jump: u8) -> u64 {
    start
        .checked_add((u64::from(jump) * CASTLE_FIGHT_SIMULATION_HZ as u64).div_ceil(4))
        .expect("Healing Wave deadline overflow")
}

impl Simulation {
    pub(super) fn start_healing_wave(
        &mut self,
        source: SimId,
        team: Team,
        origin: SimPoint,
        target_index: usize,
        profile: HealingWaveProfile,
        units: &mut [UnitSnapshot],
    ) {
        let target = &mut units[target_index];
        target.health = target
            .health
            .saturating_add(profile.healing)
            .saturating_add(profile.trigger_healing)
            .min(target.health_max);
        self.emit_healing_wave_hop(source, profile.ability, 0, origin, target.position);
        if profile.maximum_targets > 1 {
            let mut hit_targets = [SimId(0); MAX_BOUNCE_HITS];
            hit_targets[0] = target.id;
            let state = HealingWaveState {
                source,
                team,
                profile,
                started_tick: self.next_tick,
                jump_index: 1,
                current_target: target.id,
                last_position: target.position,
                next_healing: retained_healing(profile.healing, profile.retention_per_10k),
                hit_targets,
                hit_count: 1,
            };
            let id = self.allocate_id();
            self.world.spawn((id, NativeAction::HealingWave(state)));
        }
    }

    fn emit_healing_wave_hop(
        &mut self,
        source: SimId,
        ability: AbilityId,
        index: u8,
        from: SimPoint,
        to: SimPoint,
    ) {
        let mut points = [SimPoint::default(); MAX_BOUNCE_HITS + 1];
        points[0] = from;
        points[1] = to;
        self.last_chain_lightnings.push(ChainLightningEvent {
            source,
            ability,
            bounce_index: index,
            points,
            point_count: 2,
        });
    }

    pub(super) fn launch_native_bolt(
        &mut self,
        source: SimId,
        team: Team,
        origin: SimPoint,
        target: SimId,
        destination: SimPoint,
        profile: NativeBoltProfile,
    ) {
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::Bolt(NativeBoltState {
                source,
                team,
                target,
                profile,
                launch_position: origin,
                launch_tick: self.next_tick,
                position: origin,
                position_tick: self.next_tick,
                impact_tick: self.next_tick
                    + projectile_travel_ticks(
                        origin.distance_sq(destination),
                        profile.speed_per_tick,
                    ),
            }),
        ));
    }

    pub(super) fn resolve_native_actions(
        &mut self,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        let mut actions = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                Some((
                    *entity.get::<SimId>()?,
                    entity.id(),
                    entity.get::<NativeAction>()?.clone(),
                ))
            })
            .collect::<Vec<_>>();
        actions.sort_unstable_by_key(|(id, _, _)| *id);
        for (_, entity, action) in actions {
            match action {
                NativeAction::Bolt(mut state) => {
                    let destination = find_unit_index(units, state.target)
                        .filter(|&index| units[index].health > 0)
                        .map(|index| units[index].position)
                        .or_else(|| {
                            buildings
                                .iter()
                                .find(|target| target.id == state.target && target.health > 0)
                                .map(|target| {
                                    footprint_center_point(
                                        target.footprint,
                                        self.config.navigation_cell_size,
                                    )
                                })
                        });
                    let Some(destination) = destination else {
                        self.world.despawn(entity);
                        continue;
                    };
                    let distance = state.position.distance_sq(destination);
                    state.position = state
                        .position
                        .step_towards(destination, state.profile.speed_per_tick);
                    state.position_tick = self.next_tick;
                    if distance > square_i32(state.profile.speed_per_tick) {
                        state.impact_tick = self.next_tick
                            + projectile_travel_ticks(
                                state.position.distance_sq(destination),
                                state.profile.speed_per_tick,
                            );
                        self.world
                            .entity_mut(entity)
                            .insert(NativeAction::Bolt(state));
                        continue;
                    }
                    self.world.despawn(entity);
                    let profile = state.profile;
                    if let Some(index) = find_unit_index(units, state.target) {
                        let target = &mut units[index];
                        if target.health <= 0
                            || target.team == state.team
                            || target.classifications.spell_immune
                            || target.classifications.invulnerable
                            || !profile.targets.can_target_unit(target.movement_class)
                        {
                            continue;
                        }
                        let damage = self
                            .combat_rules
                            .damage_rules
                            .apply_spell(profile.damage, target.armor.armor_type);
                        let damage = spell_damage_after_defend(*target, damage, self.next_tick);
                        target.health = target.health.saturating_sub(damage);
                        if damage > 0 && profile.cleanse {
                            cleanse_native_status(&mut target.status);
                        }
                        if target.health > 0 {
                            let duration = if target.classifications.hero {
                                profile.hero_stun_ticks
                            } else {
                                profile.stun_ticks
                            };
                            target.status.stunned_until_tick = target
                                .status
                                .stunned_until_tick
                                .max(self.next_tick + u64::from(duration));
                            if profile.damage_per_second > 0 {
                                apply_native_fire_damage_over_time(
                                    &mut target.status,
                                    ModifierId(profile.ability.0),
                                    profile.damage_per_second,
                                    CASTLE_FIGHT_SIMULATION_HZ as u16,
                                    self.next_tick,
                                    self.next_tick + u64::from(profile.duration_ticks),
                                );
                            }
                        }
                        self.last_ability_casts.push(AbilityCastEvent {
                            source: state.source,
                            ability: profile.ability,
                            target: AbilityCastTarget::Unit(state.target),
                            target_position: Some(target.position),
                            effect: AbilityEffect::PhoenixFire(profile),
                        });
                    } else if let Some(target) = buildings
                        .iter_mut()
                        .find(|target| target.id == state.target)
                    {
                        let immune = target.classifications.spell_immune
                            || target.classifications.invulnerable;
                        if target.health > 0
                            && target.team != state.team
                            && !immune
                            && profile.targets.can_target_buildings()
                        {
                            target.health = target.health.saturating_sub(
                                self.combat_rules
                                    .damage_rules
                                    .apply_spell(profile.damage, target.armor.armor_type),
                            );
                            if target.health > 0 && profile.damage_per_second > 0 {
                                apply_native_fire_damage_over_time(
                                    target.status.get_or_insert_default(),
                                    ModifierId(profile.ability.0),
                                    profile.damage_per_second,
                                    CASTLE_FIGHT_SIMULATION_HZ as u16,
                                    self.next_tick,
                                    self.next_tick + u64::from(profile.duration_ticks),
                                );
                            }
                            self.last_ability_casts.push(AbilityCastEvent {
                                source: state.source,
                                ability: profile.ability,
                                target: AbilityCastTarget::Point(destination),
                                target_position: Some(destination),
                                effect: AbilityEffect::PhoenixFire(profile),
                            });
                        }
                    }
                }
                NativeAction::HealingWave(mut state) => {
                    if healing_wave_due_tick(state.started_tick, state.jump_index) > self.next_tick
                    {
                        continue;
                    }
                    let origin = find_unit_index(units, state.current_target)
                        .filter(|&i| units[i].health > 0)
                        .map_or(state.last_position, |i| units[i].position);
                    let next = units
                        .iter()
                        .enumerate()
                        .filter(|(_, target)| {
                            target.health > 0
                                && target.health < target.health_max
                                && target.team == state.team
                                && !target.mechanical
                                && !state.hit_targets[..usize::from(state.hit_count)]
                                    .contains(&target.id)
                                && origin.distance_sq(target.position)
                                    <= square_i32(state.profile.jump_radius)
                        })
                        .min_by_key(|(_, target)| (target.health, target.id))
                        .map(|(i, _)| i);
                    let Some(next) = next else {
                        self.world.despawn(entity);
                        continue;
                    };
                    units[next].health = units[next]
                        .health
                        .saturating_add(state.next_healing)
                        .min(units[next].health_max);
                    self.emit_healing_wave_hop(
                        state.source,
                        state.profile.ability,
                        state.jump_index,
                        origin,
                        units[next].position,
                    );
                    state.hit_targets[usize::from(state.hit_count)] = units[next].id;
                    state.hit_count += 1;
                    state.jump_index += 1;
                    state.current_target = units[next].id;
                    state.last_position = units[next].position;
                    state.next_healing =
                        retained_healing(state.next_healing, state.profile.retention_per_10k);
                    if state.hit_count >= state.profile.maximum_targets || state.next_healing == 0 {
                        self.world.despawn(entity);
                    } else {
                        self.world
                            .entity_mut(entity)
                            .insert(NativeAction::HealingWave(state));
                    }
                }
            }
        }
    }
}

pub(super) fn resolve_native_building_damage_over_time(
    buildings: &mut [BuildingSnapshot],
    tick: u64,
    rules: DamageRules,
) {
    for building in buildings {
        let Some(status) = &mut building.status else {
            continue;
        };
        let mut kept = 0;
        for i in 0..usize::from(status.damage_over_time_count) {
            let mut effect = status.damage_over_time[i];
            while effect.next_pulse_tick <= tick
                && (effect.next_pulse_tick < effect.expires_tick
                    || (effect.final_pulse_at_expiry
                        && effect.next_pulse_tick == effect.expires_tick))
            {
                if !building.classifications.spell_immune && !building.classifications.invulnerable
                {
                    building.health = building.health.saturating_sub(
                        rules.apply_spell(effect.damage_per_pulse, building.armor.armor_type),
                    );
                }
                effect.next_pulse_tick += u64::from(effect.pulse_interval_ticks);
            }
            if tick < effect.expires_tick {
                status.damage_over_time[kept] = effect;
                kept += 1;
            }
        }
        status.damage_over_time[kept..].fill(TimedDamageOverTime::default());
        status.damage_over_time_count = kept as u8;
    }
}

fn cleanse_native_status(status: &mut StatusState) {
    status.stunned_until_tick = 0;
    status.rooted_until_tick = 0;
    status.movement_modifier_count = 0;
    status
        .movement_modifiers
        .fill(TimedMovementModifier::default());
    status.attack_speed_modifier_count = 0;
    status
        .attack_speed_modifiers
        .fill(TimedAttackSpeedModifier::default());
    status.armor_modifier_count = 0;
    status.armor_modifiers.fill(TimedArmorModifier::default());
    status.damage_over_time_count = 0;
    status.damage_over_time.fill(TimedDamageOverTime::default());
}

fn apply_native_fire_damage_over_time(
    status: &mut StatusState,
    id: ModifierId,
    damage: i32,
    interval: u16,
    tick: u64,
    expiry: u64,
) {
    apply_timed_damage_over_time(status, id, damage, interval, tick, expiry);
    if let Some(effect) = status.damage_over_time[..usize::from(status.damage_over_time_count)]
        .iter_mut()
        .find(|effect| effect.id == id)
    {
        effect.final_pulse_at_expiry = true;
    }
}

fn retained_healing(amount: i32, retention: u16) -> i32 {
    i32::try_from(i64::from(amount) * i64::from(retention) / 10_000).expect("Healing Wave overflow")
}
