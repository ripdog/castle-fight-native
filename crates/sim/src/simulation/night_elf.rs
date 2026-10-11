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
            let maximum = u.spellcasting.map(|s| s.mana.maximum).or_else(|| {
                u.map_version.and_then(|v| {
                    crate::content::native_mana_profile_for_version(v, u.rawcode).map(|p| p.maximum)
                })
            });
            if let (Some(current), Some(maximum)) = (&mut u.mana_current, maximum) {
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
    pub(super) fn native_ability_installed(&self, u: &UnitSnapshot, ability: AbilityId) -> bool {
        u.status.native_grants.contains(ability)
            || self
                .world
                .get::<ContentIdentity>(u.entity)
                .is_some_and(|c| {
                    crate::content::unit_has_ability_for_version(c.rawcode, ability, c.map_version)
                })
    }
}

impl Simulation {
    pub(super) fn cast_wilderness(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AbilityId,
        p: crate::building_mechanics::WildernessProfile,
        sequence: u64,
        units: &mut [UnitSnapshot],
    ) -> bool {
        let bounds =
            crate::building_mechanics::snowveil_for_version(p.map_version).battlefield_bounds;
        let index = units
            .iter()
            .enumerate()
            .filter(|(_, u)| {
                u.health > 0
                    && u.team == source.team
                    && u.classifications.combat_sapper
                    && !self.native_ability_installed(u, p.locust)
                    && !self.native_ability_installed(u, p.grants[0].ability)
                    && !self.native_ability_installed(u, p.grants[1].ability)
                    && u.position.x >= bounds[0]
                    && u.position.x <= bounds[2]
                    && u.position.y >= bounds[1]
                    && u.position.y <= bounds[3]
            })
            .min_by_key(|(_, u)| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        ability,
                        sequence,
                        u.id,
                    ),
                    u.id,
                )
            })
            .map(|(i, _)| i);
        let Some(index) = index else {
            // Caster art precedes the source's empty-group return.
            self.last_ability_casts.push(AbilityCastEvent {
                source: source.id,
                ability,
                target: AbilityCastTarget::AllEnemyUnits,
                target_position: None,
                effect: AbilityEffect::Wilderness(p),
            });
            return false;
        };
        let u = &mut units[index];
        if !p
            .exclusions
            .into_iter()
            .any(|a| self.native_ability_installed(u, a))
        {
            u.status.native_grants.insert(p.grants[0]);
        }
        for grant in p.grants.into_iter().skip(1) {
            u.status.native_grants.insert(grant);
        }
        self.last_ability_casts.push(AbilityCastEvent {
            source: source.id,
            ability,
            target: AbilityCastTarget::Unit(u.id),
            target_position: Some(u.position),
            effect: AbilityEffect::Wilderness(p),
        });
        true
    }
}

/// Native flat attack reduction follows armor, before scripted incoming listeners.
/// Every landed attack event has a persistent independent target sequence, even within one tick.
pub(super) fn hardened_skin_after_armor(
    unit: &mut UnitSnapshot,
    damage: i32,
    source: SimId,
    tick: u64,
) -> i32 {
    if unit.abilities_disabled || damage <= 0 {
        return damage;
    }
    let grants =
        unit.status
            .native_grants
            .iter()
            .rev()
            .chain(unit.passive_effects.iter().rev().filter_map(|effect| {
                if let PassiveUnitEffect::HardenedSkin(grant) = effect {
                    Some(grant)
                } else {
                    None
                }
            }));
    let mut sequence = None;
    for grant in grants {
        let crate::NativeAbilityGrantEffect::HardenedSkin {
            chance_per_10k,
            ignored_damage,
            minimum_damage,
        } = grant.effect
        else {
            continue;
        };
        let sequence = *sequence.get_or_insert_with(|| {
            let sequence = unit.status.hardened_skin_sequence;
            unit.status.hardened_skin_sequence = sequence
                .checked_add(1)
                .expect("native incoming attack sequence overflow");
            sequence
        });
        let rolled = deterministic_random(
            unit.damage_seed,
            tick,
            unit.id,
            0x4841_5244_534b_494e ^ u64::from(grant.ability.0) ^ source.0,
            sequence,
        ) % 10000;
        if rolled < u64::from(chance_per_10k) {
            return damage
                .saturating_sub(ignored_damage)
                .max(minimum_damage)
                .min(damage);
        }
    }
    damage
}
