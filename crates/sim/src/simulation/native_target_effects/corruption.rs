use super::*;
use crate::components::CorruptionProfile;

fn profile(ability: u32, reduction: i16) -> CorruptionProfile {
    CorruptionProfile {
        ability: AbilityId(ability),
        buff: crate::NativeBuffIdentity {
            rawcode: 77,
            positive: false,
            stealable: false,
            organic_only: false,
        },
        armor_reduction_per_100: reduction,
        duration_ticks: 9,
        hero_duration_ticks: 3,
        targets: AttackTargetMask::GROUND_UNITS.union(AttackTargetMask::AIR_UNITS),
    }
}
#[test]
fn attack_orb_refreshes_shared_family_and_permits_mechanical_spell_immune_targets() {
    let mut sim = Simulation::new(SimulationConfig::default(), 1);
    sim.spawn_unit_with_properties(
        unit(1, 0, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            mechanical: true,
            build_time_ticks: Some(10),
            classifications: UnitClassifications {
                spell_immune: true,
                resistant_skin: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let mut u = sim.snapshot_units();
    apply_corruption(TargetIndex::Unit(0), Some(profile(1, 250)), 2, &mut u);
    assert_eq!(u[0].status.armor_modifier_count, 1);
    assert_eq!(u[0].status.armor_modifiers[0].expires_tick, 5);
    apply_corruption(TargetIndex::Unit(0), Some(profile(2, 600)), 3, &mut u);
    assert_eq!(u[0].status.armor_modifier_count, 1);
    assert_eq!(effective_armor_points_per_100(&u[0]), -600);
    u[0].classifications.invulnerable = true;
    apply_corruption(TargetIndex::Unit(0), Some(profile(1, 250)), 4, &mut u);
    assert_eq!(u[0].status.armor_modifiers[0].expires_tick, 6);
}
#[test]
fn released_orb_missile_keeps_payload_after_source_death_and_wire_restore() {
    let mut sim = Simulation::new(SimulationConfig::default(), 1);
    let p = profile(2, 400);
    let source = sim.spawn_unit_with_properties(
        unit(
            0,
            20,
            10,
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
            },
        ),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::from_slice(&[PassiveUnitEffect::Corruption(p)]),
            ..Default::default()
        },
    );
    let target = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    assert!(!sim.projectiles().is_empty());
    let source_entity = sim
        .world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&source))
        .unwrap()
        .id();
    sim.world.get_mut::<Health>(source_entity).unwrap().current = 0;
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..22 {
        sim.step();
        restored.step();
    }
    assert_eq!(sim.checksum(), restored.checksum());
    let target = sim.unit(target).unwrap();
    assert!(
        target.health < 990,
        "orb lands before ordinary armor calculation"
    );
}

#[test]
fn passive_orb_hit_art_does_not_launch_an_extra_native_sphere() {
    let mut sim = simulation(1);
    let p = profile(2, 400);
    let source = sim.spawn_unit_with_properties(
        unit(0, 20, 10, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::from_slice(&[
                PassiveUnitEffect::Corruption(p),
                PassiveUnitEffect::Sphere(crate::components::SphereProfile {
                    ability: AbilityId(3),
                    attachment_count: 3,
                    cooldown_ticks: 4,
                    speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
                }),
            ]),
            ..Default::default()
        },
    );
    sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    assert!(
        sim.ability_casts_last_tick()
            .iter()
            .any(|cast| cast.ability == p.ability)
    );
    assert_eq!(
        sim.unit(source).unwrap().status.spheres[0]
            .unwrap()
            .next_slot,
        1
    );
}
