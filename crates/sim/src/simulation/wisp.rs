//! Ancient of Wonders: released forest wisps and the death blast they leave behind.
use super::*;
use crate::{
    CastleFightUnitKind,
    components::{
        DeathBurstState, DeathSlowField, NativeAction, WispReleaseProfile, WispReleaseState,
    },
};

impl Simulation {
    /// `A0BS` cast closure: six wisps follow, one per period, for the casting player.
    pub(super) fn cast_wisp_release(
        &mut self,
        source: AbilitySourceSnapshot,
        profile: WispReleaseProfile,
        units: &[UnitSnapshot],
    ) -> usize {
        let Some(owner) = self
            .world
            .get::<Owner>(self.source_entity(source, units))
            .map(|owner| owner.0)
        else {
            return 0;
        };
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::WispRelease(WispReleaseState {
                owner,
                team: source.team,
                profile,
                iteration: 0,
                // doPeriodically fires its first callback one period after the cast.
                next_tick: self.next_tick + u64::from(profile.period_ticks),
            }),
        ));
        1
    }

    pub(super) fn advance_wisp_release(&mut self, entity: Entity, mut state: WispReleaseState) {
        if self.next_tick < state.next_tick {
            return;
        }
        let profile = state.profile;
        // The pool of the owner's team side, alternating between its two corners.
        let group = 2 * u32::from(state.team.0) + u32::from(state.iteration % 2);
        let [_, _, max_x, max_y] =
            crate::building_mechanics::snowveil_for_version(profile.map_version).battlefield_bounds;
        let side = |negative: bool, magnitude: i32| if negative { -magnitude } else { magnitude };
        let position = SimPoint {
            x: side(group < 2, profile.x_limit.min(max_x)),
            y: side(group % 2 == 0, max_y - profile.y_inset),
        };
        let mut definition = CastleFightUnitKind::from_rawcode_for_version(
            profile.wisp_rawcode,
            profile.map_version,
        )
        .unwrap()
        .expect("promoted wisp")
        .definition_for_version(profile.map_version)
        .unwrap()
        .resolved();
        // The release adds the combat-body type to the pooled neutral wisp.
        definition.properties.classifications.combat_sapper = true;
        let child = self.spawn_resolved_unit_unchecked(
            Some(state.owner),
            UnitSpawn::from_template(state.team, position, definition.template),
            definition,
        );
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::TimedLife {
                child,
                expires_tick: self.next_tick + u64::from(profile.life_ticks),
            },
        ));
        state.iteration += 1;
        state.next_tick += u64::from(profile.period_ticks);
        if state.iteration >= profile.releases {
            self.world.despawn(entity);
        } else if let Some(mut action) = self.world.get_mut::<NativeAction>(entity) {
            *action = NativeAction::WispRelease(state);
        }
    }

    /// `fJ`: a dying combat body whose rule carries a death blast leaves one at its position.
    pub(super) fn observe_death_bursts(
        &mut self,
        units: &[UnitSnapshot],
        health: &[i32],
        positions: &[SimPoint],
    ) {
        for (i, unit) in units.iter().enumerate() {
            if health[i] > 0 || unit.removed_without_death || !unit.classifications.combat_sapper {
                continue;
            }
            let (Some(rule), Some(map_version)) = (
                unit.death_rule.and_then(|rule| rule.death_burst),
                unit.map_version,
            ) else {
                continue;
            };
            let id = self.allocate_id();
            self.world.spawn((
                id,
                NativeAction::DeathBurst(DeathBurstState {
                    map_version,
                    source: unit.id,
                    team: unit.team,
                    center: positions[i],
                    rule,
                    struck: false,
                    expires_tick: self.next_tick + u64::from(rule.slow.duration_ticks),
                }),
            ));
        }
    }

    pub(super) fn advance_death_burst(
        &mut self,
        entity: Entity,
        mut state: DeathBurstState,
        units: &mut [UnitSnapshot],
    ) {
        if !state.struck {
            state.struck = true;
            let rule = state.rule;
            let radius_sq = square_i32(rule.radius);
            for index in 0..units.len() {
                let unit = &units[index];
                if unit.team == state.team
                    || unit.health <= 0
                    || !unit.classifications.combat_sapper
                    || unit.classifications.locust
                    || unit.classifications.invulnerable
                    || unit.classifications.spell_immune
                    || unit.rawcode == rule.excluded_type
                    || crate::content::unit_has_ability_for_version(
                        unit.rawcode,
                        rule.excluded_ability,
                        state.map_version,
                    )
                    || state.center.distance_sq(unit.position) > radius_sq
                {
                    continue;
                }
                self.apply_source_ability_to_unit(
                    state.source,
                    state.team,
                    index,
                    AbilityEffect::Damage {
                        amount: rule.damage,
                    },
                    units,
                );
            }
        }
        if self.next_tick >= state.expires_tick {
            self.world.despawn(entity);
            return;
        }
        self.apply_slow_field(state.center, state.team, state.rule.slow, units);
        if let Some(mut action) = self.world.get_mut::<NativeAction>(entity) {
            *action = NativeAction::DeathBurst(state);
        }
    }

    /// `Aasl`: hostile combat bodies inside the radius are slowed for as long as they stay in it.
    pub(super) fn apply_slow_field(
        &self,
        center: SimPoint,
        team: Team,
        slow: DeathSlowField,
        units: &mut [UnitSnapshot],
    ) {
        apply_slow_aura(
            self.next_tick,
            center,
            team,
            (
                slow.ability,
                slow.buff,
                slow.radius,
                slow.movement_percent_delta,
                slow.attack_speed_percent_delta,
                slow.targets,
                slow.affects_invulnerable,
            ),
            units,
        );
    }
}

/// Shared hostile slow aura: refreshed every tick, so it ends as soon as a body leaves.
pub(super) fn apply_slow_aura(
    tick: u64,
    center: SimPoint,
    team: Team,
    (ability, buff, radius, movement, attack_speed, targets, affects_invulnerable): (
        AbilityId,
        u32,
        i32,
        i16,
        i16,
        AttackTargetMask,
        bool,
    ),
    units: &mut [UnitSnapshot],
) {
    let expiry = tick + 1;
    let id = ModifierId(ability.0);
    for unit in units.iter_mut() {
        if unit.team == team
            || unit.health <= 0
            || unit.classifications.locust
            || !unit.classifications.combat_sapper
            || (unit.classifications.invulnerable && !affects_invulnerable)
            || !targets.can_target_unit(unit.target_movement_class())
            || center.distance_sq(unit.position) > square_i32(radius)
        {
            continue;
        }
        apply_timed_armor_modifier(
            &mut unit.status,
            TimedArmorModifier {
                undispellable: false,
                aura_membership: true,
                id,
                native_buff: Some(crate::NativeBuffIdentity {
                    rawcode: buff,
                    positive: false,
                    stealable: false,
                    organic_only: false,
                }),
                expires_tick: expiry,
                ..Default::default()
            },
        );
        apply_timed_movement_modifier(&mut unit.status, id, movement, expiry);
        apply_timed_attack_speed_modifier(&mut unit.status, id, attack_speed, expiry);
    }
}
