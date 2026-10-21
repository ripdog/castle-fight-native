//! Native Plague aura (`Aap1`, "Disease Cloud"): a long damage-over-time infection that follows
//! its victims, and the lingering cloud an infected body leaves where it dies.
use super::*;
use crate::PoisonProfile;
use crate::components::{NativeAction, PlagueWardState};

fn hostile(unit: &UnitSnapshot, team: Team, targets: AttackTargetMask, organic_only: bool) -> bool {
    unit.health > 0
        && unit.team != team
        && !unit.classifications.locust
        && !unit.classifications.invulnerable
        && !unit.classifications.spell_immune
        && !(organic_only && unit.mechanical)
        && targets.can_target_unit(unit.target_movement_class())
}

impl Simulation {
    pub(super) fn resolve_disease_clouds(&mut self, units: &mut [UnitSnapshot]) {
        let sources = units
            .iter()
            .filter(|u| u.health > 0 && !u.abilities_disabled)
            .flat_map(|u| {
                u.effective_passive_effects().filter_map(move |effect| {
                    let PassiveUnitEffect::DiseaseCloud(profile) = effect else {
                        return None;
                    };
                    Some((u.id, u.team, u.position, profile))
                })
            })
            .collect::<Vec<_>>();
        for (source, team, position, profile) in sources {
            self.infect_area(
                source,
                team,
                position,
                profile.radius,
                profile.targets,
                profile.organic_only,
                profile.infection,
                units,
            );
        }
    }

    /// Infects every hostile body in range that is not already carrying a fresh infection. A
    /// body inside the area keeps its infection topped up once per second rather than every tick.
    #[allow(clippy::too_many_arguments)]
    fn infect_area(
        &mut self,
        source: SimId,
        team: Team,
        center: SimPoint,
        radius: i32,
        targets: AttackTargetMask,
        organic_only: bool,
        infection: PoisonProfile,
        units: &mut [UnitSnapshot],
    ) {
        let limit = square_i32(radius);
        let refresh_window = CASTLE_FIGHT_SIMULATION_HZ as u64;
        for unit in units.iter_mut() {
            if !hostile(unit, team, targets, organic_only)
                || center.distance_sq(unit.position) > limit
            {
                continue;
            }
            let duration = u64::from(if unit.classifications.uses_hero_spell_rules() {
                infection.hero_duration_ticks
            } else {
                infection.duration_ticks
            });
            let fresh = unit.status.poison.is_some_and(|status| {
                status.buffs.iter().flatten().any(|buff| {
                    buff.buff_rawcode == infection.buff_rawcode
                        && buff.expires_tick + refresh_window > self.next_tick + duration
                })
            });
            if !fresh {
                self.apply_poison(source, unit, infection, self.next_tick);
            }
        }
    }

    /// An infected body that dies leaves the cloud where it fell.
    pub(super) fn observe_plague_deaths(
        &mut self,
        units: &[UnitSnapshot],
        health: &[i32],
        positions: &[SimPoint],
    ) {
        let mut deaths = Vec::new();
        for entity in self.world.iter_entities() {
            let Some(NativeAction::Poison(state)) = entity.get::<NativeAction>() else {
                continue;
            };
            let Some(ward) = state.profile.ward else {
                continue;
            };
            let Some(i) = find_unit_index(units, state.target) else {
                continue;
            };
            if health[i] > 0
                || units[i].removed_without_death
                || !units[i]
                    .status
                    .poison
                    .is_some_and(|status| status.generation == state.generation)
            {
                continue;
            }
            let Some(id) = entity.get::<SimId>() else {
                continue;
            };
            deaths.push((*id, entity.id(), state.clone(), ward, i));
        }
        deaths.sort_unstable_by_key(|(id, ..)| *id);
        for (_, entity, state, ward, i) in deaths {
            self.world.despawn(entity);
            let id = self.allocate_id();
            self.world.spawn((
                id,
                NativeAction::PlagueWard(PlagueWardState {
                    source: state.source,
                    // Infection is hostile, so the cloud belongs to the victim's opponents.
                    team: Team(u8::from(units[i].team.0 == 0)),
                    position: positions[i],
                    infection: state.profile,
                    ward,
                    expires_tick: self.next_tick + u64::from(ward.duration_ticks),
                }),
            ));
        }
    }

    pub(super) fn advance_plague_ward(
        &mut self,
        entity: Entity,
        state: PlagueWardState,
        units: &mut [UnitSnapshot],
    ) {
        if self.next_tick >= state.expires_tick {
            self.world.despawn(entity);
            return;
        }
        self.infect_area(
            state.source,
            state.team,
            state.position,
            state.ward.radius,
            state.ward.targets,
            state.ward.organic_only,
            state.infection,
            units,
        );
    }
}
