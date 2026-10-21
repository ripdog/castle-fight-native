//! Construction grants retain the native callback after a released weapon outlives its tower.
use super::*;
use crate::building_mechanics::BuildingDamageProcState;
use crate::components::{GuaranteedHitProjectile, NativeAction};
const RANDOM_PURPOSE_BUILDING_DAMAGE_PROC: u64 = 0x424f_554c_4445_5221;

impl Simulation {
    pub(super) fn start_building_damage_proc(&mut self, entity: Entity) {
        let source = self.world.entity(entity);
        let Some(content) = source.get::<ContentIdentity>() else {
            return;
        };
        let Some(profile) = crate::building_mechanics::damage_proc_for_version(
            content.map_version,
            content.rawcode,
        ) else {
            return;
        };
        let state = BuildingDamageProcState {
            map_version: content.map_version,
            source: *source.get::<SimId>().unwrap(),
            team: *source.get::<Team>().unwrap(),
            origin: footprint_center_point(
                *source.get::<BuildingFootprint>().unwrap(),
                self.config.navigation_cell_size,
            ),
            profile,
            event_sequence: 0,
        };
        if self.world.iter_entities().any(|e| {
            matches!(e.get::<NativeAction>(), Some(NativeAction::BuildingDamageProc(p)) if p.source == state.source)
        }) {
            return;
        }
        let id = self.allocate_id();
        self.world
            .spawn((id, NativeAction::BuildingDamageProc(state)));
    }
    pub(super) fn advance_building_damage_proc(
        &mut self,
        entity: Entity,
        state: BuildingDamageProcState,
    ) {
        let referenced = self.world.iter_entities().any(|e| {
            (e.get::<SimId>() == Some(&state.source) && e.get::<BuildingFootprint>().is_some())
                || e.get::<GuaranteedHitProjectile>()
                    .is_some_and(|p| p.source == state.source)
        });
        if !referenced {
            self.world.despawn(entity);
        }
    }
    pub(super) fn observe_building_damage_proc(
        &mut self,
        source: SimId,
        victim: &UnitSnapshot,
        position: SimPoint,
        tick: u64,
    ) {
        if !victim.classifications.combat_sapper {
            return;
        }
        let Some((entity, mut state)) = self.world.iter_entities().find_map(|e| match e
            .get::<NativeAction>()
        {
            Some(NativeAction::BuildingDamageProc(p)) if p.source == source => Some((e.id(), *p)),
            _ => None,
        }) else {
            return;
        };
        let sequence = state.event_sequence;
        state.event_sequence = sequence
            .checked_add(1)
            .expect("building callback sequence overflow");
        let p = state.profile;
        let success = deterministic_random(
            self.config.match_seed,
            tick,
            source,
            RANDOM_PURPOSE_BUILDING_DAMAGE_PROC ^ u64::from(p.helper.0),
            sequence,
        ) % 100
            * 100
            < u64::from(p.chance_per_10k);
        self.world
            .entity_mut(entity)
            .insert(NativeAction::BuildingDamageProc(state));
        // Each rolled proc creates a fresh native dummy; child cooldown is independent.
        let child_targets = p.bolt.map_or_else(
            || p.chain.expect("exactly one native child").targets,
            |bolt| bolt.targets,
        );
        if !success
            || victim.health <= 0
            || victim.team == state.team
            || victim.classifications.spell_immune
            || victim.classifications.invulnerable
            || !victim.visible_to(state.team, tick)
            || !child_targets.can_target_unit(victim.target_movement_class())
            || state.origin.distance_sq(position) > square_i32(p.range)
        {
            return;
        }
        match (p.bolt, p.chain) {
            (Some(bolt), None) => {
                self.launch_dummy_bolt(source, state.team, state.origin, victim.id, position, bolt);
            }
            (None, Some(chain)) => {
                self.launch_dummy_chain_lightning(
                    source, state.team, victim.id, position, chain, tick,
                );
            }
            _ => unreachable!("exactly one native child"),
        }
    }

    /// The dummy's Chain Lightning order on the damaged unit: the first bolt strikes that unit
    /// itself on the next chain phase, then jumps follow the ordinary hop cadence.
    fn launch_dummy_chain_lightning(
        &mut self,
        source: SimId,
        team: Team,
        target: SimId,
        position: SimPoint,
        profile: crate::ChainLightningEffectProfile,
        tick: u64,
    ) {
        // `chain_lightning_jump_due_tick` fires jump one a quarter second after `started_tick`;
        // an order issued now strikes at once, so the chain starts one jump interval in the past.
        let one_jump = chain_lightning_jump_due_tick(0, 1);
        let id = self.allocate_id();
        self.world.spawn((
            id,
            crate::components::ChainLightningState {
                source,
                source_team: team,
                profile,
                started_tick: tick.saturating_sub(one_jump),
                next_jump_index: 1,
                current_target: target,
                last_position: position,
                next_damage: profile.initial_damage,
                hit_targets: [SimId(0); MAX_BOUNCE_HITS],
                hit_count: 0,
            },
        ));
    }
}

#[cfg(test)]
mod tests;
