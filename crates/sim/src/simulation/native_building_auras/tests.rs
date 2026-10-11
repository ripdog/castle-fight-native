use super::*;

fn profile() -> NativeBuildingAuraProfile {
    NativeBuildingAuraProfile {
        map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
        source_rawcode: 500,
        inventory: AbilityId(501),
        armor_ability: AbilityId(502),
        armor_buff: 503,
        armor_radius: 10,
        armor_bonus_per_100: 350,
        armor_targets: AttackTargetMask::BUILDINGS,
        armor_invulnerable: true,
        regeneration_ability: AbilityId(504),
        regeneration_radius: 20,
        regeneration_per_second_per_10k: 60_000,
        regeneration_targets: AttackTargetMask::AIR_AND_GROUND,
        regeneration_invulnerable: true,
    }
}
fn recipient(building: bool, x: i32) -> Recipient {
    Recipient {
        team: Team(0),
        position: SimPoint::new(x, 0),
        alive: true,
        building,
        movement: MovementClass::Ground,
        invulnerable: false,
    }
}
fn sources() -> [AuraSource; 2] {
    [AuraSource {
        version: profile().map_version,
        team: Team(0),
        position: SimPoint::new(0, 0),
        alive: true,
    }; 2]
}

#[test]
fn native_aura_masks_radius_and_stacking_are_independent() {
    let p = profile();
    let sources = sources();
    let mut status = StatusState::default();
    refresh(&mut status, recipient(true, 10), p, &sources, 2);
    assert_eq!(status.armor_modifier_count, 1);
    let m = status.armor_modifiers[0];
    assert_eq!(
        (m.armor_bonus_per_100, m.regeneration_per_second_per_10k),
        (350, 0)
    );
    assert_eq!(m.native_buff.unwrap().rawcode, p.armor_buff);
    refresh(&mut status, recipient(true, 11), p, &sources, 3);
    assert_eq!(status.armor_modifier_count, 0);
    for movement in [MovementClass::Ground, MovementClass::Air] {
        let mut r = recipient(false, 20);
        r.movement = movement;
        r.invulnerable = true;
        refresh(&mut status, r, p, &sources, 4);
        let m = status.armor_modifiers[0];
        assert_eq!(
            (m.armor_bonus_per_100, m.regeneration_per_second_per_10k),
            (0, 120_000)
        );
        assert!(m.native_buff.is_none());
        super::super::native_summons::dispel_magic_buffs(&mut status);
        assert_eq!(status.armor_modifier_count, 1);
        r.position.x = 21;
        refresh(&mut status, r, p, &sources, 5);
        assert_eq!(status.armor_modifier_count, 0);
    }
}

#[test]
fn aura_membership_clears_on_death_allegiance_version_and_source_removal() {
    let p = profile();
    for change in 0..5 {
        let mut status = StatusState::default();
        refresh(&mut status, recipient(false, 0), p, &sources(), 1);
        let mut s = sources();
        let mut r = recipient(false, 0);
        match change {
            0 => r.alive = false,
            1 => r.team = Team(1),
            2 => s.iter_mut().for_each(|s| s.alive = false),
            3 => s
                .iter_mut()
                .for_each(|s| s.version = crate::MapVersion { major: 1, minor: 0 }),
            _ => {}
        }
        let sources = if change == 4 { &[][..] } else { &s[..] };
        refresh(&mut status, r, p, sources, 2);
        assert_eq!(status.armor_modifier_count, 0);
    }
}

fn entity(sim: &Simulation, id: SimId) -> Entity {
    sim.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}

#[test]
fn completed_sources_refresh_regeneration_and_unarmed_structure_armor_across_wire() {
    let mut sim = Simulation::new(SimulationConfig::default(), 1);
    let source = crate::CastleFightTowerKind::CoralStatue.definition();
    let aura = sim
        .try_start_building_construction(
            PlayerId(0),
            source.spawn(Team(0), BuildingFootprint::new(0, 0, 2, 2)),
            BuildingGameplayProperties {
                construction_time_ticks: Some(3),
                ..source.gameplay_properties()
            },
        )
        .unwrap();
    let target = sim.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(3, 0, 2, 2),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: None,
    });
    let unit = sim.spawn_unit(UnitSpawn {
        team: Team(0),
        position: SimPoint::new(0, 0),
        health: 1000,
        attack: AttackProfile {
            damage: 0,
            range: 0,
            acquisition_range: 0,
            cooldown_ticks: 1000,
            delivery: AttackDelivery::Melee,
        },
        movement: MovementProfile { speed_per_tick: 0 },
    });
    sim.world
        .get_mut::<Health>(entity(&sim, unit))
        .unwrap()
        .current = 500;
    for _ in 0..3 {
        sim.step();
        assert_eq!(sim.unit(unit).unwrap().health, 500);
        assert_eq!(sim.building(target).unwrap().status.armor_modifier_count, 0);
    }
    sim.step();
    let p =
        crate::building_mechanics::coral_auras_for_version(crate::MapVersion::CASTLE_FIGHT_9_27)
            .unwrap();
    assert_eq!(
        sim.building(target).unwrap().status.armor_modifiers[0].armor_bonus_per_100,
        p.armor_bonus_per_100
    );
    let bundle = crate::castle_fight_content_bundle(p.map_version).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), bundle)
            .unwrap();
    let mut restored = Simulation::new(sim.config.clone(), 4);
    restored.restore_snapshot(&snapshot).unwrap();
    for _ in 0..30 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    assert!(sim.unit(unit).unwrap().health > 500);
    assert!(sim.remove_building(aura));
    assert!(restored.remove_building(aura));
    assert_eq!(sim.step().checksum, restored.step().checksum);
    assert_eq!(sim.building(target).unwrap().status.armor_modifier_count, 0);
    assert_eq!(sim.unit(unit).unwrap().status.armor_modifier_count, 0);
}
