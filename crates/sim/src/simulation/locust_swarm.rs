//! Native `AUls` Locust Swarm released by a dummy at a fixed point.
//!
//! The swarm is world state, independent of its caster. Locust bodies are not simulated: each
//! locust's committed target, strike time and re-engagement time are authoritative, derived from
//! the retained release interval, flight speed and attack cooldown.
use super::*;
use crate::components::{
    LocustSwarmProfile, LocustSwarmState, LocustState, MAX_LOCUSTS, NativeAction,
};

const RANDOM_PURPOSE_LOCUST: u64 = 0x4c4f_4355_5354_0001;
/// Re-engagement polls while no eligible enemy is in the swarm's area.
const IDLE_POLL_TICKS: u64 = 6;

fn flight_ticks(distance_sq: u64, speed_per_tick: i32) -> u64 {
    let distance = (distance_sq as f64).sqrt().ceil() as u64;
    distance.div_ceil(u64::try_from(speed_per_tick.max(1)).expect("positive locust speed"))
}

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
        if tick >= state.expires_tick {
            self.world.despawn(entity);
            return;
        }
        let profile = state.profile;
        for index in 0..usize::from(profile.count) {
            let release = state.start_tick
                + (index as u64 * u64::from(profile.release_interval_millis)
                    * CASTLE_FIGHT_SIMULATION_HZ as u64)
                    .div_ceil(1000);
            if tick < release {
                continue;
            }
            if state.locusts[index].strike_tick == tick {
                self.locust_strike(&state, index, units, buildings);
                state.locusts[index].strike_tick = 0;
            }
            let locust = state.locusts[index];
            if locust.strike_tick == 0 && tick >= locust.ready_tick.max(release) {
                match self.locust_target(&state, index, units, buildings) {
                    Some((target, distance_sq)) => {
                        let flight = flight_ticks(distance_sq, profile.flight_speed_per_tick);
                        let strike = tick + flight.max(1);
                        state.locusts[index] = LocustState {
                            target: Some(target),
                            strike_tick: strike,
                            // Landing hits clear the damage-return threshold, so the locust
                            // flies back before choosing again.
                            ready_tick: strike
                                + flight.max(u64::from(profile.attack_cooldown_ticks)),
                        };
                    }
                    None => {
                        state.locusts[index] = LocustState {
                            target: None,
                            strike_tick: 0,
                            ready_tick: tick + IDLE_POLL_TICKS,
                        };
                    }
                }
            }
        }
        if let Some(mut action) = self.world.get_mut::<NativeAction>(entity) {
            *action = NativeAction::LocustSwarm(state);
        }
    }

    /// Nearest hostile unit or structure in the swarm's area, skipping targets that already hold
    /// the retained maximum of other locusts. Ties resolve by stable identity.
    fn locust_target(
        &self,
        state: &LocustSwarmState,
        index: usize,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) -> Option<(SimId, u64)> {
        let profile = state.profile;
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
        let units_best = units
            .iter()
            .filter(|unit| {
                unit.health > 0
                    && unit.team != state.team
                    && !unit.classifications.invulnerable
                    && !unit.classifications.locust
                    && profile.targets.can_target_unit(unit.target_movement_class())
                    && unit.visible_to(state.team, self.next_tick)
            })
            .map(|unit| (state.center.distance_sq(unit.position), unit.id))
            .filter(|(distance, id)| *distance <= radius_sq && assigned(*id) < usize::from(profile.max_per_target))
            .min();
        let buildings_best = profile
            .targets
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
                        *distance <= radius_sq && assigned(*id) < usize::from(profile.max_per_target)
                    })
                    .min()
            })
            .flatten();
        // Units are always preferred over structures, as in ordinary weapon acquisition.
        units_best.or(buildings_best).map(|(distance, id)| (id, distance))
    }

    fn locust_strike(
        &self,
        state: &LocustSwarmState,
        index: usize,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        let profile = state.profile;
        let Some(target) = state.locusts[index].target else {
            return;
        };
        let dice = (0..profile.dice_number)
            .map(|die| {
                let roll = deterministic_random(
                    self.config.match_seed,
                    self.next_tick,
                    state.source,
                    RANDOM_PURPOSE_LOCUST ^ u64::from(profile.ability.0),
                    (index as u64) << 8 | u64::from(die),
                );
                1 + i32::try_from(roll % u64::from(profile.dice_sides.max(1))).expect("die face")
            })
            .sum::<i32>();
        let amount = profile.base_damage + dice;
        if let Some(victim) = find_unit_index(units, target) {
            if units[victim].health <= 0 || units[victim].classifications.invulnerable {
                return;
            }
            self.apply_source_ability_to_unit(
                state.source,
                state.team,
                victim,
                AbilityEffect::Damage { amount },
                units,
            );
        } else if let Some(victim) = find_building_index(buildings, target) {
            let building = &mut buildings[victim];
            if building.health <= 0 || building.classifications.invulnerable {
                return;
            }
            let spell = self
                .combat_rules
                .damage_rules
                .apply_spell(amount, building.armor.armor_type);
            let damage = scale_damage_per_10k(spell, profile.structure_damage_per_10k);
            building.health = building.health.saturating_sub(damage);
        }
    }
}
