//! Native target Purge and separately ordered AOsf families.
use super::*;
use crate::components::{NativeAction, NativeSummonProfile, PurgeState, SirenPurgeProfile};

pub(super) fn purge_primary_eligible(
    u: &UnitSnapshot,
    team: Team,
    p: SirenPurgeProfile,
    tick: u64,
) -> bool {
    u.health > 0
        && u.team != team
        && u.classifications.combat_sapper
        && u.target_movement_class() == MovementClass::Ground
        && !u.classifications.invulnerable
        && !u.classifications.spell_immune
        && !(p.parent_nonhero_only && u.classifications.uses_hero_spell_rules())
        && u.visible_to(team, tick)
}

pub(super) fn advance_purge(status: &mut StatusState, tick: u64) {
    let Some(p) = status.purge else { return };
    if tick >= p.expires_tick {
        status.purge = None;
    }
    let factor = p.profile.movement_factor_per_10k;
    let initial = if factor <= 10_000 {
        100
    } else {
        100 - 1_000_000 / factor
    };
    // Native recovery is confined to the last fifth. Prg1 controls the number
    // of updates there; native Prg2/Prg6 are legacy unused fields, not attack slow/mana burn.
    let duration = p.expires_tick - p.applied_tick;
    let window = duration.div_ceil(5).max(1);
    let steps = u64::from(factor / 50_000);
    let recovery = tick.saturating_sub(p.expires_tick - window).min(window);
    let updates = recovery * steps / window;
    let reduction = if tick >= p.expires_tick {
        0
    } else {
        (u64::from(initial) * (steps - updates))
            .checked_div(steps)
            .unwrap_or(u64::from(initial))
    };
    let delta = -(i16::try_from(reduction).unwrap());
    if let Some(modifier) = status.movement_modifiers[..usize::from(status.movement_modifier_count)]
        .iter_mut()
        .find(|m| m.id == ModifierId(p.profile.ability.0))
    {
        modifier.percent_delta = delta;
        modifier.expires_tick = p.expires_tick;
    } else {
        super::status::apply_timed_movement_modifier(
            status,
            ModifierId(p.profile.ability.0),
            delta,
            p.expires_tick,
        );
    }
}

pub(super) fn dispel_magic_buffs(status: &mut StatusState) {
    // Poison and Purge are undispellable. Intrinsic abilities, permanent grants,
    // stun and independent order recovery are not ordinary magic buffs.
    let poison = status.poison.map(|p| p.profile.buff_rawcode);
    let purge = status.purge.map(|p| p.profile.ability.0);
    let acid = status.acid_bomb.map(|p| p.profile.ability.0);
    let ensnare = status.ensnare.map(|p| p.profile.ability.0);
    let aura_ids: [Option<ModifierId>; MAX_TIMED_ARMOR_MODIFIERS] = std::array::from_fn(|i| {
        (i < usize::from(status.armor_modifier_count) && status.armor_modifiers[i].aura_membership)
            .then_some(status.armor_modifiers[i].id)
    });
    let keep = |id: ModifierId, expiry: u64| {
        expiry == u64::MAX
            || poison == Some(id.0)
            || purge == Some(id.0)
            || acid == Some(id.0)
            || ensnare == Some(id.0)
            || aura_ids.contains(&Some(id))
    };
    super::native_carriers::retain_modifiers(
        &mut status.movement_modifiers,
        &mut status.movement_modifier_count,
        |m| m.undispellable || keep(m.id, m.expires_tick),
    );
    super::native_carriers::retain_modifiers(
        &mut status.attack_speed_modifiers,
        &mut status.attack_speed_modifier_count,
        |m| m.undispellable || keep(m.id, m.expires_tick),
    );
    super::native_carriers::retain_modifiers(
        &mut status.armor_modifiers,
        &mut status.armor_modifier_count,
        |m| m.undispellable || keep(m.id, m.expires_tick),
    );
    super::native_carriers::retain_modifiers(
        &mut status.damage_over_time,
        &mut status.damage_over_time_count,
        |m| keep(m.id, m.expires_tick),
    );
    status.rooted_until_tick = 0;
}

fn resume_attack(u: &mut UnitSnapshot) {
    u.target = None;
    u.direct_retaliation_lock = false;
    u.ally_defense_lock = false;
    u.status.pending_attack = None;
    u.status.pending_cast = None;
    u.status.action_animation = None;
}

impl Simulation {
    pub(super) fn cast_siren_purge(
        &mut self,
        source: AbilitySourceSnapshot,
        target: usize,
        p: SirenPurgeProfile,
        units: &mut [UnitSnapshot],
    ) -> usize {
        // The map schedules the follow-up regardless of whether the independent dummy order succeeds.
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::ImmediateSummonOrder {
                source: source.id,
                ability: p.summon_ability,
                due_tick: self.next_tick + u64::from(p.summon_delay_ticks),
                resume_delay_ticks: p.resume_delay_ticks,
            },
        ));
        let parent_target = units[target];
        let owner = find_unit_index(units, source.id)
            .map(|i| units[i].owner)
            .expect("native unit caster owner");
        self.launch_native_parasite(source, owner, parent_target.id, p.parent);
        let u = &mut units[target];
        if u.health <= 0
            || u.classifications.invulnerable
            || u.classifications.spell_immune
            || !u.visible_to(source.team, self.next_tick)
            || !p.child.targets.can_target_unit(u.target_movement_class())
            || self.ability_source_distance_sq(source.origin, u.position)
                > square_i32(p.child.range)
        {
            return 0;
        }
        dispel_magic_buffs(&mut u.status);
        self.cleanse_building_spell_controls(u.id, u.position, &[]);
        self.project_building_spell_control(u);
        if u.team == source.team {
            return 1;
        }
        let hero = u.classifications.uses_hero_spell_rules();
        let duration = if hero {
            p.child.hero_duration_ticks
        } else {
            p.child.duration_ticks
        };
        let pause = if hero {
            p.child.hero_pause_ticks
        } else {
            p.child.pause_ticks
        };
        u.status.purge = Some(PurgeState {
            profile: p.child,
            applied_tick: self.next_tick,
            pause_until_tick: self.next_tick + u64::from(pause),
            expires_tick: self.next_tick + u64::from(duration),
        });
        advance_purge(&mut u.status, self.next_tick);
        if pause > 0 {
            u.status.pending_attack = None;
            u.status.pending_cast = None;
            u.status.action_animation = None;
        }
        if u.classifications.summoned && p.child.summoned_damage > 0 {
            let before = u.health;
            apply_ability_effect_to_unit(
                u,
                AbilityEffect::Damage {
                    amount: p.child.summoned_damage,
                },
                source.team,
                self.next_tick,
                self.combat_rules.damage_rules,
            );
            death_healing::observe_inline_death_from(units, target, before, None);
        }
        1
    }

    pub(super) fn attempt_native_summon_order(
        &mut self,
        source: SimId,
        ability: AbilityId,
        resume_delay_ticks: u16,
        units: &mut [UnitSnapshot],
    ) {
        let Some(i) = find_unit_index(units, source) else {
            return;
        };
        let u = &mut units[i];
        let primary = u
            .spellcasting
            .filter(|s| s.ability.id == ability)
            .map(|s| (s.ability, u.ability_state.unwrap()));
        let secondary = self
            .world
            .get::<AdditionalAutomaticAbilities>(u.entity)
            .and_then(|set| set.get(ability))
            .map(|entry| (entry.profile, entry.state));
        let available = primary.or(secondary).filter(|(profile, state)| {
            matches!(profile.effect, AbilityEffect::NativeSummon(_))
                && u.health > 0
                && !u.status.is_stunned(self.next_tick)
                && !u.orders_suspended
                && !u.abilities_disabled
                && state.ready_tick <= self.next_tick
                && u.mana_current.is_some_and(|mana| mana >= profile.mana_cost)
        });
        let Some((_, mut state)) = available else {
            resume_attack(u);
            return;
        };
        resume_attack(u);
        state.manual_cast_requested = true;
        if primary.is_some() {
            u.ability_state = Some(state);
        } else {
            self.commit_additional_ability_state(u.entity, ability, state);
        }
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::ResumeAttack {
                source,
                due_tick: self.next_tick + u64::from(resume_delay_ticks),
            },
        ));
    }

    pub(super) fn cast_native_summon(
        &mut self,
        source: usize,
        p: NativeSummonProfile,
        units: &mut [UnitSnapshot],
    ) -> usize {
        let previous: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                if let NativeAction::SummonFamily {
                    source: owner,
                    ability,
                    children,
                    ..
                } = entity.get::<NativeAction>()?
                    && *owner == units[source].id
                    && *ability == p.ability
                {
                    Some((entity.id(), children.clone()))
                } else {
                    None
                }
            })
            .collect();
        for (entity, children) in previous {
            self.expire_native_summons(&children, units);
            self.world.despawn(entity);
        }
        let u = units[source];
        let owner = self.world.get::<Owner>(u.entity).unwrap().0;
        let kind =
            crate::CastleFightUnitKind::from_rawcode_for_version(p.body_rawcode, p.map_version)
                .unwrap()
                .expect("promoted summon body");
        let mut definition = kind
            .definition_for_version(p.map_version)
            .unwrap()
            .resolved();
        definition.properties.classifications.summoned = true;
        let mut heading = u.status.facing_direction;
        if heading == SimPoint::default() {
            heading = SimPoint::new(if u.team.0 == 0 { 1 } else { -1 }, 0);
        }
        let position = super::line_projectiles::offset_along_direction(
            u.position,
            SimPoint::default(),
            heading,
            p.forward_distance,
        );
        let children: Vec<_> = (0..p.count)
            .map(|_| {
                self.spawn_resolved_unit_unchecked(
                    Some(owner),
                    UnitSpawn::from_template(u.team, position, definition.template),
                    definition,
                )
            })
            .collect();
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::SummonFamily {
                source: u.id,
                ability: p.ability,
                children,
                expires_tick: self.next_tick + u64::from(p.duration_ticks),
            },
        ));
        usize::from(p.count)
    }

    /// Derived recipient art identities from authoritative native buffs. No presentation timer.
    #[must_use]
    pub fn native_parasite_buffs(&self) -> Vec<(SimId, u32)> {
        let tick = self.next_tick.saturating_sub(1);
        let mut buffs: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|e| {
                let NativeAction::Parasite(p) = e.get::<NativeAction>()? else {
                    return None;
                };
                p.buff_until_tick
                    .filter(|&until| tick < until)
                    .map(|_| (p.target, p.profile.buff_rawcode))
            })
            .collect();
        buffs.sort_unstable();
        buffs.dedup();
        buffs
    }

    pub(super) fn launch_native_parasite(
        &mut self,
        source: AbilitySourceSnapshot,
        owner: PlayerId,
        target: SimId,
        profile: crate::NativeParasiteProfile,
    ) {
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::Parasite(crate::components::NativeParasiteState {
                ability: source.spellcasting.map(|p| p.ability.id),
                position_tick: self.next_tick,
                source: source.id,
                owner,
                team: source.team,
                target,
                profile,
                position: match source.origin {
                    AbilitySourceOrigin::Unit(p) => p,
                    AbilitySourceOrigin::Building(f) => {
                        footprint_center_point(f, self.config.navigation_cell_size)
                    }
                },
                buff_until_tick: None,
            }),
        ));
    }

    pub(super) fn advance_native_parasite(
        &mut self,
        entity: Entity,
        mut p: crate::components::NativeParasiteState,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) {
        let target = find_unit_index(units, p.target)
            .map(|i| {
                let u = units[i];
                (u.health, u.position, u.classifications)
            })
            .or_else(|| {
                find_building_index(buildings, p.target).map(|i| {
                    let b = buildings[i];
                    (
                        b.health,
                        footprint_center_point(b.footprint, self.config.navigation_cell_size),
                        b.classifications,
                    )
                })
            });
        let Some((health, position, flags)) = target else {
            self.world.despawn(entity);
            return;
        };
        if p.buff_until_tick
            .is_some_and(|until| self.next_tick >= until)
            || (p.buff_until_tick.is_none() && health <= 0)
        {
            self.world.despawn(entity);
            return;
        }
        if p.buff_until_tick.is_none() {
            let distance = p.position.distance_sq(position);
            p.position = p.position.step_towards(position, p.profile.speed_per_tick);
            if distance <= square_i32(p.profile.speed_per_tick) {
                if (flags.invulnerable && !p.profile.affects_invulnerable) || flags.spell_immune {
                    self.world.despawn(entity);
                    return;
                }
                let duration = if flags.uses_hero_spell_rules() {
                    p.profile.hero_duration_ticks
                } else {
                    p.profile.duration_ticks
                };
                let replaced: Vec<_> = self
                    .world
                    .iter_entities()
                    .filter_map(|e| {
                        let NativeAction::Parasite(previous) = e.get::<NativeAction>()? else {
                            return None;
                        };
                        (e.id() != entity
                            && previous.target == p.target
                            && previous.profile.buff_rawcode == p.profile.buff_rawcode
                            && previous.buff_until_tick.is_some())
                        .then_some(e.id())
                    })
                    .collect();
                for previous in replaced {
                    self.world.despawn(previous);
                }
                p.buff_until_tick = Some(if duration == 0 {
                    u64::MAX
                } else {
                    self.next_tick + u64::from(duration)
                });
            }
        }
        p.position_tick = self.next_tick;
        self.world
            .entity_mut(entity)
            .insert(NativeAction::Parasite(p));
    }

    pub(super) fn observe_native_parasite_deaths(
        &mut self,
        units: &[UnitSnapshot],
        health: &[i32],
        positions: &[SimPoint],
        buildings: &[BuildingSnapshot],
        building_health: &[i32],
    ) {
        let mut deaths: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                let NativeAction::Parasite(p) = entity.get::<NativeAction>()? else {
                    return None;
                };
                let dead_position = find_unit_index(units, p.target)
                    .filter(|&i| health[i] <= 0 && !units[i].removed_without_death)
                    .map(|i| positions[i])
                    .or_else(|| {
                        find_building_index(buildings, p.target)
                            .filter(|&i| building_health[i] <= 0)
                            .map(|i| {
                                footprint_center_point(
                                    buildings[i].footprint,
                                    self.config.navigation_cell_size,
                                )
                            })
                    })?;
                p.buff_until_tick
                    .is_some_and(|until| self.next_tick < until)
                    .then_some((
                        *entity.get::<SimId>()?,
                        entity.id(),
                        p.clone(),
                        dead_position,
                    ))
            })
            .collect();
        deaths.sort_unstable_by_key(|(id, ..)| *id);
        for (_, entity, p, position) in deaths {
            // A Parasite whose retained body is an armed vanilla unit (the building parents'
            // unmodified base) has no body the engine can build: it ends without a spawn.
            if !crate::content::is_native_noncombat_summon(
                p.profile.map_version,
                p.profile.body_rawcode,
            ) {
                self.world.despawn(entity);
                continue;
            }
            let definition = crate::content::native_noncombat_summon_for_version(
                p.profile.body_rawcode,
                p.profile.map_version,
            );
            for _ in 0..p.profile.count {
                let child = self.spawn_resolved_unit_unchecked(
                    Some(p.owner),
                    UnitSpawn::from_template(p.team, position, definition.template),
                    definition,
                );
                let id = self.allocate_id();
                self.world.spawn((
                    id,
                    NativeAction::TimedLife {
                        child,
                        expires_tick: self.next_tick + u64::from(p.profile.body_duration_ticks),
                    },
                ));
            }
            self.world.despawn(entity);
        }
    }

    pub(super) fn rebind_native_summon_families(&mut self, old: SimId, new: SimId) {
        for mut action in self
            .world
            .query::<&mut NativeAction>()
            .iter_mut(&mut self.world)
        {
            if let NativeAction::SummonFamily { source, .. } = &mut *action
                && *source == old
            {
                *source = new;
            }
        }
    }

    pub(super) fn expire_native_summons(&self, children: &[SimId], units: &mut [UnitSnapshot]) {
        for &child in children {
            if let Some(i) = find_unit_index(units, child)
                && units[i].health > 0
            {
                death_healing::commit_health_from(units, i, 0, None);
            }
        }
    }
}

#[cfg(test)]
mod tests;
