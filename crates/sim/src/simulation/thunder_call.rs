//! Call of Thunder: stored team charges spent by the shared damage listener on an allied melee hit.
use super::*;
use crate::components::{NativeAction, ThunderCallProfile, ThunderCallState};

impl Simulation {
    pub(super) fn add_thunder_charge(&mut self, team: Team, profile: ThunderCallProfile) {
        assert!(team.0 < 2);
        let entity = self
            .world
            .iter_entities()
            .find(|e| matches!(e.get::<NativeAction>(), Some(NativeAction::ThunderCall(_))))
            .map(|e| e.id());
        if let Some(entity) = entity {
            let mut action = self.world.get_mut::<NativeAction>(entity).unwrap();
            let NativeAction::ThunderCall(state) = &mut *action else {
                unreachable!()
            };
            assert_eq!(state.profile, profile);
            state.charges[usize::from(team.0)] = state.charges[usize::from(team.0)]
                .checked_add(1)
                .expect("team charge overflow");
        } else {
            let id = self.allocate_id();
            let mut charges = [0; 2];
            charges[usize::from(team.0)] = 1;
            self.world.spawn((
                id,
                NativeAction::ThunderCall(ThunderCallState {
                    profile,
                    charges,
                    guard: None,
                }),
            ));
        }
    }

    /// The round-end signal clears every team's stored charges.
    pub(super) fn reset_thunder_charges(&mut self) {
        for mut action in self
            .world
            .query::<&mut NativeAction>()
            .iter_mut(&mut self.world)
        {
            if let NativeAction::ThunderCall(state) = &mut *action {
                state.charges = [0; 2];
                state.guard = None;
            }
        }
    }

    /// Evaluated for every landed weapon damage event, after armor and before health is committed.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn observe_thunder_call(
        &mut self,
        source_id: SimId,
        center: SimPoint,
        amount: i32,
        tick: u64,
        units: &mut [UnitSnapshot],
        health: &mut [i32],
        positions: &[SimPoint],
    ) {
        let Some((entity, mut state)) = self.world.iter_entities().find_map(|e| match e
            .get::<NativeAction>()
        {
            Some(NativeAction::ThunderCall(state)) => Some((e.id(), state.clone())),
            _ => None,
        }) else {
            return;
        };
        let profile = state.profile;
        let Some(source_index) = find_unit_index(units, source_id) else {
            return;
        };
        let source = &units[source_index];
        let team = usize::from(source.team.0);
        let secondary_range = source.secondary_attack.map_or(0, |attack| attack.attack.range);
        if team >= 2
            || state.charges[team] == 0
            || state.guard.is_some_and(|(guard, at)| guard == source_id && at >= tick)
            || amount <= profile.minimum_event_damage_exclusive
            || source.classifications.invulnerable
            || source.attack.range <= profile.weapon_range_lower_exclusive
            || source.attack.range >= profile.weapon_range_upper_exclusive
            || secondary_range >= profile.weapon_range_upper_exclusive
        {
            return;
        }
        state.charges[team] -= 1;
        state.guard = Some((source_id, tick));
        let special = self.native_ability_installed(source, profile.special_source_ability);
        let owner_team = source.team;
        self.world
            .entity_mut(entity)
            .insert(NativeAction::ThunderCall(state));
        let strike = i64::from(amount)
            .saturating_mul(i64::from(profile.strike_multiplier))
            .clamp(
                i64::from(profile.strike_minimum),
                i64::from(profile.strike_maximum),
            );
        let strike = if special {
            strike * i64::from(profile.special_source_per_10k) / 10_000
        } else {
            strike
        } as i32;
        let radius_sq = square_i32(profile.radius);
        for i in 0..units.len() {
            let unit = &units[i];
            if health[i] <= 0
                || unit.team == owner_team
                || !unit.classifications.combat_sapper
                || unit.classifications.invulnerable
                || center.distance_sq(positions[i]) > radius_sq
            {
                continue;
            }
            let raw = if unit.movement_class == MovementClass::Air {
                (i64::from(strike) * i64::from(profile.flying_per_10k) / 10_000) as i32
            } else {
                strike
            };
            let damage = self
                .combat_rules
                .damage_rules
                .apply_spell(raw, unit.armor.armor_type);
            let damage = spell_damage_after_defend(*unit, damage, tick);
            source_damage_runtime::dispatch(
                source_id,
                i,
                damage,
                source_damage_runtime::SourceDamageState {
                    units,
                    health,
                    rules: self.combat_rules.damage_rules,
                    tick,
                },
                &self.retired_damage_sources,
            );
            damage_thresholds::observe_damage_event(&mut units[i], health[i], damage);
            let after = health[i].saturating_sub(damage);
            death_healing::commit_split_health_from(
                units,
                health,
                Some(positions),
                i,
                after,
                Some(source_id),
            );
        }
        self.last_ability_casts.push(AbilityCastEvent {
            source: source_id,
            ability: profile.ability,
            target: AbilityCastTarget::Point(center),
            target_position: Some(center),
            effect: AbilityEffect::AreaDamage {
                amount: strike,
                radius: profile.radius,
                origin: AreaDamageOrigin::Target,
            },
        });
    }
}
