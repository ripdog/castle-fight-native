use super::*;
mod baseline;
mod lifecycle;
use crate::{CastleFightTowerKind, MapVersion, castle_fight_content_bundle};

fn simulation(workers: usize, seed: u64) -> Simulation {
    let mut config = SimulationConfig {
        match_seed: seed,
        ..SimulationConfig::default()
    };
    config.unit_separation_distance = 0;
    Simulation::new(config, workers)
}
fn shrine(sim: &mut Simulation, team: Team, x: i32) -> SimId {
    let definition = CastleFightTowerKind::GoldenShrineOfJustice.definition();
    // Direct authored spawns bypass purchasing; synthetic fixtures carry no point allocation.
    let mut properties = definition.gameplay_properties();
    properties.economy = None;
    sim.spawn_building_with_properties(
        definition.spawn(team, BuildingFootprint::new(x, 20, 4, 4)),
        properties,
    )
}
fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|entity| entity.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}
fn definition() -> ResolvedUnitDefinition {
    ResolvedUnitDefinition {
        template: crate::UnitTemplate {
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 10,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        properties: UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                ..UnitClassifications::default()
            },
            corpse: Some(CorpseProfile {
                definition: CorpseDefinitionId(123),
                decay_start_ticks: 0,
                lifetime_ticks: None,
            }),
            ..UnitGameplayProperties::default()
        },
        spellcasting: None,
        additional_abilities: None,
    }
}
fn unit(sim: &mut Simulation, definition: ResolvedUnitDefinition) -> SimId {
    sim.spawn_resolved_unit_unchecked(
        Some(PlayerId(0)),
        UnitSpawn::from_template(Team(0), SimPoint::new(50, 11), definition.template),
        definition,
    )
}
fn fatality(sim: &mut Simulation, id: SimId) {
    let entity = entity(sim, id);
    let position = sim.world.get::<Position>(entity).unwrap().0;
    sim.world.get_mut::<Health>(entity).unwrap().current = 0;
    let chance = sim.golden_shrine_revive_chance(Team(0));
    let state = sim.schedule_shrine_revival(entity, PlayerId(0), Team(0), position, 0, chance);
    let definition = sim.world.get::<ResurrectionProfile>(entity).unwrap().0;
    sim.world.despawn(entity);
    let corpse = sim.allocate_id();
    sim.world.spawn((
        corpse,
        Position(position),
        Corpse {
            source_unit: id,
            source_owner: PlayerId(0),
            source_team: Team(0),
            definition: CorpseDefinitionId(123),
            created_tick: sim.next_tick,
            decay_start_tick: sim.next_tick,
            expires_tick: None,
            resurrection: Some(definition),
            shrine_state: state,
        },
    ));
}
fn pending(sim: &Simulation) -> usize {
    sim.world
        .iter_entities()
        .filter(|entity| entity.get::<DelayedShrineRevival>().is_some())
        .count()
}
fn ready_sim() -> Simulation {
    // Pick a deterministic success seed rather than weakening the source-derived proc chance.
    for seed in 0..100 {
        let mut sim = simulation(1, seed);
        shrine(&mut sim, Team(0), 20);
        let id = unit(&mut sim, definition());
        fatality(&mut sim, id);
        if pending(&sim) == 1 {
            return sim;
        }
    }
    panic!("no deterministic success seed");
}
fn restored(sim: &Simulation) -> Simulation {
    let bundle = castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let wire = sim.capture_snapshot().encode_wire().unwrap();
    let snapshot = SimulationSnapshot::decode_wire(&wire, bundle).unwrap();
    let players: Vec<_> = sim
        .players
        .iter()
        .map(|player| PlayerConfig {
            id: player.id,
            team: player.team,
        })
        .collect();
    let mut restored = Simulation::new_internal(
        sim.config.clone(),
        4,
        sim.combat_rules.clone(),
        None,
        &players,
    );
    restored.restore_snapshot(&snapshot).unwrap();
    restored
}

#[test]
fn team_stacks_lifecycle_and_cap_are_derived_from_finished_canonical_buildings() {
    let mut sim = simulation(1, 0);
    let per = sim.shrine_definition().parameters.chance_percent_per_shrine;
    let cap = sim
        .shrine_definition()
        .parameters
        .maximum_effective_chance_percent;
    let first = shrine(&mut sim, Team(0), 20);
    let second = shrine(&mut sim, Team(0), 30);
    let third = shrine(&mut sim, Team(0), 40);
    assert_eq!(sim.golden_shrine_revive_chance(Team(0)), cap);
    assert!(sim.transfer_golden_shrine_owner(first, PlayerId(1)));
    assert!(sim.transfer_golden_shrine_owner(first, PlayerId(1))); // Idempotent owner event.
    assert_eq!(sim.golden_shrine_revive_chance(Team(1)), per);
    assert!(sim.remove_building(second));
    assert_eq!(sim.golden_shrine_revive_chance(Team(0)), per);
    sim.world
        .get_mut::<Health>(entity(&sim, third))
        .unwrap()
        .current = 0;
    assert_eq!(sim.golden_shrine_revive_chance(Team(0)), 0);
    let restored = restored(&sim);
    assert_eq!(restored.golden_shrine_revive_chance(Team(1)), per);
}

#[test]
fn delayed_replacement_is_exact_fresh_one_time_and_wire_restorable() {
    let mut sim = ready_sim();
    let delay =
        sim.shrine_definition().parameters.revive_delay_seconds * CASTLE_FIGHT_SIMULATION_HZ as u64;
    let mut restored = restored(&sim);
    for tick in 0..=delay {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
        assert_eq!(
            sim.shrine_revivals_last_tick(),
            restored.shrine_revivals_last_tick()
        );
        if tick < delay {
            assert!(sim.units().is_empty());
            assert_eq!(sim.corpse_count(), 1);
        }
    }
    let units = sim.units();
    assert_eq!(units.len(), 1);
    let replacement = &units[0];
    assert_eq!(replacement.owner, PlayerId(0));
    assert_eq!(replacement.position, SimPoint::new(50, 11));
    assert_eq!(replacement.health, 100);
    assert_eq!(sim.corpse_count(), 0);
    assert_eq!(
        sim.shrine_revivals_last_tick()[0].model_path,
        sim.shrine_definition().resurrection_model
    );
    let id = replacement.id;
    assert!(
        sim.world
            .get::<ShrineRevivalState>(entity(&sim, id))
            .unwrap()
            .revived
    );
    let mut again = self::restored(&sim);
    fatality(&mut sim, id);
    fatality(&mut again, id);
    assert_eq!(pending(&sim), 0);
    assert_eq!(sim.checksum(), again.checksum());
}

#[test]
fn generation_is_round_wide_not_per_death_and_consumed_corpses_do_not_cancel_callbacks() {
    let mut sim = ready_sim();
    // Additional deaths never increment A7. Corpse consumption/removal does not gate the callback.
    let id = unit(&mut sim, definition());
    fatality(&mut sim, id);
    let corpses: Vec<_> = sim
        .world
        .iter_entities()
        .filter(|entity| entity.get::<Corpse>().is_some())
        .map(|entity| entity.id())
        .collect();
    for entity in corpses {
        sim.world.despawn(entity);
    }
    let count = pending(&sim);
    let mut restored = restored(&sim);
    let delay =
        sim.shrine_definition().parameters.revive_delay_seconds * CASTLE_FIGHT_SIMULATION_HZ as u64;
    for _ in 0..=delay {
        sim.step();
        restored.step();
    }
    assert_eq!(sim.unit_count(), count);
    assert_eq!(sim.checksum(), restored.checksum());

    let mut cancelled = ready_sim();
    cancelled.invalidate_pending_golden_shrine_revivals();
    let mut restored = self::restored(&cancelled);
    for _ in 0..=delay {
        cancelled.step();
        restored.step();
    }
    assert_eq!(cancelled.unit_count(), 0);
    assert_eq!(pending(&cancelled), 0);
    assert_eq!(cancelled.corpse_count(), 1);
    assert!(cancelled.shrine_revivals_last_tick().is_empty());
    assert_eq!(cancelled.checksum(), restored.checksum());
}

#[test]
fn exclusion_flags_no_shrine_and_no_killer_are_negative_cases() {
    for flags in [
        UnitClassifications::default(),
        UnitClassifications {
            combat_sapper: true,
            legendary: true,
            ..UnitClassifications::default()
        },
        UnitClassifications {
            combat_sapper: true,
            summoned: true,
            ..UnitClassifications::default()
        },
        UnitClassifications {
            combat_sapper: true,
            summoned_marker: true,
            ..UnitClassifications::default()
        },
        UnitClassifications {
            combat_sapper: true,
            illusion: true,
            ..UnitClassifications::default()
        },
    ] {
        for seed in 0..20 {
            let mut sim = simulation(1, seed);
            shrine(&mut sim, Team(0), 20);
            let mut definition = definition();
            definition.properties.classifications = flags;
            let id = unit(&mut sim, definition);
            fatality(&mut sim, id);
            assert_eq!(pending(&sim), 0);
        }
    }
    let mut sim = simulation(1, 0);
    shrine(&mut sim, Team(1), 20);
    let id = unit(&mut sim, definition());
    fatality(&mut sim, id);
    assert_eq!(pending(&sim), 0);
    let mut sim = simulation(1, 0);
    shrine(&mut sim, Team(0), 20);
    unit(&mut sim, definition());
    sim.debug_damage_all_units(1_000);
    assert_eq!(pending(&sim), 0);
    let success = ready_sim();
    let mut sim = simulation(1, success.config.match_seed);
    shrine(&mut sim, Team(0), 20);
    let id = unit(&mut sim, definition());
    sim.world
        .get_mut::<Health>(entity(&sim, id))
        .unwrap()
        .current = 0;
    sim.step(); // An externally dead handle has no combat killer.
    assert_eq!(pending(&sim), 0);
}

#[test]
fn suppression_is_one_shot_and_survives_native_resurrection_and_wire_restore() {
    let mut sim = ready_sim();
    // Use the same successful death identity/tick/seed in a separate fixture.
    let source = sim
        .world
        .iter_entities()
        .find_map(|entity| entity.get::<DelayedShrineRevival>().copied())
        .unwrap();
    let mut fresh = simulation(1, sim.config.match_seed);
    shrine(&mut fresh, Team(0), 20);
    let id = unit(&mut fresh, definition());
    assert_eq!(id, source.source_unit);
    assert!(fresh.suppress_next_golden_shrine_revival(id));
    let mut wire_restored = restored(&fresh);
    fatality(&mut fresh, id);
    fatality(&mut wire_restored, id);
    assert_eq!(pending(&fresh), 0);
    assert_eq!(fresh.checksum(), wire_restored.checksum());
    let corpse = fresh
        .world
        .iter_entities()
        .find_map(|entity| entity.get::<Corpse>().copied())
        .unwrap();
    assert!(!corpse.shrine_state.suppress_next_death);

    // Native resurrection cannot preserve the removed handle alongside the replacement.
    assert_eq!(
        sim.resurrect_friendly_corpses(Team(0), AbilitySourceOrigin::Unit(source.position), 100, 1),
        1
    );
    let native = sim.units()[0].id;
    let mut wire_restored = restored(&sim);
    sim.next_tick = source.due_tick;
    wire_restored.next_tick = source.due_tick;
    sim.resolve_shrine_revivals();
    wire_restored.resolve_shrine_revivals();
    assert!(sim.unit(native).is_none());
    assert_eq!(sim.unit_count(), 1);
    assert_eq!(sim.checksum(), wire_restored.checksum());
}

#[test]
fn seed_rolls_are_repeatable_failed_rolls_do_not_schedule_and_stacking_changes_only_threshold() {
    let mut successes = [0; 3];
    for seed in 0..200 {
        let mut outcomes = [false; 3];
        for count in 1..=3 {
            let mut sim = simulation(1, seed);
            // Allocate identical unit IDs across team-stacking fixtures.
            let id = unit(&mut sim, definition());
            for index in 0..count {
                shrine(&mut sim, Team(0), 20 + index * 10);
            }
            fatality(&mut sim, id);
            outcomes[count as usize - 1] = pending(&sim) == 1;
            successes[count as usize - 1] += pending(&sim);
        }
        assert!(!outcomes[0] || outcomes[1]);
        assert_eq!(outcomes[1], outcomes[2]);
    }
    assert!(successes[0] > 0 && successes[0] < successes[1]);
    assert!(successes[1] < 200);
}
