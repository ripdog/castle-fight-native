use super::*;
use crate::building_mechanics::{SilverGladeProfile, SilverGladeState};
use crate::components::NativeAction;
impl Simulation {
    pub(super) fn add_silver_charge(&mut self, team: Team, profile: SilverGladeProfile) {
        assert!(team.0 < 2);
        let entity = self
            .world
            .iter_entities()
            .find(|e| matches!(e.get::<NativeAction>(), Some(NativeAction::SilverGlade(_))))
            .map(|e| e.id());
        if let Some(entity) = entity {
            let mut action = self.world.get_mut::<NativeAction>(entity).unwrap();
            let NativeAction::SilverGlade(state) = &mut *action else {
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
                NativeAction::SilverGlade(SilverGladeState { profile, charges }),
            ));
        }
    }
    pub(super) fn reset_silver_charges(&mut self) {
        for mut action in self
            .world
            .query::<&mut NativeAction>()
            .iter_mut(&mut self.world)
        {
            if let NativeAction::SilverGlade(s) = &mut *action {
                s.charges = [0; 2];
            }
        }
    }
    pub(super) fn silver_attack_attempt(
        &mut self,
        index: usize,
        units: &mut [UnitSnapshot],
        health: &mut [i32],
        tick: u64,
    ) {
        let Some((entity, mut state)) = self.world.iter_entities().find_map(|e| match e
            .get::<NativeAction>()
        {
            Some(NativeAction::SilverGlade(s)) => Some((e.id(), s.clone())),
            _ => None,
        }) else {
            return;
        };
        let target = &units[index];
        let team = target.team;
        if !target.classifications.combat_sapper
            || team.0 >= 2
            || state.charges[usize::from(team.0)] == 0
            || self.native_ability_installed(target, state.profile.excluded)
        {
            return;
        }
        let center = target.position;
        let source = target.id;
        let profile = state.profile;
        state.charges[usize::from(team.0)] -= 1;
        self.world
            .entity_mut(entity)
            .insert(NativeAction::SilverGlade(state));
        let effect = AbilityEffect::StatBuff {
            modifier: ModifierId(profile.effect.0),
            buff: profile.buff,
            armor_bonus_per_100: profile.armor_per_100,
            damage_bonus_per_10k: 0,
            regeneration_per_second_per_10k: 0,
            duration_ticks: profile.duration_ticks,
            hero_duration_ticks: profile.hero_duration_ticks,
            autocast_range: 0,
        };
        for (i, u) in units.iter_mut().enumerate() {
            if health[i] <= 0
                || u.team != team
                || u.classifications.invulnerable
                || center.distance_sq(u.position) > square_i32(profile.radius)
            {
                continue;
            }
            u.health = health[i].saturating_add(profile.health).min(u.health_max);
            health[i] = u.health;
            if let (Some(current), Some(maximum)) =
                (&mut u.mana_current, u.spellcasting.map(|s| s.mana.maximum))
            {
                *current = current.saturating_add(profile.mana).min(maximum);
            }
            apply_ability_effect_to_unit(u, effect, team, tick, self.combat_rules.damage_rules);
        }
        self.last_ability_casts.push(AbilityCastEvent {
            source,
            ability: profile.effect,
            target: AbilityCastTarget::Point(center),
            target_position: Some(center),
            effect,
        });
    }
    fn native_ability_installed(&self, u: &UnitSnapshot, ability: AbilityId) -> bool {
        u.status.native_grants.contains(ability)
            || self
                .world
                .get::<ContentIdentity>(u.entity)
                .is_some_and(|c| {
                    crate::content::unit_has_ability_for_version(c.rawcode, ability, c.map_version)
                })
    }
}
