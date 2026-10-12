use super::*;
use crate::{ImpaleFanProfile, ImpaleProfile};

fn point(x: i32, y: i32) -> SimPoint {
    SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, y * SUBUNITS_PER_WORLD_UNIT)
}
fn profile() -> ImpaleFanProfile {
    ImpaleFanProfile {
        cast_animation: ActionAnimationKind::CastDeath,
        parent: crate::NativeParasiteProfile {
            buff_rawcode: 991,
            map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
            speed_per_tick: 60 * SUBUNITS_PER_WORLD_UNIT,
            affects_invulnerable: false,
            duration_ticks: 1,
            hero_duration_ticks: 1,
            body_rawcode: u32::from_be_bytes(*b"nfbr"),
            count: 1,
            body_duration_ticks: 1,
        },
        child: ImpaleProfile {
            ability: AbilityId(992),
            damage: 30,
            distance: 60 * SUBUNITS_PER_WORLD_UNIT,
            radius: 2 * SUBUNITS_PER_WORLD_UNIT,
            wave_ticks: 6,
            air_ticks: 2,
            stun_ticks: 7,
            hero_stun_ticks: 3,
            targets: AttackTargetMask::GROUND_UNITS,
        },
        relative_angles_millidegrees: [0, 90_000, 180_000, 270_000],
        cast_point_distance: 2 * SUBUNITS_PER_WORLD_UNIT,
        dummy_lifetime_ticks: 20,
        dummy_rawcode: u32::from_be_bytes(*b"x001"),
        dummy_flying: true,
    }
}
fn caster(sim: &mut Simulation, p: ImpaleFanProfile) -> SimId {
    let mut spawn = unit(0, 30, 0, AttackDelivery::Melee);
    spawn.position = point(30, 30);
    sim.spawn_unit_with_spellcasting(
        spawn,
        SpellcastingProfile {
            mana: ManaProfile::per_second(9, 9, 0),
            ability: AutomaticAbilityProfile {
                id: AbilityId(993),
                mana_cost: 3,
                cooldown_ticks: 1000,
                range: 50 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::RandomGroundEnemyUnit,
                effect: AbilityEffect::ImpaleFan(p),
            },
        },
    )
}
fn recipient(sim: &mut Simulation, x: i32, y: i32, properties: UnitGameplayProperties) -> SimId {
    let mut spawn = unit(1, x, 0, AttackDelivery::Melee);
    spawn.position = point(x, y);
    sim.spawn_unit_with_properties(spawn, properties)
}
fn sapper() -> UnitGameplayProperties {
    UnitGameplayProperties {
        classifications: UnitClassifications {
            combat_sapper: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn impale_fan_uses_caster_origin_and_independent_lanes_with_damage_and_hero_stun_on_landing() {
    let mut sim = simulation(1);
    let source = caster(&mut sim, profile());
    let east = recipient(&mut sim, 70, 30, sapper());
    let north = recipient(
        &mut sim,
        30,
        50,
        UnitGameplayProperties {
            mechanical: true,
            build_time_ticks: Some(1),
            ..Default::default()
        },
    );
    let west = recipient(
        &mut sim,
        10,
        30,
        UnitGameplayProperties {
            classifications: UnitClassifications {
                hero: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let south = recipient(&mut sim, 30, 10, UnitGameplayProperties::default());
    let outside = recipient(&mut sim, 50, 50, UnitGameplayProperties::default());
    let air = recipient(
        &mut sim,
        75,
        30,
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    let immune = recipient(
        &mut sim,
        80,
        30,
        UnitGameplayProperties {
            classifications: UnitClassifications {
                spell_immune: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let locust = recipient(
        &mut sim,
        85,
        30,
        UnitGameplayProperties {
            classifications: UnitClassifications {
                locust: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let rooted = recipient(&mut sim, 90, 30, UnitGameplayProperties::default());
    sim.world
        .get_mut::<StatusState>(entity(&sim, rooted))
        .unwrap()
        .rooted_until_tick = 100;
    sim.step();
    assert_eq!(sim.unit(source).unwrap().mana_current, Some(6));
    sim.step();
    sim.step();
    for id in [north, west, south] {
        let view = sim.unit(id).unwrap();
        assert_eq!(view.health, 1000);
        assert!(view.classifications.invulnerable);
        assert_eq!(view.status.impale.unwrap().landed_tick, 4);
        assert_eq!(view.status.native_stun_ability, None);
    }
    let mut other = wire_restored(&sim, 4);
    let mut units = sim.snapshot_units();
    let i = find_unit_index(&units, north).unwrap();
    assert!(!apply_ability_effect_to_unit(
        &mut units[i],
        AbilityEffect::Damage { amount: 19 },
        Team(0),
        sim.next_tick,
        sim.combat_rules.damage_rules
    ));
    assert_eq!(units[i].health, 1000);
    for tick in 3..=12 {
        assert_eq!(sim.step().checksum, other.step().checksum);
        for id in [north, west, south] {
            assert_eq!(
                sim.unit(id).unwrap().health,
                if tick < 4 { 1000 } else { 970 }
            );
        }
        assert_eq!(
            sim.unit(east).unwrap().health,
            if tick < 6 { 1000 } else { 970 }
        );
    }
    for id in [outside, air, immune, locust, rooted] {
        assert_eq!(sim.unit(id).unwrap().health, 1000);
    }
    assert_eq!(sim.unit(west).unwrap().status.stunned_until_tick, 7);
    assert_eq!(sim.unit(north).unwrap().status.stunned_until_tick, 11);
    assert_eq!(sim.unit(east).unwrap().status.stunned_until_tick, 13);
    assert!(sim.unit(west).unwrap().status.impale.is_none());
}

#[test]
fn overlapping_impale_lanes_hit_once_and_released_waves_survive_source_death_and_block_airborne_missiles()
 {
    let mut sim = simulation(1);
    let mut p = profile();
    p.relative_angles_millidegrees = [0; 4];
    let source = caster(&mut sim, p);
    let target = recipient(&mut sim, 70, 30, sapper());
    let mut archer = unit(
        0,
        30,
        19,
        AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
        },
    );
    archer.position = point(30, 30);
    sim.spawn_unit(archer);
    sim.step();
    sim.step();
    sim.world
        .get_mut::<Health>(entity(&sim, source))
        .unwrap()
        .current = 0;
    for _ in 0..3 {
        sim.step();
    }
    assert_eq!(sim.unit(target).unwrap().health, 1000);
    assert!(sim.unit(target).unwrap().classifications.invulnerable);
    let mut other = wire_restored(&sim, 4);
    for _ in 0..8 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    assert_eq!(sim.unit(target).unwrap().health, 970);
    assert!(!sim.unit(target).unwrap().classifications.invulnerable);
}

#[test]
fn impale_parent_rejects_native_and_controller_ineligible_targets_before_committing_resources() {
    for (hero, immune, invulnerable, locust, air, combat_sapper, mechanical, distance, expected) in [
        (false, false, false, false, false, true, false, 50, true),
        (false, false, false, false, false, true, true, 50, true),
        (true, false, false, false, false, true, false, 40, false),
        (false, true, false, false, false, true, false, 40, false),
        (false, false, true, false, false, true, false, 40, false),
        (false, false, false, true, false, true, false, 40, false),
        (false, false, false, false, true, true, false, 40, false),
        (false, false, false, false, false, false, false, 40, false),
        (false, false, false, false, false, true, false, 51, false),
    ] {
        let mut sim = simulation(1);
        let source = caster(&mut sim, profile());
        recipient(
            &mut sim,
            30 + distance,
            30,
            UnitGameplayProperties {
                movement_class: if air {
                    MovementClass::Air
                } else {
                    MovementClass::Ground
                },
                mechanical,
                build_time_ticks: mechanical.then_some(1),
                classifications: UnitClassifications {
                    hero,
                    spell_immune: immune,
                    invulnerable,
                    locust,
                    combat_sapper,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        sim.step();
        assert_eq!(
            sim.unit(source).unwrap().mana_current,
            Some(if expected { 6 } else { 9 })
        );
    }
}

#[test]
fn impale_revalidates_native_parent_eligibility_at_windup_release_after_wire_restore() {
    let mut sim = simulation(1);
    let source = caster(&mut sim, profile());
    let target = recipient(&mut sim, 70, 30, sapper());
    {
        let mut timing = sim
            .world
            .get_mut::<ActionTimingProfile>(entity(&sim, source))
            .unwrap();
        timing.cast_point_ticks = 3;
        timing.cast_ticks = 6;
    }
    sim.step();
    assert!(sim.unit(source).unwrap().status.pending_cast.is_some());
    assert_eq!(
        sim.unit(source)
            .unwrap()
            .status
            .action_animation
            .unwrap()
            .kind,
        ActionAnimationKind::CastDeath
    );
    sim.world
        .get_mut::<UnitClassifications>(entity(&sim, target))
        .unwrap()
        .hero = true;
    let mut other = wire_restored(&sim, 4);
    for _ in 0..5 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    assert_eq!(sim.unit(source).unwrap().mana_current, Some(9));
    assert_eq!(sim.unit(target).unwrap().health, 1000);
    assert!(sim.unit(target).unwrap().status.impale.is_none());
}
