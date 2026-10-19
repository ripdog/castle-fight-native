use super::*;
use crate::{AcidBombProfile, building_mechanics::BuildingAcidBombProfile};

fn profile() -> AcidBombProfile {
    AcidBombProfile {
        ability: AbilityId(96),
        range: 300 * SUBUNITS_PER_WORLD_UNIT,
        radius: 40 * SUBUNITS_PER_WORLD_UNIT,
        speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
        homing: false,
        armor_penalty_per_100: 350,
        movement_percent_delta: -40,
        attack_speed_percent_delta: -30,
        primary_damage: 17,
        secondary_damage: 7,
        interval_ticks: 10,
        duration_ticks: 25,
        hero_duration_ticks: 15,
        targets: AttackTargetMask::AIR_AND_GROUND,
    }
}

fn launch(sim: &mut Simulation, target: SimId, profile: AcidBombProfile) -> SimId {
    let source = sim.spawn_unit(unit(0, 20, 0, AttackDelivery::Melee));
    sim.launch_acid_bomb(
        source,
        Team(0),
        unit(0, 20, 0, AttackDelivery::Melee).position,
        target,
        sim.unit(target).unwrap().position,
        profile,
    );
    source
}

fn impacted(sim: &mut Simulation, target: SimId) -> u64 {
    for _ in 0..100 {
        sim.step();
        if sim.unit(target).unwrap().status.acid_bomb.is_some() {
            return sim.tick() - 1;
        }
    }
    panic!("missile did not arrive");
}

#[test]
fn acid_bomb_splashes_at_the_recipient_with_independent_native_masks_and_hero_lifetime() {
    let mut sim = simulation(1);
    let primary = sim.spawn_unit(unit(1, 100, 0, AttackDelivery::Melee));
    let mut recipients = Vec::new();
    for (team, x, air, mechanical, immune, invulnerable, hero, affected) in [
        (1, 140, false, false, false, false, false, true),
        (1, 141, false, false, false, false, false, false),
        (1, 120, true, false, false, false, true, true),
        (0, 120, false, false, false, false, false, false),
        (1, 120, false, true, false, false, false, false),
        (1, 120, false, false, true, false, false, false),
        (1, 120, false, false, false, true, false, false),
    ] {
        let id = sim.spawn_unit_with_properties(
            unit(team, x, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                movement_class: if air {
                    MovementClass::Air
                } else {
                    MovementClass::Ground
                },
                mechanical,
                build_time_ticks: mechanical.then_some(13),
                collision_radius: Some(CollisionRadius(0)),
                classifications: UnitClassifications {
                    spell_immune: immune,
                    invulnerable,
                    hero,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        recipients.push((id, hero, affected));
    }
    let building = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(7, 1, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    let source = launch(&mut sim, primary, profile());
    sim.world.despawn(entity(&sim, source));
    let mut restored = wire_restored(&sim, 4);
    while sim.unit(primary).unwrap().status.acid_bomb.is_none() {
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                restored.step();
                restored.checksum()
            }
        );
    }
    let impact = sim.tick() - 1;
    assert_eq!(sim.unit(primary).unwrap().health, 983);
    for &(id, hero, affected) in &recipients {
        let view = sim.unit(id).unwrap();
        assert_eq!(view.health, 1000 - if affected { 7 } else { 0 });
        assert_eq!(
            view.status.acid_bomb.map(|acid| acid.expires_tick),
            affected.then_some(impact + if hero { 15 } else { 25 })
        );
    }
    restored = wire_restored(&sim, 5);
    while sim.tick() <= impact + 25 {
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                restored.step();
                restored.checksum()
            }
        );
    }
    assert_eq!(sim.unit(primary).unwrap().health, 949);
    assert!(sim.unit(primary).unwrap().status.acid_bomb.is_none());
    for &(id, hero, affected) in &recipients {
        assert_eq!(
            sim.unit(id).unwrap().health,
            1000 - if affected {
                if hero { 14 } else { 21 }
            } else {
                0
            }
        );
        assert!(sim.unit(id).unwrap().status.acid_bomb.is_none());
    }
    assert_eq!(sim.building(building).unwrap().health, 1000);
}

#[test]
fn acid_bomb_replaces_one_native_family_and_full_removal_stops_its_damage() {
    let mut sim = simulation(1);
    let target = sim.spawn_unit(unit(1, 30, 0, AttackDelivery::Melee));
    launch(&mut sim, target, profile());
    impacted(&mut sim, target);
    let old = sim.unit(target).unwrap().status.acid_bomb.unwrap();
    let replacement = AcidBombProfile {
        ability: AbilityId(97),
        primary_damage: 9,
        armor_penalty_per_100: 200,
        movement_percent_delta: 25,
        attack_speed_percent_delta: 20,
        ..profile()
    };
    launch(&mut sim, target, replacement);
    while sim
        .unit(target)
        .unwrap()
        .status
        .acid_bomb
        .unwrap()
        .profile
        .ability
        != replacement.ability
    {
        sim.step();
    }
    let view = sim.unit(target).unwrap();
    assert_eq!(view.health, 974);
    assert_eq!(view.status.acid_bomb.unwrap().profile, replacement);
    assert_eq!(
        view.status.effective_armor_points_per_100(view.armor),
        i32::from(view.armor.armor_points) * 100 - 200
    );
    assert_eq!(effective_attack_cooldown_ticks(120, view.status), 100);
    let mut snapshots = sim.snapshot_units();
    let snapshot = snapshots.iter_mut().find(|unit| unit.id == target).unwrap();
    snapshot.movement.speed_per_tick = 100;
    assert_eq!(
        super::super::movement::effective_movement_speed(snapshot),
        125
    );
    assert!(
        sim.unit(target)
            .unwrap()
            .status
            .acid_bomb
            .unwrap()
            .expires_tick
            > old.expires_tick
    );
    super::super::native_actions::cleanse_native_status(
        &mut sim
            .world
            .get_mut::<StatusState>(entity(&sim, target))
            .unwrap(),
    );
    let mut restored = wire_restored(&sim, 6);
    for _ in 0..35 {
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                restored.step();
                restored.checksum()
            }
        );
    }
    assert_eq!(sim.unit(target).unwrap().health, 974);
    assert!(sim.unit(target).unwrap().status.acid_bomb.is_none());
}

#[test]
fn acid_bomb_keeps_fixed_flight_but_resolves_the_live_recipient_and_rejects_live_protection() {
    for homing in [false, true] {
        let mut sim = simulation(1);
        let target = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
        let initial_neighbor = sim.spawn_unit(unit(1, 70, 0, AttackDelivery::Melee));
        let live_neighbor = sim.spawn_unit(unit(1, 150, 0, AttackDelivery::Melee));
        launch(
            &mut sim,
            target,
            AcidBombProfile {
                homing,
                ..profile()
            },
        );
        sim.step();
        sim.world
            .get_mut::<Position>(entity(&sim, target))
            .unwrap()
            .0
            .x = 140 * SUBUNITS_PER_WORLD_UNIT;
        let mut restored = wire_restored(&sim, 4);
        let mut elapsed = 0;
        while sim.unit(target).unwrap().status.acid_bomb.is_none() {
            assert_eq!(
                {
                    sim.step();
                    sim.checksum()
                },
                {
                    restored.step();
                    restored.checksum()
                }
            );
            elapsed += 1;
            assert!(elapsed < 20);
        }
        assert!(if homing { elapsed > 4 } else { elapsed <= 4 });
        assert!(sim.unit(live_neighbor).unwrap().status.acid_bomb.is_some());
        assert!(
            sim.unit(initial_neighbor)
                .unwrap()
                .status
                .acid_bomb
                .is_none()
        );
    }
    for (immune, invulnerable) in [(true, false), (false, true)] {
        let mut sim = simulation(1);
        let target = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
        let neighbor = sim.spawn_unit(unit(1, 70, 0, AttackDelivery::Melee));
        launch(&mut sim, target, profile());
        sim.world
            .entity_mut(entity(&sim, target))
            .insert(UnitClassifications {
                spell_immune: immune,
                invulnerable,
                ..Default::default()
            });
        for _ in 0..10 {
            sim.step();
        }
        assert_eq!(sim.unit(target).unwrap().health, 1000);
        assert_eq!(sim.unit(neighbor).unwrap().health, 1000);
        assert_eq!(sim.projectile_count(), 0);
    }
}

fn parent(sim: &mut Simulation, child: AcidBombProfile) -> SimId {
    sim.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(1, 1, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: Some(SpellcastingProfile {
            mana: ManaProfile::per_second(10, 10, 0),
            ability: AutomaticAbilityProfile {
                id: AbilityId(98),
                mana_cost: 3,
                cooldown_ticks: 100,
                range: 500 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::BuildingAcidBomb(BuildingAcidBombProfile {
                    map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
                    trigger_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
                    trigger_invulnerable: true,
                    trigger_spell_immune: false,
                    child,
                    vision_radius: 50 * SUBUNITS_PER_WORLD_UNIT,
                    vision_ticks: 3,
                    attack_delay_ticks: 3,
                    attack_animation_ticks: 5,
                }),
            },
        }),
    })
}

#[test]
fn independent_native_parent_spends_resources_before_script_selection_and_child_failures() {
    for (combat, immune, mechanical, excluded, short_range, launches, animates) in [
        (true, false, false, false, false, true, true),
        (true, true, false, false, false, false, true),
        (true, false, true, false, false, false, true),
        (true, false, false, false, true, false, true),
        (true, false, false, true, false, false, false),
        (false, false, false, false, false, false, false),
    ] {
        let mut sim = simulation(1);
        let caster = parent(
            &mut sim,
            AcidBombProfile {
                range: if short_range { 1 } else { profile().range },
                ..profile()
            },
        );
        // A ground structure triggers the parent even though script selection chooses an air unit.
        sim.spawn_building(BuildingSpawn {
            team: Team(1),
            footprint: BuildingFootprint::new(5, 1, 1, 1),
            health: 1000,
            production: None,
            attack: None,
            spellcasting: None,
        });
        let target = sim.spawn_unit_with_properties(
            unit(1, 60, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                movement_class: MovementClass::Air,
                mechanical,
                build_time_ticks: mechanical.then_some(13),
                classifications: UnitClassifications {
                    combat_sapper: combat,
                    spell_immune: immune,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        if excluded {
            assert!(sim.set_negative_building_markers(target, false, true));
        }
        assert!(sim.set_negative_building_shield(target, 2, None));
        sim.step();
        let view = sim.building(caster).unwrap();
        assert_eq!(view.mana_current, Some(7));
        assert_eq!(view.ability_ready_tick, Some(100));
        assert_eq!(sim.projectile_count(), usize::from(launches));
        assert_eq!(view.status.action_animation.is_some(), animates);
        if animates {
            let animation = view.status.action_animation.unwrap();
            assert_eq!(animation.started_tick, 3);
            assert_eq!(animation.until_tick, 8);
            assert!(!animation.is_active(2));
            assert!(animation.is_active(3));
            assert!(!animation.is_active(8));
        }
        assert_eq!(sim.unit(target).unwrap().negative_building_shield_level, 2);
        let mut restored = wire_restored(&sim, 4);
        for _ in 0..9 {
            assert_eq!(
                {
                    sim.step();
                    sim.checksum()
                },
                {
                    restored.step();
                    restored.checksum()
                }
            );
        }
        assert!(
            sim.building(caster)
                .unwrap()
                .status
                .action_animation
                .is_none()
        );
    }
}

#[test]
fn acid_bomb_zero_area_still_hits_primary_and_expiry_does_not_add_a_damage_pulse() {
    let mut sim = simulation(1);
    let target = sim.spawn_unit(unit(1, 30, 0, AttackDelivery::Melee));
    let outside = sim.spawn_unit(unit(1, 31, 0, AttackDelivery::Melee));
    launch(
        &mut sim,
        target,
        AcidBombProfile {
            radius: 0,
            duration_ticks: 20,
            ..profile()
        },
    );
    let impact = impacted(&mut sim, target);
    while sim.tick() <= impact + 20 {
        sim.step();
    }
    assert_eq!(sim.unit(target).unwrap().health, 966);
    assert_eq!(sim.unit(outside).unwrap().health, 1000);
    assert!(sim.unit(target).unwrap().status.acid_bomb.is_none());
}
