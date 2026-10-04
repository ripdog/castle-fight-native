use super::*;

#[test]
fn committed_native_missiles_follow_moving_targets_and_restore_live_flight() {
    let version = MapVersion::CASTLE_FIGHT_9_27;
    for damage_type in [None, Some(DamageType::Magic)] {
        let mut sim = simulation(1);
        let target = victim(
            &mut sim,
            2000 * SUBUNITS_PER_WORLD_UNIT,
            UnitGameplayProperties::default(),
        );
        let source = sim.allocate_id();
        let (ability, speed) = if damage_type.is_some() {
            let profile = barrage_for_version(version).unwrap();
            (profile.ability, profile.speed_per_tick)
        } else {
            let profile = carrier_for_version(version).unwrap();
            (profile.ability, profile.speed_per_tick)
        };
        let origin = SimPoint::new(0, SUBUNITS_PER_WORLD_UNIT);
        let first_destination = sim.unit(target).unwrap().position;
        let original_deadline =
            projectile_travel_ticks(origin.distance_sq(first_destination), speed);
        sim.spawn_native_bolt(NativeCarrierBolt {
            source,
            visual_source: source,
            source_team: Team(0),
            target,
            ability,
            map_version: version,
            damage: 17,
            attack_damage_type: damage_type,
            launch_position: origin,
            launch_tick: 0,
            position: origin,
            position_tick: 0,
            impact_tick: original_deadline,
        });
        sim.step();
        sim.step();
        let old_position = sim.projectiles()[0].launch_position;
        assert_ne!(old_position, origin);
        let target_entity = entity(&sim, target);
        sim.world.get_mut::<Position>(target_entity).unwrap().0 =
            SimPoint::new(3000 * SUBUNITS_PER_WORLD_UNIT, SUBUNITS_PER_WORLD_UNIT);
        sim.step();
        let mut restored = restore(&sim, 4);
        for _ in sim.next_tick..=original_deadline {
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
        }
        assert_eq!(sim.unit(target).unwrap().health, 10_000);
        assert_eq!(sim.projectiles().len(), 1);
        assert!(sim.projectiles()[0].impact_tick > original_deadline);
        for _ in 0..400 {
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
            if sim.projectiles().is_empty() {
                break;
            }
        }
        assert!(
            sim.projectiles().is_empty(),
            "committed homing missile must reach the moved target"
        );
        assert!(sim.unit(target).unwrap().health < 10_000);
    }
}
