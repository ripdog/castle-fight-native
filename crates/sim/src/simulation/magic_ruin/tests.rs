use super::*;
use crate::{ManaProfile, MapVersion, NativeAbilityGrantEffect};
const VERSION: MapVersion = MapVersion::CASTLE_FIGHT_9_27;

fn profile() -> BuildingMagicRuinProfile {
    let AbilityEffect::BuildingMagicRuin(mut p) =
        crate::building_mechanics::magic_ruin_spellcasting_for_version(VERSION)
            .ability
            .effect
    else {
        panic!()
    };
    p.trigger_mechanical_only = false;
    p.death_damage = [10_000, 11];
    p.grants[0].effect = NativeAbilityGrantEffect::MaximumLife(17);
    p.grants[1].effect = NativeAbilityGrantEffect::AttackDamage(9);
    p.hex.duration_ticks = 8;
    p.hex.hero_duration_ticks = 3;
    p.hex.initial_reengage_ticks = 2;
    p.hex.defender_restore_ticks = 3;
    p.hex.defender_resume_ticks = 1;
    p.banish.duration_ticks = 4;
    p.banish.hero_duration_ticks = 3;
    p.control_dummy_ticks = [1, 1, 2];
    p.bolt.damage = 7;
    p.bolt.stun_ticks = 4;
    p.bolt.hero_stun_ticks = 2;
    p
}
fn sim() -> Simulation {
    Simulation::new(
        SimulationConfig {
            navigation_max: NavCell::new(500, 500),
            ..Default::default()
        },
        1,
    )
}
fn entity(s: &Simulation, id: SimId) -> Entity {
    s.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}
fn setup(s: &mut Simulation, outcome: u8, p: BuildingMagicRuinProfile) -> SimId {
    let seed = (0..1000)
        .find(|&seed| {
            deterministic_random(seed, 0, SimId(1), RANDOM_PURPOSE_MAGIC_RUIN, 0)
                % u64::from(p.outcome_count)
                == u64::from(outcome)
        })
        .unwrap();
    *s = Simulation::new(
        SimulationConfig {
            match_seed: seed,
            ..s.config.clone()
        },
        1,
    );
    spawn_caster(s, p)
}
fn spawn_caster(s: &mut Simulation, p: BuildingMagicRuinProfile) -> SimId {
    let caster = s.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(1, 1, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: Some(SpellcastingProfile {
            mana: ManaProfile::per_second(7, 7, 0),
            ability: AutomaticAbilityProfile {
                id: p.ability,
                mana_cost: 7,
                cooldown_ticks: 100,
                range: 1000 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::BuildingMagicRuin(p),
            },
        }),
    });
    // Independent native mechanical/ground trigger; the callback only enumerates combat sappers.
    s.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(100, 20, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: None,
    });
    caster
}
fn victim(s: &mut Simulation, movement: MovementClass, flags: UnitClassifications) -> SimId {
    s.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(1),
            position: SimPoint::new(50 * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT),
            health: 1000,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 100,
                delivery: AttackDelivery::Melee,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        UnitGameplayProperties {
            movement_class: movement,
            classifications: UnitClassifications {
                combat_sapper: true,
                ..flags
            },
            corpse: Some(CorpseProfile {
                definition: CorpseDefinitionId(900),
                decay_start_ticks: 3,
                lifetime_ticks: Some(100),
            }),
            armor: ArmorProfile {
                armor_type: ArmorType::Unarmored,
                armor_points: 100,
            },
            collision_radius: Some(CollisionRadius(0)),
            ..Default::default()
        },
    )
}
fn restored(s: &Simulation) -> Simulation {
    let content = crate::castle_fight_content_bundle(VERSION).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&s.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut result = Simulation::new(s.config.clone(), 4);
    result.restore_snapshot(&snapshot).unwrap();
    result
}

#[test]
fn outcome_table_distinguishes_explosion_damage_and_empty_rolls_with_parent_commitment() {
    for outcome in [0, 1, 2, 8, 9] {
        let mut s = sim();
        let caster = setup(&mut s, outcome, profile());
        let target = victim(&mut s, MovementClass::Ground, Default::default());
        s.step();
        assert_eq!(s.building(caster).unwrap().mana_current, Some(0));
        assert_eq!(s.building(caster).unwrap().ability_ready_tick, Some(100));
        match outcome {
            0 => {
                assert!(s.unit(target).is_none());
                assert!(s.corpses().is_empty());
            }
            1 => {
                assert!(s.unit(target).is_none());
                assert_eq!(s.corpses()[0].source_unit, target);
            }
            2 => assert_eq!(s.unit(target).unwrap().health, 989), // Numeric armor is bypassed.
            8 | 9 => assert_eq!(s.unit(target).unwrap().health, 1000),
            _ => unreachable!(),
        }
        let mut other = restored(&s);
        for _ in 0..3 {
            assert_eq!(s.step().checksum, other.step().checksum);
        }
    }
}

#[test]
fn callback_visuals_and_shield_precede_all_outcomes_and_nil_selection_still_spends_parent() {
    for outcome in 0..10 {
        let mut s = sim();
        let caster = setup(&mut s, outcome, profile());
        let target = victim(&mut s, MovementClass::Ground, Default::default());
        s.set_negative_building_shield(target, 1, None);
        s.step();
        let view = s.unit(target).unwrap();
        assert_eq!(view.health, 1000);
        assert_eq!(view.negative_building_shield_level, 0);
        assert_eq!(view.status.native_grants.iter().count(), 0);
        assert_eq!(
            s.ability_casts_last_tick()[0].target_position,
            Some(view.position)
        );
        assert_eq!(s.building(caster).unwrap().mana_current, Some(0));
        assert!(!s.world.iter_entities().any(|e| matches!(
            e.get::<NativeAction>(),
            Some(NativeAction::NativeSpellActor { .. })
        )));
    }
    let mut s = sim();
    let caster = setup(&mut s, 7, profile());
    s.step();
    assert_eq!(s.building(caster).unwrap().mana_current, Some(0));
    assert!(s.units().is_empty());
    assert_eq!(s.ability_casts_last_tick()[0].target_position, None);
}

#[test]
fn native_permanent_grants_are_idempotent_and_full_heal_remains_repeatable() {
    let mut s = sim();
    let p = profile();
    let caster = setup(&mut s, 3, p);
    let target = victim(&mut s, MovementClass::Ground, Default::default());
    s.step();
    assert_eq!(s.unit(target).unwrap().health_max, 1017);
    assert_eq!(s.unit(target).unwrap().health, 1017);
    let mut units = s.snapshot_units();
    let i = find_unit_index(&units, target).unwrap();
    units[i].health = 1;
    let b = s.snapshot_buildings()[0];
    let source = AbilitySourceSnapshot {
        source: AbilitySourceIndex::Building(0),
        id: caster,
        team: b.team,
        map_version: b.map_version,
        origin: AbilitySourceOrigin::Building(b.footprint),
        health: b.health,
        stunned_until_tick: 0,
        spellcasting: b.spellcasting,
        mana_current: b.mana_current,
        ability_state: b.ability_state,
    };
    s.apply_magic_ruin_outcome(source, PlayerId(0), p, (3, i), 1, &mut units);
    assert_eq!(units[i].health_max, 1017);
    assert_eq!(units[i].health, 1017);
    assert_eq!(units[i].status.native_grants.iter().count(), 2);
    assert_eq!(units[i].status.native_grants.attack_damage(), 9);
}

#[test]
fn native_controls_survive_dummy_removal_and_failed_orders_still_reengage_across_wire() {
    for outcome in [4, 5] {
        for immune in [false, true] {
            let mut s = sim();
            let p = profile();
            setup(&mut s, outcome, p);
            let target = victim(
                &mut s,
                MovementClass::Air,
                UnitClassifications {
                    spell_immune: immune,
                    hero: true,
                    ..Default::default()
                },
            );
            s.step();
            assert_eq!(
                s.unit(target).unwrap().hex.is_some(),
                outcome == 4 && !immune
            );
            assert_eq!(
                s.unit(target).unwrap().status.ethereal_at(0).is_some(),
                outcome == 5 && !immune
            );
            let state = s
                .world
                .iter_entities()
                .find_map(|e| {
                    e.get::<building_spells::BuildingSpellTargetState>()
                        .filter(|state| state.target == target)
                })
                .unwrap();
            assert_eq!(state.callbacks.len(), 1);
            assert_eq!(state.callbacks[0].due_tick, 2);
            let mut other = restored(&s);
            for _ in 0..9 {
                assert_eq!(s.step().checksum, other.step().checksum);
            }
            assert!(s.unit(target).unwrap().hex.is_none());
            assert!(
                s.unit(target)
                    .unwrap()
                    .status
                    .ethereal_at(s.tick())
                    .is_none()
            );
        }
    }
}

#[test]
fn stun_retargets_ground_without_a_second_shield_check_and_uses_target_point_dummy() {
    let mut s = sim();
    let p = profile();
    let caster = setup(&mut s, 6, p);
    let air = victim(&mut s, MovementClass::Air, Default::default());
    let ground = victim(&mut s, MovementClass::Ground, Default::default());
    s.set_negative_building_shield(ground, 1, None);
    let seed = (0..10_000)
        .find(|&seed| {
            deterministic_random(seed, 0, caster, RANDOM_PURPOSE_MAGIC_RUIN, 0) % 10 == 6
                && deterministic_ability_target_rank(seed, caster, p.hex_ability, 0, air)
                    < deterministic_ability_target_rank(seed, caster, p.hex_ability, 0, ground)
        })
        .unwrap();
    s = Simulation::new(
        SimulationConfig {
            match_seed: seed,
            ..s.config.clone()
        },
        1,
    );
    spawn_caster(&mut s, p);
    let air = victim(&mut s, MovementClass::Air, Default::default());
    let ground = victim(&mut s, MovementClass::Ground, Default::default());
    s.set_negative_building_shield(ground, 1, None);
    s.step();
    let actor = s
        .world
        .iter_entities()
        .find_map(|e| match e.get::<NativeAction>()? {
            NativeAction::NativeSpellActor { position, .. } => Some(*position),
            _ => None,
        })
        .unwrap();
    assert_eq!(actor, s.unit(ground).unwrap().position);
    assert_eq!(s.unit(ground).unwrap().negative_building_shield_level, 1);
    let mut other = restored(&s);
    for _ in 0..2 {
        assert_eq!(s.step().checksum, other.step().checksum);
    }
    assert_eq!(s.unit(ground).unwrap().health, 993);
    assert_eq!(s.unit(air).unwrap().health, 1000);
    assert!(s.unit(ground).unwrap().stunned_until_tick > s.tick());
    assert_eq!(s.unit(ground).unwrap().negative_building_shield_level, 1);
}

#[test]
fn mutation_replacement_is_a_fresh_owned_cold_body_without_death_or_corpse() {
    let mut s = sim();
    setup(&mut s, 7, profile());
    let target = victim(&mut s, MovementClass::Ground, Default::default());
    let facing = SimPoint::new(2, -1);
    s.world
        .get_mut::<StatusState>(entity(&s, target))
        .unwrap()
        .facing_direction = facing;
    s.step();
    assert!(s.unit(target).is_none());
    assert!(s.corpses().is_empty());
    let mutation = s
        .units()
        .into_iter()
        .find(|u| {
            u.content
                .is_some_and(|c| c.rawcode == profile().mutation_rawcode)
        })
        .unwrap();
    let cold = crate::CastleFightUnitKind::Mutation.definition().resolved();
    assert_ne!(mutation.id, target);
    assert_eq!(mutation.owner, PlayerId(0));
    assert_eq!(mutation.team, Team(0));
    assert_eq!(mutation.health_max, cold.template.health);
    assert_eq!(mutation.status.facing_direction, facing);
    assert_eq!(
        mutation.mana_current,
        cold.spellcasting.map(|s| s.mana.starting)
    );
    assert!(!mutation.classifications.summoned);
    assert_eq!(mutation.status.native_grants.iter().count(), 0);
    assert!(matches!(
        cold.spellcasting.unwrap().ability.effect,
        AbilityEffect::NativeDebuff(_)
    ));
    assert!(cold.properties.corpse.is_none());
    let mut other = restored(&s);
    for _ in 0..4 {
        assert_eq!(s.step().checksum, other.step().checksum);
    }
}

#[test]
fn banish_expiry_does_not_cancel_defender_followups_or_disable_native_defend() {
    let mut s = sim();
    let p = profile();
    setup(&mut s, 5, p);
    let mut definition = crate::CastleFightUnitKind::Defender.definition().resolved();
    let mut defend = definition
        .properties
        .passive_effects
        .iter()
        .find_map(|effect| {
            if let PassiveUnitEffect::Defend(profile) = effect {
                Some(profile)
            } else {
                None
            }
        })
        .unwrap();
    defend.activation_delay_ticks = 0;
    definition.properties.passive_effects =
        PassiveUnitEffects::single(PassiveUnitEffect::Defend(defend));
    definition.template.attack.damage = 0;
    definition.template.attack.range = 0;
    definition.template.attack.acquisition_range = 0;
    definition.template.movement.speed_per_tick = 0;
    assert_eq!(
        definition.properties.content.unwrap().rawcode,
        p.hex.defender_rawcode
    );
    let target = s.spawn_resolved_unit_unchecked(
        Some(PlayerId(1)),
        UnitSpawn::from_template(
            Team(1),
            SimPoint::new(50 * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT),
            definition.template,
        ),
        definition,
    );
    let defend = s.unit(target).unwrap().active_defend_ability;
    assert!(defend.is_some());
    s.step();
    let mut other = restored(&s);
    for tick in 1..=6 {
        assert_eq!(s.step().checksum, other.step().checksum);
        let state = s
            .world
            .iter_entities()
            .find_map(|e| {
                e.get::<building_spells::BuildingSpellTargetState>()
                    .filter(|state| state.target == target)
            })
            .unwrap();
        assert!(!state.defend_disabled);
        assert!(state.hex.is_none());
        assert_eq!(s.unit(target).unwrap().active_defend_ability, defend);
        if tick == 2 {
            assert_eq!(state.callbacks[0].due_tick, 5);
        }
        if tick == 5 {
            assert!(state.orders_suspended);
            assert_eq!(state.callbacks[0].due_tick, 6);
            assert!(s.unit(target).unwrap().status.ethereal_at(tick).is_none());
        }
        if tick == 6 {
            assert!(state.callbacks.is_empty());
            assert!(!state.orders_suspended);
        }
    }
}

#[test]
fn callback_selector_excludes_allies_locust_invulnerable_nonsappers_and_outside_units() {
    for case in 0..5 {
        let mut s = sim();
        let caster = setup(&mut s, 3, profile());
        let target = victim(&mut s, MovementClass::Ground, Default::default());
        let entity = entity(&s, target);
        match case {
            0 => {
                s.world
                    .entity_mut(entity)
                    .insert((Team(0), Owner(PlayerId(0))));
            }
            1 => {
                s.world
                    .get_mut::<UnitClassifications>(entity)
                    .unwrap()
                    .locust = true;
            }
            2 => {
                s.world
                    .get_mut::<UnitClassifications>(entity)
                    .unwrap()
                    .invulnerable = true;
            }
            3 => {
                s.world
                    .get_mut::<UnitClassifications>(entity)
                    .unwrap()
                    .combat_sapper = false;
            }
            4 => {
                s.world.entity_mut(entity).insert(Position(SimPoint::new(
                    crate::building_mechanics::snowveil_for_version(VERSION).battlefield_bounds[0]
                        - 1,
                    0,
                )));
            }
            _ => unreachable!(),
        }
        s.step();
        assert_eq!(
            s.unit(target).unwrap().health_max,
            1000,
            "selector case {case}"
        );
        assert_eq!(
            s.ability_casts_last_tick()[0].target_position,
            None,
            "selector case {case}"
        );
        assert_eq!(s.building(caster).unwrap().mana_current, Some(0));
    }
}
