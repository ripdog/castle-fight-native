use super::*;
use crate::{ManaProfile, MapVersion, NativeAbilityGrantEffect as Effect};
const VERSION: MapVersion = MapVersion::CASTLE_FIGHT_9_27;
fn grant(id: u32, effect: Effect) -> NativeAbilityGrant {
    NativeAbilityGrant {
        ability: AbilityId(id),
        effect,
    }
}
fn profile() -> BuildingEnchantmentProfile {
    BuildingEnchantmentProfile {
        map_version: VERSION,
        trigger_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
        trigger_invulnerable: true,
        trigger_spell_immune: false,
        heal_if_missing_hp_gt: 75,
        heal_amount: 100,
        retry_marker: AbilityId(99),
        dispatcher: AbilityId(98),
        dispatcher_option_index: 2,
        options: [
            grant(
                91,
                Effect::Speed {
                    attack: 20,
                    movement: 30,
                    buff_rawcode: 90,
                },
            ),
            grant(
                92,
                Effect::SpellBookArmor {
                    bonus_per_100: 100,
                    child: AbilityId(95),
                    badge: AbilityId(96),
                },
            ),
            grant(
                93,
                Effect::SourceDamageSplash {
                    dispatcher: AbilityId(98),
                    damage_per_10k: 5000,
                    radius: 20 * SUBUNITS_PER_WORLD_UNIT,
                },
            ),
        ],
    }
}
fn simulation(workers: usize) -> Simulation {
    Simulation::new(
        SimulationConfig {
            navigation_cell_size: 16 * SUBUNITS_PER_WORLD_UNIT,
            navigation_max: NavCell::new(500, 64),
            ..Default::default()
        },
        workers,
    )
}
fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}
fn unit(sim: &mut Simulation, team: u8, x: i32, sapper: bool) -> SimId {
    sim.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(team),
            position: SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 10 * SUBUNITS_PER_WORLD_UNIT),
            health: 1000,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1000,
                delivery: AttackDelivery::Melee,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: sapper,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}
fn tower(sim: &mut Simulation) -> SimId {
    sim.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(1, 1, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: Some(SpellcastingProfile {
            mana: ManaProfile::per_second(100, 100, 0),
            ability: AutomaticAbilityProfile {
                id: AbilityId(89),
                mana_cost: 3,
                cooldown_ticks: 1,
                range: 2000 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::BuildingEnchantment(profile()),
            },
        }),
    })
}
fn trigger(sim: &mut Simulation) -> SimId {
    sim.spawn_building_with_properties(
        BuildingSpawn {
            team: Team(1),
            footprint: BuildingFootprint::new(10, 1, 1, 1),
            health: 1000,
            attack: None,
            production: None,
            spellcasting: None,
        },
        BuildingGameplayProperties {
            classifications: UnitClassifications {
                invulnerable: true,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}
fn restore(sim: &Simulation) -> Simulation {
    let bundle = crate::castle_fight_content_bundle(VERSION).unwrap();
    let snap =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), bundle)
            .unwrap();
    let mut other = Simulation::new(sim.config.clone(), 4);
    other.restore_snapshot(&snap).unwrap();
    other
}
#[test]
fn healing_does_not_mark_targets_and_native_enemy_trigger_is_separate_from_ally_selection() {
    let mut sim = simulation(1);
    let source = tower(&mut sim);
    let target = unit(&mut sim, 0, 40, true);
    sim.world
        .get_mut::<Health>(entity(&sim, target))
        .unwrap()
        .current = 800;
    sim.step();
    assert_eq!(sim.building(source).unwrap().ability_cast_sequence, Some(0));
    trigger(&mut sim);
    sim.step();
    assert_eq!(sim.unit(target).unwrap().health, 900);
    assert!(
        !sim.unit(target)
            .unwrap()
            .status
            .native_grants
            .contains(profile().retry_marker)
    );
    sim.step();
    assert_eq!(sim.unit(target).unwrap().health, 1000);
    sim.step();
    assert!(
        sim.unit(target)
            .unwrap()
            .status
            .native_grants
            .contains(profile().retry_marker)
    );
    let mut other = restore(&sim);
    for _ in 0..4 {
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                other.step();
                other.checksum()
            }
        );
        let status = sim.unit(target).unwrap().status;
        if status.native_grants.contains(profile().options[0].ability) {
            assert_eq!(effective_attack_cooldown_ticks(120, status), 100);
        }
    }
    let grants = sim.unit(target).unwrap().status.native_grants;
    assert!(
        profile()
            .options
            .into_iter()
            .all(|g| grants.contains(g.ability))
    );
    let status = sim.unit(target).unwrap().status;
    assert_eq!(effective_attack_cooldown_ticks(120, status), 100);
    assert!(
        status.movement_modifiers[..usize::from(status.movement_modifier_count)]
            .iter()
            .any(|m| m.percent_delta == 30)
    );
    assert_eq!(grants.armor_bonus_per_100(), 100);
    assert!(grants.contains(AbilityId(95)) && grants.contains(AbilityId(96)));
}
#[test]
fn compacted_option_index_can_leave_forge_without_its_dispatcher() {
    let mut sim = simulation(1);
    tower(&mut sim);
    trigger(&mut sim);
    let target = unit(&mut sim, 0, 40, true);
    let mut status = StatusState::default();
    status.native_grants.insert(profile().options[0]);
    status.native_grants.insert(profile().options[1]);
    sim.world.entity_mut(entity(&sim, target)).insert(status);
    sim.step();
    let grants = sim.unit(target).unwrap().status.native_grants;
    assert!(grants.contains(profile().options[2].ability));
    assert!(!grants.contains(profile().dispatcher));
    assert!(grants.contains(profile().retry_marker));
    let before = grants;
    sim.step();
    assert_eq!(sim.unit(target).unwrap().status.native_grants, before);
}
#[test]
fn preference_retries_and_no_target_still_consumes_parent_resources() {
    let mut sim = simulation(1);
    let source = tower(&mut sim);
    trigger(&mut sim);
    let marked = unit(&mut sim, 0, 40, true);
    let unmarked = unit(&mut sim, 0, 50, true);
    let excluded = unit(&mut sim, 0, 60, false);
    let mut status = StatusState::default();
    status.native_grants.insert(grant(99, Effect::AuraMarker));
    sim.world.entity_mut(entity(&sim, marked)).insert(status);
    sim.step();
    assert_eq!(
        sim.ability_casts_last_tick()[0].target,
        AbilityCastTarget::Unit(unmarked)
    );
    sim.world.despawn(entity(&sim, unmarked));
    sim.step();
    assert_eq!(
        sim.ability_casts_last_tick()[0].target,
        AbilityCastTarget::Unit(marked)
    );
    sim.world.despawn(entity(&sim, marked));
    sim.step();
    assert!(
        sim.unit(excluded)
            .unwrap()
            .status
            .native_grants
            .iter()
            .next()
            .is_none()
    );
    assert!(sim.ability_casts_last_tick().is_empty());
    assert_eq!(sim.building(source).unwrap().mana_current, Some(91));
}

fn forge(sim: &mut Simulation, source: SimId, dispatcher: bool) {
    let mut status = StatusState::default();
    status.native_grants.insert(profile().options[2]);
    if dispatcher {
        status.native_grants.insert(grant(
            98,
            Effect::SourceDamageDispatcher { aftershock: None },
        ));
    }
    sim.world
        .entity_mut(entity(sim, source))
        .insert((status, AttackTargetMask::GROUND_UNITS));
}

fn kill_caster(sim: &mut Simulation, source: SimId, attacker: SimId) {
    sim.world
        .get_mut::<Health>(entity(sim, source))
        .unwrap()
        .current = 1;
    sim.world
        .entity_mut(entity(sim, attacker))
        .insert(AttackProfile {
            damage: 5,
            range: 200 * SUBUNITS_PER_WORLD_UNIT,
            acquisition_range: 200 * SUBUNITS_PER_WORLD_UNIT,
            cooldown_ticks: 1000,
            delivery: AttackDelivery::Melee,
        });
    for _ in 0..10 {
        if sim.unit(source).is_none() {
            return;
        }
        sim.step();
    }
    panic!("fixture caster survived native damage");
}

#[test]
fn installed_haste_is_a_self_aura_and_obeys_source_suppression_and_zero_linger() {
    let mut sim = simulation(1);
    let source = unit(&mut sim, 0, 40, true);
    let neighbor = unit(&mut sim, 0, 40, true);
    let mut original = sim.snapshot_units();
    let source_index = find_unit_index(&original, source).unwrap();
    let neighbor_index = find_unit_index(&original, neighbor).unwrap();
    original[source_index]
        .status
        .native_grants
        .insert(profile().options[0]);
    for (disabled, invisible) in [(false, false), (true, false), (false, true)] {
        let mut units = original.clone();
        units[source_index].abilities_disabled = disabled;
        units[source_index].classifications.invisible = invisible;
        sim.apply_speed_auras(&mut units);
        assert_eq!(
            units[source_index].status.attack_speed_modifier_count,
            u8::from(!disabled && !invisible)
        );
        assert_eq!(units[neighbor_index].status.attack_speed_modifier_count, 0);
        units[source_index].status.native_grants = Default::default();
        sim.next_tick = 2;
        purge_expired_status_modifiers(&mut units[source_index].status, sim.next_tick);
        sim.apply_speed_auras(&mut units);
        assert_eq!(units[source_index].status.attack_speed_modifier_count, 0);
        sim.next_tick = 0;
    }
}
#[test]
fn forge_is_victim_centered_includes_primary_and_requires_dispatcher_and_native_weapon_class() {
    let mut sim = simulation(1);
    let source = unit(&mut sim, 0, 40, true);
    let victim = unit(&mut sim, 1, 100, true);
    let near = unit(&mut sim, 1, 110, true);
    let caster_neighbor = unit(&mut sim, 1, 45, true);
    let air = unit(&mut sim, 1, 108, true);
    let excluded = unit(&mut sim, 1, 109, false);
    sim.world
        .entity_mut(entity(&sim, air))
        .insert(MovementClass::Air);
    sim.world
        .entity_mut(entity(&sim, victim))
        .insert(UnitClassifications {
            combat_sapper: true,
            spell_immune: true,
            ..Default::default()
        });
    forge(&mut sim, source, false);
    forge(&mut sim, near, true);
    let mut units = sim.snapshot_units();
    let i = find_unit_index(&units, victim).unwrap();
    super::super::source_damage_runtime::dispatch_inline(
        source,
        i,
        40,
        &mut units,
        sim.combat_rules.damage_rules,
        0,
        &sim.retired_damage_sources,
    );
    assert!(units.iter().all(|u| u.health == 1000));
    let s = find_unit_index(&units, source).unwrap();
    units[s].status.native_grants.insert(grant(
        98,
        Effect::SourceDamageDispatcher { aftershock: None },
    ));
    super::super::source_damage_runtime::dispatch_inline(
        source,
        i,
        40,
        &mut units,
        sim.combat_rules.damage_rules,
        0,
        &sim.retired_damage_sources,
    );
    assert_eq!(units[i].health, 980);
    assert_eq!(units[find_unit_index(&units, near).unwrap()].health, 980); // Dummy damage cannot trigger the recipient's own Forge.
    for id in [caster_neighbor, air, excluded] {
        assert_eq!(units[find_unit_index(&units, id).unwrap()].health, 1000);
    }
}
#[test]
fn native_self_damage_and_periodic_source_survive_restore_but_dummy_damage_does_not_dispatch() {
    for dummy in [false, true] {
        let mut sim = simulation(1);
        let source = unit(&mut sim, 0, 40, true);
        let victim = unit(&mut sim, 1, 100, true);
        let near = unit(&mut sim, 1, 110, true);
        forge(&mut sim, source, true);
        let bolt = crate::NativeBoltProfile {
            ability: AbilityId(77),
            damage: 40,
            stun_ticks: 0,
            hero_stun_ticks: 0,
            duration_ticks: 60,
            damage_per_second: 10,
            speed_per_tick: 200 * SUBUNITS_PER_WORLD_UNIT,
            cleanse: false,
            targets: AttackTargetMask::GROUND_UNITS,
        };
        let from = sim.unit(source).unwrap().position;
        let to = sim.unit(victim).unwrap().position;
        if dummy {
            sim.launch_dummy_bolt(source, Team(0), from, victim, to, bolt)
        } else {
            sim.launch_native_bolt(source, Team(0), from, victim, to, bolt)
        }
        let mut other = restore(&sim);
        for _ in 0..35 {
            assert_eq!(
                {
                    sim.step();
                    sim.checksum()
                },
                {
                    other.step();
                    other.checksum()
                }
            );
        }
        assert_eq!(
            sim.unit(victim).unwrap().health,
            if dummy { 950 } else { 925 }
        );
        assert_eq!(
            sim.unit(near).unwrap().health,
            if dummy { 1000 } else { 975 }
        );
    }
}

#[test]
fn released_native_damage_retains_the_casters_final_forge_state_after_death_and_restore() {
    for (cleansed_before_death, decays) in [(false, false), (true, false), (false, true)] {
        let mut sim = simulation(1);
        let source = unit(&mut sim, 0, 40, true);
        let victim = unit(&mut sim, 1, 100, true);
        let near = unit(&mut sim, 1, 110, true);
        forge(&mut sim, source, true);
        let bolt = crate::NativeBoltProfile {
            ability: AbilityId(77),
            damage: 40,
            stun_ticks: 0,
            hero_stun_ticks: 0,
            duration_ticks: 60,
            damage_per_second: 10,
            speed_per_tick: 2 * SUBUNITS_PER_WORLD_UNIT,
            cleanse: false,
            targets: AttackTargetMask::GROUND_UNITS,
        };
        sim.launch_native_bolt(
            source,
            Team(0),
            sim.unit(source).unwrap().position,
            victim,
            sim.unit(victim).unwrap().position,
            bolt,
        );
        let e = entity(&sim, source);
        if decays {
            sim.world
                .entity_mut(e)
                .insert(CorpseProducer(CorpseProfile {
                    definition: CorpseDefinitionId(1),
                    decay_start_ticks: 0,
                    lifetime_ticks: Some(2),
                }));
        }
        if cleansed_before_death {
            sim.world.get_mut::<StatusState>(e).unwrap().native_grants = Default::default();
        }
        kill_caster(&mut sim, source, victim);
        assert!(sim.unit(source).is_none());
        assert_eq!(
            sim.retired_damage_sources.contains_key(&source),
            !cleansed_before_death
        );
        let mut other = restore(&sim);
        for _ in 0..65 {
            assert_eq!(
                {
                    sim.step();
                    sim.checksum()
                },
                {
                    other.step();
                    other.checksum()
                }
            );
        }
        assert_eq!(
            sim.retired_damage_sources.contains_key(&source),
            !cleansed_before_death && !decays
        );
        assert_eq!(
            sim.unit(victim).unwrap().health,
            if cleansed_before_death || decays {
                950
            } else {
                925
            }
        );
        assert_eq!(
            sim.unit(near).unwrap().health,
            if cleansed_before_death || decays {
                1000
            } else {
                975
            }
        );
    }
}
#[test]
fn a_fatal_dummy_splash_does_not_activate_the_weapon_casters_kill_berserk() {
    for enchanted in [false, true] {
        let mut sim = simulation(1);
        let source = unit(&mut sim, 0, 40, true);
        let victim = unit(&mut sim, 1, 100, true);
        if enchanted {
            forge(&mut sim, source, true);
        }
        let e = entity(&sim, source);
        sim.world.entity_mut(e).insert((
            AttackProfile {
                damage: 60,
                range: 200 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 200 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1000,
                delivery: AttackDelivery::Melee,
            },
            PassiveUnitEffects::single(PassiveUnitEffect::KillBerserk(crate::KillBerserkProfile {
                ability: AbilityId(60),
                duration_ticks: 30,
                movement_percent_delta: 10,
                attack_speed_percent_delta: 20,
                damage_taken_bonus_per_10k: 0,
            })),
        ));
        sim.world
            .get_mut::<Health>(entity(&sim, victim))
            .unwrap()
            .current = 20;
        for _ in 0..30 {
            if sim.unit(victim).is_none() {
                break;
            }
            sim.step();
        }
        assert!(sim.unit(victim).is_none());
        assert_eq!(
            sim.unit(source).unwrap().status.attack_speed_modifier_count,
            u8::from(!enchanted)
        );
    }
}

#[test]
fn native_feedback_and_lightning_packets_dispatch_the_actual_owner_even_after_death() {
    for kind in 0..3 {
        for dead in [false, true] {
            let mut sim = simulation(1);
            let source = unit(&mut sim, 0, 40, true);
            let victim = unit(&mut sim, 1, 100, true);
            let near = unit(&mut sim, 1, 110, true);
            forge(&mut sim, source, true);
            let origin = sim.unit(source).unwrap().position;
            if dead {
                kill_caster(&mut sim, source, victim);
            }
            let mut units = sim.snapshot_units();
            let victim_index = find_unit_index(&units, victim).unwrap();
            units[victim_index].mana_current = Some(40);
            let mut health: Vec<_> = units.iter().map(|unit| unit.health).collect();
            let mut effects = PendingAttackEffects::default();
            match kind {
                0 => {
                    effects.feedback = Some(crate::FeedbackEffectProfile {
                        ability: AbilityId(78),
                        maximum_mana_drained: 40,
                        damage_per_mana_per_10k: 10_000,
                        summoned_damage: 0,
                        targets: AttackTargetMask::GROUND_UNITS,
                    })
                }
                1 => {
                    effects.triggered_spell = Some(TriggeredAttackEffect::ForkedLightning(
                        crate::ForkedLightningProfile {
                            ability: AbilityId(78),
                            damage: 40,
                            maximum_targets: 1,
                            distance: 200 * SUBUNITS_PER_WORLD_UNIT,
                            initial_radius: 0,
                            final_radius: 20 * SUBUNITS_PER_WORLD_UNIT,
                            targets: AttackTargetMask::GROUND_UNITS,
                        },
                    ))
                }
                2 => {
                    effects.triggered_spell = Some(TriggeredAttackEffect::ChainLightning(
                        crate::ChainLightningEffectProfile {
                            ability: AbilityId(78),
                            initial_damage: 40,
                            maximum_targets: 1,
                            jump_radius: 20 * SUBUNITS_PER_WORLD_UNIT,
                            damage_reduction_per_10k: 5000,
                            targets: AttackTargetMask::GROUND_UNITS,
                        },
                    ))
                }
                _ => unreachable!(),
            }
            apply_pending_attack_effects(
                TargetIndex::Unit(victim_index),
                effects,
                PendingAttackEffectSource {
                    id: source,
                    position: origin,
                    team: Team(0),
                },
                PendingAttackEffectState {
                    navigation_cell_size: sim.config.navigation_cell_size,
                    completed_tick: sim.next_tick,
                    units: &mut units,
                    unit_health: &mut health,
                    buildings: &mut [],
                    building_health: &mut [],
                    damage_rules: sim.combat_rules.damage_rules,
                    retired_sources: &sim.retired_damage_sources,
                },
            );
            assert_eq!(health[victim_index], 940, "packet {kind}, dead={dead}");
            assert_eq!(
                health[find_unit_index(&units, near).unwrap()],
                980,
                "packet {kind}, dead={dead}"
            );
        }
    }
}

#[test]
fn regranting_a_cleansed_dispatcher_restores_the_retained_earth_controller() {
    let mut sim = simulation(1);
    let definition = crate::CastleFightTowerKind::ObeliskOfElements.definition();
    let source = sim.spawn_building_with_properties(
        definition.spawn(Team(0), BuildingFootprint::new(1, 1, 1, 1)),
        definition.gameplay_properties(),
    );
    let earth = crate::CastleFightUnitKind::ElementalOfEarth.definition();
    let target = sim.spawn_unit_with_properties(
        UnitSpawn::from_template(
            Team(0),
            SimPoint::new(80 * SUBUNITS_PER_WORLD_UNIT, 80 * SUBUNITS_PER_WORLD_UNIT),
            earth.template(),
        ),
        earth.gameplay_properties(),
    );
    let e = entity(&sim, target);
    sim.world
        .get_mut::<PassiveUnitEffects>(e)
        .unwrap()
        .retain(|p| {
            !matches!(
                p,
                PassiveUnitEffect::HealthScaledAftershock(_)
                    | PassiveUnitEffect::SourceDamageDispatcher(_)
            )
        });
    let p = match definition.spellcasting.unwrap().ability.effect {
        AbilityEffect::BuildingEnchantment(p) => p,
        _ => unreachable!(),
    };
    let sequence = (0..1000)
        .find(|seq| {
            deterministic_random(
                sim.config.match_seed,
                sim.next_tick,
                source,
                RANDOM_PURPOSE_ENCHANTMENT,
                *seq,
            ) % 3
                == u64::from(p.dispatcher_option_index)
        })
        .unwrap();
    let se = entity(&sim, source);
    sim.world.get_mut::<ManaState>(se).unwrap().current =
        definition.spellcasting.unwrap().ability.mana_cost;
    sim.world
        .get_mut::<AutomaticAbilityState>(se)
        .unwrap()
        .cast_sequence = sequence;
    trigger(&mut sim);
    sim.step();
    let grants = sim.unit(target).unwrap().status.native_grants;
    assert!(grants.iter().any(|g| matches!(
        g.effect,
        Effect::SourceDamageDispatcher {
            aftershock: Some(_)
        }
    )));
    let victim = unit(&mut sim, 1, 300, true);
    let mut units = sim.snapshot_units();
    let index = find_unit_index(&units, victim).unwrap();
    super::super::source_damage_runtime::dispatch_inline(
        target,
        index,
        0,
        &mut units,
        sim.combat_rules.damage_rules,
        sim.next_tick,
        &sim.retired_damage_sources,
    );
    assert!(units[index].health < 1000); // Zero parent damage still dispatches restored Aftershock.
}
