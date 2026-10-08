use super::*;
use crate::EnsnareProfile;

fn profile() -> SpellcastingProfile {
    SpellcastingProfile {
        mana: ManaProfile::per_second(10, 10, 0),
        ability: AutomaticAbilityProfile {
            id: AbilityId(101),
            mana_cost: 3,
            cooldown_ticks: 1000,
            range: 100 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::FlyingEnemyUnit,
            effect: AbilityEffect::Ensnare {
                profile: EnsnareProfile {
                    ability: AbilityId(102),
                    duration_ticks: 5,
                    hero_duration_ticks: 2,
                    range: 100 * SUBUNITS_PER_WORLD_UNIT,
                    speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
                    lowering_ticks: 2,
                    air_height: 0,
                    melee_range: 200 * SUBUNITS_PER_WORLD_UNIT,
                },
                search_radius: 0,
                maximum_targets: 1,
            },
        },
    }
}

fn flying(
    sim: &mut Simulation,
    team: u8,
    x: i32,
    damage: i32,
    classes: UnitClassifications,
) -> SimId {
    sim.spawn_unit_with_properties(
        unit(team, x, damage, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            classifications: classes,
            ..UnitGameplayProperties::default()
        },
    )
}

fn combat() -> UnitClassifications {
    UnitClassifications {
        combat_sapper: true,
        ..UnitClassifications::default()
    }
}

#[test]
fn net_waits_for_impact_roots_only_movement_and_opens_ground_weapon_targeting() {
    let mut sim = simulation(1);
    let caster = sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), profile());
    let target = flying(&mut sim, 1, 60, 100, combat());
    sim.spawn_unit_with_properties(
        unit(0, 40, 10, AttackDelivery::Melee),
        UnitGameplayProperties {
            attack_targets: AttackTargetMask::GROUND_UNITS,
            ..UnitGameplayProperties::default()
        },
    );
    sim.step();
    assert_eq!(sim.unit(caster).unwrap().mana_current, Some(7));
    let projectile = sim.projectiles().into_iter().next().unwrap();
    assert_eq!(projectile.ability, Some(AbilityId(102)));
    assert_eq!(
        projectile.launch_position,
        sim.unit(caster).unwrap().position
    );
    let mut restored = wire_restored(&sim, 6);
    for _ in 0..3 {
        sim.step();
        restored.step();
        assert!(sim.unit(target).unwrap().status.ensnare.is_none());
        assert_eq!(sim.unit(target).unwrap().health, 1000);
        assert_eq!(sim.checksum(), restored.checksum());
    }
    sim.step();
    restored.step();
    assert_eq!(sim.checksum(), restored.checksum());
    let caught = sim.unit(target).unwrap();
    assert_eq!(caught.health, 990);
    assert_eq!(caught.movement_class, MovementClass::Air);
    assert_eq!(caught.status.ensnare.unwrap().expires_tick, 9);
    assert!(!caught.status.attacks_disabled(sim.next_tick));
    let before = caught.position;
    let target_entity = entity(&sim, target);
    let mut target_entity = sim.world.entity_mut(target_entity);
    target_entity
        .get_mut::<MovementProfile>()
        .unwrap()
        .speed_per_tick = 4 * SUBUNITS_PER_WORLD_UNIT;
    target_entity.get_mut::<AttackCooldown>().unwrap().remaining = 0;
    sim.step();
    assert_eq!(sim.unit(target).unwrap().position, before);
    assert!(
        sim.attacks_last_tick()
            .iter()
            .any(|event| event.source == target)
    );
}

#[test]
fn committed_net_survives_caster_death_permits_acquired_immunity_and_uses_hero_expiry() {
    let mut sim = simulation(1);
    let caster = sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), profile());
    let target = flying(
        &mut sim,
        1,
        60,
        0,
        UnitClassifications {
            hero: true,
            ..combat()
        },
    );
    sim.step();
    sim.world
        .entity_mut(entity(&sim, caster))
        .get_mut::<Health>()
        .unwrap()
        .current = 0;
    sim.world
        .entity_mut(entity(&sim, target))
        .get_mut::<UnitClassifications>()
        .unwrap()
        .spell_immune = true;
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..4 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(
        sim.unit(target)
            .unwrap()
            .status
            .ensnare
            .unwrap()
            .expires_tick,
        6
    );
    sim.step();
    restored.step();
    assert!(sim.unit(target).unwrap().status.ensnare.is_some());
    sim.step();
    restored.step();
    assert!(sim.unit(target).unwrap().status.ensnare.is_none());
    assert_eq!(sim.checksum(), restored.checksum());
}

#[test]
fn parent_requires_viable_hostile_flying_combat_target_before_spending_resources() {
    for (team, movement, classes) in [
        (1, MovementClass::Air, UnitClassifications::default()),
        (1, MovementClass::Ground, combat()),
        (0, MovementClass::Air, combat()),
        (
            1,
            MovementClass::Air,
            UnitClassifications {
                invulnerable: true,
                ..combat()
            },
        ),
        (
            1,
            MovementClass::Air,
            UnitClassifications {
                spell_immune: true,
                ..combat()
            },
        ),
    ] {
        let mut sim = simulation(1);
        let caster =
            sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), profile());
        sim.spawn_unit_with_properties(
            unit(team, 60, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                movement_class: movement,
                classifications: classes,
                ..UnitGameplayProperties::default()
            },
        );
        sim.step();
        assert_eq!(sim.unit(caster).unwrap().mana_current, Some(10));
        assert_eq!(sim.projectile_count(), 0);
    }
}

#[test]
fn a_competing_net_during_cast_windup_prevents_duplicate_resource_commitment() {
    let mut sim = simulation(1);
    let p = profile();
    let caster = sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), p);
    sim.world
        .entity_mut(entity(&sim, caster))
        .insert(ActionTimingProfile {
            cast_point_ticks: 2,
            cast_ticks: 2,
            ..ActionTimingProfile::default()
        });
    let target = flying(&mut sim, 1, 60, 0, combat());
    sim.step();
    assert!(sim.unit(caster).unwrap().status.pending_cast.is_some());
    let AbilityEffect::Ensnare { profile, .. } = p.ability.effect else {
        unreachable!()
    };
    sim.world
        .entity_mut(entity(&sim, target))
        .get_mut::<StatusState>()
        .unwrap()
        .ensnare = Some(crate::EnsnareState {
        profile,
        applied_tick: 1,
        expires_tick: 10,
    });
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..2 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    let state = *sim
        .world
        .entity(entity(&sim, caster))
        .get::<AutomaticAbilityState>()
        .unwrap();
    let caster = sim.unit(caster).unwrap();
    assert_eq!(caster.mana_current, Some(10));
    assert_eq!(state.cast_sequence, 0);
    assert!(caster.status.pending_cast.is_none());
    assert_eq!(sim.projectile_count(), 0);
}

#[test]
fn bounded_search_is_target_centered_but_each_native_attempt_uses_caster_range() {
    let mut sim = simulation(1);
    let mut p = profile();
    p.ability.range = 40 * SUBUNITS_PER_WORLD_UNIT;
    let AbilityEffect::Ensnare {
        profile: mut net, ..
    } = p.ability.effect
    else {
        unreachable!()
    };
    net.range = 80 * SUBUNITS_PER_WORLD_UNIT;
    p.ability.effect = AbilityEffect::Ensnare {
        profile: net,
        search_radius: 55 * SUBUNITS_PER_WORLD_UNIT,
        maximum_targets: 3,
    };
    sim.spawn_unit_with_spellcasting(unit(0, 20, 0, AttackDelivery::Melee), p);
    let selected = flying(&mut sim, 1, 60, 0, combat());
    let out_of_native_range = flying(&mut sim, 1, 115, 0, combat());
    let immune = flying(
        &mut sim,
        1,
        70,
        0,
        UnitClassifications {
            spell_immune: true,
            ..combat()
        },
    );
    let fourth = flying(&mut sim, 1, 80, 0, combat());
    sim.step();
    assert_eq!(sim.projectile_count(), 2);
    for _ in 0..6 {
        sim.step();
    }
    for target in [selected, immune] {
        assert!(sim.unit(target).unwrap().status.ensnare.is_some());
    }
    for target in [out_of_native_range, fourth] {
        assert!(sim.unit(target).unwrap().status.ensnare.is_none());
    }
}
