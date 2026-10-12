use super::*;

fn profile() -> crate::ParasiteAreaBuffProfile {
    crate::ParasiteAreaBuffProfile {
        parent: crate::NativeParasiteProfile {
            buff_rawcode: 902,
            map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
            speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
            affects_invulnerable: false,
            duration_ticks: 1,
            hero_duration_ticks: 1,
            body_rawcode: u32::from_be_bytes(*b"nfbr"),
            count: 1,
            body_duration_ticks: 1,
        },
        parent_nonhero_only: true,
        ability: AbilityId(903),
        buff: crate::NativeBuffIdentity {
            rawcode: 904,
            positive: true,
            stealable: true,
            organic_only: false,
        },
        radius: 12 * SUBUNITS_PER_WORLD_UNIT,
        targets: AttackTargetMask::AIR_AND_GROUND,
        armor_bonus_per_100: 200,
        damage_bonus_per_10k: 1700,
        regeneration_per_second_per_10k: 0,
        duration_ticks: 8,
        hero_duration_ticks: 4,
    }
}

fn caster(sim: &mut Simulation) -> SimId {
    sim.spawn_unit_with_spellcasting(
        unit(0, 20, 0, AttackDelivery::Melee),
        SpellcastingProfile {
            mana: ManaProfile::per_second(9, 9, 0),
            ability: AutomaticAbilityProfile {
                id: AbilityId(901),
                mana_cost: 3,
                cooldown_ticks: 1000,
                range: 60 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::RandomGroundEnemyUnit,
                effect: AbilityEffect::ParasiteAreaBuff(profile()),
            },
        },
    )
}

#[test]
fn parasite_buff_uses_caster_center_and_independent_friendly_mask_and_survives_source_death() {
    let mut sim = simulation(1);
    let source = caster(&mut sim);
    let trigger = sim.spawn_unit_with_properties(
        unit(1, 70, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let ordinary = sim.spawn_unit(unit(0, 32, 0, AttackDelivery::Melee));
    let mechanical_air = sim.spawn_unit_with_properties(
        unit(0, 25, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            mechanical: true,
            build_time_ticks: Some(1),
            classifications: UnitClassifications {
                spell_immune: true,
                hero: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let protected = sim.spawn_unit_with_properties(
        unit(0, 23, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                invulnerable: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let near_target = sim.spawn_unit(unit(0, 69, 0, AttackDelivery::Melee));
    let outside = sim.spawn_unit(unit(0, 33, 0, AttackDelivery::Melee));
    sim.step();
    assert_eq!(sim.unit(source).unwrap().mana_current, Some(6));
    for id in [source, ordinary, mechanical_air] {
        assert_eq!(sim.unit(id).unwrap().status.armor_modifier_count, 1);
    }
    for id in [trigger, protected, near_target, outside] {
        assert_eq!(sim.unit(id).unwrap().status.armor_modifier_count, 0);
    }
    assert_eq!(
        sim.unit(mechanical_air).unwrap().status.armor_modifiers[0].expires_tick,
        4
    );
    assert_eq!(
        sim.unit(ordinary).unwrap().status.armor_modifiers[0].expires_tick,
        8
    );
    assert_eq!(
        sim.ability_casts_last_tick()[0].target_position,
        Some(SimPoint::new(
            20 * SUBUNITS_PER_WORLD_UNIT,
            20 * SUBUNITS_PER_WORLD_UNIT
        ))
    );
    sim.world.despawn(entity(&sim, source));
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..10 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(sim.unit(ordinary).unwrap().status.armor_modifier_count, 0);
}

#[test]
fn parasite_buff_rejects_ineligible_primary_before_committing_resources() {
    for (flags, movement) in [
        (UnitClassifications::default(), MovementClass::Ground),
        (
            UnitClassifications {
                combat_sapper: true,
                hero: true,
                ..Default::default()
            },
            MovementClass::Ground,
        ),
        (
            UnitClassifications {
                combat_sapper: true,
                invulnerable: true,
                ..Default::default()
            },
            MovementClass::Ground,
        ),
        (
            UnitClassifications {
                combat_sapper: true,
                spell_immune: true,
                ..Default::default()
            },
            MovementClass::Ground,
        ),
        (
            UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            MovementClass::Air,
        ),
    ] {
        let mut sim = simulation(1);
        let source = caster(&mut sim);
        sim.spawn_unit_with_properties(
            unit(1, 25, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: flags,
                movement_class: movement,
                ..Default::default()
            },
        );
        sim.step();
        assert_eq!(sim.unit(source).unwrap().mana_current, Some(9));
        assert_eq!(sim.unit(source).unwrap().status.armor_modifier_count, 0);
        assert!(sim.ability_casts_last_tick().is_empty());
    }
}
