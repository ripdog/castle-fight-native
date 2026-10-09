//! Script damage-event orders composed with native Mirror Image lifecycle.
use super::*;
use crate::components::{IllusionState, MirrorImageState};

pub(super) fn initial_state(effects: PassiveUnitEffects) -> Option<MirrorImageState> {
    effects.iter().find_map(|effect| match effect {
        PassiveUnitEffect::MirrorImage(profile) => Some(MirrorImageState {
            profile,
            event_sequence: 0,
            ready_tick: 0,
            cast_tick: None,
            split_tick: None,
        }),
        _ => None,
    })
}

pub(super) fn observe_source_damage(source: &mut UnitSnapshot, tick: u64, seed: u64) {
    let stunned = source.status.is_stunned(tick);
    let Some(state) = source.status.mirror_image.as_mut() else {
        return;
    };
    let sequence = state.event_sequence;
    state.event_sequence = sequence
        .checked_add(1)
        .expect("mirror damage-event sequence overflow");
    let success = deterministic_random(
        seed,
        tick,
        source.id,
        RANDOM_PURPOSE_ATTACK_PROC ^ u64::from(state.profile.ability.0),
        sequence,
    ) % 10_000
        < u64::from(state.profile.chance_per_10k);
    // The script rolls before issuing the order; native cooldown/control gates can reject it.
    if !success
        || tick < state.ready_tick
        || state.cast_tick.is_some()
        || state.split_tick.is_some()
        || source.abilities_disabled
        || source.orders_suspended
        || stunned
        || source.classifications.illusion
    {
        return;
    }
    state.cast_tick = Some(tick + u64::from(source.action_timing.cast_point_ticks.max(1)));
    source.status.pending_cast = None;
    source.status.begin_action_animation(
        ActionAnimationKind::Cast,
        tick,
        source
            .action_timing
            .cast_ticks
            .max(source.action_timing.cast_point_ticks + 1),
    );
    source.attacks_disabled = true;
    source.orders_suspended = true;
}

pub(super) fn project_control(unit: &mut UnitSnapshot) {
    if let Some(state) = unit.status.mirror_image {
        let pending = state.cast_tick.is_some() || state.split_tick.is_some();
        unit.orders_suspended |= pending;
        unit.attacks_disabled |= pending;
        unit.classifications.invulnerable |= state.split_tick.is_some();
    }
}

pub(super) fn clear_native_magic(status: &mut StatusState) {
    status.poison = None;
    status.acid_bomb = None;
    status.ensnare = None;
    status.rooted_until_tick = 0;
    status.native_stun_until_tick = 0;
    status.native_stun_ability = None;
    status.movement_modifier_count = 0;
    status.attack_speed_modifier_count = 0;
    status.damage_over_time_count = 0;
    let old = usize::from(status.armor_modifier_count);
    let mut count = 0;
    for index in 0..old {
        let modifier = status.armor_modifiers[index];
        if modifier.native_buff.is_none() {
            status.armor_modifiers[count] = modifier;
            count += 1;
        }
    }
    status.armor_modifiers[count..old].fill(TimedArmorModifier::default());
    status.armor_modifier_count = u8::try_from(count).expect("bounded modifiers");
}

impl Simulation {
    pub(super) fn advance_mirror_images(&mut self) -> usize {
        let tick = self.next_tick;
        if !self
            .world
            .query::<&StatusState>()
            .iter(&self.world)
            .any(|status| status.mirror_image.is_some() || status.illusion.is_some())
        {
            return 0;
        }
        let units = self.snapshot_units();
        // Native replacement/removal is unsummoning, without corpse or death-trigger effects.
        let removals: Vec<_> = units
            .iter()
            .filter(|unit| {
                unit.status.illusion.is_some_and(|image| {
                    tick >= image.expires_tick
                        || units
                            .binary_search_by_key(&image.source, |source| source.id)
                            .ok()
                            .is_none_or(|index| {
                                units[index].abilities_disabled
                                    || units[index].status.mirror_image.is_none()
                            })
                })
            })
            .map(|unit| unit.entity)
            .collect();
        for entity in removals {
            self.world.despawn(entity);
        }
        let mut spawned = 0;
        for source in units
            .iter()
            .filter(|unit| unit.status.mirror_image.is_some())
        {
            let mut status = source.status;
            let mut state = status.mirror_image.expect("mirror source");
            if source.abilities_disabled
                || (state.split_tick.is_none()
                    && (source.status.is_stunned(tick)
                        || self
                            .world
                            .get::<BuildingSpellControl>(source.entity)
                            .is_some_and(|control| control.orders_suspended)))
            {
                state.cast_tick = None;
                status.action_animation = None;
            }
            if state.cast_tick.is_some_and(|due| tick >= due) {
                state.cast_tick = None;
                state.ready_tick = tick + u64::from(state.profile.cooldown_ticks);
                state.split_tick = Some(tick + u64::from(state.profile.split_ticks.max(1)));
                clear_native_magic(&mut status);
                // A new native cast removes this caster's previous images immediately.
                for image in units.iter().filter(|unit| {
                    unit.status
                        .illusion
                        .is_some_and(|image| image.source == source.id)
                }) {
                    self.world.despawn(image.entity);
                }
                self.last_ability_casts.push(AbilityCastEvent {
                    source: source.id,
                    ability: state.profile.ability,
                    target: AbilityCastTarget::Unit(source.id),
                    target_position: Some(source.position),
                    effect: AbilityEffect::Damage { amount: 0 },
                });
            }
            if state.split_tick.is_some_and(|due| tick >= due) {
                state.split_tick = None;
                status.action_animation = None;
                let mut definition = self
                    .world
                    .get::<ResurrectionProfile>(source.entity)
                    .expect("resolved mirror source")
                    .0;
                definition.template.attack =
                    *self.world.get::<AttackProfile>(source.entity).unwrap();
                definition.properties.classifications.illusion = true;
                definition.properties.classifications.summoned = false;
                definition.properties.corpse = None;
                definition.properties.passive_effects.retain(|effect| {
                    !matches!(
                        effect,
                        PassiveUnitEffect::MirrorImage(_)
                            | PassiveUnitEffect::TriggeredSpellProc(_)
                            | PassiveUnitEffect::Feedback(_)
                            | PassiveUnitEffect::Bash(_)
                    )
                });
                definition.spellcasting = None;
                definition.additional_abilities = None;
                let orientation = deterministic_random(
                    self.config.match_seed,
                    tick,
                    source.id,
                    RANDOM_PURPOSE_ABILITY_TARGET ^ u64::from(state.profile.ability.0),
                    state.event_sequence,
                ) % 4;
                let delta = state.profile.displacement;
                let offset = match orientation {
                    0 => SimPoint::new(delta, 0),
                    1 => SimPoint::new(0, delta),
                    2 => SimPoint::new(-delta, 0),
                    _ => SimPoint::new(0, -delta),
                };
                let point = |sign: i32| {
                    SimPoint::new(
                        source.position.x + sign * offset.x,
                        source.position.y + sign * offset.y,
                    )
                };
                let legal = |sim: &Self, point: SimPoint| {
                    let cell = sim.topology.cell_of_point(point);
                    sim.topology.component_id(cell).is_some_and(|component| {
                        sim.topology.circle_is_traversable_in_component(
                            point,
                            source.collision_radius,
                            component,
                        )
                    })
                };
                let original = if legal(self, point(1)) {
                    point(1)
                } else {
                    source.position
                };
                self.world.entity_mut(source.entity).insert((
                    Position(original),
                    NavigationState::default(),
                    TargetState::default(),
                ));
                for index in 0..state.profile.images {
                    let preferred = point(-i32::from(index + 1));
                    let position = if legal(self, preferred) {
                        preferred
                    } else {
                        source.position
                    };
                    let id = self.spawn_resolved_unit_unchecked(
                        Some(source.owner),
                        UnitSpawn::from_template(source.team, position, definition.template),
                        definition,
                    );
                    let entity = self
                        .world
                        .iter_entities()
                        .find(|entity| entity.get::<SimId>() == Some(&id))
                        .unwrap()
                        .id();
                    self.world.entity_mut(entity).insert((
                        Health {
                            current: source.health,
                            max: source.health_max,
                        },
                        StatusState {
                            illusion: Some(IllusionState {
                                source: source.id,
                                ability: state.profile.ability,
                                expires_tick: tick + u64::from(state.profile.lifetime_ticks),
                                damage_dealt_per_10k: state.profile.damage_dealt_per_10k,
                                damage_taken_per_10k: state.profile.damage_taken_per_10k,
                            }),
                            ..Default::default()
                        },
                    ));
                    self.last_ability_casts.push(AbilityCastEvent {
                        source: source.id,
                        ability: state.profile.ability,
                        target: AbilityCastTarget::Unit(id),
                        target_position: Some(position),
                        effect: AbilityEffect::Damage { amount: 0 },
                    });
                    spawned += 1;
                }
            }
            status.mirror_image = Some(state);
            self.world.entity_mut(source.entity).insert(status);
        }
        spawned
    }
}
