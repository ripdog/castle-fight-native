use super::*;
use crate::components::SphereProfile;

#[test]
fn native_sphere_flies_independently_without_damage_and_restores_each_attachment() {
    let mut sim = simulation(1);
    let source = sim.spawn_unit_with_properties(
        unit(0, 20, 7, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Sphere(SphereProfile {
                ability: AbilityId(107),
                attachment_count: 3,
                cooldown_ticks: 3,
                speed_per_tick: 2 * SUBUNITS_PER_WORLD_UNIT,
            })),
            ..Default::default()
        },
    );
    let target = sim.spawn_unit(unit(1, 50, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    assert_eq!(sim.unit(target).unwrap().health, 993);
    assert_eq!(
        sim.next_id,
        target.0 + 1,
        "visual flights must not consume gameplay identities or change future random rolls"
    );
    let sphere = sim.unit(source).unwrap().status.spheres[0].unwrap();
    assert_eq!(sphere.hidden_until, [4, 0, 0]);
    assert_eq!(sphere.next_slot, 1);
    let projectiles = sim.projectiles();
    let visual = projectiles
        .iter()
        .find(|projectile| projectile.ability == Some(AbilityId(107)))
        .unwrap();
    assert_eq!(visual.impact_tick, 16);
    assert!(matches!(
        visual.kind,
        ProjectileViewKind::SphereFlight {
            source_movement: MovementClass::Air,
            target_movement: MovementClass::Ground,
            ..
        }
    ));
    // The visual remains authoritative after its source disappears; it cannot damage on arrival.
    sim.world.despawn(entity(&sim, source));
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..16 {
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
    assert_eq!(sim.unit(target).unwrap().health, 993);
    assert!(sim.projectiles().is_empty());
}

#[test]
fn native_sphere_cycles_independent_slots_on_attack_and_spell_without_target_payload() {
    let mut sim = simulation(1);
    let source = sim.spawn_unit_with_properties(
        unit(0, 20, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Sphere(SphereProfile {
                ability: AbilityId(107),
                attachment_count: 3,
                cooldown_ticks: 3,
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
            })),
            ..Default::default()
        },
    );
    let mut units = sim.snapshot_units();
    for _ in 0..4 {
        sim.last_ability_casts = vec![AbilityCastEvent {
            source,
            ability: AbilityId(108),
            target: AbilityCastTarget::Point(units[0].position),
            target_position: None,
            effect: AbilityEffect::Damage { amount: 0 },
        }];
        sim.release_spheres(&mut units);
        sim.next_tick += 1;
    }
    let state = units[0].status.spheres[0].unwrap();
    assert_eq!(state.next_slot, 1);
    assert_eq!(state.hidden_until, [6, 4, 5]);
    assert!(sim.projectiles().is_empty());
}

#[test]
fn zero_speed_native_sphere_preserves_attachment_recovery_without_inventing_a_flight() {
    let mut sim = simulation(1);
    let source = sim.spawn_unit_with_properties(
        unit(0, 20, 7, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Sphere(SphereProfile {
                ability: AbilityId(107),
                attachment_count: 2,
                cooldown_ticks: 3,
                speed_per_tick: 0,
            })),
            ..Default::default()
        },
    );
    let target = sim.spawn_unit(unit(1, 50, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    assert_eq!(sim.unit(target).unwrap().health, 993);
    assert_eq!(
        sim.unit(source).unwrap().status.spheres[0]
            .unwrap()
            .hidden_until,
        [4, 0, 0]
    );
    assert!(sim.projectiles().is_empty());
    let mut other = wire_restored(&sim, 4);
    for _ in 0..3 {
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
    }
    assert_eq!(sim.next_id, target.0 + 1);
}
