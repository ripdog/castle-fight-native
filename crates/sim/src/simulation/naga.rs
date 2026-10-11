//! Independent map callback selection and native dummy effects.
use super::*;
use crate::building_mechanics::NagaEarthquakeProfile;
#[cfg(test)]
mod tests;

impl Simulation {
    pub(super) fn cast_naga_earthquake(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AbilityId,
        trigger: SimId,
        p: NagaEarthquakeProfile,
        sequence: u64,
        units: &mut [UnitSnapshot],
    ) -> usize {
        let owner = self
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source.id))
            .and_then(|e| e.get::<Owner>())
            .map(|o| o.0)
            .unwrap_or(PlayerId(source.team.0));
        self.launch_native_parasite(source, owner, trigger, p.parent);
        let bounds =
            crate::building_mechanics::snowveil_for_version(p.map_version).battlefield_bounds;
        let selected = units
            .iter()
            .filter(|u| {
                u.health > 0
                    && u.team != source.team
                    && u.classifications.combat_sapper
                    && u.target_movement_class() == MovementClass::Ground
                    && !p
                        .selector_exclusions
                        .into_iter()
                        .any(|a| self.native_ability_installed(u, a))
                    && u.position.x >= bounds[0]
                    && u.position.y >= bounds[1]
                    && u.position.x <= bounds[2]
                    && u.position.y <= bounds[3]
            })
            .min_by_key(|u| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        ability,
                        sequence,
                        u.id,
                    ),
                    u.id,
                )
            });
        let Some(center) = selected.map(|u| u.position) else {
            return 0;
        };
        let effect = AbilityEffect::FrostNova {
            // The shared native child identity owns the slow and its presentation binding.
            ability: p.child,
            radius: p.radius,
            primary_damage: 0,
            area_damage: p.damage,
            duration_ticks: p.duration_ticks,
            hero_duration_ticks: p.hero_duration_ticks,
            movement_percent_delta: p.movement_percent_delta,
            attack_speed_percent_delta: p.attack_speed_percent_delta,
            targets: p.targets,
        };
        self.last_ability_casts.push(AbilityCastEvent {
            source: source.id,
            ability: p.child,
            target: AbilityCastTarget::Point(center),
            target_position: Some(center),
            effect: AbilityEffect::NagaEarthquake(p),
        });
        let mut count = 0;
        for i in 0..units.len() {
            let u = units[i];
            if u.health <= 0
                || u.team == source.team
                || center.distance_sq(u.position) > square_i32(p.radius)
            {
                continue;
            }
            let before = u.health;
            if apply_ability_effect_to_unit(
                &mut units[i],
                effect,
                source.team,
                self.next_tick,
                self.combat_rules.damage_rules,
            ) {
                // AHtc's native slow is undispellable even though the packet is spell damage.
                status::mark_modifier_undispellable(&mut units[i].status, ModifierId(p.child.0));
                // The actual source is an invulnerable native dummy, not the logical building.
                death_healing::observe_inline_death_from(units, i, before, None);
                count += 1;
            }
        }
        count
    }
}

impl Simulation {
    pub(super) fn cast_naga_charm(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AbilityId,
        trigger: SimId,
        p: crate::building_mechanics::NagaCharmProfile,
        sequence: u64,
        units: &mut [UnitSnapshot],
    ) -> bool {
        let owner = self
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source.id))
            .and_then(|e| e.get::<Owner>())
            .map_or(PlayerId(source.team.0), |o| o.0);
        self.launch_native_parasite(source, owner, trigger, p.parent);
        let bounds =
            crate::building_mechanics::snowveil_for_version(p.map_version).battlefield_bounds;
        let selected = units
            .iter()
            .enumerate()
            .filter(|(_, u)| {
                u.health > 0
                    && u.team != source.team
                    && u.classifications.combat_sapper
                    && u.rawcode != p.selector_excluded_rawcode
                    && !p
                        .selector_exclusions
                        .into_iter()
                        .any(|a| self.native_ability_installed(u, a))
                    && u.position.x >= bounds[0]
                    && u.position.y >= bounds[1]
                    && u.position.x <= bounds[2]
                    && u.position.y <= bounds[3]
            })
            .min_by_key(|(_, u)| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        ability,
                        sequence,
                        u.id,
                    ),
                    u.id,
                )
            })
            .map(|(i, _)| i);
        let Some(i) = selected else {
            return false;
        };
        let u = &mut units[i];
        if self.check_negative_building_shield(u, p.map_version, source.id, sequence) {
            return false;
        }
        if self.visible_teams(u.position) & (1 << source.team.0) == 0 {
            self.reveal_area(
                source.team,
                u.position,
                p.vision_radius,
                u64::from(p.vision_ticks),
                false,
            );
        }
        if u.classifications.uses_hero_spell_rules()
            || u.classifications.spell_immune
            || u.classifications.invulnerable
            || !p.targets.can_target_unit(u.target_movement_class())
            || self.ability_source_distance_sq(source.origin, u.position)
                > square_i32(p.child_range)
            || (u.is_invisible(self.next_tick) && !u.visible_to(source.team, self.next_tick))
            || u.classifications
                .neutral_hostile_creep_level
                .is_some_and(|level| p.maximum_creep_level > 0 && level > p.maximum_creep_level)
        {
            return false;
        }
        // Native Charm clears negative effects, including undispellable effects, before the owner event.
        clear_charm_debuffs(&mut u.status);
        self.cleanse_building_spell_controls(u.id, u.position, &[]);
        let parasites: Vec<_> = self.world.iter_entities().filter_map(|e| {
            matches!(e.get::<crate::components::NativeAction>(), Some(crate::components::NativeAction::Parasite(p)) if p.target == u.id && p.buff_until_tick.is_some()).then_some(e.id())
        }).collect();
        for e in parasites {
            self.world.despawn(e);
        }
        let old_owner = u.owner;
        let old_team = u.team;
        let old_creep = u.classifications.neutral_hostile_creep_level;
        u.owner = owner;
        u.team = source.team;
        u.classifications.neutral_hostile_creep_level = None;
        self.world
            .entity_mut(u.entity)
            .insert((Owner(owner), source.team, u.classifications));
        // Ownership listener calls hasShield independently; its second random attempt has its own domain.
        let rejected = self.native_ability_installed(u, p.ownership_rejection)
            || self.check_negative_building_shield(
                u,
                p.map_version,
                source.id,
                sequence ^ (1 << 63),
            );
        if rejected {
            u.owner = old_owner;
            u.team = old_team;
            u.classifications.neutral_hostile_creep_level = old_creep;
            self.world
                .entity_mut(u.entity)
                .insert((Owner(old_owner), old_team, u.classifications));
            self.last_building_spell_visuals
                .push(BuildingSpellVisualEvent {
                    target: u.id,
                    position: u.position,
                    kind: BuildingSpellVisualKind::ShieldBlocked,
                });
        } else {
            self.project_building_spell_control(u);
            if u.rawcode == p.defender_rawcode {
                let id = self.allocate_id();
                self.world.spawn((
                    id,
                    crate::components::NativeAction::ResumeAttack {
                        source: u.id,
                        due_tick: self.next_tick + u64::from(p.defender_delay_ticks),
                    },
                ));
            } else if !self.native_ability_installed(u, p.order_block) {
                resume_charmed_attack(u);
            }
        }
        self.last_ability_casts.push(AbilityCastEvent {
            source: source.id,
            ability: p.child,
            target: AbilityCastTarget::Unit(u.id),
            target_position: Some(u.position),
            effect: AbilityEffect::NagaCharm(p),
        });
        true
    }
}
fn resume_charmed_attack(u: &mut UnitSnapshot) {
    u.target = None;
    u.direct_retaliation_lock = false;
    u.ally_defense_lock = false;
    u.status.pending_attack = None;
    u.status.pending_cast = None;
    u.status.action_animation = None;
}
fn clear_charm_debuffs(s: &mut StatusState) {
    s.acid_bomb = None;
    s.poison = None;
    s.purge = None;
    s.ensnare = None;
    s.stunned_until_tick = 0;
    s.native_stun_until_tick = 0;
    s.native_stun_ability = None;
    s.rooted_until_tick = 0;
    s.frozen_until_tick = 0;
    s.frozen_ability = None;
    super::native_carriers::retain_modifiers(
        &mut s.movement_modifiers,
        &mut s.movement_modifier_count,
        |m| m.percent_delta >= 0,
    );
    super::native_carriers::retain_modifiers(
        &mut s.attack_speed_modifiers,
        &mut s.attack_speed_modifier_count,
        |m| m.percent_delta >= 0,
    );
    super::native_carriers::retain_modifiers(
        &mut s.armor_modifiers,
        &mut s.armor_modifier_count,
        |m| {
            m.native_buff.is_none_or(|b| b.positive)
                && m.armor_bonus_per_100 >= 0
                && m.damage_bonus_per_10k >= 0
                && m.damage_taken_bonus_per_10k <= 0
        },
    );
    s.damage_over_time_count = 0;
    s.damage_over_time
        .fill(crate::components::TimedDamageOverTime::default());
}
