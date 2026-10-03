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

fn restored(source: &Simulation, workers: usize) -> Simulation {
    let content = crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&source.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut result = simulation(workers);
    result.restore_snapshot(&snapshot).unwrap();
    result
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

#[test]
fn unspawned_production_definitions_survive_wire_restore_and_initialize_every_child() {
    let additional = [ability(10), ability(20)];
    let (spawn, properties) = factory(&additional);
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
