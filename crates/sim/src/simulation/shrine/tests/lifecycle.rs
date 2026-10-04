use super::*;

#[test]
fn original_handle_identity_survives_repeated_native_resurrection_and_redeath() {
    let mut sim = ready_sim();
    let source = sim
        .world
        .iter_entities()
        .find_map(|entity| entity.get::<DelayedShrineRevival>().copied())
        .unwrap();
    for _ in 0..2 {
        assert_eq!(
            sim.resurrect_friendly_corpses(
                Team(0),
                AbilitySourceOrigin::Unit(source.position),
                100,
                1,
            ),
            1
        );
        let unit = sim.units()[0].id;
        assert_eq!(
            sim.world
                .get::<ShrineRevivalState>(entity(&sim, unit))
                .unwrap()
                .death_identity,
            Some(source.source_unit)
        );
        // No-killer death cannot schedule another proc, but it must retain the old handle identity
        // on the corpse so the original committed callback still removes that handle.
        sim.debug_damage_all_units(1_000);
        assert_eq!(pending(&sim), 1);
        let corpse = sim
            .world
            .iter_entities()
            .find_map(|entity| entity.get::<Corpse>())
            .unwrap();
        assert_eq!(corpse.shrine_state.death_identity, Some(source.source_unit));
        sim = restored(&sim);
    }
    let mut wire_restored = restored(&sim);
    sim.next_tick = source.due_tick;
    wire_restored.next_tick = source.due_tick;
    sim.resolve_shrine_revivals();
    wire_restored.resolve_shrine_revivals();
    assert_eq!(sim.corpse_count(), 0);
    assert_eq!(sim.unit_count(), 1);
    assert_eq!(sim.checksum(), wire_restored.checksum());
    // Native resurrection of the replacement retains its permanent one-time block.
    sim.debug_damage_all_units(1_000);
    assert_eq!(sim.resurrect_friendly_corpses(
        Team(0), AbilitySourceOrigin::Unit(source.position), 100, 1,
    ), 1);
    let id = sim.units()[0].id;
    assert!(
        sim.world
            .get::<ShrineRevivalState>(entity(&sim, id))
            .unwrap()
            .revived
    );
    fatality(&mut sim, id);
    assert_eq!(pending(&sim), 0);
}

#[test]
fn consumed_suppression_allows_the_same_handle_to_proc_after_native_resurrection() {
    let success = ready_sim();
    let mut sim = simulation(1, success.config.match_seed);
    shrine(&mut sim, Team(0), 20);
    let id = unit(&mut sim, definition());
    assert!(sim.suppress_next_golden_shrine_revival(id));
    fatality(&mut sim, id);
    assert_eq!(pending(&sim), 0);
    assert_eq!(
        sim.resurrect_friendly_corpses(
            Team(0),
            AbilitySourceOrigin::Unit(SimPoint::new(50, 11)),
            100,
            1,
        ),
        1
    );
    sim = restored(&sim);
    let native = sim.units()[0].id;
    fatality(&mut sim, native);
    assert_eq!(pending(&sim), 1);
    let callback = sim
        .world
        .iter_entities()
        .find_map(|entity| entity.get::<DelayedShrineRevival>())
        .unwrap();
    assert_eq!(callback.source_unit, id);
}

#[test]
fn suppression_is_not_consumed_for_excluded_units_or_teams_without_a_shrine() {
    let mut sim = simulation(1, 0);
    let id = unit(&mut sim, definition());
    assert!(sim.suppress_next_golden_shrine_revival(id));
    fatality(&mut sim, id);
    let corpse = sim
        .world
        .iter_entities()
        .find_map(|entity| entity.get::<Corpse>())
        .unwrap();
    assert!(corpse.shrine_state.suppress_next_death);
    assert_eq!(pending(&sim), 0);
    let mut sim = ready_sim();
    let mut excluded = definition();
    excluded.properties.classifications.legendary = true;
    let id = unit(&mut sim, excluded);
    assert!(sim.suppress_next_golden_shrine_revival(id));
    fatality(&mut sim, id);
    let corpse = sim
        .world
        .iter_entities()
        .filter_map(|entity| entity.get::<Corpse>())
        .find(|corpse| corpse.source_unit == id)
        .unwrap();
    assert!(corpse.shrine_state.suppress_next_death);
}

#[test]
fn allied_owner_lifecycle_preserves_team_contribution_and_migrates_allocated_points() {
    let players = [
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
    ];
    let mut sim = Simulation::new_internal(
        SimulationConfig::default(),
        1,
        CombatRules::default(),
        None,
        &players,
    );
    let tower = CastleFightTowerKind::GoldenShrineOfJustice.definition();
    let properties = tower.gameplay_properties();
    let cost = properties.economy.unwrap().legendary_points_cost;
    // Mimic the purchased allocation without involving lobby/build-command setup.
    sim.player_state_mut(PlayerId(0))
        .unwrap()
        .resources
        .legendary_points_used = cost;
    let id = sim.spawn_building_for_player_with_properties(
        PlayerId(0),
        tower.spawn(Team(0), BuildingFootprint::new(20, 20, 4, 4)),
        properties,
    );
    let chance = sim.golden_shrine_revive_chance(Team(0));
    assert!(chance > 0);
    assert!(!sim.transfer_golden_shrine_owner(id, PlayerId(99)));
    assert!(sim.transfer_golden_shrine_owner(id, PlayerId(1)));
    assert_eq!(sim.golden_shrine_revive_chance(Team(0)), chance);
    assert_eq!(
        sim.player(PlayerId(0))
            .unwrap()
            .resources
            .legendary_points_used,
        0
    );
    assert_eq!(
        sim.player(PlayerId(1))
            .unwrap()
            .resources
            .legendary_points_used,
        cost
    );
    assert!(sim.transfer_golden_shrine_owner(id, PlayerId(2)));
    assert_eq!(sim.golden_shrine_revive_chance(Team(0)), 0);
    assert_eq!(sim.golden_shrine_revive_chance(Team(1)), chance);
    sim = restored(&sim);
    assert!(sim.remove_building(id));
    assert_eq!(
        sim.player(PlayerId(2))
            .unwrap()
            .resources
            .legendary_points_used,
        0
    );
    assert_eq!(sim.golden_shrine_revive_chance(Team(1)), 0);
}

#[test]
fn match_completion_invalidates_callbacks_once_and_freezes_cosmetic_events() {
    let mut sim = ready_sim();
    let generation = sim.shrine_death_generation;
    assert!(sim.finish_match_from_control(MatchOutcome::Draw));
    assert_eq!(sim.shrine_death_generation, generation + 1);
    assert!(!sim.finish_match_from_control(MatchOutcome::Draw));
    assert_eq!(sim.shrine_death_generation, generation + 1);
    let mut restored = restored(&sim);
    sim.step();
    restored.step();
    assert_eq!(sim.checksum(), restored.checksum());
    assert!(sim.shrine_revivals_last_tick().is_empty());
    assert_eq!(sim.unit_count(), 0);
}

#[test]
fn delayed_parameters_generation_and_handle_flags_are_all_checksum_authoritative() {
    let mut sim = ready_sim();
    let before = sim.checksum();
    let pending_entity = sim
        .world
        .iter_entities()
        .find(|entity| entity.get::<DelayedShrineRevival>().is_some())
        .unwrap()
        .id();
    sim.world
        .get_mut::<DelayedShrineRevival>(pending_entity)
        .unwrap()
        .due_tick += 1;
    assert_ne!(sim.checksum(), before);
    let before = sim.checksum();
    sim.invalidate_pending_golden_shrine_revivals();
    assert_ne!(sim.checksum(), before);
    let id = unit(&mut sim, definition());
    let before = sim.checksum();
    assert!(sim.suppress_next_golden_shrine_revival(id));
    assert_ne!(sim.checksum(), before);
    let wire_restored = restored(&sim);
    assert_eq!(sim.checksum(), wire_restored.checksum());
}
