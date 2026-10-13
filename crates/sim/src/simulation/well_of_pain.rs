//! Well of Pain: the scripted callback behind the native Parasite building trigger.
use super::*;
use crate::building_mechanics::WellOfPainProfile;

impl Simulation {
    pub(super) fn cast_well_of_pain(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AbilityId,
        trigger: SimId,
        p: WellOfPainProfile,
        sequence: u64,
        units: &mut [UnitSnapshot],
    ) -> bool {
        let owner = self
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source.id))
            .and_then(|e| e.get::<Owner>())
            .map_or(PlayerId(source.team.0), |o| o.0);
        self.launch_native_parasite(source, owner, trigger, p.parent);
        let bounds =
            crate::building_mechanics::snowveil_for_version(p.map_version).battlefield_bounds;
        let eligible = |u: &UnitSnapshot, wounded: bool| {
            u.health > 0
                && u.team != source.team
                && u.classifications.combat_sapper
                && u.rawcode != p.selector_excluded_rawcode
                && !p
                    .selector_exclusions
                    .into_iter()
                    .any(|a| self.native_ability_installed(u, a))
                && (!wounded
                    || (u.health < u.health_max
                        && !self.native_ability_installed(u, p.wounded_exclusion)))
                && u.position.x >= bounds[0]
                && u.position.y >= bounds[1]
                && u.position.x <= bounds[2]
                && u.position.y <= bounds[3]
        };
        let choose = |wounded: bool| {
            units
                .iter()
                .enumerate()
                .filter(|(_, u)| eligible(u, wounded))
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
        // A wounded enemy is always preferred; the fallback is any living enemy combat unit.
        let Some(i) = choose(true).or_else(|| choose(false)) else {
            return false;
        };
        self.last_ability_casts.push(AbilityCastEvent {
            source: source.id,
            ability,
            target: AbilityCastTarget::Unit(units[i].id),
            target_position: Some(units[i].position),
            effect: AbilityEffect::WellOfPain(p),
        });
        let u = &mut units[i];
        if self.check_negative_building_shield(u, p.map_version, source.id, sequence) {
            return false;
        }
        // Sonic spell damage: spell-armor-type rules, no numeric armor; magic immunity blocks it.
        if !u.classifications.spell_immune && !u.classifications.invulnerable {
            let damage = self
                .combat_rules
                .damage_rules
                .apply_spell(p.damage, u.armor.armor_type);
            let damage = spell_damage_after_defend(*u, damage, self.next_tick);
            let before = u.health;
            u.health = u.health.saturating_sub(damage);
            death_healing::observe_inline_death_from(units, i, before, None);
        }
        let u = units[i];
        if u.health <= 0 {
            let definition = crate::CastleFightUnitKind::from_rawcode_for_version(
                p.ghost_rawcode,
                p.map_version,
            )
            .expect("supported Ghost of Sorrow version")
            .expect("complete Ghost of Sorrow body")
            .definition_for_version(p.map_version)
            .unwrap()
            .resolved();
            let child = self.spawn_resolved_unit_unchecked(
                Some(owner),
                UnitSpawn::from_template(source.team, u.position, definition.template),
                definition,
            );
            let entity = self
                .world
                .iter_entities()
                .find(|e| e.get::<SimId>() == Some(&child))
                .unwrap()
                .id();
            self.world
                .get_mut::<StatusState>(entity)
                .unwrap()
                .facing_direction = u.status.facing_direction;
        }
        true
    }
}

#[cfg(test)]
mod tests;
