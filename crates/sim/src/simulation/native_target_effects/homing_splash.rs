use super::*;
use crate::{PoisonProfile, SplashFalloffProfile};

fn source(sim: &mut Simulation) -> SimId {
    sim.spawn_unit_with_properties(
        unit(
            0,
            20,
            100,
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
            },
        ),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::from_slice(&[
                PassiveUnitEffect::SplashFalloff(SplashFalloffProfile {
                    full_radius: 5 * SUBUNITS_PER_WORLD_UNIT,
                    medium_radius: 15 * SUBUNITS_PER_WORLD_UNIT,
                    outer_radius: 25 * SUBUNITS_PER_WORLD_UNIT,
                    medium_damage_per_10k: 5000,
                    outer_damage_per_10k: 2500,
                    targets: AttackTargetMask::GROUND_UNITS,
                }),
                PassiveUnitEffect::Poison(PoisonProfile {
                    ability: AbilityId(17),
                    buff_rawcode: 18,
                    damage_per_second: 3,
                    movement_percent_delta: -7,
                    attack_speed_percent_delta: -11,
                    duration_ticks: 60,
                    hero_duration_ticks: 20,
                    targets: AttackTargetMask::AIR_AND_GROUND,
                    stack_damage: true,
                    can_kill: false,
                    organic_only: true,
                }),
            ]),
            ..Default::default()
        },
    )
}

#[test]
fn homing_splash_tracks_primary_and_keeps_primary_modifiers_out_of_collateral() {
    let mut sim = simulation(1);
    let shooter = source(&mut sim);
    let primary = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    // Pin the primary so ground collateral cannot change the launch selection.
    sim.world
        .get_mut::<TargetState>(entity(&sim, shooter))
        .unwrap()
        .current = Some(primary);
    sim.step();
    sim.step();
    assert_eq!(sim.projectile_count(), 1);
    sim.world.despawn(entity(&sim, shooter));
    let moved = SimPoint::new(100 * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT);
    sim.world
        .get_mut::<Position>(entity(&sim, primary))
        .unwrap()
        .0 = moved;
    let full = sim.spawn_unit(unit(1, 103, 0, AttackDelivery::Melee));
    let medium = sim.spawn_unit(unit(1, 110, 0, AttackDelivery::Melee));
    let outer = sim.spawn_unit(unit(1, 120, 0, AttackDelivery::Melee));
    let outside = sim.spawn_unit(unit(1, 130, 0, AttackDelivery::Melee));
    let ally = sim.spawn_unit(unit(0, 102, 0, AttackDelivery::Melee));
    let air = sim.spawn_unit_with_properties(
        unit(1, 102, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    let old_center = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
    let mut copy = wire_restored(&sim, 4);
    while sim.projectile_count() > 0 {
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                copy.step();
                copy.checksum()
            }
        );
    }
    assert_eq!(
        sim.unit(primary).unwrap().health,
        900,
        "primary is hit outside collateral's ground mask"
    );
    assert!(sim.unit(primary).unwrap().status.poison.is_some());
    for (id, expected) in [
        (full, 900),
        (medium, 950),
        (outer, 975),
        (outside, 1000),
        (ally, 1000),
        (air, 1000),
        (old_center, 1000),
    ] {
        assert_eq!(sim.unit(id).unwrap().health, expected);
        assert!(sim.unit(id).unwrap().status.poison.is_none());
    }
}

#[test]
fn protected_primary_still_centers_splash_but_dead_primary_invalidates_the_missile() {
    for removed in [false, true] {
        let mut sim = simulation(1);
        let shooter = source(&mut sim);
        let primary = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
        sim.world
            .get_mut::<TargetState>(entity(&sim, shooter))
            .unwrap()
            .current = Some(primary);
        sim.step();
        sim.step();
        let collateral = sim.spawn_unit(unit(1, 63, 0, AttackDelivery::Melee));
        if removed {
            sim.world.despawn(entity(&sim, primary));
        } else {
            sim.world
                .entity_mut(entity(&sim, primary))
                .insert(UnitClassifications {
                    invulnerable: true,
                    ..Default::default()
                });
        }
        let mut copy = wire_restored(&sim, 4);
        while sim.projectile_count() > 0 {
            assert_eq!(
                {
                    sim.step();
                    sim.checksum()
                },
                {
                    copy.step();
                    copy.checksum()
                }
            );
        }
        assert_eq!(
            sim.unit(collateral).unwrap().health,
            if removed { 1000 } else { 900 }
        );
        if !removed {
            assert_eq!(sim.unit(primary).unwrap().health, 1000);
            assert!(sim.unit(primary).unwrap().status.poison.is_none());
        }
    }
}

#[test]
fn primary_structure_gets_one_direct_hit_and_collateral_uses_its_own_structure_bit() {
    for structures in [false, true] {
        let mut sim = simulation(1);
        let shooter = source(&mut sim);
        let e = entity(&sim, shooter);
        let passive = *sim.world.get::<PassiveUnitEffects>(e).unwrap();
        let effects: Vec<_> = passive
            .iter()
            .map(|effect| match effect {
                PassiveUnitEffect::SplashFalloff(mut p) => {
                    p.medium_radius = 17 * SUBUNITS_PER_WORLD_UNIT;
                    p.targets = if structures {
                        AttackTargetMask::ALL
                    } else {
                        AttackTargetMask::AIR_AND_GROUND
                    };
                    PassiveUnitEffect::SplashFalloff(p)
                }
                other => other,
            })
            .collect();
        sim.world
            .entity_mut(e)
            .insert(PassiveUnitEffects::from_slice(&effects));
        let primary = sim.spawn_building(BuildingSpawn {
            team: Team(1),
            footprint: BuildingFootprint::new(60, 15, 10, 10),
            health: 1000,
            production: None,
            attack: None,
            spellcasting: None,
        });
        sim.world.get_mut::<TargetState>(e).unwrap().current = Some(primary);
        sim.step();
        sim.step();
        assert_eq!(sim.projectile_count(), 1);
        let secondary = sim.spawn_building(BuildingSpawn {
            team: Team(1),
            footprint: BuildingFootprint::new(76, 10, 20, 20),
            health: 1000,
            production: None,
            attack: None,
            spellcasting: None,
        });
        // The second footprint edge is in the medium ring; its center is outside it.
        let mut copy = wire_restored(&sim, 4);
        while sim.projectile_count() > 0 {
            assert_eq!(
                {
                    sim.step();
                    sim.checksum()
                },
                {
                    copy.step();
                    copy.checksum()
                }
            );
        }
        assert_eq!(sim.building(primary).unwrap().health, 900);
        assert_eq!(
            sim.building(secondary).unwrap().health,
            if structures { 950 } else { 1000 }
        );
    }
}
