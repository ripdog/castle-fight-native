//! Independent dummy casts: fixed heading, wave arrival, suppression, then native landing.
use super::status::ability_damage_for_unit;
use super::*;
use crate::components::{
    ImpaleFanProfile, ImpaleLandingState, ImpaleProfile, ImpaleStatus, ImpaleWaveState,
    NativeAction,
};

pub(super) fn primary_eligible(u: &UnitSnapshot, team: Team, tick: u64) -> bool {
    u.health > 0
        && u.team != team
        && u.classifications.combat_sapper
        && u.target_movement_class() == MovementClass::Ground
        && !u.classifications.uses_hero_spell_rules()
        && !u.classifications.invulnerable
        && !u.classifications.spell_immune
        && !u.classifications.locust
        && u.visible_to(team, tick)
}

fn line_distance(state: &ImpaleWaveState, point: SimPoint, to: i32) -> Option<i64> {
    let dx = i128::from(state.destination.x) - i128::from(state.origin.x);
    let dy = i128::from(state.destination.y) - i128::from(state.origin.y);
    let px = i128::from(point.x) - i128::from(state.origin.x);
    let py = i128::from(point.y) - i128::from(state.origin.y);
    let length = ((dx * dx + dy * dy) as u128).isqrt() as i128;
    if length == 0 {
        return None;
    }
    let dot = dx * px + dy * py;
    let cross = dx * py - dy * px;
    (dot >= i128::from(state.traveled) * length
        && dot <= i128::from(to) * length
        && cross * cross <= i128::from(state.profile.radius).pow(2) * length * length)
        .then_some((dot / length) as i64)
}

impl Simulation {
    pub(super) fn cast_impale_fan(
        &mut self,
        source: AbilitySourceSnapshot,
        target: usize,
        p: ImpaleFanProfile,
        units: &mut [UnitSnapshot],
    ) -> usize {
        let caster =
            find_unit_index(units, source.id).expect("Impale parent requires a unit caster");
        let origin = units[caster].position;
        let owner = units[caster].owner;
        let heading = SimPoint::new(
            units[target].position.x - origin.x,
            units[target].position.y - origin.y,
        );
        let heading = if heading == SimPoint::default() {
            SimPoint::new(1_000_000, 0)
        } else {
            heading
        };
        self.launch_native_parasite(source, owner, units[target].id, p.parent);
        for angle in p.relative_angles_millidegrees {
            let direction = super::master_projectiles::rotate(heading, angle);
            let point = SimPoint::new(
                origin.x
                    + (i64::from(direction.x) * i64::from(p.cast_point_distance) / 1_000_000)
                        as i32,
                origin.y
                    + (i64::from(direction.y) * i64::from(p.cast_point_distance) / 1_000_000)
                        as i32,
            );
            let actor = self.allocate_id();
            self.world.spawn((
                actor,
                NativeAction::NativeSpellActor {
                    owner,
                    team: source.team,
                    map_version: p.parent.map_version,
                    rawcode: p.dummy_rawcode,
                    flying: p.dummy_flying,
                    position: origin,
                    expires_tick: self.next_tick + u64::from(p.dummy_lifetime_ticks),
                },
            ));
            self.launch_impale_wave(actor, source.team, origin, point, p.child);
        }
        p.relative_angles_millidegrees.len()
    }

    pub(super) fn launch_impale_wave(
        &mut self,
        source: SimId,
        team: Team,
        origin: SimPoint,
        target: SimPoint,
        profile: ImpaleProfile,
    ) {
        let dx = i64::from(target.x) - i64::from(origin.x);
        let dy = i64::from(target.y) - i64::from(origin.y);
        let length = ((i128::from(dx) * i128::from(dx) + i128::from(dy) * i128::from(dy)) as u128)
            .isqrt()
            .max(1) as i64;
        let destination = SimPoint::new(
            origin.x + (dx * i64::from(profile.distance) / length) as i32,
            origin.y + (dy * i64::from(profile.distance) / length) as i32,
        );
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::ImpaleWave(ImpaleWaveState {
                source,
                team,
                profile,
                origin,
                destination,
                launch_tick: self.next_tick,
                traveled: 0,
                hit_targets: Vec::new(),
            }),
        ));
    }

    pub(super) fn advance_impale_wave(
        &mut self,
        entity: Entity,
        mut state: ImpaleWaveState,
        units: &mut [UnitSnapshot],
    ) {
        let elapsed = self
            .next_tick
            .saturating_sub(state.launch_tick)
            .min(u64::from(state.profile.wave_ticks));
        let to = (i64::from(state.profile.distance) * elapsed as i64
            / i64::from(state.profile.wave_ticks)) as i32;
        let mut hits = units
            .iter()
            .enumerate()
            .filter_map(|(i, u)| {
                (u.health > 0
                    && u.team != state.team
                    && !u.classifications.locust
                    && !u.classifications.invulnerable
                    && !u.classifications.spell_immune
                    && state
                        .profile
                        .targets
                        .can_target_unit(u.target_movement_class())
                    && !u
                        .status
                        .impale
                        .is_some_and(|p| self.next_tick < p.expires_tick)
                    && !u
                        .status
                        .ensnare
                        .is_some_and(|p| self.next_tick < p.expires_tick)
                    && self.next_tick >= u.status.rooted_until_tick
                    && !state.hit_targets.contains(&u.id))
                .then(|| line_distance(&state, u.position, to).map(|distance| (distance, u.id, i)))
                .flatten()
            })
            .collect::<Vec<_>>();
        hits.sort_unstable_by_key(|(distance, id, _)| (*distance, *id));
        for (_, id, i) in hits {
            let landed_tick = self.next_tick + u64::from(state.profile.air_ticks);
            let duration = if units[i].classifications.uses_hero_spell_rules() {
                state.profile.hero_stun_ticks
            } else {
                state.profile.stun_ticks
            };
            let expires_tick = landed_tick + u64::from(duration);
            units[i].status.impale = Some(ImpaleStatus {
                ability: state.profile.ability,
                landed_tick,
                expires_tick,
            });
            units[i].status.stunned_until_tick =
                units[i].status.stunned_until_tick.max(expires_tick);
            units[i].status.pending_attack = None;
            units[i].status.pending_cast = None;
            units[i].status.action_animation = None;
            units[i].classifications.invulnerable |= self.next_tick < landed_tick;
            self.last_ability_casts.push(AbilityCastEvent {
                source: state.source,
                ability: state.profile.ability,
                target: AbilityCastTarget::Unit(id),
                target_position: Some(units[i].position),
                effect: AbilityEffect::Stun {
                    duration_ticks: duration,
                },
            });
            let landing = ImpaleLandingState {
                source: state.source,
                target: id,
                profile: state.profile,
                due_tick: landed_tick,
            };
            let action_id = self.allocate_id();
            let action = self
                .world
                .spawn((action_id, NativeAction::ImpaleLanding(landing)))
                .id();
            if landed_tick == self.next_tick {
                self.advance_impale_landing(action, landing, units);
            }
            state.hit_targets.push(id);
        }
        state.traveled = to;
        if elapsed >= u64::from(state.profile.wave_ticks) {
            self.world.despawn(entity);
        } else {
            self.world
                .entity_mut(entity)
                .insert(NativeAction::ImpaleWave(state));
        }
    }

    pub(super) fn advance_impale_landing(
        &mut self,
        entity: Entity,
        state: ImpaleLandingState,
        units: &mut [UnitSnapshot],
    ) {
        if self.next_tick < state.due_tick {
            return;
        }
        self.world.despawn(entity);
        let Some(i) = find_unit_index(units, state.target).filter(|&i| {
            units[i].health > 0
                && units[i].status.impale.is_some_and(|p| {
                    p.ability == state.profile.ability && p.landed_tick == state.due_tick
                })
        }) else {
            return;
        };
        units[i].status.native_stun_ability = Some(state.profile.ability);
        units[i].status.native_stun_until_tick = units[i]
            .status
            .native_stun_until_tick
            .max(units[i].status.impale.unwrap().expires_tick);
        if units[i].classifications.invulnerable || units[i].classifications.spell_immune {
            return;
        }
        let damage = ability_damage_for_unit(
            &units[i],
            AbilityEffect::Damage {
                amount: state.profile.damage,
            },
            self.next_tick,
            self.combat_rules.damage_rules,
        )
        .expect("native Impale damage");
        let health = units[i].health;
        damage_thresholds::observe_damage_event(&mut units[i], health, damage);
        death_healing::commit_health_from(
            units,
            i,
            health.saturating_sub(damage),
            Some(state.source),
        );
    }
}
