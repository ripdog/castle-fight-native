use super::*;
use crate::{MapVersion, NativeBoltProfile, UnitTemplate, castle_fight_content_bundle};
const VERSION: MapVersion = MapVersion::CASTLE_FIGHT_9_27;

fn sim(workers: usize) -> Simulation {
    Simulation::new(
        SimulationConfig {
            navigation_max: NavCell::new(500, 500),
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
fn building(sim: &mut Simulation, team: Team, x: i32) -> SimId {
    sim.spawn_building(BuildingSpawn {
        team,
        footprint: BuildingFootprint::new(x, 5, 1, 1),
        health: 10_000,
        production: None,
        attack: None,
        spellcasting: None,
    })
}
fn launcher(sim: &mut Simulation) -> (SimId, HailstoneProfile) {
    let definition = crate::CastleFightTowerKind::FrostLauncher.definition();
    let mut spawn = definition.spawn(Team(0), BuildingFootprint::new(2, 5, 1, 1));
    let casting = spawn.spellcasting.as_mut().unwrap();
    casting.mana.starting = casting.ability.mana_cost;
    casting.mana = crate::ManaProfile::per_second(casting.mana.maximum, casting.mana.starting, 0);
    let AbilityEffect::Hailstone(profile) = casting.ability.effect else {
        panic!()
    };
    (
        sim.spawn_building_with_properties(spawn, definition.gameplay_properties()),
        profile,
    )
}
fn restored(sim: &Simulation) -> Simulation {
    let bundle = castle_fight_content_bundle(VERSION).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), bundle)
            .unwrap();
    let mut other = Simulation::new(sim.config.clone(), 4);
    other.restore_snapshot(&snapshot).unwrap();
    other
}
fn source(sim: &Simulation, id: SimId, profile: HailstoneProfile) -> AbilitySourceSnapshot {
    let b = sim.building(id).unwrap();
    AbilitySourceSnapshot {
        source: AbilitySourceIndex::Building(0),
        map_version: Some(VERSION),
        id,
        team: b.team,
        origin: AbilitySourceOrigin::Building(b.footprint),
        health: b.health,
        stunned_until_tick: 0,
        spellcasting: Some(SpellcastingProfile {
            mana: crate::ManaProfile::per_second(1, 1, 0),
            ability: AutomaticAbilityProfile {
                id: AbilityId(13),
                mana_cost: 1,
                cooldown_ticks: 1,
                range: 0,
                target_policy: AbilityTargetPolicy::HailstoneSpellTrigger,
                effect: AbilityEffect::Hailstone(profile),
            },
        }),
        mana_current: Some(1),
        ability_state: None,
    }
}

#[test]
fn hailstone_is_delayed_survives_source_death_and_restores_in_flight_without_base_splash() {
    let mut sim = sim(1);
    let (source, p) = launcher(&mut sim);
    let target = building(&mut sim, Team(1), 20);
    sim.step();
    assert_eq!(sim.projectile_count(), 1);
    assert_eq!(sim.ability_casts_last_tick().len(), 1);
    assert_eq!(sim.building(target).unwrap().health, 10_000);
    let neighbor = building(&mut sim, Team(1), 22);
    let source_entity = entity(&sim, source);
    sim.world.get_mut::<Health>(source_entity).unwrap().current = 0;
    let impact = sim.projectiles()[0].impact_tick;
    let mut other = restored(&sim);
    while sim.next_tick <= impact {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    let victim = sim.building(target).unwrap();
    assert_eq!(
        victim.health,
        10_000
            - sim
                .combat_rules
                .damage_rules
                .apply_attack(p.damage, DamageType::Chaos, victim.armor)
    );
    assert_eq!(
        victim.status.frozen_until_tick,
        impact + u64::from(p.freeze_ticks)
    );
    assert_eq!(sim.building(neighbor).unwrap().health, 10_000);
    assert_eq!(sim.building(neighbor).unwrap().status.frozen_until_tick, 0);
}

#[test]
fn global_structure_selector_distinguishes_script_exclusions_from_native_attack_failure() {
    let mut sim = sim(1);
    let (caster, p) = launcher(&mut sim);
    let target = building(&mut sim, Team(1), 20);
    let e = entity(&sim, target);
    let source = source(&sim, caster, p);
    assert!(sim.hailstone_target_eligible(source, p, sim.world.entity(e)));
    sim.world.entity_mut(e).insert(UnitClassifications {
        invulnerable: true,
        spell_immune: true,
        ..Default::default()
    });
    assert!(sim.hailstone_target_eligible(source, p, sim.world.entity(e)));
    sim.world
        .get_mut::<UnitClassifications>(e)
        .unwrap()
        .spell_immune = false;
    sim.step();
    assert_eq!(sim.projectile_count(), 0);
    assert_eq!(sim.ability_casts_last_tick().len(), 1);
    assert_eq!(sim.building(caster).unwrap().mana_current, Some(0));
    sim.world.entity_mut(e).insert(ContentIdentity {
        map_version: VERSION,
        rawcode: p.excluded_rawcodes[0],
        name: "excluded",
    });
    assert!(!sim.hailstone_target_eligible(source, p, sim.world.entity(e)));
    sim.world.entity_mut(e).remove::<ContentIdentity>();
    let mut s = StatusState::default();
    apply_timed_armor_modifier(
        &mut s,
        TimedArmorModifier {
            id: ModifierId(p.excluded_buff),
            expires_tick: 1000,
            ..Default::default()
        },
    );
    sim.world.entity_mut(e).insert(s);
    assert!(!sim.hailstone_target_eligible(source, p, sim.world.entity(e)));
    sim.world.entity_mut(e).remove::<StatusState>();
    sim.world.entity_mut(e).insert(Team(0));
    assert!(!sim.hailstone_target_eligible(source, p, sim.world.entity(e)));
}

#[test]
fn freeze_pauses_training_remaining_time_and_restores_without_catching_up() {
    let mut sim = sim(1);
    let producer = crate::CastleFightProductionKind::Barracks.definition();
    let id = sim.spawn_building_with_properties(
        producer.spawn(Team(1), BuildingFootprint::new(10, 5, 2, 2)),
        producer.gameplay_properties(),
    );
    let e = entity(&sim, id);
    let deadline = sim.world.get::<ProductionState>(e).unwrap().next_spawn_tick;
    let mut status = StatusState::default();
    freeze(&mut status, 0, 5, AbilityId(17));
    sim.world.entity_mut(e).insert(status);
    let mut other = restored(&sim);
    for _ in 0..5 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    assert_eq!(
        sim.world.get::<ProductionState>(e).unwrap().next_spawn_tick,
        deadline + 5
    );
    sim.step();
    assert_eq!(
        sim.world.get::<ProductionState>(e).unwrap().next_spawn_tick,
        deadline + 5
    );
}

#[test]
fn splash_freeze_uses_live_masks_and_hero_duration_independently_of_chaos_damage() {
    let mut sim = sim(1);
    let (caster, mut p) = launcher(&mut sim);
    p.damage = 17;
    p.full_radius = 100 * SUBUNITS_PER_WORLD_UNIT;
    p.splash_targets = AttackTargetMask::GROUND_AND_BUILDINGS;
    p.freeze_targets = p.splash_targets;
    p.freeze_ticks = 30;
    p.hero_freeze_ticks = 7;
    let target = building(&mut sim, Team(1), 20);
    let position = footprint_center_point(
        sim.building(target).unwrap().footprint,
        sim.config.navigation_cell_size,
    );
    let template = UnitTemplate {
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
    let mut victims = Vec::new();
    for (team, movement_class, hero, spell_immune, invulnerable) in [
        (Team(1), MovementClass::Ground, false, false, false),
        (Team(1), MovementClass::Ground, true, false, false),
        (Team(1), MovementClass::Air, false, false, false),
        (Team(0), MovementClass::Ground, false, false, false),
        (Team(1), MovementClass::Ground, false, true, false),
        (Team(1), MovementClass::Ground, false, false, true),
    ] {
        victims.push(sim.spawn_unit_with_properties(
            UnitSpawn::from_template(team, position, template),
            UnitGameplayProperties {
                movement_class,
                classifications: UnitClassifications {
                    hero,
                    spell_immune,
                    invulnerable,
                    ..Default::default()
                },
                ..Default::default()
            },
        ));
    }
    let mut units = sim.snapshot_units();
    let mut buildings = sim.snapshot_buildings();
    sim.resolve_hailstone(
        HailstoneState {
            source: caster,
            team: Team(0),
            target,
            profile: p,
            origin: position,
            destination: position,
            launch_tick: 0,
            impact_tick: 0,
        },
        &mut units,
        &mut buildings,
    );
    for (i, id) in victims.iter().enumerate() {
        let victim = &units[find_unit_index(&units, *id).unwrap()];
        assert_eq!(victim.health < 1000, matches!(i, 0 | 1 | 4));
        assert_eq!(
            victim.status.frozen_until_tick,
            match i {
                0 => 30,
                1 => 7,
                _ => 0,
            }
        );
    }
}

#[test]
fn freeze_blocks_autonomous_native_fire_and_pending_ordered_casts_until_thaw() {
    let mut sim = sim(1);
    let profile = NativeBoltProfile {
        ability: AbilityId(42),
        damage: 17,
        stun_ticks: 0,
        hero_stun_ticks: 0,
        damage_per_second: 0,
        duration_ticks: 0,
        speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
        cleanse: false,
        targets: AttackTargetMask::GROUND_UNITS,
    };
    let casting = SpellcastingProfile {
        mana: crate::ManaProfile::per_second(0, 0, 0),
        ability: AutomaticAbilityProfile {
            id: AbilityId(41),
            mana_cost: 0,
            cooldown_ticks: 100,
            range: 100 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::RandomEnemyUnitOrBuilding,
            effect: AbilityEffect::PhoenixFire(profile),
        },
    };
    let template = UnitTemplate {
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
    let caster = sim.spawn_unit_with_spellcasting(
        UnitSpawn::from_template(Team(0), SimPoint::new(100, 100), template),
        casting,
    );
    sim.spawn_unit(UnitSpawn::from_template(
        Team(1),
        SimPoint::new(500, 100),
        template,
    ));
    let e = entity(&sim, caster);
    let mut status = *sim.world.get::<StatusState>(e).unwrap();
    status.pending_cast = Some(crate::PendingCastState {
        ability: casting.ability,
        cast_sequence: 0,
        target: crate::PendingCastTarget::AllEnemyUnits,
        release_tick: 1,
    });
    freeze(&mut status, 0, 5, AbilityId(43));
    assert!(status.pending_cast.is_none());
    sim.world.entity_mut(e).insert(status);
    for _ in 0..5 {
        sim.step();
        assert_eq!(sim.projectile_count(), 0);
        assert_eq!(sim.unit(caster).unwrap().ability_cast_sequence.unwrap(), 0);
    }
    sim.step();
    assert_eq!(sim.projectile_count(), 1);
    assert_eq!(sim.unit(caster).unwrap().ability_cast_sequence.unwrap(), 1);
}

#[test]
fn freeze_pauses_construction_rejects_new_upgrades_and_survives_upgrade_cancellation() {
    let mut sim = sim(1);
    sim.debug_grant_player_resources_for(PlayerId(0), 100_000, 100_000);
    let source = crate::CastleFightProductionKind::Barracks.definition();
    let target = crate::CastleFightProductionKind::Stronghold.definition();
    let footprint = BuildingFootprint::new(10, 5, 2, 2);
    let id = sim.spawn_building_with_properties(
        source.spawn(Team(0), footprint),
        source.gameplay_properties(),
    );
    let e = entity(&sim, id);
    sim.world.get_mut::<ProductionState>(e).unwrap().queued = 0;
    let mut status = StatusState::default();
    freeze(&mut status, 0, 5, AbilityId(91));
    sim.world.entity_mut(e).insert(status);
    assert_eq!(
        sim.start_building_upgrade_as(
            PlayerId(0),
            id,
            source.spawn(Team(0), footprint),
            source.gameplay_properties(),
            target.spawn(Team(0), footprint),
            target.gameplay_properties()
        ),
        Err(BuildingUpgradeError::SourceDisabled)
    );
    sim.world.entity_mut(e).insert(StatusState::default());
    sim.start_building_upgrade_as(
        PlayerId(0),
        id,
        source.spawn(Team(0), footprint),
        source.gameplay_properties(),
        target.spawn(Team(0), footprint),
        target.gameplay_properties(),
    )
    .unwrap();
    let deadline = sim
        .world
        .get::<BuildingConstruction>(e)
        .unwrap()
        .complete_tick;
    sim.world.entity_mut(e).insert(status);
    sim.step();
    assert_eq!(
        sim.world
            .get::<BuildingConstruction>(e)
            .unwrap()
            .complete_tick,
        deadline + 1
    );
    let mut other = restored(&sim);
    sim.cancel_building_construction_for_player(PlayerId(0), id)
        .unwrap();
    other
        .cancel_building_construction_for_player(PlayerId(0), id)
        .unwrap();
    assert_eq!(sim.checksum(), other.checksum());
    assert_eq!(sim.building(id).unwrap().status.frozen_until_tick, 5);
    assert_eq!(
        sim.building(id).unwrap().status.frozen_ability,
        Some(AbilityId(91))
    );
}

#[test]
fn native_parent_spends_resources_even_when_independent_callback_has_no_building() {
    let mut sim = sim(1);
    let (caster, p) = launcher(&mut sim);
    let target = building(&mut sim, Team(1), 20);
    sim.world
        .entity_mut(entity(&sim, target))
        .insert(ContentIdentity {
            map_version: VERSION,
            rawcode: p.excluded_rawcodes[0],
            name: "castle",
        });
    sim.step();
    assert_eq!(sim.building(caster).unwrap().mana_current, Some(0));
    assert_eq!(sim.projectile_count(), 0);
    assert!(
        sim.ability_casts_last_tick().is_empty(),
        "callback has no cast art before it finds a building"
    );
    let mut empty = self::sim(1);
    let (caster, _) = launcher(&mut empty);
    empty.step();
    assert!(empty.building(caster).unwrap().mana_current.unwrap() > 0);
}

#[test]
fn live_impact_uses_post_movement_centers_and_intersecting_structure_footprints() {
    let mut sim = sim(1);
    let (caster, mut p) = launcher(&mut sim);
    let primary = building(&mut sim, Team(1), 20);
    let footprint = sim.building(primary).unwrap().footprint;
    let destination = footprint_center_point(footprint, sim.config.navigation_cell_size);
    p.full_radius = 2 * sim.config.navigation_cell_size;
    p.splash_targets = AttackTargetMask::GROUND_AND_BUILDINGS;
    p.freeze_targets = p.splash_targets;
    let neighbor = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(22, 5, 8, 8),
        health: 10_000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    let unit = sim.spawn_unit(UnitSpawn::from_template(
        Team(1),
        SimPoint::new(destination.x + 10 * p.full_radius, destination.y),
        UnitTemplate {
            health: 10_000,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
                delivery: AttackDelivery::Melee,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
    ));
    let id = sim.allocate_id();
    sim.world.spawn((
        id,
        NativeAction::Hailstone(HailstoneState {
            source: caster,
            team: Team(0),
            target: primary,
            profile: p,
            origin: destination,
            destination,
            launch_tick: 0,
            impact_tick: 0,
        }),
    ));
    let mut units = sim.snapshot_units();
    let mut buildings = sim.snapshot_buildings();
    let mut unit_health = units.iter().map(|u| u.health).collect::<Vec<_>>();
    let mut building_health = buildings.iter().map(|b| b.health).collect::<Vec<_>>();
    sim.resolve_due_hailstone_impacts(
        &mut units,
        &mut buildings,
        &[destination],
        &mut unit_health,
        &mut building_health,
    );
    assert!(
        units
            .iter()
            .find(|u| u.id == unit)
            .unwrap()
            .status
            .frozen_until_tick
            > 0
    );
    assert!(
        buildings
            .iter()
            .find(|b| b.id == neighbor)
            .unwrap()
            .status
            .unwrap()
            .frozen_until_tick
            > 0
    );
    assert_eq!(sim.projectile_count(), 0);
}
