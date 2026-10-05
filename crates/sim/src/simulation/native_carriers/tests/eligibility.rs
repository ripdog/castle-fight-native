use super::*;

fn wire_restore(sim: &Simulation) -> Simulation {
    let content = crate::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let wire = sim.capture_snapshot().encode_wire().unwrap();
    let snapshot = SimulationSnapshot::decode_wire(&wire, content).unwrap();
    let mut restored = simulation(4);
    restored.restore_snapshot(&snapshot).unwrap();
    assert_eq!(restored.checksum(), sim.checksum());
    restored
}

fn setup(barrage: bool) -> (Simulation, SimId, SimId, SimId) {
    let mut sim = simulation(1);
    let range = if barrage {
        barrage_for_version(MapVersion::CASTLE_FIGHT_9_27)
            .unwrap()
            .range
    } else {
        carrier_for_version(MapVersion::CASTLE_FIGHT_9_27)
            .unwrap()
            .range
    };
    let source = if barrage {
        tower(
            &mut sim,
            crate::CastleFightTowerKind::ArcaneTower,
            Some(AttackProfile {
                damage: 17,
                range: range + 100 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: range + 100 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1000,
                delivery: AttackDelivery::RangedGuaranteedHit {
                    speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
                },
            }),
        )
    } else {
        tower(&mut sim, crate::CastleFightTowerKind::ObeliskOfLight, None)
    };
    if barrage {
        let primary = victim(
            &mut sim,
            range + 20 * SUBUNITS_PER_WORLD_UNIT,
            Default::default(),
        );
        sim.world
            .get_mut::<TargetState>(entity(&sim, source))
            .unwrap()
            .current = Some(primary);
    }
    // Carrier evidence can express a whole-map native radius. Geometry fixtures
    // stay in the synthetic navigation bounds instead of placing bodies at that radius.
    let test_distance = range.min(1000 * SUBUNITS_PER_WORLD_UNIT);
    let excluded = victim(
        &mut sim,
        test_distance / 2,
        UnitGameplayProperties {
            classifications: UnitClassifications {
                invulnerable: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    // Ordinary Barrage arrows may affect spell-immune, non-sapper units. A native
    // spell carrier uses its own immunity rule, not the script selector's class.
    let target = victim(
        &mut sim,
        test_distance * 3 / 4,
        UnitGameplayProperties {
            classifications: UnitClassifications {
                spell_immune: barrage,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    sim.step();
    sim.step();
    (sim, source, excluded, target)
}

#[test]
fn barrage_and_native_carrier_skip_avul_without_requiring_the_script_sapper_class() {
    for barrage in [false, true] {
        let (mut sim, source, excluded, target) = setup(barrage);
        let bolts: Vec<_> = sim
            .projectiles()
            .into_iter()
            .filter(|projectile| {
                matches!(
                    projectile.kind,
                    ProjectileViewKind::NativeCarrierBolt { .. }
                )
            })
            .collect();
        assert_eq!(bolts.len(), 1);
        assert!(
            matches!(bolts[0].kind, ProjectileViewKind::NativeCarrierBolt { target: id, .. } if id == target)
        );
        let due = bolts[0].impact_tick;
        assert!(sim.remove_building(source));
        while sim.next_tick <= due {
            sim.step();
        }
        assert_eq!(sim.unit(excluded).unwrap().health, 10_000);
        assert!(sim.unit(target).unwrap().health < 10_000);
    }
}

#[test]
fn avul_acquired_during_native_flight_blocks_damage_buff_and_cleanse_across_wire() {
    for barrage in [false, true] {
        let (mut sim, _source, excluded, target) = setup(barrage);
        let due = sim
            .projectiles()
            .into_iter()
            .find(|projectile| {
                matches!(
                    projectile.kind,
                    ProjectileViewKind::NativeCarrierBolt { .. }
                )
            })
            .unwrap()
            .impact_tick;
        status(&mut sim, target);
        let target_entity = entity(&sim, target);
        let mut flags = sim
            .world
            .get::<UnitClassifications>(target_entity)
            .copied()
            .unwrap_or_default();
        flags.invulnerable = true;
        sim.world.entity_mut(target_entity).insert(flags);
        let before = sim.unit(target).unwrap();
        let mut restored = wire_restore(&sim);
        // Keep the source live: otherwise the script's source-ability removal
        // predicate, rather than Avul, would independently explain no cleanse.
        while sim.next_tick <= due {
            assert_eq!(sim.step().checksum, restored.step().checksum);
        }
        let after = sim.unit(target).unwrap();
        assert_eq!(after.health, before.health);
        assert_eq!(
            after.status.stunned_until_tick,
            before.status.stunned_until_tick
        );
        assert_eq!(
            after.status.order_recovery_until_tick,
            before.status.order_recovery_until_tick
        );
        assert_eq!(
            after.status.armor_modifier_count,
            before.status.armor_modifier_count
        );
        assert_eq!(
            after.status.movement_modifier_count,
            before.status.movement_modifier_count
        );
        assert_eq!(after.status.damage_over_time_count, 0);
        assert_eq!(sim.unit(excluded).unwrap().health, 10_000);
        assert!(!sim.projectiles().iter().any(|projectile| matches!(
            projectile.kind,
            ProjectileViewKind::NativeCarrierBolt { .. }
        )));
    }
}
