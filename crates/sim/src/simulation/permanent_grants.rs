//! Script-installed native abilities persist by rawcode identity, independently of buffs.
use super::*;
use crate::{NativeAbilityGrantEffect, building_mechanics::BuildingPermanentGrantProfile};

const RANDOM_PURPOSE_GRANT_OPTION: u64 = 0x4752_414e_544f_5054;

impl Simulation {
    pub(super) fn cast_building_permanent_grant(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AbilityId,
        profile: BuildingPermanentGrantProfile,
        sequence: u64,
        units: &mut [UnitSnapshot],
    ) -> bool {
        // The source rolls before either picker, including casts with no eligible ally.
        let option = (deterministic_random(
            self.config.match_seed,
            self.next_tick,
            source.id,
            RANDOM_PURPOSE_GRANT_OPTION,
            sequence,
        ) % profile.options.len() as u64) as usize;
        let bounds =
            crate::building_mechanics::snowveil_for_version(profile.map_version).battlefield_bounds;
        let installed = |unit: &UnitSnapshot, id| {
            unit.status.native_grants.contains(id)
                || self
                    .world
                    .get::<ContentIdentity>(unit.entity)
                    .is_some_and(|content| {
                        crate::content::unit_has_ability_for_version(
                            content.rawcode,
                            id,
                            content.map_version,
                        )
                    })
        };
        let pick = |allow_marker| {
            units
                .iter()
                .enumerate()
                .filter(|(_, unit)| {
                    unit.health > 0
                        && unit.team == source.team
                        && unit.classifications.combat_sapper
                        && !unit.classifications.invulnerable
                        && (allow_marker || !installed(unit, profile.retry_marker))
                        && unit.position.x >= bounds[0]
                        && unit.position.x <= bounds[2]
                        && unit.position.y >= bounds[1]
                        && unit.position.y <= bounds[3]
                })
                .min_by_key(|(_, unit)| {
                    (
                        deterministic_ability_target_rank(
                            self.config.match_seed,
                            source.id,
                            ability,
                            sequence,
                            unit.id,
                        ),
                        unit.id,
                    )
                })
                .map(|(index, _)| index)
        };
        let Some(index) = pick(false).or_else(|| pick(true)) else {
            return false;
        };
        let target = &mut units[index];
        for grant in profile.always.into_iter().chain(profile.options[option]) {
            if installed(target, grant.ability) {
                continue;
            }
            if target.status.native_grants.insert(grant)
                && let NativeAbilityGrantEffect::MaximumLife(amount) = grant.effect
            {
                target.health_max = target
                    .health_max
                    .checked_add(amount)
                    .expect("native life grant overflowed")
                    .max(1);
                target.health = target.health.min(target.health_max);
            }
        }
        self.last_ability_casts.push(AbilityCastEvent {
            source: source.id,
            ability,
            target: AbilityCastTarget::Unit(target.id),
            target_position: Some(target.position),
            effect: AbilityEffect::BuildingPermanentGrant(profile),
        });
        true
    }
}

#[cfg(test)]
mod tests;
