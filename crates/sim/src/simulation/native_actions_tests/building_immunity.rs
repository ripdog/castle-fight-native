use super::*;

fn immune(invulnerable: bool) -> UnitClassifications {
    UnitClassifications {
        invulnerable,
        spell_immune: !invulnerable,
        ..Default::default()
    }
}

fn structure(sim: &mut Simulation, classifications: UnitClassifications) -> SimId {
    sim.spawn_building_with_properties(
        BuildingSpawn {
            team: Team(1),
            footprint: BuildingFootprint::new(1, 0, 1, 1),
            health: 100,
            production: None,
            attack: None,
            spellcasting: None,
        },
        BuildingGameplayProperties {
            classifications,
            ..Default::default()
        },
    )
}

fn wire_restore(original: &Simulation) -> Simulation {
    let content =
        crate::castle_fight_content_bundle(crate::CASTLE_FIGHT_DEFAULT_MAP_VERSION).unwrap();
    let wire = original.capture_snapshot().encode_wire().unwrap();
    let decoded = SimulationSnapshot::decode_wire(&wire, content).unwrap();
    let mut restored = sim(4);
    restored.restore_snapshot(&decoded).unwrap();
    assert_eq!(original.checksum(), restored.checksum());
    restored
}

fn status(sim: &Simulation, id: SimId) -> StatusState {
    sim.world
        .get::<StatusState>(entity(sim, id))
        .copied()
        .unwrap_or_default()
}

fn burnt_structure() -> (Simulation, SimId, TimedDamageOverTime) {
    let mut original = sim(1);
    caster(&mut original, fire());
    let target = structure(&mut original, UnitClassifications::default());
    original.step();
    let impact = original.projectiles()[0].impact_tick;
    while original.next_tick <= impact {
        original.step();
    }
    assert_eq!(original.building(target).unwrap().health, 93);
    let status = status(&original, target);
    assert_eq!(status.damage_over_time_count, 1);
    assert_eq!(original.building(target).unwrap().status, status);
    assert_eq!(
        wire_restore(&original).building(target).unwrap().status,
        status
    );
    (original, target, status.damage_over_time[0])
}

#[test]
fn declared_structure_immunity_blocks_launch_and_acquired_immunity_blocks_committed_impact() {
    for invulnerable in [false, true] {
        for before_launch in [false, true] {
            let mut original = sim(1);
            caster(&mut original, fire());
            let target = structure(
                &mut original,
                if before_launch {
                    immune(invulnerable)
                } else {
                    UnitClassifications::default()
                },
            );
            original.step();
            if before_launch {
                assert!(original.projectiles().is_empty());
            } else {
                assert_eq!(original.projectiles().len(), 1);
                let e = entity(&original, target);
                original.world.entity_mut(e).insert(immune(invulnerable));
            }
            let mut restored = wire_restore(&original);
            for _ in 0..10 {
                assert_eq!(original.step().checksum, restored.step().checksum);
            }
            assert_eq!(restored.building(target).unwrap().health, 100);
            assert_eq!(status(&restored, target).damage_over_time_count, 0);
            assert!(restored.projectiles().is_empty());
        }
    }
}

#[test]
fn live_structure_dot_immunity_advances_deadlines_and_expires_without_deferred_damage() {
    for invulnerable in [false, true] {
        let (mut original, target, effect) = burnt_structure();
        let e = entity(&original, target);
        original.world.entity_mut(e).insert(immune(invulnerable));
        let mut restored = wire_restore(&original);
        while original.next_tick <= effect.expires_tick {
            let tick = original.next_tick;
            assert_eq!(original.step().checksum, restored.step().checksum);
            assert_eq!(original.building(target).unwrap().health, 93);
            let state = status(&original, target);
            if tick < effect.expires_tick {
                assert_eq!(state.damage_over_time_count, 1);
                assert!(state.damage_over_time[0].next_pulse_tick > tick);
            } else {
                assert_eq!(state.damage_over_time_count, 0);
            }
        }
        for simulation in [&mut original, &mut restored] {
            let e = entity(simulation, target);
            simulation
                .world
                .entity_mut(e)
                .remove::<UnitClassifications>();
        }
        let metrics = original.step();
        assert_eq!(metrics.checksum, restored.step().checksum);
        assert_eq!(metrics.ability_casts, 1);
        assert_eq!(original.building(target).unwrap().health, 93);
    }
}

#[test]
fn temporary_structure_immunity_skips_only_covered_pulses_and_preserves_the_final_pulse() {
    for invulnerable in [false, true] {
        let (mut original, target, effect) = burnt_structure();
        let e = entity(&original, target);
        original.world.entity_mut(e).insert(immune(invulnerable));
        let mut restored = wire_restore(&original);
        while original.next_tick <= effect.next_pulse_tick {
            assert_eq!(original.step().checksum, restored.step().checksum);
            assert_eq!(original.building(target).unwrap().health, 93);
        }
        assert_eq!(
            status(&original, target).damage_over_time[0].next_pulse_tick,
            effect.next_pulse_tick + u64::from(effect.pulse_interval_ticks)
        );
        for simulation in [&mut original, &mut restored] {
            let e = entity(simulation, target);
            simulation
                .world
                .entity_mut(e)
                .remove::<UnitClassifications>();
        }
        while original.next_tick <= effect.expires_tick {
            let tick = original.next_tick;
            assert_eq!(original.step().checksum, restored.step().checksum);
            let elapsed = (tick - effect.next_pulse_tick) / u64::from(effect.pulse_interval_ticks);
            assert_eq!(
                original.building(target).unwrap().health,
                93 - elapsed as i32 * effect.damage_per_pulse
            );
        }
        assert_eq!(status(&original, target).damage_over_time_count, 0);
        assert_eq!(
            original.projectiles().len(),
            1,
            "buff expiry allows new fire on the same tick as its final pulse"
        );
    }
}
