//! Independent script selection followed by an invisible native actor's ordinary weapon.
use super::*;
use crate::components::{BuildingAttackProxyState, NativeAction};
#[cfg(test)]
mod tests;

impl Simulation {
    pub(super) fn cast_building_attack_proxy(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        trigger: SimId,
        sequence: u64,
        buildings: &[BuildingSnapshot],
    ) -> bool {
        let AbilityEffect::BuildingAttackProxy(p) = ability.effect else {
            unreachable!()
        };
        let owner = self
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source.id))
            .and_then(|e| e.get::<Owner>())
            .map_or(PlayerId(source.team.0), |o| o.0);
        self.launch_native_parasite(source, owner, trigger, p.parent);
        let bounds =
            crate::building_mechanics::snowveil_for_version(p.map_version).battlefield_bounds;
        let selected = buildings
            .iter()
            .filter(|b| {
                let e = self.world.entity(b.entity);
                let point = footprint_center_point(b.footprint, self.config.navigation_cell_size);
                b.health > 0
                    && b.team != source.team
                    && e.get::<Owner>()
                        .is_some_and(|o| self.player_state(o.0).is_some())
                    && !e
                        .get::<ContentIdentity>()
                        .is_some_and(|c| p.excluded_rawcodes.contains(&c.rawcode))
                    && !e.get::<ContentIdentity>().is_some_and(|c| {
                        crate::content::unit_has_ability_for_version(
                            c.rawcode,
                            p.excluded_ability,
                            c.map_version,
                        )
                    })
                    && point.x >= bounds[0]
                    && point.x <= bounds[2]
                    && point.y >= bounds[1]
                    && point.y <= bounds[3]
            })
            .min_by_key(|b| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        p.freeze_ability,
                        sequence,
                        b.id,
                    ),
                    b.id,
                )
            });
        let Some(target) = selected else {
            return false;
        };
        let origin = match source.origin {
            AbilitySourceOrigin::Unit(p) => p,
            AbilitySourceOrigin::Building(f) => {
                footprint_center_point(f, self.config.navigation_cell_size)
            }
        };
        let destination =
            footprint_center_point(target.footprint, self.config.navigation_cell_size);
        let dummy = self.allocate_id();
        self.world.spawn((
            dummy,
            NativeAction::AttackProxyActor {
                owner,
                team: source.team,
                map_version: p.map_version,
                rawcode: p.dummy_rawcode,
                position: origin,
                expires_tick: self.next_tick + u64::from(p.dummy_ticks),
            },
        ));
        self.reveal_area(
            source.team,
            destination,
            p.vision_radius,
            u64::from(p.vision_ticks),
            false,
        );
        self.last_ability_casts.push(AbilityCastEvent {
            source: source.id,
            ability: ability.id,
            target: AbilityCastTarget::Point(destination),
            target_position: Some(destination),
            effect: ability.effect,
        });
        // The mark and fog are unconditional; only the ordinary attack order can fail.
        if target.classifications.invulnerable
            || origin.distance_sq(destination) > square_i32(p.range)
        {
            return false;
        }
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::BuildingAttackProxy(BuildingAttackProxyState {
                source: dummy,
                team: source.team,
                target: target.id,
                profile: p,
                level: p.level(self.next_tick.saturating_sub(self.round_start_tick)),
                origin,
                destination,
                launch_tick: self.next_tick,
                impact_tick: self.next_tick
                    + projectile_travel_ticks(origin.distance_sq(destination), p.speed_per_tick),
            }),
        ));
        true
    }

    pub(super) fn resolve_due_attack_proxy_impacts(
        &mut self,
        buildings: &mut [BuildingSnapshot],
        building_health: &mut [i32],
    ) -> (usize, usize) {
        let mut due = self
            .world
            .iter_entities()
            .filter_map(|e| {
                let NativeAction::BuildingAttackProxy(state) = e.get::<NativeAction>()? else {
                    return None;
                };
                (state.impact_tick <= self.next_tick)
                    .then(|| (*e.get::<SimId>().unwrap(), e.id(), state.clone()))
            })
            .collect::<Vec<_>>();
        due.sort_unstable_by_key(|(id, _, _)| *id);
        let impacts = due.len();
        let mut effects = 0;
        for (_, entity, state) in due {
            self.world.despawn(entity);
            let Some(i) = find_building_index(buildings, state.target) else {
                continue;
            };
            let b = &mut buildings[i];
            b.health = building_health[i];
            if b.health <= 0 || b.classifications.invulnerable || b.team == state.team {
                continue;
            }
            if !self.debug_buildings_invulnerable {
                let damage = self.combat_rules.damage_rules.apply_attack(
                    state.profile.damage,
                    DamageType::Chaos,
                    b.armor,
                );
                b.health = b
                    .health
                    .saturating_sub(scale_damage_per_10k(damage, b.snow_damage_taken_per_10k));
            }
            let level = state.profile.levels[usize::from(state.level)];
            if b.health > 0 {
                let status = b.status.get_or_insert_default();
                if !b.classifications.spell_immune {
                    super::frost_launcher::freeze(
                        status,
                        self.next_tick,
                        if b.classifications.uses_hero_spell_rules() {
                            level.hero_freeze_ticks
                        } else {
                            level.freeze_ticks
                        },
                        state.profile.freeze_ability,
                    );
                }
                // Native weapon poison admits structures and magic immunity independently of freeze.
                self.apply_poison_status(
                    state.source,
                    b.id,
                    status,
                    level.poison,
                    self.next_tick,
                    b.classifications.uses_hero_spell_rules(),
                );
            }
            building_health[i] = b.health;
            effects += 1;
        }
        (impacts, effects)
    }
}
