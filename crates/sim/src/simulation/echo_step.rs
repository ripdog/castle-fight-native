//! Script-owned Echo Step: a damage listener blinks the bearer toward its own castle and leaves a
//! remnant mine behind. The remnant is an independent authoritative action.
use super::*;
use crate::components::{EchoRemnantProfile, EchoRemnantState, EchoStepProfile, NativeAction};

impl Simulation {
    /// Evaluated on every weapon damage event aimed at a bearer, before its health is committed.
    pub(super) fn observe_echo_step(
        &mut self,
        source_id: SimId,
        victim_index: usize,
        tick: u64,
        units: &mut [UnitSnapshot],
        health: &[i32],
        positions: &[SimPoint],
    ) {
        let Some(profile) = units[victim_index]
            .effective_passive_effects()
            .find_map(|effect| {
                if let PassiveUnitEffect::EchoStep(profile) = effect {
                    Some(profile)
                } else {
                    None
                }
            })
        else {
            return;
        };
        let Some(source_index) = find_unit_index(units, source_id) else {
            return;
        };
        if health[victim_index] <= 0
            || health[source_index] <= 0
            || !units[source_index].classifications.combat_sapper
            || tick < units[victim_index].status.echo_step_ready_tick
            || positions[victim_index].distance_sq(positions[source_index])
                >= square_i32(profile.trigger_range)
        {
            return;
        }
        let victim = &units[victim_index];
        let origin = positions[victim_index];
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::EchoRemnant(EchoRemnantState {
                profile: profile.remnant,
                source: victim.id,
                owner: victim.owner,
                team: victim.team,
                position: origin,
                created_tick: tick,
                armed_tick: tick + u64::from(profile.remnant.activation_ticks),
                expires_tick: tick + u64::from(profile.remnant.lifetime_ticks),
            }),
        ));
        let destination = self.echo_step_destination(&units[victim_index], origin, profile);
        let victim = &mut units[victim_index];
        victim.pending_blink = Some(destination);
        victim.status.echo_step_ready_tick = tick + u64::from(profile.cooldown_ticks);
        // The script re-issues an attack order: the bearer drops its target and reacquires.
        victim.target = None;
        victim.direct_retaliation_lock = false;
        victim.ally_defense_lock = false;
        victim.navigation = Default::default();
    }

    /// `SetUnitPosition` toward the bearer's own castle: the full blink distance, pulled back to
    /// the first traversable point when the destination is not legal for the bearer.
    fn echo_step_destination(
        &self,
        unit: &UnitSnapshot,
        origin: SimPoint,
        profile: EchoStepProfile,
    ) -> SimPoint {
        let team = usize::from(unit.team.0);
        let castle = self
            .config
            .team_castle_regions
            .get(team)
            .copied()
            .flatten()
            .map_or_else(
                || self.config.team_objective[1 - team.min(1)],
                |region| footprint_center_point(region, self.config.navigation_cell_size),
            );
        let (dx, dy) = (
            i64::from(castle.x) - i64::from(origin.x),
            i64::from(castle.y) - i64::from(origin.y),
        );
        let length = (dx * dx + dy * dy).isqrt();
        if length == 0 {
            return origin;
        }
        let component = self
            .topology
            .component_id(self.topology.cell_of_point(origin));
        let mut distance = i64::from(profile.blink_distance);
        while distance > 0 {
            let candidate = SimPoint::new(
                origin.x + i32::try_from(dx * distance / length).expect("blink x"),
                origin.y + i32::try_from(dy * distance / length).expect("blink y"),
            );
            let legal = match (unit.movement_class, component) {
                (MovementClass::Air, _) => self
                    .topology
                    .circle_is_inside_bounds(candidate, unit.collision_radius),
                (_, Some(component)) => self.topology.circle_is_traversable_in_component(
                    candidate,
                    unit.collision_radius,
                    component,
                ),
                (_, None) => false,
            };
            if legal {
                return candidate;
            }
            distance -= i64::from(self.config.navigation_cell_size);
        }
        origin
    }

    pub(super) fn advance_echo_remnant(
        &mut self,
        entity: Entity,
        state: EchoRemnantState,
        units: &mut [UnitSnapshot],
    ) {
        let tick = self.next_tick;
        let profile = state.profile;
        let triggered = tick >= state.armed_tick
            && units.iter().any(|unit| {
                unit.health > 0
                    && unit.team != state.team
                    && !unit.classifications.invulnerable
                    && profile
                        .trigger_targets
                        .can_target_unit(unit.target_movement_class())
                    && state.position.distance_sq(unit.position)
                        <= square_i32(profile.detection_radius)
            });
        if !triggered && tick < state.expires_tick {
            return;
        }
        self.world.despawn(entity);
        self.explode_echo_remnant(state, units);
    }

    /// The remnant's death handler: magic damage with the normal attack table, no armor points.
    fn explode_echo_remnant(&mut self, state: EchoRemnantState, units: &mut [UnitSnapshot]) {
        let profile: EchoRemnantProfile = state.profile;
        let radius_sq = square_i32(profile.radius);
        for i in 0..units.len() {
            let unit = &units[i];
            if unit.health <= 0
                || unit.team == state.team
                || !unit.classifications.combat_sapper
                || unit.classifications.invulnerable
                || unit.classifications.spell_immune
                || unit.rawcode == profile.excluded_unit_rawcode
                || unit.status.native_grants.contains(profile.excluded_ability)
                || state.position.distance_sq(unit.position) > radius_sq
            {
                continue;
            }
            let damage = self
                .combat_rules
                .damage_rules
                .apply_attack_with_armor_per_100(
                    profile.damage,
                    DamageType::Normal,
                    unit.armor.armor_type,
                    0,
                );
            let damage = spell_damage_after_defend(*unit, damage, self.next_tick);
            let before = units[i].health;
            damage_thresholds::observe_damage_event(&mut units[i], before, damage);
            let after = before.saturating_sub(damage);
            death_healing::commit_health_from(units, i, after, Some(state.source));
        }
    }
}
