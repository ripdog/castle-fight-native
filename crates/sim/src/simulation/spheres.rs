use super::*;
use crate::components::{NativeAction, SphereAttachmentState, SphereFlightState};

impl Simulation {
    pub(super) fn release_spheres(&mut self, units: &mut [UnitSnapshot]) {
        let movements = units
            .iter()
            .map(|unit| (unit.id, unit.movement_class))
            .collect::<Vec<_>>();
        let target_movement = |id| {
            movements
                .iter()
                .find_map(|(unit, movement)| (*unit == id).then_some(*movement))
                .unwrap_or(MovementClass::Ground)
        };
        for unit in units.iter_mut().filter(|unit| {
            !unit.abilities_disabled
                && unit
                    .effective_passive_effects()
                    .any(|effect| matches!(effect, PassiveUnitEffect::Sphere(_)))
        }) {
            let actions = self
                .last_attacks
                .iter()
                .filter(|attack| attack.source == unit.id)
                .map(|attack| {
                    (
                        attack.source_position,
                        Some(attack.target_position),
                        target_movement(attack.target),
                    )
                })
                .chain(
                    self.last_ability_casts
                        .iter()
                        .filter(|cast| cast.source == unit.id && !unit.effective_passive_effects().any(|effect| matches!(effect, PassiveUnitEffect::Corruption(p) if p.ability == cast.ability)))
                        .map(|cast| {
                            (
                                unit.position,
                                cast.target_position,
                                match cast.target {
                                    AbilityCastTarget::Unit(target) => target_movement(target),
                                    _ => MovementClass::Ground,
                                },
                            )
                        }),
                )
                .collect::<Vec<_>>();
            let profiles = unit
                .effective_passive_effects()
                .filter_map(|effect| {
                    if let PassiveUnitEffect::Sphere(profile) = effect {
                        Some(profile)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            for (origin, destination, target_movement) in actions {
                for profile in &profiles {
                    let index = unit
                        .status
                        .spheres
                        .iter()
                        .position(|state| {
                            state.is_some_and(|state| state.ability == profile.ability)
                        })
                        .or_else(|| unit.status.spheres.iter().position(Option::is_none))
                        .expect("native sphere capacity");
                    let state = unit.status.spheres[index].get_or_insert(SphereAttachmentState {
                        ability: profile.ability,
                        hidden_until: [0; 3],
                        next_slot: 0,
                    });
                    state.hidden_until[usize::from(state.next_slot)] =
                        self.next_tick + u64::from(profile.cooldown_ticks);
                    state.next_slot = (state.next_slot + 1) % profile.attachment_count;
                    let Some(destination) = destination else {
                        continue;
                    };
                    let flight = projectile_travel_ticks(
                        origin.distance_sq(destination),
                        profile.speed_per_tick,
                    );
                    let id = SimId(VISUAL_ID_BIT | self.next_visual_id);
                    self.next_visual_id = self
                        .next_visual_id
                        .checked_add(1)
                        .filter(|next| *next < VISUAL_ID_BIT)
                        .expect("visual SimId space exhausted");
                    self.world.spawn((
                        id,
                        NativeAction::SphereFlight(SphereFlightState {
                            source: unit.id,
                            ability: profile.ability,
                            source_movement: unit.movement_class,
                            target_movement,
                            origin,
                            destination,
                            launch_tick: self.next_tick,
                            impact_tick: self.next_tick + flight,
                        }),
                    ));
                }
            }
        }
    }
}
