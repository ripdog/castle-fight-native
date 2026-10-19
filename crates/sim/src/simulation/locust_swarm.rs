//! Native `AUls` Locust Swarm released by a dummy at a fixed point.
//!
//! The swarm is world state, independent of its caster, and its locusts are real flying units of
//! the retained swarm body. The controller releases them at the retained interval, sends each at
//! the nearest eligible enemy near the swarm center (no more than the retained number per target),
//! and calls it back to the center after it strikes. Movement, contact and damage are the
//! ordinary unit rules; only the order is scripted.
use super::*;
use crate::components::{
    LocustState, LocustSwarmProfile, LocustSwarmState, MAX_LOCUSTS, NativeAction, SupportOrderState,
};

/// A returning locust is back once within this distance of the swarm center.
const RETURN_TOUCH_DISTANCE: i32 = 32 * SUBUNITS_PER_WORLD_UNIT;

impl Simulation {
    pub(super) fn launch_locust_swarm(
        &mut self,
        source: AbilitySourceSnapshot,
        owner: PlayerId,
        center: SimPoint,
        profile: LocustSwarmProfile,
    ) {
        assert!(usize::from(profile.count) <= MAX_LOCUSTS);
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::LocustSwarm(LocustSwarmState {
                profile,
                source: source.id,
                owner,
                team: source.team,
                map_version: source
                    .map_version
                    .unwrap_or(crate::MapVersion::CASTLE_FIGHT_9_27),
                center,
                start_tick: self.next_tick,
                expires_tick: self.next_tick + u64::from(profile.duration_ticks),
                locusts: [LocustState::default(); MAX_LOCUSTS],
            }),
        ));
    }

    pub(super) fn advance_locust_swarm(
        &mut self,
        entity: Entity,
        mut state: LocustSwarmState,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        let tick = self.next_tick;
        let profile = state.profile;
        if tick >= state.expires_tick {
            let bodies: Vec<_> = state
                .locusts
                .iter()
                .filter_map(|locust| locust.unit)
                .collect();
            self.expire_native_summons(&bodies, units);
            self.world.despawn(entity);
            return;
        }
        for index in 0..usize::from(profile.count) {
            let release = state.start_tick
                + (index as u64
                    * u64::from(profile.release_interval_millis)
                    * CASTLE_FIGHT_SIMULATION_HZ as u64)
                    .div_ceil(1000);
            if !state.locusts[index].released {
                if tick >= release {
                    state.locusts[index].unit = self.spawn_locust(&state);
                    state.locusts[index].released = true;
                }
                continue;
            }
            let Some(body) = state.locusts[index]
                .unit
                .and_then(|id| find_unit_index(units, id))
                .filter(|body| units[*body].health > 0)
            else {
                state.locusts[index].unit = None;
                continue;
            };
            let mut locust = state.locusts[index];
            // A launched attack (the body's attack counter advanced) is the strike: the damage
            // clears the return threshold, so the locust flies back to the center.
            if units[body].attack_sequence != locust.attack_sequence {
                locust.attack_sequence = units[body].attack_sequence;
                locust.returning = true;
                locust.target = None;
            }
            if locust.returning
                && units[body].position.distance_sq(state.center)
                    <= square_i32(RETURN_TOUCH_DISTANCE)
            {
                locust.returning = false;
            }
            if !locust.returning {
                let valid = locust.target.is_some_and(|target| {
                    self.locust_target_valid(&state, target, units, buildings)
                });
                if !valid {
                    locust.target = self.locust_target(&state, index, units, buildings);
                }
            }
            let order = if locust.returning || locust.target.is_none() {
                SupportOrderState {
                    move_point: Some(state.center),
                    ..SupportOrderState::default()
                }
            } else {
                SupportOrderState {
                    order_target: locust.target,
                    ..SupportOrderState::default()
                }
            };
            units[body].status.support_order = Some(order);
            if locust.returning || locust.target.is_none() {
                units[body].target = None;
            } else {
                units[body].target = locust.target;
            }
            state.locusts[index] = locust;
        }
        if let Some(mut action) = self.world.get_mut::<NativeAction>(entity) {
            *action = NativeAction::LocustSwarm(state);
        }
    }

    fn spawn_locust(&mut self, state: &LocustSwarmState) -> Option<SimId> {
        let kind = crate::CastleFightUnitKind::from_rawcode_for_version(
            state.profile.body_rawcode,
            state.map_version,
        )
        .ok()??;
        let mut definition = kind
            .definition_for_version(state.map_version)
            .ok()?
            .resolved();
        definition.properties.classifications.summoned = true;
        Some(self.spawn_resolved_unit_unchecked(
            Some(state.owner),
            UnitSpawn::from_template(state.team, state.center, definition.template),
            definition,
        ))
    }

    fn locust_target_valid(
        &self,
        state: &LocustSwarmState,
        target: SimId,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> bool {
        let radius_sq = square_i32(state.profile.radius);
        if let Some(index) = find_unit_index(units, target) {
            let unit = &units[index];
            return unit.health > 0
                && unit.team != state.team
                && !unit.classifications.invulnerable
                && state.center.distance_sq(unit.position) <= radius_sq;
        }
        find_building_index(buildings, target).is_some_and(|index| {
            let building = &buildings[index];
            building.health > 0
                && building.team != state.team
                && point_to_footprint_distance_sq(
                    state.center,
                    building.footprint,
                    self.config.navigation_cell_size,
                ) <= radius_sq
        })
    }

    /// Nearest hostile unit or structure in the swarm's area, skipping targets that already hold
    /// the retained maximum of other locusts. Units are preferred over structures; ties resolve
    /// by stable identity.
    fn locust_target(
        &self,
        state: &LocustSwarmState,
        index: usize,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> Option<SimId> {
        let profile = state.profile;
        let body_targets = state.locusts[index]
            .unit
            .and_then(|id| find_unit_index(units, id))
            .map(|body| units[body].attack_targets)?;
        let radius_sq = square_i32(profile.radius);
        let assigned = |id: SimId| {
            state
                .locusts
                .iter()
                .enumerate()
                .filter(|(other, locust)| {
                    *other != index
                        && *other < usize::from(profile.count)
                        && locust.target == Some(id)
                })
                .count()
        };
        let best_unit = units
            .iter()
            .filter(|unit| {
                unit.health > 0
                    && unit.team != state.team
                    && !unit.classifications.invulnerable
                    && !unit.classifications.locust
                    && body_targets.can_target_unit(unit.target_movement_class())
                    && unit.visible_to(state.team, self.next_tick)
            })
            .map(|unit| (state.center.distance_sq(unit.position), unit.id))
            .filter(|(distance, id)| {
                *distance <= radius_sq && assigned(*id) < usize::from(profile.max_per_target)
            })
            .min();
        let best_building = body_targets
            .can_target_buildings()
            .then(|| {
                buildings
                    .iter()
                    .filter(|building| {
                        building.health > 0
                            && building.team != state.team
                            && !building.classifications.invulnerable
                            && building.visible_teams & (1 << state.team.0) != 0
                    })
                    .map(|building| {
                        (
                            point_to_footprint_distance_sq(
                                state.center,
                                building.footprint,
                                self.config.navigation_cell_size,
                            ),
                            building.id,
                        )
                    })
                    .filter(|(distance, id)| {
                        *distance <= radius_sq
                            && assigned(*id) < usize::from(profile.max_per_target)
                    })
                    .min()
            })
            .flatten();
        best_unit.or(best_building).map(|(_, id)| id)
    }
}
