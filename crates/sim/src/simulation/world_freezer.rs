//! One shared script timer and ordered mover array, independent of caster lifetime.
use super::*;
use crate::building_mechanics::WorldFreezerProfile;
use bevy_ecs::prelude::Component;
use serde::{Deserialize, Serialize};

#[cfg(test)]
mod tests;

// Motion retains fractional subunits. Authored facings are multiples of 45 degrees.
const MOTION_SCALE: i64 = 1_000_000_000;
const DIAGONAL: i64 = 707_106_781;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct PendingFieldCast {
    pub source: SimId,
    pub owner: PlayerId,
    pub team: Team,
    pub position: SimPoint,
    pub due_tick: u64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct FrostOrb {
    pub id: SimId,
    pub source: SimId,
    pub owner: PlayerId,
    pub team: Team,
    pub position_fractional: [i64; 2],
    pub facing_degrees: i16,
    pub counter: u16,
    pub sequence: u64,
    pub fire_due_time: u64,
    pub fire_sequence: u64,
    pub born_tick: u64,
}
impl FrostOrb {
    pub(super) fn position(&self) -> SimPoint {
        SimPoint::new(
            (self.position_fractional[0] / MOTION_SCALE) as i32,
            (self.position_fractional[1] / MOTION_SCALE) as i32,
        )
    }
}
#[derive(Component, Debug, Clone, Serialize, Deserialize)]
pub(super) struct WorldFreezerState {
    pub profile: WorldFreezerProfile,
    pub pending: Vec<PendingFieldCast>,
    pub orbs: Vec<FrostOrb>,
    pub timer_origin_tick: u64,
    pub timer_step: u64,
}

fn direction(facing: i16) -> (i64, i64) {
    match facing.rem_euclid(360) {
        0 => (MOTION_SCALE, 0),
        45 => (DIAGONAL, DIAGONAL),
        90 => (0, MOTION_SCALE),
        135 => (-DIAGONAL, DIAGONAL),
        180 => (-MOTION_SCALE, 0),
        225 => (-DIAGONAL, -DIAGONAL),
        270 => (0, -MOTION_SCALE),
        315 => (DIAGONAL, -DIAGONAL),
        _ => panic!("source mover facings must be multiples of 45 degrees"),
    }
}

impl Simulation {
    pub(super) fn start_world_freezer(
        &mut self,
        source: AbilitySourceSnapshot,
        profile: WorldFreezerProfile,
    ) {
        let position = match source.origin {
            AbilitySourceOrigin::Unit(p) => p,
            AbilitySourceOrigin::Building(f) => {
                footprint_center_point(f, self.config.navigation_cell_size)
            }
        };
        let owner = self
            .world
            .iter_entities()
            .find(|entity| entity.get::<SimId>() == Some(&source.id))
            .and_then(|entity| entity.get::<Owner>().map(|owner| owner.0))
            .unwrap_or(PlayerId(source.team.0));
        let cast = PendingFieldCast {
            source: source.id,
            owner,
            team: source.team,
            position,
            due_tick: self.next_tick + u64::from(profile.delay_ticks),
        };
        let entity = self
            .world
            .iter_entities()
            .find_map(|e| e.get::<WorldFreezerState>().map(|_| e.id()));
        if let Some(entity) = entity {
            let mut state = self.world.get_mut::<WorldFreezerState>(entity).unwrap();
            assert_eq!(
                state.profile, profile,
                "one registered mover profile per match"
            );
            state.pending.push(cast);
        } else {
            let id = self.allocate_id();
            self.world.spawn((
                id,
                WorldFreezerState {
                    profile,
                    pending: vec![cast],
                    orbs: Vec::new(),
                    timer_origin_tick: self.next_tick,
                    timer_step: 1,
                },
            ));
        }
    }

    pub(super) fn clear_world_freezer(&mut self) {
        let entities = self
            .world
            .iter_entities()
            .filter_map(|e| e.get::<WorldFreezerState>().map(|_| e.id()))
            .collect::<Vec<_>>();
        for entity in entities {
            self.world.despawn(entity);
        }
    }

    pub(super) fn resolve_world_freezer(
        &mut self,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        let Some(entity) = self
            .world
            .iter_entities()
            .find_map(|e| e.get::<WorldFreezerState>().map(|_| e.id()))
        else {
            return;
        };
        // Temporarily own the cold mover state so callbacks can launch independent native actions.
        let mut state = self
            .world
            .entity_mut(entity)
            .take::<WorldFreezerState>()
            .unwrap();
        let p = state.profile;
        let mut pending_index = 0;
        while pending_index < state.pending.len() {
            if state.pending[pending_index].due_tick > self.next_tick {
                pending_index += 1;
                continue;
            }
            let cast = state.pending.remove(pending_index);
            if state.orbs.is_empty() {
                state.timer_origin_tick = self.next_tick;
                state.timer_step = 1;
            }
            for angle in p.angle_offsets {
                let id = self.allocate_id();
                state.orbs.push(FrostOrb {
                    id,
                    source: cast.source,
                    owner: cast.owner,
                    team: cast.team,
                    position_fractional: [
                        i64::from(cast.position.x) * MOTION_SCALE,
                        i64::from(cast.position.y) * MOTION_SCALE,
                    ],
                    facing_degrees: (i16::from(cast.team.0) * 180 + angle).rem_euclid(360),
                    counter: 0,
                    sequence: 0,
                    fire_due_time: self.next_tick * 1000,
                    fire_sequence: 0,
                    born_tick: self.next_tick,
                });
            }
        }
        while !state.orbs.is_empty()
            && state.timer_origin_tick
                + (state.timer_step
                    * u64::from(p.interval_millis)
                    * CASTLE_FIGHT_SIMULATION_HZ as u64)
                    .div_ceil(1000)
                <= self.next_tick
        {
            let mut i = 0;
            while i < state.orbs.len() {
                let orb = &mut state.orbs[i];
                let (dx, dy) = direction(orb.facing_degrees);
                let x = orb.position_fractional[0] + i64::from(p.step) * dx;
                if x <= i64::from(p.horizontal_bounds[0]) * MOTION_SCALE
                    || x >= i64::from(p.horizontal_bounds[1]) * MOTION_SCALE
                {
                    state.orbs.swap_remove(i); // The source processes the replacement immediately.
                    continue;
                }
                let mut y = orb.position_fractional[1] + i64::from(p.step) * dy;
                if y <= i64::from(p.vertical_bounds[0]) * MOTION_SCALE
                    || y >= i64::from(p.vertical_bounds[1]) * MOTION_SCALE
                {
                    orb.facing_degrees = (360 - orb.facing_degrees).rem_euclid(360);
                    y = orb.position_fractional[1]
                        + i64::from(p.bounce_step) * direction(orb.facing_degrees).1;
                }
                orb.position_fractional = [x, y];
                orb.counter += 1;
                if orb.counter > p.counter_limit {
                    orb.counter = 0;
                    orb.sequence += 1;
                    self.resolve_field_target(orb, p, units);
                }
                i += 1;
            }
            state.timer_step += 1;
        }
        for orb in &mut state.orbs {
            let position = orb.position();
            for u in units.iter_mut() {
                if u.health > 0
                    && u.team != orb.team
                    && !u.classifications.invulnerable
                    && p.aura_targets.can_target_unit(u.movement_class)
                    && position.distance_sq(u.position) <= square_i32(p.aura_radius)
                {
                    // Identical aura identity refreshes rather than stacking for overlapping orbs.
                    apply_timed_movement_modifier(
                        &mut u.status,
                        ModifierId(p.aura.0),
                        p.movement_percent_delta,
                        self.next_tick + 2,
                    );
                    apply_timed_attack_speed_modifier(
                        &mut u.status,
                        ModifierId(p.aura.0),
                        p.attack_speed_percent_delta,
                        self.next_tick + 2,
                    );
                }
            }
            self.resolve_field_fire(orb, p, units, buildings);
        }
        if state.orbs.is_empty() && state.pending.is_empty() {
            self.world.despawn(entity);
        } else {
            self.world.entity_mut(entity).insert(state);
        }
    }

    fn resolve_field_target(
        &mut self,
        orb: &FrostOrb,
        p: WorldFreezerProfile,
        units: &mut [UnitSnapshot],
    ) {
        let position = orb.position();
        let target = units
            .iter()
            .enumerate()
            .filter(|(_, u)| {
                u.team != orb.team
                    && self.hex_trigger_eligible(u, p.map_version)
                    && u.health > 0
                    && position.distance_sq(u.position) <= square_i32(p.target_radius)
            })
            .min_by_key(|(_, u)| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        orb.id,
                        p.parent,
                        orb.sequence,
                        u.id,
                    ),
                    u.id,
                )
            })
            .map(|(i, _)| i);
        let Some(i) = target else { return };
        let target = &mut units[i];
        if self.visible_teams(target.position) & (1 << orb.team.0) == 0 {
            self.reveal_area(
                orb.team,
                target.position,
                p.vision_radius,
                u64::from(p.vision_ticks),
                false,
            );
        }
        // The helper spawns at the selected target, independently of the orb's distance.
        // There is deliberately no checkForShield invocation in this source callback.
        if target.classifications.spell_immune
            || target.classifications.invulnerable
            || (target.classifications.invisible && !target.visible_to(orb.team, self.next_tick))
        {
            return;
        }
        let bolt = if target.movement_class == MovementClass::Air {
            p.air
        } else {
            p.ground
        };
        if bolt.targets.can_target_unit(target.movement_class) {
            self.launch_native_bolt(
                orb.id,
                orb.team,
                target.position,
                target.id,
                target.position,
                bolt,
            );
        }
        if target.movement_class == MovementClass::Ground
            && (!p.roots.nonhero_only || !target.classifications.hero)
            && p.roots.targets.can_target_unit(target.movement_class)
        {
            apply_roots_status(
                &mut target.status,
                p.roots,
                self.next_tick,
                target.classifications.hero,
                true,
            );
            self.last_ability_casts.push(AbilityCastEvent {
                source: orb.id,
                ability: p.roots.ability,
                target: AbilityCastTarget::Unit(target.id),
                target_position: Some(target.position),
                effect: AbilityEffect::Damage { amount: 0 },
            });
        }
    }

    fn resolve_field_fire(
        &mut self,
        orb: &mut FrostOrb,
        p: WorldFreezerProfile,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) {
        let time = self.next_tick * 1000;
        if time < orb.fire_due_time {
            return;
        }
        let position = orb.position();
        let fire = p.fire;
        let unit_target = units
            .iter()
            .filter(|u| {
                u.health > 0
                    && u.team != orb.team
                    && !u.classifications.invulnerable
                    && !u.classifications.spell_immune
                    && !field_fire_buff_active(&u.status, p, self.next_tick)
                    && u.visible_to(orb.team, self.next_tick)
                    && fire.targets.can_target_unit(u.movement_class)
                    && position.distance_sq(u.position) <= square_i32(p.fire_radius)
            })
            .map(|u| (u.id, u.position));
        let building_target = buildings
            .iter()
            .filter(|b| {
                b.health > 0
                    && b.team != orb.team
                    && !b.classifications.invulnerable
                    && !b.classifications.spell_immune
                    && !b
                        .status
                        .as_ref()
                        .is_some_and(|status| field_fire_buff_active(status, p, self.next_tick))
                    && b.visible_teams & (1 << orb.team.0) != 0
                    && fire.targets.can_target_buildings()
                    && point_to_footprint_distance_sq(
                        position,
                        b.footprint,
                        self.config.navigation_cell_size,
                    ) <= square_i32(p.fire_radius)
            })
            .map(|b| {
                (
                    b.id,
                    footprint_center_point(b.footprint, self.config.navigation_cell_size),
                )
            });
        let target = unit_target.chain(building_target).min_by_key(|(id, _)| {
            (
                deterministic_ability_target_rank(
                    self.config.match_seed,
                    orb.id,
                    fire.ability,
                    orb.fire_sequence,
                    *id,
                ),
                *id,
            )
        });
        let Some((target, destination)) = target else {
            return;
        };
        self.launch_native_bolt(orb.id, orb.team, position, target, destination, fire);
        orb.fire_sequence += 1;
        // Preserve fractional native cadence while continuously firing; reset after idle.
        if time.saturating_sub(orb.fire_due_time) >= 1000 {
            orb.fire_due_time = time;
        }
        orb.fire_due_time += u64::from(p.fire_cooldown_millis) * CASTLE_FIGHT_SIMULATION_HZ as u64;
    }
}

// Phoenix Fire avoids a live copy of its native buff. Source Storm Bolts can use
// that same buff identity, independently of their longer stun deadline.
fn field_fire_buff_active(status: &StatusState, p: WorldFreezerProfile, tick: u64) -> bool {
    abilities::native_fire_buff_active(status, p.fire.ability, tick)
        || (tick < status.native_stun_until_tick
            && status.native_stun_ability.is_some_and(|ability| {
                (ability == p.air.ability && p.air_buff == p.fire_buff)
                    || (ability == p.ground.ability && p.ground_buff == p.fire_buff)
            }))
}
