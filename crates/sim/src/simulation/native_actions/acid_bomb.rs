use super::*;
use crate::{
    AcidBombProfile, AcidBombStatus, ActionAnimationState,
    building_mechanics::BuildingAcidBombProfile, components::AcidBombMissileState,
};

pub(in crate::simulation) fn eligible(
    unit: &UnitSnapshot,
    team: Team,
    profile: AcidBombProfile,
) -> bool {
    unit.health > 0
        && unit.team != team
        && !unit.mechanical
        && !unit.classifications.invulnerable
        && !unit.classifications.spell_immune
        && profile
            .targets
            .can_target_unit(unit.target_movement_class())
}

impl Simulation {
    pub(in crate::simulation) fn cast_building_acid_bomb(
        &mut self,
        source: AbilitySourceSnapshot,
        profile: BuildingAcidBombProfile,
        sequence: u64,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) -> bool {
        let bounds =
            crate::building_mechanics::snowveil_for_version(profile.map_version).battlefield_bounds;
        let target = units
            .iter()
            .filter(|unit| {
                unit.health > 0
                    && unit.team != source.team
                    && self.hex_trigger_eligible(unit, profile.map_version)
                    && unit.position.x >= bounds[0]
                    && unit.position.x <= bounds[2]
                    && unit.position.y >= bounds[1]
                    && unit.position.y <= bounds[3]
            })
            .min_by_key(|unit| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        profile.child.ability,
                        sequence,
                        unit.id,
                    ),
                    unit.id,
                )
            });
        let Some(target) = target else {
            return false;
        };
        // The animation callbacks run after script selection, including failed native child orders.
        let started_tick = self.next_tick + u64::from(profile.attack_delay_ticks);
        let animation = ActionAnimationState {
            kind: ActionAnimationKind::Attack,
            started_tick,
            until_tick: started_tick + u64::from(profile.attack_animation_ticks),
        };
        if let Some(building) = buildings
            .iter_mut()
            .find(|building| building.id == source.id)
            && let Some(status) = building.status.as_mut()
        {
            status.action_animation = Some(animation);
        }
        if self.visible_teams(target.position) & (1 << source.team.0) == 0 {
            self.reveal_area(
                source.team,
                target.position,
                profile.vision_radius,
                u64::from(profile.vision_ticks),
                false,
            );
        }
        let origin = match source.origin {
            AbilitySourceOrigin::Unit(position) => position,
            AbilitySourceOrigin::Building(footprint) => {
                footprint_center_point(footprint, self.config.navigation_cell_size)
            }
        };
        if !eligible(target, source.team, profile.child)
            || (target.classifications.invisible && !target.visible_to(source.team, self.next_tick))
            || origin.distance_sq(target.position) > square_i32(profile.child.range)
        {
            return false;
        }
        self.launch_acid_bomb(
            source.id,
            source.team,
            origin,
            target.id,
            target.position,
            profile.child,
        );
        true
    }

    pub(in crate::simulation) fn launch_acid_bomb(
        &mut self,
        source: SimId,
        team: Team,
        origin: SimPoint,
        target: SimId,
        destination: SimPoint,
        profile: AcidBombProfile,
    ) {
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::AcidBomb(AcidBombMissileState {
                source,
                team,
                target,
                profile,
                launch_position: origin,
                destination,
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

    pub(super) fn advance_acid_bomb(
        &mut self,
        entity: Entity,
        mut state: AcidBombMissileState,
        units: &mut [UnitSnapshot],
    ) {
        if self.next_tick <= state.position_tick {
            return;
        }
        let Some(index) =
            find_unit_index(units, state.target).filter(|&index| units[index].health > 0)
        else {
            self.world.despawn(entity);
            return;
        };
        let destination = if state.profile.homing {
            units[index].position
        } else {
            state.destination
        };
        let distance = state.position.distance_sq(destination);
        state.position = state
            .position
            .step_towards(destination, state.profile.speed_per_tick);
        state.position_tick = self.next_tick;
        if distance > square_i32(state.profile.speed_per_tick) {
            if state.profile.homing {
                state.impact_tick = self.next_tick
                    + projectile_travel_ticks(
                        state.position.distance_sq(destination),
                        state.profile.speed_per_tick,
                    );
            }
            self.world
                .entity_mut(entity)
                .insert(NativeAction::AcidBomb(state));
            return;
        }
        self.world.despawn(entity);
        if !eligible(&units[index], state.team, state.profile) {
            return;
        }
        // Missile homing controls flight. The native targeted effect still uses its live recipient.
        let center = units[index].position;
        for target in units.iter_mut() {
            if !eligible(target, state.team, state.profile)
                || (target.id != state.target
                    && center.distance_sq(target.position) > square_i32(state.profile.radius))
            {
                continue;
            }
            let damage = if target.id == state.target {
                state.profile.primary_damage
            } else {
                state.profile.secondary_damage
            };
            let duration = if target.classifications.hero {
                state.profile.hero_duration_ticks
            } else {
                state.profile.duration_ticks
            };
            target.status.acid_bomb = Some(AcidBombStatus {
                profile: state.profile,
                damage_per_pulse: damage,
                next_pulse_tick: self.next_tick,
                expires_tick: self.next_tick + u64::from(duration),
            });
            resolve_periodic_acid_bomb(target, self.next_tick, self.combat_rules.damage_rules);
        }
    }
}

pub(in crate::simulation) fn resolve_periodic_acid_bomb(
    unit: &mut UnitSnapshot,
    tick: u64,
    rules: DamageRules,
) {
    let Some(mut acid) = unit.status.acid_bomb else {
        return;
    };
    while unit.health > 0
        && acid.next_pulse_tick <= tick
        && acid.next_pulse_tick < acid.expires_tick
    {
        if !unit.classifications.invulnerable && !unit.classifications.spell_immune {
            let damage = rules.apply_spell(acid.damage_per_pulse, unit.armor.armor_type);
            let damage = spell_damage_after_defend(*unit, damage, tick);
            super::super::damage_thresholds::observe_damage(unit, unit.health);
            unit.health = unit.health.saturating_sub(damage);
        }
        acid.next_pulse_tick = acid
            .next_pulse_tick
            .checked_add(u64::from(acid.profile.interval_ticks))
            .expect("acid pulse timeline exhausted");
    }
    unit.status.acid_bomb = (tick < acid.expires_tick).then_some(acid);
}
