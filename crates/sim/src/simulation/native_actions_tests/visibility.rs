use super::*;

fn wire_restore(original: &Simulation) -> Simulation {
    let content =
        crate::castle_fight_content_bundle(crate::CASTLE_FIGHT_DEFAULT_MAP_VERSION).unwrap();
    let wire = original.capture_snapshot().encode_wire().unwrap();
    let snapshot = SimulationSnapshot::decode_wire(&wire, content).unwrap();
    let mut restored = sim(4);
    restored.restore_snapshot(&snapshot).unwrap();
    assert_eq!(original.checksum(), restored.checksum());
    restored
}

#[test]
fn native_fire_requires_own_team_reveal_but_committed_flight_survives_reveal_expiry() {
    let mut original = sim(1);
    caster(&mut original, fire());
    let target = unit(&mut original, 1, 25, MovementClass::Ground, false);
    let target_entity = entity(&original, target);
    original
        .world
        .entity_mut(target_entity)
        .insert(UnitClassifications {
            invisible: true,
            combat_sapper: true,
            ..Default::default()
        });
    assert_eq!(original.step().ability_casts, 0);
    assert!(original.projectiles().is_empty());
    {
        let mut status = original
            .world
            .get_mut::<StatusState>(target_entity)
            .unwrap();
        apply_timed_armor_modifier(
            &mut status,
            TimedArmorModifier {
                id: ModifierId(200),
                expires_tick: 20,
                revealed_to: Some(Team(1)),
                ..Default::default()
            },
        );
    }
    assert_eq!(
        original.step().ability_casts,
        0,
        "another team's reveal does not grant native targeting"
    );
    let expiry = original.next_tick + 1;
    {
        let mut status = original
            .world
            .get_mut::<StatusState>(target_entity)
            .unwrap();
        apply_timed_armor_modifier(
            &mut status,
            TimedArmorModifier {
                id: ModifierId(201),
                expires_tick: expiry,
                revealed_to: Some(Team(0)),
                ..Default::default()
            },
        );
    }
    assert_eq!(original.step().ability_casts, 1);
    let mut restored = wire_restore(&original);
    assert_eq!(original.step().checksum, restored.step().checksum);
    let view = original.unit(target).unwrap();
    assert!(!view.status.is_revealed_to(Team(0), expiry));
    assert_eq!(
        view.health, 93,
        "impact does not rerun native launch visibility"
    );
}

#[test]
fn live_native_dot_immunity_advances_pulses_and_expiry_without_deferred_damage() {
    for invulnerable in [false, true] {
        let mut original = sim(1);
        caster(&mut original, fire());
        let target = unit(&mut original, 1, 25, MovementClass::Ground, false);
        original.step();
        original.step();
        let target_entity = entity(&original, target);
        let before = original.unit(target).unwrap();
        let expiry = before.status.damage_over_time[0].expires_tick;
        original
            .world
            .entity_mut(target_entity)
            .insert(UnitClassifications {
                invulnerable,
                spell_immune: !invulnerable,
                combat_sapper: true,
                ..Default::default()
            });
        // Restore with active native buff and changed live classification, not just
        // pre-hit immunity that would have prevented the buff from existing.
        let mut restored = wire_restore(&original);
        while original.next_tick <= expiry {
            assert_eq!(original.step().checksum, restored.step().checksum);
            assert_eq!(original.unit(target).unwrap().health, before.health);
        }
        assert_eq!(
            original.unit(target).unwrap().status.damage_over_time_count,
            0
        );
        original
            .world
            .entity_mut(target_entity)
            .insert(UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            });
        let restored_entity = entity(&restored, target);
        restored
            .world
            .entity_mut(restored_entity)
            .insert(UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            });
        let metrics = original.step();
        assert_eq!(metrics.checksum, restored.step().checksum);
        assert_eq!(
            metrics.ability_casts, 1,
            "expired native buff no longer excludes the vulnerable target"
        );
        assert_eq!(
            original.unit(target).unwrap().health,
            before.health,
            "skipped pulses must not be replayed after immunity ends"
        );
    }
}
