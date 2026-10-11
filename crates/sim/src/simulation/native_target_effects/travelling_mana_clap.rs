use super::*;

fn profile() -> crate::TravellingManaClapProfile {
    crate::TravellingManaClapProfile {
        parent: crate::NativeParasiteProfile {
            buff_rawcode: u32::from_be_bytes(*b"BUFF"),
            map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
            speed_per_tick: 60 * SUBUNITS_PER_WORLD_UNIT,
            affects_invulnerable: false,
            duration_ticks: 1,
            hero_duration_ticks: 1,
            body_rawcode: u32::from_be_bytes(*b"nfbr"),
            count: 1,
            body_duration_ticks: 1,
        },
        parent_nonhero_only: true,
        mover_rawcode: u32::from_be_bytes(*b"h06N"),
        callback_speed_per_second: 500 * SUBUNITS_PER_WORLD_UNIT,
        mover_speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
        mover_has_visible_model: false,
        mover_height_world_units: 17,
        mover_duration_ticks: 2,
        clap: crate::NativeNovaProfile {
            ability: AbilityId(793),
            radius: 7 * SUBUNITS_PER_WORLD_UNIT,
            primary_damage: 0,
            area_damage: 11,
            duration_ticks: 8,
            hero_duration_ticks: 4,
            movement_percent_delta: -33,
            attack_speed_percent_delta: -45,
        },
        burn_radius: 15 * SUBUNITS_PER_WORLD_UNIT,
        maximum_mana_burn: 27,
        excluded_ability: AbilityId(794),
        feedback_visual_ability: AbilityId(795),
    }
}

fn caster(sim: &mut Simulation) -> SimId {
    sim.spawn_unit_with_spellcasting(
        unit(0, 20, 0, AttackDelivery::Melee),
        SpellcastingProfile {
            mana: ManaProfile {
                maximum: 9,
                starting: 9,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(795),
                mana_cost: 3,
                cooldown_ticks: 1000,
                range: 60 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::RandomGroundEnemyUnit,
                effect: AbilityEffect::TravellingManaClap(profile()),
            },
        },
    )
}

fn recipient(
    sim: &mut Simulation,
    x: i32,
    flags: UnitClassifications,
    mechanical: bool,
    movement: MovementClass,
    mana: i32,
) -> SimId {
    sim.spawn_unit_with_properties_and_spellcasting(
        unit(1, x, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: flags,
            collision_radius: Some(CollisionRadius(0)),
            mechanical,
            build_time_ticks: Some(2),
            repair_time_ticks: Some(2),
            movement_class: movement,
            armor: ArmorProfile::new(ArmorType::Large, 0),
            ..Default::default()
        },
        mana_profile(mana),
    )
}

#[test]
fn travelling_mana_clap_retains_release_point_and_independent_masks_after_source_and_mover_expiry()
{
    let mut sim = simulation(1);
    let source = caster(&mut sim);
    let sapper = UnitClassifications {
        combat_sapper: true,
        ..Default::default()
    };
    let original = recipient(&mut sim, 70, sapper, false, MovementClass::Ground, 100);
    sim.step();
    assert_eq!(sim.unit(source).unwrap().mana_current, Some(6));
    assert!(
        sim.world
            .iter_entities()
            .any(|e| matches!(e.get::<crate::components::NativeAction>(),
        Some(crate::components::NativeAction::TravellingManaClap(s)) if s.due_tick == 3))
    );
    let e = entity(&sim, original);
    sim.world.get_mut::<Position>(e).unwrap().0.x = 200 * SUBUNITS_PER_WORLD_UNIT;
    let e = entity(&sim, source);
    sim.world.despawn(e);
    let shield = recipient(&mut sim, 75, sapper, false, MovementClass::Ground, 100);
    sim.world
        .get_mut::<StatusState>(entity(&sim, shield))
        .unwrap()
        .native_grants
        .insert(crate::NativeAbilityGrant {
            ability: profile().excluded_ability,
            effect: crate::NativeAbilityGrantEffect::AttackDamage(0),
        });
    let non_sapper = recipient(
        &mut sim,
        73,
        UnitClassifications::default(),
        false,
        MovementClass::Ground,
        100,
    );
    let immune = recipient(
        &mut sim,
        72,
        UnitClassifications {
            spell_immune: true,
            ..sapper
        },
        false,
        MovementClass::Ground,
        100,
    );
    let hero = recipient(
        &mut sim,
        74,
        UnitClassifications {
            hero: true,
            ..sapper
        },
        false,
        MovementClass::Ground,
        100,
    );
    let mechanical = recipient(&mut sim, 80, sapper, true, MovementClass::Ground, 100);
    let air = recipient(&mut sim, 72, sapper, false, MovementClass::Air, 100);
    let empty = recipient(&mut sim, 76, sapper, false, MovementClass::Ground, 0);
    let invulnerable = recipient(
        &mut sim,
        73,
        UnitClassifications {
            invulnerable: true,
            ..sapper
        },
        false,
        MovementClass::Ground,
        100,
    );
    let outside = recipient(&mut sim, 86, sapper, false, MovementClass::Ground, 100);
    let mut other = wire_restored(&sim, 4);
    for _ in 0..2 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    for id in [
        shield,
        non_sapper,
        immune,
        hero,
        mechanical,
        air,
        empty,
        invulnerable,
        outside,
    ] {
        assert_eq!(sim.unit(id).unwrap().health, 1000);
    }
    assert_eq!(sim.step().checksum, other.step().checksum);
    let spell = sim
        .combat_rules
        .damage_rules
        .apply_spell(profile().clap.area_damage, ArmorType::Large);
    for id in [shield, non_sapper, empty] {
        assert_eq!(sim.unit(id).unwrap().health, 1000 - spell);
    }
    for id in [immune, mechanical] {
        assert_eq!(sim.unit(id).unwrap().health, 1000 - 27);
    }
    assert_eq!(sim.unit(hero).unwrap().health, 1000 - spell - 27);
    assert_eq!(
        sim.unit(hero).unwrap().status.movement_modifiers[0].expires_tick,
        7
    );
    for id in [immune, hero, mechanical] {
        assert_eq!(sim.unit(id).unwrap().mana_current, Some(73));
    }
    for id in [shield, non_sapper, original, air, invulnerable, outside] {
        assert_eq!(sim.unit(id).unwrap().mana_current, Some(100));
    }
    for id in [original, air, invulnerable, outside] {
        assert_eq!(sim.unit(id).unwrap().health, 1000);
    }
    assert_eq!(
        sim.ability_casts_last_tick()
            .iter()
            .filter(|e| e.effect == AbilityEffect::ManaBurnVisual)
            .count(),
        3
    );
    assert!(sim.projectiles().is_empty());
}

#[test]
fn travelling_mana_clap_rejects_invalid_primary_handles_before_resource_commit() {
    for flags in [
        UnitClassifications::default(),
        UnitClassifications {
            combat_sapper: true,
            hero: true,
            ..Default::default()
        },
        UnitClassifications {
            combat_sapper: true,
            resistant_skin: true,
            ..Default::default()
        },
        UnitClassifications {
            combat_sapper: true,
            spell_immune: true,
            ..Default::default()
        },
        UnitClassifications {
            combat_sapper: true,
            invulnerable: true,
            ..Default::default()
        },
        UnitClassifications {
            combat_sapper: true,
            locust: true,
            ..Default::default()
        },
    ] {
        let mut sim = simulation(1);
        let source = caster(&mut sim);
        recipient(&mut sim, 70, flags, false, MovementClass::Ground, 100);
        sim.step();
        assert_eq!(sim.unit(source).unwrap().mana_current, Some(9));
        assert!(
            sim.ability_casts_last_tick()
                .iter()
                .all(|e| e.ability != AbilityId(795))
        );
        assert!(!sim.world.iter_entities().any(|e| matches!(
            e.get::<crate::components::NativeAction>(),
            Some(crate::components::NativeAction::TravellingManaClap(_))
        )));
    }
}
