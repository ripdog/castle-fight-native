use super::*;
use crate::{
    CastleFightProductionKind, CastleFightTowerKind, CastleFightUnitKind, MapVersion, UnitTemplate,
};

fn sim(workers: usize) -> Simulation {
    Simulation::new(SimulationConfig::default(), workers)
}
fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}
fn unit(sim: &mut Simulation, team: u8, x: i32) -> SimId {
    sim.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(team),
            position: SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 100,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}
fn support() -> DeathHealSupport {
    DeathHealSupport {
        version: MapVersion::CASTLE_FIGHT_9_27,
        active: true,
        healing: 23,
        radius: 30 * SUBUNITS_PER_WORLD_UNIT,
        other_type_per_10k: 5000,
    }
}
#[test]
fn death_heal_uses_exact_type_and_living_ally_sapper_radius_without_spell_immunity_gate() {
    let mut s = sim(1);
    for (team, x) in [(0, 0), (0, 30), (0, 10), (0, 10), (1, 10), (0, 31), (0, 10)] {
        unit(&mut s, team, x);
    }
    let mut units = s.snapshot_units();
    for u in &mut units {
        u.health = 50;
        u.rawcode = 100;
    }
    units[0].death_heal = Some(support());
    units[2].rawcode = 101;
    units[3].classifications.combat_sapper = false;
    units[6].classifications.spell_immune = true;
    units[6].classifications.invulnerable = true;
    commit_health(&mut units, 0, 0);
    assert_eq!(
        units.iter().map(|u| u.health).collect::<Vec<_>>(),
        [0, 73, 61, 50, 50, 50, 73]
    );
    assert_eq!(units[6].death_heal_visuals, 1);
    commit_health(&mut units, 0, -10);
    assert_eq!(
        units[1].health, 73,
        "already-dead handle does not heal twice"
    );
}
#[test]
fn source_exclusions_and_split_storage_commit_before_the_next_fatal_event() {
    for flags in [
        UnitClassifications {
            legendary: true,
            ..Default::default()
        },
        UnitClassifications {
            summoned_marker: true,
            ..Default::default()
        },
        UnitClassifications {
            summoned: true,
            ..Default::default()
        },
        UnitClassifications {
            illusion: true,
            ..Default::default()
        },
        UnitClassifications::default(),
    ] {
        let mut s = sim(1);
        unit(&mut s, 0, 0);
        unit(&mut s, 0, 0);
        let mut units = s.snapshot_units();
        units[0].classifications = flags;
        units[0].death_heal = Some(support());
        units[1].health = 10;
        commit_health(&mut units, 0, 0);
        assert_eq!(units[1].health, 10);
    }
    let mut s = sim(1);
    unit(&mut s, 0, 0);
    unit(&mut s, 0, 0);
    let mut units = s.snapshot_units();
    units[0].death_heal = Some(support());
    let mut health = [10, 10];
    commit_split_health(&mut units, &mut health, None, 0, 0);
    assert_eq!(health, [0, 33]);
    let after = health[1] - 20;
    commit_split_health(&mut units, &mut health, None, 1, after);
    assert_eq!(
        health[1], 13,
        "heal rescues a later victim within this tick"
    );
    assert_eq!(
        units[1].health, 100,
        "split storage does not accidentally mutate the frozen health view"
    );
}
fn production(s: &mut Simulation, x: i32) -> SimId {
    let d = CastleFightProductionKind::RockPile.definition();
    let mut spawn = d.spawn(Team(0), BuildingFootprint::new(x, 4, 1, 1));
    spawn.production = None;
    let mut p = d.gameplay_properties();
    p.health_regen_per_second_per_10k = 0;
    p.economy = Some(BuildingEconomyProfile {
        gold_cost: 0,
        lumber_cost: 0,
        lumber_refund: 0,
        legendary_points_cost: 0,
        income_per_10k: 0,
    });
    s.spawn_building_for_player_with_properties(PlayerId(0), spawn, p)
}
fn linker(s: &mut Simulation, x: i32) -> SimId {
    let d = CastleFightTowerKind::Linker.definition();
    let mut p = d.gameplay_properties();
    p.economy = Some(BuildingEconomyProfile {
        gold_cost: 0,
        lumber_cost: 0,
        lumber_refund: 0,
        legendary_points_cost: 0,
        income_per_10k: 0,
    });
    s.spawn_building_for_player_with_properties(
        PlayerId(0),
        d.spawn(Team(0), BuildingFootprint::new(x, 6, 1, 1)),
        p,
    )
}
fn native_unit(s: &mut Simulation, x: i32) -> SimId {
    let d = CastleFightUnitKind::LesserElementalOfWind.definition();
    let mut spawn = UnitSpawn::from_template(
        Team(0),
        SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 0),
        d.template(),
    );
    spawn.health = 100;
    spawn.attack.damage = 0;
    spawn.attack.range = 0;
    spawn.attack.acquisition_range = 0;
    spawn.movement.speed_per_tick = 0;
    let mut p = d.gameplay_properties();
    p.passive_effects = PassiveUnitEffects::EMPTY;
    p.health_regen_per_second_per_10k = 0;
    p.action_timing = Default::default();
    p.collision_radius = Some(CollisionRadius(0));
    s.spawn_resolved_unit_for_player(
        PlayerId(0),
        spawn.team,
        spawn.position,
        ResolvedUnitDefinition {
            template: UnitTemplate {
                health: spawn.health,
                attack: spawn.attack,
                movement: spawn.movement,
            },
            properties: p,
            spellcasting: None,
            additional_abilities: None,
        },
    )
}
#[test]
fn support_is_owner_scoped_team_gated_nonstacking_and_updates_on_building_death() {
    let mut s = sim(1);
    production(&mut s, 0);
    let second = production(&mut s, 2);
    let tower = linker(&mut s, 0);
    linker(&mut s, 2);
    native_unit(&mut s, 0);
    let p =
        crate::building_mechanics::death_heal_for_version(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let mut units = s.snapshot_units();
    let mut buildings = s.snapshot_buildings();
    refresh_support(&mut units, &buildings, None);
    assert_eq!(
        units[0].death_heal.unwrap().healing,
        2 * p.heal_per_building
    );
    let mut health: Vec<_> = buildings.iter().map(|b| b.health).collect();
    health[find_building_index(&buildings, second).unwrap()] = 0;
    refresh_support(&mut units, &buildings, Some(&health));
    assert_eq!(units[0].death_heal.unwrap().healing, p.heal_per_building);
    for b in &mut buildings {
        if matches!(
            b.death_heal_contribution,
            Some(DeathHealContribution::Production(_))
        ) {
            b.death_heal_contribution = Some(DeathHealContribution::Production(PlayerId(1)));
        }
    }
    refresh_support(&mut units, &buildings, None);
    assert_eq!(units[0].death_heal.unwrap().healing, 0);
    assert!(units[0].death_heal.unwrap().active);
    let index = find_building_index(&buildings, tower).unwrap();
    buildings[index].death_heal_contribution = None;
    for b in &mut buildings {
        if matches!(
            b.death_heal_contribution,
            Some(DeathHealContribution::Linker)
        ) {
            b.health = 0;
        }
    }
    refresh_support(&mut units, &buildings, None);
    assert!(!units[0].death_heal.unwrap().active);
}
#[test]
fn native_death_during_released_bolt_survives_wire_restore_and_has_recipient_visuals() {
    let mut s = sim(1);
    production(&mut s, 0);
    linker(&mut s, 0);
    let dead = native_unit(&mut s, 0);
    let ally = native_unit(&mut s, 10);
    s.world.get_mut::<Health>(entity(&s, dead)).unwrap().current = 5;
    s.world.get_mut::<Health>(entity(&s, ally)).unwrap().current = 20;
    s.launch_dummy_bolt(
        SimId(9999),
        Team(1),
        SimPoint::new(-10 * SUBUNITS_PER_WORLD_UNIT, 0),
        dead,
        SimPoint::new(0, 0),
        crate::NativeBoltProfile {
            ability: AbilityId(777),
            damage: 10,
            stun_ticks: 0,
            hero_stun_ticks: 0,
            damage_per_second: 0,
            duration_ticks: 0,
            speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
            cleanse: false,
            targets: AttackTargetMask::AIR_AND_GROUND,
        },
    );
    let content = crate::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let snap =
        SimulationSnapshot::decode_wire(&s.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut restored = sim(4);
    restored.restore_snapshot(&snap).unwrap();
    let p =
        crate::building_mechanics::death_heal_for_version(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let mut visual = false;
    for _ in 0..12 {
        assert_eq!(s.step().checksum, restored.step().checksum);
        assert_eq!(s.death_heals_last_tick(), restored.death_heals_last_tick());
        visual |= !s.death_heals_last_tick().is_empty();
    }
    assert!(s.unit(dead).is_none());
    assert_eq!(s.unit(ally).unwrap().health, 20 + p.heal_per_building);
    assert!(visual);
}

fn current_support(s: &mut Simulation, id: SimId) -> DeathHealSupport {
    let mut units = s.snapshot_units();
    let buildings = s.snapshot_buildings();
    refresh_support(&mut units, &buildings, None);
    units[find_unit_index(&units, id).unwrap()]
        .death_heal
        .unwrap()
}
#[test]
fn unfinished_support_is_inert_and_upgrade_preserves_one_registration_through_restore_and_cancel() {
    for cancel in [false, true] {
        let mut s = sim(1);
        let source = production(&mut s, 0);
        let victim = native_unit(&mut s, 0);
        let d = CastleFightTowerKind::Linker.definition();
        let mut properties = d.gameplay_properties();
        properties.construction_time_ticks = Some(3);
        properties.economy = Some(BuildingEconomyProfile {
            gold_cost: 0,
            lumber_cost: 0,
            lumber_refund: 0,
            legendary_points_cost: 0,
            income_per_10k: 0,
        });
        s.try_start_building_construction(
            PlayerId(0),
            d.spawn(Team(0), BuildingFootprint::new(4, 4, 1, 1)),
            properties,
        )
        .unwrap();
        assert!(!current_support(&mut s, victim).active);
        for _ in 0..4 {
            s.step();
        }
        assert!(current_support(&mut s, victim).active);
        let d = CastleFightProductionKind::RockPile.definition();
        let upgraded = CastleFightProductionKind::GreaterFissure.definition();
        let footprint = BuildingFootprint::new(0, 4, 1, 1);
        let mut base = d.spawn(Team(0), footprint);
        base.production = None;
        let mut target = upgraded.spawn(Team(0), footprint);
        target.production = None;
        let mut from = d.gameplay_properties();
        from.economy = Some(BuildingEconomyProfile {
            gold_cost: 0,
            lumber_cost: 0,
            lumber_refund: 0,
            legendary_points_cost: 0,
            income_per_10k: 0,
        });
        let mut to = upgraded.gameplay_properties();
        to.economy = from.economy;
        to.construction_time_ticks = Some(3);
        let before = current_support(&mut s, victim).healing;
        s.start_building_upgrade_as(PlayerId(0), source, base, from, target, to)
            .unwrap();
        assert_eq!(current_support(&mut s, victim).healing, before);
        let snapshot = s.capture_snapshot();
        let mut restored = sim(4);
        restored.restore_snapshot(&snapshot).unwrap();
        if cancel {
            s.cancel_building_construction_for_player(PlayerId(0), source)
                .unwrap();
            restored
                .cancel_building_construction_for_player(PlayerId(0), source)
                .unwrap();
        }
        for _ in 0..5 {
            assert_eq!(s.step().checksum, restored.step().checksum);
            assert_eq!(current_support(&mut s, victim).healing, before);
        }
    }
}
#[test]
fn transferring_completed_support_moves_owner_counts_and_team_presence_immediately() {
    let content = crate::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let mut s = Simulation::new_with_gameplay_bundle_and_players(
        SimulationConfig::default(),
        1,
        CombatRules::default(),
        content.identity.into(),
        &[
            PlayerConfig {
                id: PlayerId(0),
                team: Team(0),
            },
            PlayerConfig {
                id: PlayerId(1),
                team: Team(1),
            },
            PlayerConfig {
                id: PlayerId(2),
                team: Team(0),
            },
        ],
    );
    let producer = production(&mut s, 0);
    let tower = linker(&mut s, 0);
    let victim = native_unit(&mut s, 0);
    let before = current_support(&mut s, victim).healing;
    assert!(before > 0);
    assert!(s.transfer_building_owner(producer, PlayerId(2)));
    assert_eq!(current_support(&mut s, victim).healing, 0);
    assert!(current_support(&mut s, victim).active);
    assert!(s.transfer_building_owner(producer, PlayerId(0)));
    assert_eq!(current_support(&mut s, victim).healing, before);
    assert!(s.transfer_building_owner(tower, PlayerId(1)));
    assert!(!current_support(&mut s, victim).active);
}
#[test]
fn zero_count_keeps_native_recipient_art_and_healing_clamps_to_life_capacity() {
    let mut s = sim(1);
    for _ in 0..3 {
        unit(&mut s, 0, 0);
    }
    let mut units = s.snapshot_units();
    units[0].death_heal = Some(DeathHealSupport {
        healing: 0,
        ..support()
    });
    commit_health(&mut units, 0, 0);
    assert_eq!(units[1].death_heal_visuals, 1);
    assert_eq!(units[1].health, 100);
    units[1].death_heal = Some(support());
    units[2].health = 90;
    commit_health(&mut units, 1, 0);
    assert_eq!(units[2].health, 100);
}

#[test]
fn credited_kill_healing_uses_victim_class_and_source_handle_for_inline_and_split_life() {
    for split in [false, true] {
        let mut s = sim(1);
        for (team, x) in [(0, 0), (1, 1), (1, 2)] {
            unit(&mut s, team, x);
        }
        let mut units = s.snapshot_units();
        units[0].health = 40;
        units[0].death_rule = Some(crate::native_effects::NativeUnitDeathRule {
            rawcode: 0,
            heal_percent: 17,
            retaliation_damage: 0,
        });
        units[2].classifications.combat_sapper = false;
        let source = units[0].id;
        if split {
            let mut h: Vec<_> = units.iter().map(|u| u.health).collect();
            commit_split_health_from(&mut units, &mut h, None, 1, 0, Some(source));
            assert_eq!(h[0], 57);
            commit_split_health_from(&mut units, &mut h, None, 1, -1, Some(source));
            commit_split_health_from(&mut units, &mut h, None, 2, 0, Some(source));
            assert_eq!(h[0], 57, "already dead/non-sapper deaths cannot heal again");
        } else {
            commit_health_from(&mut units, 1, 0, Some(source));
            assert_eq!(units[0].health, 57);
            commit_health_from(&mut units, 1, -1, Some(source));
            commit_health_from(&mut units, 2, 0, Some(source));
            assert_eq!(units[0].health, 57);
        }
    }
}

#[test]
fn death_retaliation_precedes_healing_and_cannot_revive_a_recursively_dead_killer() {
    for split in [false, true] {
        let mut s = sim(1);
        for (team, x) in [(0, 0), (1, 1)] {
            unit(&mut s, team, x);
        }
        let mut u = s.snapshot_units();
        u[0].health = 20;
        u[0].death_rule = Some(crate::native_effects::NativeUnitDeathRule {
            rawcode: 0,
            heal_percent: 90,
            retaliation_damage: 40,
        });
        u[1].death_rule = Some(crate::native_effects::NativeUnitDeathRule {
            rawcode: 0,
            heal_percent: 0,
            retaliation_damage: 30,
        });
        u[0].classifications.spell_immune = true;
        let killer = u[0].id;
        if split {
            let mut h = vec![20, 100];
            commit_split_health_from(&mut u, &mut h, None, 1, 0, Some(killer));
            assert!(h[0] <= 0 && h[1] <= 0);
        } else {
            commit_health_from(&mut u, 1, 0, Some(killer));
            assert!(u[0].health <= 0 && u[1].health <= 0);
        }
    }
}

#[test]
fn kill_heal_is_immediate_before_linker_and_missing_source_does_not_claim_credit() {
    let mut s = sim(1);
    for (team, x) in [(0, 0), (0, 1), (0, 2)] {
        unit(&mut s, team, x);
    }
    let mut u = s.snapshot_units();
    u[0].health = 40;
    u[0].death_rule = Some(crate::native_effects::NativeUnitDeathRule {
        rawcode: 0,
        heal_percent: 30,
        retaliation_damage: 0,
    });
    u[1].death_heal = Some(support());
    u[0].rawcode = 1;
    u[1].rawcode = 1;
    let killer = u[0].id;
    commit_health_from(&mut u, 1, 0, Some(killer));
    assert_eq!(u[0].health, 93);
    commit_health_from(&mut u, 2, 0, None);
    assert_eq!(u[0].health, 93);
}

#[test]
fn spell_resolution_carries_actual_source_credit_into_shared_death_callbacks() {
    let mut s = sim(1);
    for (team, x) in [(0, 0), (1, 1)] {
        unit(&mut s, team, x);
    }
    let mut u = s.snapshot_units();
    u[0].health = 40;
    u[0].death_rule = Some(crate::native_effects::NativeUnitDeathRule {
        rawcode: 0,
        heal_percent: 19,
        retaliation_damage: 0,
    });
    let source = u[0].id;
    s.apply_source_ability_to_unit(
        source,
        Team(0),
        1,
        AbilityEffect::Damage { amount: 1000 },
        &mut u,
    );
    assert_eq!(u[0].health, 59);
    assert!(u[1].health <= 0);
}
