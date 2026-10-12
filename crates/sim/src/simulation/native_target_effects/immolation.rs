use super::*;

fn profile() -> crate::PermanentImmolationProfile {
    crate::PermanentImmolationProfile {
        ability: AbilityId(791),
        buff_rawcode: u32::from_be_bytes(*b"BURN"),
        radius: 55 * SUBUNITS_PER_WORLD_UNIT,
        damage: 18,
        interval_ticks: 3,
        targets: AttackTargetMask::AIR_AND_GROUND,
        organic_only: true,
    }
}

#[test]
fn permanent_immolation_samples_native_membership_and_pulses_across_wire_without_casting() {
    let mut sim = simulation(1);
    let source = sim.spawn_unit_with_properties(
        unit(0, 110, 0, AttackDelivery::RangedInstant),
        UnitGameplayProperties {
            collision_radius: Some(CollisionRadius(0)),
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::PermanentImmolation(
                profile(),
            )),
            ..Default::default()
        },
    );
    let eligible = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            collision_radius: Some(CollisionRadius(0)),
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    let mechanical = sim.spawn_unit_with_properties(
        unit(1, 70, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            collision_radius: Some(CollisionRadius(0)),
            mechanical: true,
            build_time_ticks: Some(2),
            repair_time_ticks: Some(2),
            ..Default::default()
        },
    );
    let immune = sim.spawn_unit_with_properties(
        unit(1, 80, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            collision_radius: Some(CollisionRadius(0)),
            classifications: UnitClassifications {
                spell_immune: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let locust = sim.spawn_unit_with_properties(
        unit(1, 90, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            collision_radius: Some(CollisionRadius(0)),
            classifications: UnitClassifications {
                locust: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let outside = sim.spawn_unit(unit(1, 166, 0, AttackDelivery::Melee));
    let allied = sim.spawn_unit(unit(0, 140, 0, AttackDelivery::Melee));
    for _ in 0..profile().interval_ticks {
        sim.step();
    }
    assert_eq!(sim.unit(eligible).unwrap().health, 1000);
    let mut other = wire_restored(&sim, 4);
    assert_eq!(sim.step().checksum, other.step().checksum);
    assert_eq!(
        sim.unit(eligible).unwrap().health,
        1000 - sim.combat_rules.damage_rules.apply_spell(
            profile().damage,
            sim.unit(eligible).unwrap().armor.armor_type
        )
    );
    for id in [mechanical, immune, locust, outside, allied] {
        assert_eq!(sim.unit(id).unwrap().health, 1000);
    }
    assert!(sim.ability_casts_last_tick().is_empty());
    let e = entity(&sim, source);
    sim.world.despawn(e);
    let mut other = wire_restored(&sim, 4);
    let health = sim.unit(eligible).unwrap().health;
    for _ in 0..6 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    assert_eq!(sim.unit(eligible).unwrap().health, health);
    assert_eq!(sim.unit(eligible).unwrap().status.armor_modifier_count, 0);
}

#[test]
fn locust_handles_do_not_become_weapon_targets_or_ordinary_aura_recipients() {
    let mut sim = simulation(1);
    let attacker = sim.spawn_unit(unit(0, 20, 100, AttackDelivery::Melee));
    let target = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            collision_radius: Some(CollisionRadius(0)),
            classifications: UnitClassifications {
                locust: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let donor = sim.spawn_unit_with_properties(
        unit(1, 110, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            collision_radius: Some(CollisionRadius(0)),
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Aura(
                crate::components::AuraEffectProfile {
                    self_only: false,
                    melee_life_steal_per_10k: 0,
                    ability: AbilityId(792),
                    radius: 55 * SUBUNITS_PER_WORLD_UNIT,
                    armor_bonus_per_100: 700,
                    affects_melee: true,
                    affects_ranged: true,
                    damage_bonus_per_10k: 0,
                    melee_damage_return_per_10k: 0,
                    mana_regeneration_per_second_per_10k: 0,
                    linger_ticks: 0,
                    suspend_during_spell_cooldown: false,
                },
            )),
            ..Default::default()
        },
    );
    let mut other = wire_restored(&sim, 4);
    for _ in 0..6 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    assert_ne!(sim.unit(attacker).unwrap().target, Some(target));
    assert_eq!(sim.unit(target).unwrap().health, 1000);
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 0);
    assert!(sim.unit(donor).unwrap().status.armor_modifier_count > 0);
}

#[test]
fn locust_handles_do_not_block_ordinary_movement() {
    let mut sim = simulation(1);
    let mut moving = unit(0, 20, 0, AttackDelivery::Melee);
    moving.movement.speed_per_tick = 10 * SUBUNITS_PER_WORLD_UNIT;
    moving.attack.range = SUBUNITS_PER_WORLD_UNIT;
    let actor = sim.spawn_unit_with_properties(
        moving,
        UnitGameplayProperties {
            collision_radius: Some(CollisionRadius(4 * SUBUNITS_PER_WORLD_UNIT)),
            ..Default::default()
        },
    );
    sim.spawn_unit_with_properties(
        unit(1, 24, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            collision_radius: Some(CollisionRadius(15 * SUBUNITS_PER_WORLD_UNIT)),
            classifications: UnitClassifications {
                locust: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    sim.spawn_unit(unit(1, 100, 0, AttackDelivery::Melee));
    let mut other = wire_restored(&sim, 4);
    for _ in 0..8 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    assert!(
        sim.unit(actor).unwrap().position.x > 24 * SUBUNITS_PER_WORLD_UNIT,
        "{:?}",
        sim.unit(actor)
    );
}
