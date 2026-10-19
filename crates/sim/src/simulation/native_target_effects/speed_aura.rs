use super::*;
use crate::components::SpeedAuraProfile;

fn aura(ability: u32, family: u32, movement: i16, attack: i16) -> PassiveUnitEffect {
    PassiveUnitEffect::SpeedAura(SpeedAuraProfile {
        ability: AbilityId(ability),
        buff_rawcode: family,
        radius: 60 * SUBUNITS_PER_WORLD_UNIT,
        targets: AttackTargetMask::AIR_UNITS,
        hostile: true,
        organic_only: false,
        affects_invulnerable: false,
        movement_percent_delta: movement,
        attack_speed_percent_delta: attack,
    })
}

#[test]
fn intrinsic_speed_aura_samples_hostile_air_and_native_immunities() {
    for (team, movement_class, mechanical, spell_immune, invulnerable, x, expected) in [
        (1, MovementClass::Air, false, false, false, 80, true),
        (1, MovementClass::Air, true, false, false, 80, true),
        (0, MovementClass::Air, false, false, false, 80, false),
        (1, MovementClass::Ground, false, false, false, 80, false),
        (1, MovementClass::Air, false, true, false, 80, false),
        (1, MovementClass::Air, false, false, true, 80, false),
        (1, MovementClass::Air, false, false, false, 81, false),
    ] {
        let mut sim = simulation(1);
        sim.spawn_unit_with_properties(
            unit(0, 20, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(aura(101, 201, -30, -40)),
                ..Default::default()
            },
        );
        let target = sim.spawn_unit_with_properties(
            unit(team, x, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                movement_class,
                mechanical,
                build_time_ticks: mechanical.then_some(1),
                classifications: UnitClassifications {
                    spell_immune,
                    invulnerable,
                    invisible: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        sim.step();
        let status = sim.unit(target).unwrap().status;
        assert_eq!(status.movement_modifier_count > 0, expected);
        assert_eq!(status.attack_speed_modifier_count > 0, expected);
        if expected {
            assert_eq!(status.movement_modifiers[0].percent_delta, -30);
            assert_eq!(status.attack_speed_modifiers[0].percent_delta, -40);
            assert_eq!(status.armor_modifiers[0].native_buff.unwrap().rawcode, 201);
        }
    }
}

#[test]
fn intrinsic_speed_families_choose_each_strongest_delta_and_recover_without_linger() {
    let mut sim = simulation(1);
    let weak = sim.spawn_unit_with_properties(
        unit(0, 20, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(aura(101, 201, -10, -50)),
            ..Default::default()
        },
    );
    let strong = sim.spawn_unit_with_properties(
        unit(0, 25, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(aura(102, 201, -40, -20)),
            ..Default::default()
        },
    );
    let target = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    sim.step();
    let status = sim.unit(target).unwrap().status;
    assert_eq!(status.movement_modifier_count, 1);
    assert_eq!(status.movement_modifiers[0].percent_delta, -40);
    assert_eq!(status.attack_speed_modifiers[0].percent_delta, -50);
    sim.world.despawn(entity(&sim, strong));
    let mut restored = wire_restored(&sim, 4);
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
    assert_eq!(
        sim.unit(target).unwrap().status.movement_modifiers[0].percent_delta,
        -10
    );
    let source = entity(&sim, weak);
    sim.world.entity_mut(source).insert(UnitClassifications {
        invisible: true,
        ..Default::default()
    });
    let mut restored = wire_restored(&sim, 2);
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
    let status = sim.unit(target).unwrap().status;
    assert_eq!(status.movement_modifier_count, 0);
    assert_eq!(status.attack_speed_modifier_count, 0);
    assert_eq!(status.armor_modifier_count, 0);
}

fn friendly_aura(ability: u32, attack: i16) -> PassiveUnitEffect {
    let PassiveUnitEffect::SpeedAura(mut p) = aura(ability, 201, 0, attack) else {
        unreachable!()
    };
    p.hostile = false;
    p.affects_invulnerable = true;
    p.targets = AttackTargetMask::AIR_AND_GROUND;
    PassiveUnitEffect::SpeedAura(p)
}

#[test]
fn friendly_speed_aura_is_universal_including_invulnerable_air_and_mechanical_allies_and_self() {
    for (movement_class, mechanical, invulnerable, spell_immune) in [
        (MovementClass::Ground, false, false, false),
        (MovementClass::Air, true, true, true),
    ] {
        let mut sim = simulation(1);
        let source = sim.spawn_unit_with_properties(
            unit(0, 20, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(friendly_aura(101, 37)),
                ..Default::default()
            },
        );
        let target = sim.spawn_unit_with_properties(
            unit(0, 80, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                movement_class,
                mechanical,
                build_time_ticks: mechanical.then_some(1),
                classifications: UnitClassifications {
                    invulnerable,
                    spell_immune,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let enemy = sim.spawn_unit(unit(1, 50, 0, AttackDelivery::Melee));
        let outside = sim.spawn_unit(unit(0, 81, 0, AttackDelivery::Melee));
        sim.step();
        for id in [source, target] {
            let status = sim.unit(id).unwrap().status;
            assert_eq!(status.attack_speed_modifiers[0].percent_delta, 37);
            assert!(status.armor_modifiers[0].native_buff.unwrap().positive);
        }
        for id in [enemy, outside] {
            assert_eq!(sim.unit(id).unwrap().status.attack_speed_modifier_count, 0);
        }
    }
}

#[test]
fn friendly_speed_aura_shared_family_never_stacks_and_expires_after_source_loss_across_wire() {
    let mut sim = simulation(1);
    let weak = sim.spawn_unit_with_properties(
        unit(0, 20, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(friendly_aura(101, 37)),
            ..Default::default()
        },
    );
    let strong = sim.spawn_unit_with_properties(
        unit(0, 25, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(friendly_aura(102, 53)),
            ..Default::default()
        },
    );
    let target = sim.spawn_unit(unit(0, 60, 0, AttackDelivery::Melee));
    sim.step();
    let status = sim.unit(target).unwrap().status;
    assert_eq!(status.attack_speed_modifier_count, 1);
    assert_eq!(status.attack_speed_modifiers[0].percent_delta, 53);
    sim.world.despawn(entity(&sim, strong));
    let mut other = wire_restored(&sim, 4);
    assert_eq!(
        {
            sim.step();
            sim.checksum()
        },
        {
            other.step();
            other.checksum()
        }
    );
    assert_eq!(
        sim.unit(target).unwrap().status.attack_speed_modifiers[0].percent_delta,
        37
    );
    sim.world.despawn(entity(&sim, weak));
    other.world.despawn(entity(&other, weak));
    assert_eq!(
        {
            sim.step();
            sim.checksum()
        },
        {
            other.step();
            other.checksum()
        }
    );
    assert_eq!(
        sim.unit(target).unwrap().status.attack_speed_modifier_count,
        0
    );
}

#[test]
fn external_speed_aura_excludes_locust_recipients() {
    let mut sim = simulation(1);
    sim.spawn_unit_with_properties(
        unit(0, 20, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(friendly_aura(101, 37)),
            ..Default::default()
        },
    );
    let target = sim.spawn_unit_with_properties(
        unit(0, 50, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                locust: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    sim.step();
    assert_eq!(
        sim.unit(target).unwrap().status.attack_speed_modifier_count,
        0
    );
}
