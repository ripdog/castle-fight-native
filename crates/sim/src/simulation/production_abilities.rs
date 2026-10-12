//! Synthetic integration regressions for future production definitions, not entity tuning.
use super::*;

fn ability(id: u32) -> AutomaticAbilityProfile {
    AutomaticAbilityProfile {
        id: AbilityId(id),
        mana_cost: 1,
        cooldown_ticks: 3 + id as u16,
        range: 2000 * SUBUNITS_PER_WORLD_UNIT,
        target_policy: AbilityTargetPolicy::RandomEnemyUnit,
        effect: AbilityEffect::Damage { amount: 1 },
    }
}

fn factory(additional: &[AutomaticAbilityProfile]) -> (BuildingSpawn, BuildingGameplayProperties) {
    let template = crate::components::UnitTemplate {
        health: 1000,
        attack: AttackProfile {
            damage: 0,
            range: 0,
            acquisition_range: 0,
            cooldown_ticks: 100,
            delivery: AttackDelivery::Melee,
        },
        movement: MovementProfile { speed_per_tick: 0 },
    };
    let spawn = BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(0, 0, 1, 1),
        health: 1000,
        production: Some(ProductionProfile {
            unit: template,
            interval_ticks: 4,
            initial_delay_ticks: 4,
            search_radius_cells: 12,
        }),
        attack: None,
        spellcasting: None,
    };
    let properties = BuildingGameplayProperties {
        economy: Some(BuildingEconomyProfile {
            gold_cost: 0,
            lumber_cost: 0,
            lumber_refund: 0,
            legendary_points_cost: 0,
            income_per_10k: 0,
        }),
        production_spellcasting: Some(SpellcastingProfile {
            mana: crate::components::ManaProfile {
                maximum: 100,
                starting: 100,
                regen_per_tick_per_10k: 0,
            },
            ability: ability(30),
        }),
        production_additional_abilities: Some(
            AdditionalAutomaticAbilityDefinitions::try_from_profiles(additional.iter().copied())
                .unwrap(),
        ),
        ..BuildingGameplayProperties::default()
    };
    (spawn, properties)
}

fn simulation(workers: usize) -> Simulation {
    Simulation::new(SimulationConfig::default(), workers)
}

fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}

fn restored(source: &Simulation, workers: usize) -> Simulation {
    let content = crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&source.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut result = simulation(workers);
    result.restore_snapshot(&snapshot).unwrap();
    result
}

#[test]
fn registered_training_variants_replace_handles_with_fresh_bodies_and_clone_without_random_grants()
{
    let version = crate::MapVersion::CASTLE_FIGHT_9_27;
    let bundle = crate::castle_fight_content_bundle(version).unwrap();
    let profile = crate::building_mechanics::training_variants_for_version(version);
    let shrine = crate::CastleFightTowerKind::HeroicShrine
        .definition_for_version(version)
        .unwrap();
    for root in &profile.roots {
        let original = bundle
            .unit_definitions()
            .find(|unit| unit.rawcode == *root)
            .unwrap();
        let mut saw_clone = false;
        let mut saw_grants = false;
        for seed in 0..48 {
            let mut sim = Simulation::new_with_gameplay_bundle_and_players(
                SimulationConfig {
                    match_seed: seed,
                    ..Default::default()
                },
                1,
                CombatRules::default(),
                bundle.identity.into(),
                &[
                    PlayerConfig {
                        id: PlayerId(0),
                        team: Team(0),
                    },
                    PlayerConfig {
                        id: PlayerId(1),
                        team: Team(0),
                    },
                ],
            );
            sim.spawn_building_for_player_with_properties(
                PlayerId(1),
                shrine.spawn(Team(0), BuildingFootprint::new(20, 0, 4, 4)),
                shrine.gameplay_properties(),
            );
            let (mut spawn, mut properties) = factory(&[ability(10)]);
            properties.production_unit.content = original.gameplay_properties().content;
            spawn.production.as_mut().unwrap().initial_delay_ticks = 0;
            let producer =
                sim.spawn_building_for_player_with_properties(PlayerId(0), spawn, properties);
            sim.step();
            let units = sim.units();
            assert!(
                !units
                    .iter()
                    .any(|unit| unit.content.is_some_and(|content| content.rawcode == *root))
            );
            let trained = units.iter().find(|unit| unit.owner == PlayerId(0)).unwrap();
            let rawcode = trained.content.unwrap().rawcode;
            assert!(profile.bodies().any(|body| body == rawcode));
            let cold = bundle
                .unit_definitions()
                .find(|body| body.rawcode == rawcode)
                .unwrap();
            assert_eq!(trained.health, cold.health);
            assert_eq!(trained.status.spawn_building, Some(producer));
            assert!(trained.id.0 > producer.0 + 1);
            let trained_entity = entity(&sim, trained.id);
            let passives = sim.world.get::<PassiveUnitEffects>(trained_entity).unwrap();
            saw_grants |= passives.iter().next().is_some()
                || trained.status.native_grants.iter().next().is_some();
            for clone in units.iter().filter(|unit| unit.owner == PlayerId(1)) {
                saw_clone = true;
                assert_eq!(clone.content.unwrap().rawcode, rawcode);
                assert_eq!(clone.health, cold.health);
                assert_eq!(
                    *sim.world
                        .get::<PassiveUnitEffects>(entity(&sim, clone.id))
                        .unwrap(),
                    cold.passive_effects
                );
                assert_eq!(
                    sim.world
                        .get::<SpellcastingProfile>(entity(&sim, clone.id))
                        .copied(),
                    cold.spellcasting
                );
                assert!(clone.status.native_grants.iter().next().is_none());
                assert_eq!(clone.status.spawn_building, Some(producer));
            }
            let snapshot = SimulationSnapshot::decode_wire(
                &sim.capture_snapshot().encode_wire().unwrap(),
                bundle,
            )
            .unwrap();
            let mut other = Simulation::new_with_gameplay_bundle_and_players(
                SimulationConfig {
                    match_seed: seed,
                    ..Default::default()
                },
                4,
                CombatRules::default(),
                bundle.identity.into(),
                &[
                    PlayerConfig {
                        id: PlayerId(0),
                        team: Team(0),
                    },
                    PlayerConfig {
                        id: PlayerId(1),
                        team: Team(0),
                    },
                ],
            );
            other.restore_snapshot(&snapshot).unwrap();
            for _ in 0..2 {
                assert_eq!(sim.step().checksum, other.step().checksum);
            }
            if saw_clone && saw_grants {
                break;
            }
        }
        assert!(saw_clone && saw_grants);
    }
}

#[test]
fn source_marked_training_copies_initialize_complete_children_and_restore_across_workers() {
    let version = crate::MapVersion::CASTLE_FIGHT_9_27;
    let replication = crate::building_mechanics::production_replication_for_version(version);
    let marked = crate::CastleFightUnitKind::ALL
        .into_iter()
        .filter(|kind| {
            crate::content::unit_has_ability_for_version(
                kind.definition_for_version(version).unwrap().rawcode,
                AbilityId(replication.marker),
                version,
            )
        })
        .collect::<Vec<_>>();
    assert!(!marked.is_empty());
    let additional = [ability(10), ability(20)];
    for kind in marked {
        let (spawn, mut properties) = factory(&additional);
        properties.production_unit.content = kind
            .definition_for_version(version)
            .unwrap()
            .gameplay_properties()
            .content;
        let mut first = simulation(1);
        first.spawn_building_with_properties(spawn, properties);
        first.step();
        first.step();
        let mut second = restored(&first, 4);
        for _ in 0..3 {
            let a = first.step();
            let b = second.step();
            assert_eq!(a.units_spawned, b.units_spawned);
            assert_eq!(first.checksum(), second.checksum());
        }
        assert_eq!(first.units().len(), replication.local_copies(true));
        assert_spawned_definitions(&first, &additional);
        let ids = first.units().into_iter().map(|u| u.id).collect::<Vec<_>>();
        assert_ne!(ids[0], ids[1]);
        for id in ids {
            let entity = entity(&first, id);
            assert_eq!(first.world.get::<Owner>(entity).unwrap().0, PlayerId(0));
            assert_eq!(first.world.get::<ManaState>(entity).unwrap().current, 100);
        }
    }
}

#[test]
fn marked_shrine_rolls_create_source_owned_pairs_without_recursive_training() {
    let version = crate::MapVersion::CASTLE_FIGHT_9_27;
    let replication = crate::building_mechanics::production_replication_for_version(version);
    let body = crate::CastleFightUnitKind::ALL
        .into_iter()
        .find(|kind| {
            crate::content::unit_has_ability_for_version(
                kind.definition_for_version(version).unwrap().rawcode,
                AbilityId(replication.marker),
                version,
            )
        })
        .unwrap()
        .definition_for_version(version)
        .unwrap();
    let shrine = crate::CastleFightTowerKind::HeroicShrine
        .definition_for_version(version)
        .unwrap();
    let mut observed_success = false;
    let mut observed_failure = false;
    for seed in 0..128 {
        let mut sim = Simulation::new_with_gameplay_bundle_and_players(
            SimulationConfig {
                match_seed: seed,
                ..SimulationConfig::default()
            },
            1,
            CombatRules::default(),
            crate::castle_fight_content_bundle(version)
                .unwrap()
                .identity
                .into(),
            &[
                PlayerConfig {
                    id: PlayerId(0),
                    team: Team(0),
                },
                PlayerConfig {
                    id: PlayerId(1),
                    team: Team(0),
                },
                PlayerConfig {
                    id: PlayerId(2),
                    team: Team(1),
                },
            ],
        );
        sim.spawn_building_for_player_with_properties(
            PlayerId(1),
            shrine.spawn(Team(0), BuildingFootprint::new(20, 0, 4, 4)),
            shrine.gameplay_properties(),
        );
        let (mut spawn, mut properties) = factory(&[ability(10)]);
        spawn.production.as_mut().unwrap().initial_delay_ticks = 0;
        properties.production_unit.content = body.gameplay_properties().content;
        sim.spawn_building_for_player_with_properties(PlayerId(0), spawn, properties);
        let count = sim.step().units_spawned;
        let local = replication.local_copies(true);
        if count == local {
            observed_failure = true;
        } else {
            assert_eq!(count, local + replication.shrine_copies(true));
            observed_success = true;
        }
        let (mut local_owned, mut shrine_owned) = (0, 0);
        for unit in sim.units() {
            match sim.world.get::<Owner>(entity(&sim, unit.id)).unwrap().0 {
                PlayerId(0) => local_owned += 1,
                PlayerId(1) => shrine_owned += 1,
                other => panic!("unexpected clone owner {other:?}"),
            }
        }
        assert_eq!(local_owned, local);
        assert_eq!(shrine_owned, count - local);
        if observed_success && observed_failure {
            break;
        }
    }
    assert!(observed_success && observed_failure);
}

fn assert_spawned_definitions(sim: &Simulation, expected: &[AutomaticAbilityProfile]) {
    let units = sim
        .world
        .iter_entities()
        .filter(|entity| entity.get::<MovementProfile>().is_some())
        .collect::<Vec<_>>();
    assert!(
        !units.is_empty(),
        "the test must observe at least one production spawn"
    );
    for entity in units {
        let definitions = entity.get::<AdditionalAutomaticAbilities>().unwrap();
        assert_eq!(
            definitions
                .iter()
                .map(|entry| entry.profile)
                .collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            entity.get::<SpellcastingProfile>().unwrap().ability,
            ability(30)
        );
        assert_eq!(
            entity
                .get::<ResurrectionProfile>()
                .unwrap()
                .0
                .additional_abilities
                .unwrap()
                .iter()
                .collect::<Vec<_>>(),
            expected
        );
    }
}

fn companion_carrier() -> crate::CastleFightUnitDefinition {
    let version = crate::MapVersion::CASTLE_FIGHT_9_27;
    let p = crate::building_mechanics::production_replication_for_version(version);
    crate::castle_fight_content_bundle(version)
        .unwrap()
        .unit_definitions()
        .find(|unit| {
            crate::content::unit_has_ability_for_version(
                unit.rawcode,
                AbilityId(p.twins_marker),
                version,
            ) && !crate::content::unit_has_ability_for_version(
                unit.rawcode,
                AbilityId(p.legendary_exclusion),
                version,
            )
        })
        .expect("source-marked training carrier")
}

#[test]
fn source_training_companions_receive_fresh_definitions_and_one_producer_cycle_across_wire() {
    let body = companion_carrier();
    let p = crate::building_mechanics::production_replication_for_version(body.map_version);
    let (spawn, mut properties) = factory(&[ability(10)]);
    properties.production_unit.content = body.gameplay_properties().content;
    let mut first = simulation(1);
    let producer = first.spawn_building_with_properties(spawn, properties);
    first.step();
    first.step();
    let mut second = restored(&first, 4);
    for _ in 0..3 {
        assert_eq!(first.step().checksum, second.step().checksum);
    }
    assert_eq!(first.units().len(), 1 + p.twins_children.len());
    let bundle = crate::castle_fight_content_bundle(body.map_version).unwrap();
    for rawcode in p.twins_children {
        let cold = bundle
            .unit_definitions()
            .find(|unit| unit.rawcode == rawcode)
            .unwrap();
        let child = first
            .units()
            .into_iter()
            .find(|u| u.content.is_some_and(|c| c.rawcode == rawcode))
            .unwrap();
        let e = entity(&first, child.id);
        assert_eq!(child.health, cold.health);
        assert_eq!(child.owner, PlayerId(0));
        assert_eq!(child.status.spawn_building, Some(producer));
        assert!(child.status.native_grants.iter().next().is_none());
        assert_eq!(
            first.world.get::<ResurrectionProfile>(e).unwrap().0,
            cold.resolved()
        );
        assert_eq!(
            first.world.get::<ManaState>(e).map(|m| m.current),
            cold.spellcasting.map(|s| s.mana.starting)
        );
    }
    let carrier = first
        .units()
        .into_iter()
        .find(|u| u.content.is_some_and(|c| c.rawcode == body.rawcode))
        .unwrap();
    assert_eq!(carrier.health, 1000);
    assert_eq!(carrier.mana_current, Some(100));
    assert_eq!(
        first
            .world
            .get::<ProductionState>(entity(&first, producer))
            .unwrap()
            .next_spawn_tick,
        8
    );
}

#[test]
fn shrine_clones_of_training_carriers_do_not_reenter_native_training_or_create_companions() {
    let body = companion_carrier();
    let p = crate::building_mechanics::production_replication_for_version(body.map_version);
    let shrine = crate::CastleFightTowerKind::HeroicShrine
        .definition_for_version(body.map_version)
        .unwrap();
    let mut outcomes = [false; 2];
    for seed in 0..128 {
        let mut sim = Simulation::new_with_gameplay_bundle_and_players(
            SimulationConfig {
                match_seed: seed,
                ..Default::default()
            },
            1,
            CombatRules::default(),
            crate::castle_fight_content_bundle(body.map_version)
                .unwrap()
                .identity
                .into(),
            &[
                PlayerConfig {
                    id: PlayerId(0),
                    team: Team(0),
                },
                PlayerConfig {
                    id: PlayerId(1),
                    team: Team(0),
                },
            ],
        );
        sim.spawn_building_for_player_with_properties(
            PlayerId(1),
            shrine.spawn(Team(0), BuildingFootprint::new(20, 0, 4, 4)),
            shrine.gameplay_properties(),
        );
        let (mut spawn, mut properties) = factory(&[]);
        properties.production_unit.content = body.gameplay_properties().content;
        spawn.production.as_mut().unwrap().initial_delay_ticks = 0;
        sim.spawn_building_for_player_with_properties(PlayerId(0), spawn, properties);
        let count = sim.step().units_spawned;
        let local = 1 + p.twins_children.len();
        let clones = count - local;
        assert!(clones <= 1);
        outcomes[clones] = true;
        assert_eq!(
            sim.units()
                .iter()
                .filter(|u| u.owner == PlayerId(1))
                .count(),
            clones
        );
        for rawcode in p.twins_children {
            assert_eq!(
                sim.units()
                    .iter()
                    .filter(|u| u.content.is_some_and(|c| c.rawcode == rawcode))
                    .count(),
                1
            );
        }
        if outcomes.into_iter().all(|seen| seen) {
            break;
        }
    }
    assert!(outcomes.into_iter().all(|seen| seen));
}

#[test]
fn failed_training_search_creates_no_carrier_or_companions_and_advances_one_cycle() {
    let body = companion_carrier();
    let mut sim = simulation(1);
    let (mut spawn, mut properties) = factory(&[]);
    properties.production_unit.content = body.gameplay_properties().content;
    let p = spawn.production.as_mut().unwrap();
    p.search_radius_cells = 0;
    p.initial_delay_ticks = 0;
    let blocked = preferred_spawn_cell(spawn.team, spawn.footprint);
    sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(blocked.x, blocked.y, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    let producer = sim.spawn_building_with_properties(spawn, properties);
    let result = sim.step();
    assert_eq!(result.units_spawned, 0);
    assert!(sim.units().is_empty());
    assert_eq!(
        sim.world
            .get::<ProductionState>(entity(&sim, producer))
            .unwrap()
            .next_spawn_tick,
        4
    );
}

#[test]
fn native_locust_training_carriers_ignore_unit_occupancy() {
    let body = companion_carrier();
    let mut sim = simulation(1);
    let (mut spawn, mut properties) = factory(&[]);
    spawn.footprint = BuildingFootprint::new(40, 20, 1, 1);
    properties.production_unit.content = body.gameplay_properties().content;
    properties.production_unit.classifications.locust = true;
    properties.production_unit.collision_radius = Some(CollisionRadius(0));
    properties.production_unit.movement_class = MovementClass::Air;
    let p = spawn.production.as_mut().unwrap();
    p.search_radius_cells = 0;
    p.initial_delay_ticks = 0;
    let preferred = preferred_spawn_cell(spawn.team, spawn.footprint);
    let template = spawn.production.unwrap().unit;
    sim.spawn_unit_with_properties(
        UnitSpawn::from_template(Team(1), sim.topology.center_of_cell(preferred), template),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            collision_radius: Some(CollisionRadius(16 * SUBUNITS_PER_WORLD_UNIT)),
            ..Default::default()
        },
    );
    sim.spawn_building_with_properties(spawn, properties);
    assert_eq!(sim.step().units_spawned, 3);
}

#[test]
fn unspawned_production_definitions_survive_wire_restore_and_initialize_every_child() {
    let additional = [ability(10), ability(20)];
    let (spawn, mut properties) = factory(&additional);
    properties.production_unit.classifications = UnitClassifications {
        hero: true,
        summoned: true,
        spell_immune: true,
        ..UnitClassifications::default()
    };
    let mut first = simulation(1);
    first.spawn_building_with_properties(spawn, properties);
    for _ in 0..2 {
        first.step();
    }
    assert!(first.units().is_empty());
    let mut second = restored(&first, 4);
    let mut newborns = 0;
    for _ in 0..15 {
        let tick = first.tick();
        let first_step = first.step();
        let second_step = second.step();
        assert_eq!(first.checksum(), second.checksum());
        for entity in first.world.iter_entities().filter(|entity| {
            entity.get::<SpawnTick>().is_some_and(|born| born.0 == tick)
                && entity.get::<MovementProfile>().is_some()
        }) {
            newborns += 1;
            assert_eq!(
                entity.get::<UnitClassifications>(),
                Some(&properties.production_unit.classifications)
            );
            let slots = entity.get::<AdditionalAutomaticAbilities>().unwrap();
            for slot in slots.iter() {
                // No hostile targets: each new child must start with its own unspent state.
                assert_eq!(slot.state.ready_tick, tick);
                assert_eq!(slot.state.cast_sequence, 0);
                assert_eq!(
                    slot.secondary_resurrection,
                    SecondaryResurrectionState::default()
                );
            }
            assert_eq!(
                entity.get::<ManaState>().unwrap().current,
                properties.production_spellcasting.unwrap().mana.starting
            );
        }
        assert_eq!(first_step.units_spawned, second_step.units_spawned);
    }
    assert!(newborns >= 2);
    assert_spawned_definitions(&first, &additional);
    assert_spawned_definitions(&second, &additional);
}

#[test]
fn production_definitions_affect_checksum_before_any_child_exists_and_invalid_alias_is_atomic() {
    let (spawn, properties) = factory(&[ability(10)]);
    let mut first = simulation(1);
    first.spawn_building_with_properties(spawn, properties);
    let (_, changed) = factory(&[AutomaticAbilityProfile {
        cooldown_ticks: 1,
        ..ability(10)
    }]);
    let mut second = simulation(1);
    second.spawn_building_with_properties(spawn, changed);
    assert!(first.units().is_empty() && second.units().is_empty());
    assert_ne!(first.checksum(), second.checksum());
    let mut empty = simulation(1);
    let before = empty.checksum();
    let invalid = BuildingGameplayProperties {
        production_additional_abilities: Some(
            AdditionalAutomaticAbilityDefinitions::try_from_profiles([ability(30)]).unwrap(),
        ),
        ..properties
    };
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(
            || empty.spawn_building_with_properties(spawn, invalid)
        ))
        .is_err()
    );
    assert_eq!(
        before,
        empty.checksum(),
        "rejection must not allocate a gameplay ID or create a shell"
    );
}

#[test]
fn pending_upgrade_preserves_precursor_definitions_on_cancel_and_replaces_them_on_completion() {
    let original = [ability(10), ability(20)];
    let replacement = [ability(40)];
    let (source, source_properties) = factory(&original);
    let (target, mut target_properties) = factory(&replacement);
    target_properties.construction_time_ticks = Some(4);
    let mut pending = simulation(1);
    let id = pending.spawn_building_with_properties(source, source_properties);
    for _ in 0..2 {
        pending
            .cancel_production_unit_for_player(PlayerId(0), id)
            .unwrap();
    }
    pending
        .start_building_upgrade(id, source, source_properties, target, target_properties)
        .unwrap();
    pending.step();
    let mut cancelled = restored(&pending, 2);
    let mut completed = restored(&pending, 4);
    cancelled.cancel_building_construction(Team(0), id).unwrap();
    let source_entity = cancelled
        .world
        .iter_entities()
        .find(|entity| entity.get::<SimId>() == Some(&id))
        .unwrap();
    assert_eq!(
        source_entity
            .get::<ProductionAdditionalAutomaticAbilities>()
            .unwrap()
            .0
            .iter()
            .collect::<Vec<_>>(),
        original
    );
    // The saved empty queue is restored, so explicitly resume it through the ordinary API.
    cancelled
        .queue_production_unit_for_player(PlayerId(0), id)
        .unwrap();
    for _ in 0..12 {
        cancelled.step();
        completed.step();
        pending.step();
        assert_eq!(completed.checksum(), pending.checksum());
    }
    assert_spawned_definitions(&cancelled, &original);
    assert_spawned_definitions(&completed, &replacement);
    let entity = completed
        .world
        .iter_entities()
        .find(|entity| entity.get::<SimId>() == Some(&id))
        .unwrap();
    assert_eq!(
        entity
            .get::<ProductionAdditionalAutomaticAbilities>()
            .unwrap()
            .0
            .iter()
            .collect::<Vec<_>>(),
        replacement
    );
}

#[test]
fn fractional_building_regeneration_survives_pending_upgrade_restore_and_cancellation() {
    let (source, mut source_properties) = factory(&[]);
    source_properties.health_regen_per_second_per_10k = 75_000;
    let (target, mut target_properties) = factory(&[]);
    target_properties.construction_time_ticks = Some(4);
    target_properties.health_regen_per_second_per_10k = 125_000;
    let mut pending = simulation(1);
    let id = pending.spawn_building_with_properties(source, source_properties);
    let entity = pending
        .world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id();
    pending.world.get_mut::<Health>(entity).unwrap().current = 900;
    pending.step();
    let before = *pending.world.get::<HealthRegeneration>(entity).unwrap();
    assert!(before.remainder_per_10k_hz > 0);
    for _ in 0..2 {
        pending
            .cancel_production_unit_for_player(PlayerId(0), id)
            .unwrap();
    }
    pending
        .start_building_upgrade(id, source, source_properties, target, target_properties)
        .unwrap();
    let mut other = restored(&pending, 4);
    pending.cancel_building_construction(Team(0), id).unwrap();
    other.cancel_building_construction(Team(0), id).unwrap();
    assert_eq!(
        *pending.world.get::<HealthRegeneration>(entity).unwrap(),
        before
    );
    for _ in 0..8 {
        assert_eq!(pending.step().checksum, other.step().checksum);
    }
    assert!(pending.building(id).unwrap().health > 900);
}
