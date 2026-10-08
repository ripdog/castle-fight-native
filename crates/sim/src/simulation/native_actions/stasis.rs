use super::*;
use crate::{
    ActionAnimationState, building_mechanics::BuildingStasisProfile, components::StasisTrapState,
};

fn eligible(unit: &UnitSnapshot, team: Team, profile: BuildingStasisProfile) -> bool {
    unit.health > 0
        && unit.team != team
        && !unit.classifications.invulnerable
        && !unit.classifications.spell_immune
        && profile
            .targets
            .can_target_unit(unit.target_movement_class())
}

impl Simulation {
    pub(in crate::simulation) fn start_stasis_trap(
        &mut self,
        source: AbilitySourceSnapshot,
        profile: BuildingStasisProfile,
        buildings: &mut [BuildingSnapshot],
    ) {
        let position = match source.origin {
            AbilitySourceOrigin::Unit(point) => point,
            AbilitySourceOrigin::Building(footprint) => {
                footprint_center_point(footprint, self.config.navigation_cell_size)
            }
        };
        // The native summon is independent of the one-second dummy recycling callback.
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::StasisTrap(StasisTrapState {
                source: source.id,
                team: source.team,
                position,
                profile,
                armed_tick: self.next_tick + u64::from(profile.activation_ticks),
                detonation_tick: None,
                expires_tick: self.next_tick + u64::from(profile.lifetime_ticks),
            }),
        ));
        if let Some(building) = buildings
            .iter_mut()
            .find(|building| building.id == source.id)
            && let Some(status) = building.status.as_mut()
        {
            status.action_animation = Some(ActionAnimationState {
                kind: ActionAnimationKind::Cast,
                started_tick: self.next_tick,
                until_tick: self.next_tick + u64::from(profile.stand_ticks),
            });
        }
        self.last_ability_casts.push(AbilityCastEvent {
            source: source.id,
            ability: profile.ability,
            target: AbilityCastTarget::Point(position),
            target_position: Some(position),
            effect: AbilityEffect::BuildingStasis(profile),
        });
    }

    pub(super) fn advance_stasis_trap(
        &mut self,
        entity: Entity,
        mut state: StasisTrapState,
        units: &mut [UnitSnapshot],
    ) {
        if self.next_tick >= state.expires_tick {
            self.world.despawn(entity);
            return;
        }
        if self.next_tick < state.armed_tick {
            return;
        }
        if state.detonation_tick.is_none() {
            if !units.iter().any(|unit| {
                eligible(unit, state.team, state.profile)
                    && state.position.distance_sq(unit.position)
                        <= square_i32(state.profile.detection_radius)
            }) {
                return;
            }
            // Round the two native phases independently; do not collapse sub-tick delays.
            state.detonation_tick =
                Some(self.next_tick + u64::from(state.profile.detonation_ticks));
            self.world
                .entity_mut(entity)
                .insert(NativeAction::StasisTrap(state.clone()));
        }
        if self.next_tick < state.detonation_tick.unwrap() {
            return;
        }
        for unit in units {
            if !eligible(unit, state.team, state.profile)
                || state.position.distance_sq(unit.position)
                    > square_i32(state.profile.detonation_radius)
            {
                continue;
            }
            let duration = if unit.classifications.hero {
                state.profile.hero_stun_ticks
            } else {
                state.profile.stun_ticks
            };
            let expires = self.next_tick + u64::from(duration);
            unit.status.stunned_until_tick = unit.status.stunned_until_tick.max(expires);
            if duration > 0 && expires >= unit.status.native_stun_until_tick {
                unit.status.native_stun_until_tick = expires;
                unit.status.native_stun_ability = Some(state.profile.ability);
            }
        }
        // Native detonation consumes nearby traps of the same ward type, even unarmed traps.
        let removals: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|other| {
                let NativeAction::StasisTrap(trap) = other.get::<NativeAction>()? else {
                    return None;
                };
                (trap.profile.ward_rawcode == state.profile.ward_rawcode
                    && state.position.distance_sq(trap.position)
                        <= square_i32(state.profile.detonation_radius))
                .then_some(other.id())
            })
            .collect();
        for trap in removals {
            self.world.despawn(trap);
        }
    }
}
