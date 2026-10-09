use super::*;
use crate::{
    NativeAbilityGrant, NativeAbilityGrantEffect, building_mechanics::BuildingEnchantmentProfile,
};
const RANDOM_PURPOSE_ENCHANTMENT: u64 = 0x454e_4348_414e_544d;

impl Simulation {
    pub(super) fn cast_building_enchantment(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AbilityId,
        profile: BuildingEnchantmentProfile,
        sequence: u64,
        units: &mut [UnitSnapshot],
    ) -> bool {
        let bounds =
            crate::building_mechanics::snowveil_for_version(profile.map_version).battlefield_bounds;
        let installed = |unit: &UnitSnapshot, id| {
            unit.status.native_grants.contains(id)
                || self
                    .world
                    .get::<ContentIdentity>(unit.entity)
                    .is_some_and(|c| {
                        crate::content::unit_has_ability_for_version(c.rawcode, id, c.map_version)
                    })
        };
        let pick = |relax| {
            units
                .iter()
                .enumerate()
                .filter(|(_, u)| {
                    u.health > 0
                        && u.team == source.team
                        && u.classifications.combat_sapper
                        && !u.classifications.invulnerable
                        && (relax || !installed(u, profile.retry_marker))
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
                .map(|(i, _)| i)
        };
        let Some(index) = pick(false).or_else(|| pick(true)) else {
            return false;
        };
        let target = &mut units[index];
        let mut granted_self_aura = false;
        if target.health_max - target.health > profile.heal_if_missing_hp_gt {
            target.health = target
                .health
                .saturating_add(profile.heal_amount)
                .min(target.health_max);
        } else {
            let mut candidates = profile.options;
            let mut count = candidates.len();
            let mut cursor = 0;
            while cursor < count {
                if installed(target, candidates[cursor].ability) {
                    count -= 1;
                    candidates[cursor] = candidates[count];
                } else {
                    cursor += 1;
                }
            }
            if count == 0 {
                target.health = target
                    .health
                    .saturating_add(profile.heal_amount)
                    .min(target.health_max);
            } else {
                let selected = (deterministic_random(
                    self.config.match_seed,
                    self.next_tick,
                    source.id,
                    RANDOM_PURPOSE_ENCHANTMENT,
                    sequence,
                ) % count as u64) as usize;
                if target.status.native_grants.insert(candidates[selected]) {
                    granted_self_aura = matches!(
                        candidates[selected].effect,
                        NativeAbilityGrantEffect::Speed { .. }
                    );
                }
                if selected == usize::from(profile.dispatcher_option_index) {
                    // The source re-adds the dispatcher, which re-enables retained Earth Aftershock after cleanse.
                    let aftershock =
                        self.world
                            .get::<ContentIdentity>(target.entity)
                            .and_then(|c| {
                                crate::native_effects::aftershock_for_version(
                                    c.map_version,
                                    c.rawcode,
                                )
                            });
                    target.status.native_grants.insert(NativeAbilityGrant {
                        ability: profile.dispatcher,
                        effect: NativeAbilityGrantEffect::SourceDamageDispatcher { aftershock },
                    });
                }
                target.status.native_grants.insert(NativeAbilityGrant {
                    ability: profile.retry_marker,
                    effect: NativeAbilityGrantEffect::AuraMarker,
                });
            }
        }
        self.last_ability_casts.push(AbilityCastEvent {
            source: source.id,
            ability,
            target: AbilityCastTarget::Unit(target.id),
            target_position: Some(target.position),
            effect: AbilityEffect::BuildingEnchantment(profile),
        });
        if granted_self_aura {
            self.apply_speed_auras(units);
        }
        true
    }
}

#[cfg(test)]
mod tests;
