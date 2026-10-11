use super::*;
use crate::components::{NativeAction, TravellingManaClapProfile, TravellingManaClapState};

pub(super) fn primary_eligible(
    u: &UnitSnapshot,
    team: Team,
    p: TravellingManaClapProfile,
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

impl Simulation {
    pub(super) fn cast_travelling_mana_clap(
        &mut self,
        source: AbilitySourceSnapshot,
        target: usize,
        p: TravellingManaClapProfile,
        units: &[UnitSnapshot],
    ) {
        let owner = self
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source.id))
            .and_then(|e| e.get::<Owner>())
            .expect("clap caster owner")
            .0;
        let origin = match source.origin {
            AbilitySourceOrigin::Unit(point) => point,
            AbilitySourceOrigin::Building(f) => {
                footprint_center_point(f, self.config.navigation_cell_size)
            }
        };
        let destination = units[target].position;
        let distance_sq = origin.distance_sq(destination);
        let floor = distance_sq.isqrt();
        let distance = floor + u64::from(floor * floor < distance_sq);
        let delay = (distance * CASTLE_FIGHT_SIMULATION_HZ as u64)
            .div_ceil(p.callback_speed_per_second as u64)
            .max(1);
        self.launch_native_parasite(source, owner, units[target].id, p.parent);
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::TravellingManaClap(TravellingManaClapState {
                source: source.id,
                owner,
                team: source.team,
                origin,
                destination,
                position: origin,
                position_tick: self.next_tick,
                created_tick: self.next_tick,
                due_tick: self.next_tick + delay,
                profile: p,
                impacted: false,
            }),
        ));
    }

    pub(super) fn advance_travelling_mana_clap(
        &mut self,
        entity: Entity,
        mut state: TravellingManaClapState,
        units: &mut [UnitSnapshot],
    ) {
        let expiry = state.created_tick + u64::from(state.profile.mover_duration_ticks);
        if self.next_tick < expiry {
            state.position = state
                .position
                .step_towards(state.destination, state.profile.mover_speed_per_tick);
        }
        state.position_tick = self.next_tick;
        if !state.impacted && self.next_tick >= state.due_tick {
            let p = state.profile;
            let clap = p.clap_effect();
            // Immediate native AHtc and the script's mana enumeration have independent masks.
            self.last_ability_casts.push(AbilityCastEvent {
                source: state.source,
                ability: p.clap.ability,
                target: AbilityCastTarget::Point(state.destination),
                target_position: Some(state.destination),
                effect: AbilityEffect::TravellingManaClap(p),
            });
            for i in 0..units.len() {
                let target = units[i];
                if target.health <= 0 || target.team == state.team || target.classifications.locust
                {
                    continue;
                }
                if state.destination.distance_sq(target.position) <= square_i32(p.clap.radius)
                    && apply_ability_effect_to_unit(
                        &mut units[i],
                        clap,
                        state.team,
                        self.next_tick,
                        self.combat_rules.damage_rules,
                    )
                {
                    status::mark_modifier_undispellable(
                        &mut units[i].status,
                        ModifierId(p.clap.ability.0),
                    );
                    death_healing::observe_inline_death_from(units, i, target.health, None);
                }
            }
            for i in 0..units.len() {
                let target = units[i];
                if target.health <= 0
                    || target.team == state.team
                    || target.classifications.locust
                    || !target.classifications.combat_sapper
                    || target.classifications.invulnerable
                    || target.target_movement_class() != MovementClass::Ground
                    || self.native_ability_installed(&target, p.excluded_ability)
                    || state.destination.distance_sq(target.position) > square_i32(p.burn_radius)
                {
                    continue;
                }
                let burn = target
                    .mana_current
                    .unwrap_or(0)
                    .min(p.maximum_mana_burn)
                    .max(0);
                if burn == 0 {
                    continue;
                }
                units[i].mana_current = Some(target.mana_current.expect("positive mana") - burn);
                self.last_ability_casts.push(AbilityCastEvent {
                    source: state.source,
                    ability: p.feedback_visual_ability,
                    target: AbilityCastTarget::Unit(target.id),
                    target_position: Some(target.position),
                    effect: AbilityEffect::ManaBurnVisual,
                });
                let damage = self
                    .combat_rules
                    .damage_rules
                    .apply_attack_with_armor_per_100(
                        burn,
                        DamageType::Chaos,
                        target.armor.armor_type,
                        effective_armor_points_per_100(&target),
                    );
                let damage = scale_damage_per_10k(
                    native_kill_effects::damage_after_native_incoming(
                        &target,
                        damage,
                        self.next_tick,
                    ),
                    target.snow_damage_taken_per_10k,
                );
                damage_thresholds::observe_damage_event(&mut units[i], target.health, damage);
                death_healing::commit_health_from(
                    units,
                    i,
                    target.health.saturating_sub(damage),
                    None,
                );
            }
            state.impacted = true;
        }
        if state.impacted && self.next_tick >= expiry {
            self.world.despawn(entity);
        } else {
            self.world
                .entity_mut(entity)
                .insert(NativeAction::TravellingManaClap(state));
        }
    }
}
