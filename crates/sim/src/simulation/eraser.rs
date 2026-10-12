//! The source picker and the type-wide enumeration have independent eligibility.
use super::*;
#[cfg(test)]
mod tests;

impl Simulation {
    pub(super) fn cast_building_eraser(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        trigger: SimId,
        sequence: u64,
        units: &mut [UnitSnapshot],
    ) -> usize {
        let AbilityEffect::BuildingEraser(p) = ability.effect else {
            unreachable!()
        };
        let owner = self
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source.id))
            .and_then(|e| e.get::<Owner>())
            .map_or(PlayerId(source.team.0), |o| o.0);
        self.launch_native_parasite(source, owner, trigger, p.parent);
        let origin = match source.origin {
            AbilitySourceOrigin::Unit(point) => point,
            AbilitySourceOrigin::Building(f) => {
                footprint_center_point(f, self.config.navigation_cell_size)
            }
        };
        self.last_ability_casts.push(AbilityCastEvent {
            source: source.id,
            ability: p.ability,
            target: AbilityCastTarget::Point(origin),
            target_position: None,
            effect: ability.effect,
        });
        let bounds =
            crate::building_mechanics::snowveil_for_version(p.map_version).battlefield_bounds;
        let inside = |u: &UnitSnapshot| {
            u.position.x >= bounds[0]
                && u.position.x <= bounds[2]
                && u.position.y >= bounds[1]
                && u.position.y <= bounds[3]
        };
        let selected = units
            .iter()
            .filter(|u| {
                u.health > 0
                    && u.team != source.team
                    && u.classifications.combat_sapper
                    && !u.classifications.locust
                    && !u.classifications.invulnerable
                    && u.rawcode != p.selector_excluded_rawcode
                    && !p
                        .selector_exclusions
                        .iter()
                        .any(|&a| self.native_ability_installed(u, a))
                    && inside(u)
            })
            .min_by_key(|u| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        p.ability,
                        sequence ^ 0x4552_4153_4552,
                        u.id,
                    ),
                    u.id,
                )
            })
            .map(|u| u.rawcode);
        let Some(kind) = selected else { return 0 };
        let mut effects = 0;
        for index in 0..units.len() {
            let target = units[index];
            // The second enumeration admits non-sappers, protected handles and both movement
            // classes of the selected type. Native Locust enumeration still does not admit Aloc.
            if target.health <= 0
                || target.team == source.team
                || target.rawcode != kind
                || target.classifications.locust
                || !inside(&target)
            {
                continue;
            }
            if self.erase_strong_building_shield(
                &mut units[index],
                p.map_version,
                p.shield_minimum_level,
            ) {
                effects += 1;
                continue;
            }
            self.last_ability_casts.push(AbilityCastEvent {
                source: source.id,
                ability: p.ability,
                target: AbilityCastTarget::Unit(target.id),
                target_position: Some(target.position),
                effect: ability.effect,
            });
            // A070 is tested after target art. This does not invoke Shredder or the normal
            // negative-spell shield path, and a level-one shield remains installed.
            if self.native_ability_installed(&target, p.excluded_ability)
                || self.has_negative_building_protection(target.id)
                || target.classifications.invulnerable
            {
                continue;
            }
            let amount = scale_damage_per_10k(
                p.damage,
                self.combat_rules
                    .damage_rules
                    .bonus_per_10k(DamageType::Chaos, target.armor.armor_type),
            );
            let amount = scale_damage_per_10k(
                super::native_kill_effects::damage_after_native_incoming(
                    &units[index],
                    amount,
                    self.next_tick,
                ),
                target.snow_damage_taken_per_10k,
            );
            super::damage_thresholds::observe_damage_event(
                &mut units[index],
                target.health,
                amount,
            );
            death_healing::commit_health_from(
                units,
                index,
                target.health.saturating_sub(amount),
                Some(source.id),
            );
            effects += 1;
        }
        effects
    }
}
