//! Independent map callback selection and native dummy effects.
use super::*;
use crate::building_mechanics::NagaEarthquakeProfile;
#[cfg(test)]
mod tests;

impl Simulation {
    pub(super) fn cast_naga_earthquake(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AbilityId,
        trigger: SimId,
        p: NagaEarthquakeProfile,
        sequence: u64,
        units: &mut [UnitSnapshot],
    ) -> usize {
        let owner = self
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source.id))
            .and_then(|e| e.get::<Owner>())
            .map(|o| o.0)
            .unwrap_or(PlayerId(source.team.0));
        self.launch_native_parasite(source, owner, trigger, p.parent);
        let bounds =
            crate::building_mechanics::snowveil_for_version(p.map_version).battlefield_bounds;
        let selected = units
            .iter()
            .filter(|u| {
                u.health > 0
                    && u.team != source.team
                    && u.classifications.combat_sapper
                    && u.target_movement_class() == MovementClass::Ground
                    && !p
                        .selector_exclusions
                        .into_iter()
                        .any(|a| self.native_ability_installed(u, a))
                    && u.position.x >= bounds[0]
                    && u.position.y >= bounds[1]
                    && u.position.x <= bounds[2]
                    && u.position.y <= bounds[3]
            })
            .min_by_key(|u| {
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
            });
        let Some(center) = selected.map(|u| u.position) else {
            return 0;
        };
        let effect = AbilityEffect::FrostNova {
            // The shared native child identity owns the slow and its presentation binding.
            ability: p.child,
            radius: p.radius,
            primary_damage: 0,
            area_damage: p.damage,
            duration_ticks: p.duration_ticks,
            hero_duration_ticks: p.hero_duration_ticks,
            movement_percent_delta: p.movement_percent_delta,
            attack_speed_percent_delta: p.attack_speed_percent_delta,
            targets: p.targets,
        };
        self.last_ability_casts.push(AbilityCastEvent {
            source: source.id,
            ability: p.child,
            target: AbilityCastTarget::Point(center),
            target_position: Some(center),
            effect: AbilityEffect::NagaEarthquake(p),
        });
        let mut count = 0;
        for i in 0..units.len() {
            let u = units[i];
            if u.health <= 0
                || u.team == source.team
                || center.distance_sq(u.position) > square_i32(p.radius)
            {
                continue;
            }
            let before = u.health;
            if apply_ability_effect_to_unit(
                &mut units[i],
                effect,
                source.team,
                self.next_tick,
                self.combat_rules.damage_rules,
            ) {
                // AHtc's native slow is undispellable even though the packet is spell damage.
                status::mark_modifier_undispellable(&mut units[i].status, ModifierId(p.child.0));
                // The actual source is an invulnerable native dummy, not the logical building.
                death_healing::observe_inline_death_from(units, i, before, None);
                count += 1;
            }
        }
        count
    }
}
