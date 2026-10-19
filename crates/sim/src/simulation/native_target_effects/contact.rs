use super::*;

#[test]
fn acquisition_can_be_shorter_than_reach_without_blocking_retaliatory_attacks() {
    let mut sim = simulation(1);
    let mut spawn = unit(0, 80, 11, AttackDelivery::RangedInstant);
    spawn.attack.range = 160 * SUBUNITS_PER_WORLD_UNIT;
    spawn.attack.acquisition_range = 80 * SUBUNITS_PER_WORLD_UNIT;
    let source = sim.spawn_unit(spawn);
    let mut passive = unit(1, 200, 0, AttackDelivery::Melee);
    passive.attack.range = 0;
    passive.attack.acquisition_range = 0;
    let target = sim.spawn_unit(passive);
    for _ in 0..2 {
        sim.step();
    }
    assert_eq!(sim.unit(source).unwrap().target, None);
    assert_eq!(sim.unit(target).unwrap().health, 1000);
    *sim.world
        .get_mut::<AttackProfile>(entity(&sim, target))
        .unwrap() = unit(1, 200, 3, AttackDelivery::RangedInstant).attack;
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..3 {
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
    assert_eq!(sim.unit(source).unwrap().target, Some(target));
    assert_eq!(sim.unit(target).unwrap().health, 989);
    assert_eq!(sim.unit(source).unwrap().attack, spawn.attack);
}

#[test]
fn zero_range_melee_uses_contact_for_pursuit_and_release_after_wire_restore() {
    let mut sim = simulation(1);
    let mut spawn = unit(0, 80, 10, AttackDelivery::Melee);
    spawn.attack.range = 0;
    spawn.movement.speed_per_tick = SUBUNITS_PER_WORLD_UNIT;
    let source =
        sim.spawn_unit_with_collision_radius(spawn, CollisionRadius(8 * SUBUNITS_PER_WORLD_UNIT));
    let target = sim.spawn_unit_with_collision_radius(
        unit(1, 104, 0, AttackDelivery::Melee),
        CollisionRadius(12 * SUBUNITS_PER_WORLD_UNIT),
    );
    sim.step();
    assert_eq!(sim.unit(target).unwrap().health, 1000);
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..20 {
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
    assert_eq!(sim.unit(target).unwrap().health, 990);
    let source = sim.unit(source).unwrap();
    assert_eq!(source.attack.range, 0);
    assert!(
        source
            .position
            .distance_sq(sim.unit(target).unwrap().position)
            >= square_i32(20 * SUBUNITS_PER_WORLD_UNIT)
    );
}

#[test]
fn zero_range_melee_contact_boundary_includes_both_bodies_or_the_structure_edge() {
    for (structure, gap, expected_damage) in
        [(false, 0, 10), (false, 3, 0), (true, 0, 10), (true, 3, 0)]
    {
        let mut sim = simulation(1);
        let mut source = unit(0, 80, 10, AttackDelivery::Melee);
        source.position.x -= gap;
        source.attack.range = 0;
        sim.spawn_unit_with_collision_radius(source, CollisionRadius(8 * SUBUNITS_PER_WORLD_UNIT));
        let target = if structure {
            sim.spawn_building(BuildingSpawn {
                team: Team(1),
                footprint: BuildingFootprint::new(88, 18, 4, 4),
                health: 1000,
                production: None,
                attack: None,
                spellcasting: None,
            })
        } else {
            sim.spawn_unit_with_collision_radius(
                unit(1, 100, 0, AttackDelivery::Melee),
                CollisionRadius(12 * SUBUNITS_PER_WORLD_UNIT),
            )
        };
        sim.step();
        sim.step();
        let health = if structure {
            sim.building(target).unwrap().health
        } else {
            sim.unit(target).unwrap().health
        };
        assert_eq!(
            health,
            1000 - expected_damage,
            "structure={structure} gap={gap}"
        );
    }
    let attack = AttackProfile {
        delivery: AttackDelivery::RangedInstant,
        range: 0,
        damage: 10,
        acquisition_range: 100,
        cooldown_ticks: 1,
    };
    assert_eq!(melee_contact_attack(attack, 23), attack);
    assert_eq!(
        melee_contact_attack(
            AttackProfile {
                delivery: AttackDelivery::Melee,
                range: 17,
                ..attack
            },
            23
        )
        .range,
        17
    );
}
