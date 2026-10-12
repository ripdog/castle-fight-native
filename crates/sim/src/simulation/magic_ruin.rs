//! Literal source outcome order, independent of the native parent order target.
use super::*;
use crate::{building_mechanics::BuildingMagicRuinProfile, components::NativeAction};
#[cfg(test)]
mod tests;

const RANDOM_PURPOSE_MAGIC_RUIN: u64 = 0x4d41_4749_4352_5549;

impl Simulation {
    fn magic_ruin_target(
        &self,
        source: AbilitySourceSnapshot,
        p: BuildingMagicRuinProfile,
        sequence: u64,
        units: &[UnitSnapshot],
        ground_only: bool,
    ) -> Option<usize> {
        let bounds =
            crate::building_mechanics::snowveil_for_version(p.map_version).battlefield_bounds;
        units
            .iter()
            .enumerate()
            .filter(|(_, u)| {
                u.health > 0
                    && u.team != source.team
                    && u.classifications.combat_sapper
                    && !u.classifications.locust
                    && (!ground_only || u.target_movement_class() == MovementClass::Ground)
                    && !p
                        .selector_exclusions
                        .iter()
                        .any(|&a| self.native_ability_installed(u, a))
                    && !u.classifications.invulnerable
                    && u.rawcode != p.selector_excluded_rawcode
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
                        if ground_only {
                            p.bolt.ability
                        } else {
                            p.hex_ability
                        },
                        sequence,
                        u.id,
                    ),
                    u.id,
                )
            })
            .map(|(index, _)| index)
    }

    pub(super) fn cast_building_magic_ruin(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        trigger: SimId,
        sequence: u64,
        units: &mut [UnitSnapshot],
    ) -> usize {
        let AbilityEffect::BuildingMagicRuin(p) = ability.effect else {
            unreachable!()
        };
        let owner = self
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source.id))
            .and_then(|e| e.get::<Owner>())
            .map_or(PlayerId(source.team.0), |o| o.0);
        self.launch_native_parasite(source, owner, trigger, p.parent);
        // The source draws even if the callback finds no recipient or a shield blocks it.
        let outcome = (deterministic_random(
            self.config.match_seed,
            self.next_tick,
            source.id,
            RANDOM_PURPOSE_MAGIC_RUIN,
            sequence,
        ) % u64::from(p.outcome_count)) as u8;
        let selected = self.magic_ruin_target(source, p, sequence, units, false);
        let origin = match source.origin {
            AbilitySourceOrigin::Unit(point) => point,
            AbilitySourceOrigin::Building(f) => {
                footprint_center_point(f, self.config.navigation_cell_size)
            }
        };
        self.last_ability_casts.push(AbilityCastEvent {
            source: source.id,
            ability: ability.id,
            target: selected.map_or(AbilityCastTarget::Point(origin), |i| {
                AbilityCastTarget::Unit(units[i].id)
            }),
            target_position: selected.map(|i| units[i].position),
            effect: ability.effect,
        });
        let Some(index) = selected else { return 0 };
        if self.check_negative_building_shield(
            &mut units[index],
            p.map_version,
            source.id,
            sequence,
        ) {
            return 0;
        }
        self.apply_magic_ruin_outcome(source, owner, p, (outcome, index), sequence, units)
    }

    fn magic_ruin_dummy(
        &mut self,
        source: AbilitySourceSnapshot,
        owner: PlayerId,
        p: BuildingMagicRuinProfile,
        target: UnitSnapshot,
        branch: usize,
    ) -> SimId {
        if self.visible_teams(target.position) & (1 << source.team.0) == 0 {
            self.reveal_area(
                source.team,
                target.position,
                p.vision_radius,
                u64::from(p.vision_ticks),
                false,
            );
        }
        let actor = self.allocate_id();
        self.world.spawn((
            actor,
            NativeAction::NativeSpellActor {
                owner,
                team: source.team,
                map_version: p.map_version,
                rawcode: p.dummy_rawcode,
                flying: p.dummy_flying,
                position: target.position,
                expires_tick: self.next_tick + u64::from(p.control_dummy_ticks[branch]),
            },
        ));
        actor
    }

    fn magic_ruin_child_eligible(&self, source: AbilitySourceSnapshot, u: UnitSnapshot) -> bool {
        !u.classifications.locust
            && !u.classifications.invulnerable
            && !u.classifications.spell_immune
            && (!u.is_invisible(self.next_tick) || u.visible_to(source.team, self.next_tick))
    }

    fn apply_magic_ruin_outcome(
        &mut self,
        source: AbilitySourceSnapshot,
        owner: PlayerId,
        p: BuildingMagicRuinProfile,
        selected: (u8, usize),
        sequence: u64,
        units: &mut [UnitSnapshot],
    ) -> usize {
        let (outcome, index) = selected;
        match outcome {
            0 => {
                // ExplodeUnit is a death without an attributable damage killer or corpse.
                units[index].corpse = None;
                death_healing::commit_health_from(units, index, 0, None);
            }
            1 | 2 => {
                let target = units[index];
                if target.classifications.invulnerable || target.classifications.spell_immune {
                    return 0;
                }
                // JASS ATTACK_TYPE_NORMAL is the spell attack category; DAMAGE_TYPE_DEATH
                // bypasses numeric armor. It is not the ordinary melee Normal weapon class.
                let damage = spell_damage_after_defend(
                    target,
                    self.combat_rules.damage_rules.apply_spell(
                        p.death_damage[usize::from(outcome - 1)],
                        target.armor.armor_type,
                    ),
                    self.next_tick,
                );
                let damage = scale_damage_per_10k(
                    super::native_kill_effects::damage_after_native_incoming(
                        &target,
                        damage,
                        self.next_tick,
                    ),
                    target.snow_damage_taken_per_10k,
                );
                super::damage_thresholds::observe_damage_event(
                    &mut units[index],
                    target.health,
                    damage,
                );
                death_healing::commit_health_from(
                    units,
                    index,
                    target.health.saturating_sub(damage),
                    Some(source.id),
                );
            }
            3 => {
                for grant in p.grants {
                    if self.native_ability_installed(&units[index], grant.ability) {
                        continue;
                    }
                    let target = &mut units[index];
                    if target.status.native_grants.insert(grant)
                        && let crate::NativeAbilityGrantEffect::MaximumLife(amount) = grant.effect
                    {
                        target.health_max = target
                            .health_max
                            .checked_add(amount)
                            .expect("native maximum life overflow")
                            .max(1);
                    }
                }
                let target = &mut units[index];
                target.health = target.health_max;
                self.last_ability_casts.push(AbilityCastEvent {
                    source: source.id,
                    ability: p.grants[0].ability,
                    target: AbilityCastTarget::Unit(target.id),
                    target_position: Some(target.position),
                    effect: AbilityEffect::BuildingMagicRuin(p),
                });
            }
            4 => {
                let actor = self.magic_ruin_dummy(source, owner, p, units[index], 0);
                if self.magic_ruin_child_eligible(source, units[index]) {
                    self.apply_building_hex_after_shield(&mut units[index], p.hex);
                    self.last_ability_casts.push(AbilityCastEvent {
                        source: actor,
                        ability: p.hex_ability,
                        target: AbilityCastTarget::Unit(units[index].id),
                        target_position: Some(units[index].position),
                        effect: AbilityEffect::Hex { profile: p.hex },
                    });
                } else {
                    self.queue_building_reengagement(&mut units[index], p.hex);
                }
            }
            5 => {
                let actor = self.magic_ruin_dummy(source, owner, p, units[index], 1);
                if self.magic_ruin_child_eligible(source, units[index]) {
                    apply_ability_effect_to_unit(
                        &mut units[index],
                        AbilityEffect::NativeDebuff(p.banish),
                        source.team,
                        self.next_tick,
                        self.combat_rules.damage_rules,
                    );
                    self.last_ability_casts.push(AbilityCastEvent {
                        source: actor,
                        ability: p.banish.ability,
                        target: AbilityCastTarget::Unit(units[index].id),
                        target_position: Some(units[index].position),
                        effect: AbilityEffect::NativeDebuff(p.banish),
                    });
                }
                self.queue_building_reengagement(&mut units[index], p.hex);
            }
            6 => {
                let ground = if units[index].target_movement_class() == MovementClass::Ground {
                    Some(index)
                } else {
                    self.magic_ruin_target(source, p, sequence, units, true)
                };
                let Some(index) = ground else { return 0 };
                let target = units[index];
                let actor = self.magic_ruin_dummy(source, owner, p, target, 2);
                // The independent ground retarget does not run checkForShield again.
                if self.magic_ruin_child_eligible(source, target)
                    && p.bolt
                        .targets
                        .can_target_unit(target.target_movement_class())
                {
                    self.launch_native_bolt(
                        actor,
                        source.team,
                        target.position,
                        target.id,
                        target.position,
                        p.bolt,
                    );
                }
            }
            7 => {
                let target = &mut units[index];
                let definition = crate::CastleFightUnitKind::from_rawcode_for_version(
                    p.mutation_rawcode,
                    p.map_version,
                )
                .expect("supported Mutation version")
                .expect("complete Mutation body")
                .definition_for_version(p.map_version)
                .unwrap()
                .resolved();
                let child = self.spawn_resolved_unit_unchecked(
                    Some(owner),
                    UnitSpawn::from_template(source.team, target.position, definition.template),
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
                    .facing_direction = target.status.facing_direction;
                // RemoveUnit does not fire death callbacks, native death-spawns or create a corpse.
                // Commit removal at the phase boundary so existing spatial indices stay valid.
                target.removed_without_death = true;
                target.health = 0;
                target.corpse = None;
                self.last_ability_casts.push(AbilityCastEvent {
                    source: source.id,
                    ability: p.ability,
                    target: AbilityCastTarget::Point(target.position),
                    target_position: Some(target.position),
                    effect: AbilityEffect::BuildingMagicRuin(p),
                });
            }
            8 | 9 => {}
            _ => unreachable!("source-projected outcome table"),
        }
        1
    }
}
