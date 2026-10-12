use super::*;
use crate::components::AuraEffectProfile;

fn aura() -> PassiveUnitEffects {
    PassiveUnitEffects::single(PassiveUnitEffect::Aura(AuraEffectProfile {
        self_only: false,
        melee_life_steal_per_10k: 0,
        ability: AbilityId(619),
        radius: 55 * SUBUNITS_PER_WORLD_UNIT,
        melee_damage_return_per_10k: 2700,
        damage_bonus_per_10k: 0,
        armor_bonus_per_100: 0,
        mana_regeneration_per_second_per_10k: 0,
        affects_melee: true,
        affects_ranged: true,
        linger_ticks: 0,
        suspend_during_spell_cooldown: false,
    }))
}

fn donor(sim: &mut Simulation, x: i32) -> SimId {
    sim.spawn_unit_with_properties(
        unit(1, x, 0, AttackDelivery::RangedInstant),
        UnitGameplayProperties {
            passive_effects: aura(),
            classifications: UnitClassifications {
                invulnerable: true,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}

#[test]
fn thorns_uses_recipient_membership_and_post_armor_damage_without_reducing_the_hit() {
    let mut sim = simulation(1);
    let attacker = sim.spawn_unit_with_properties(
        unit(0, 20, 100, AttackDelivery::Melee),
        UnitGameplayProperties {
            armor: ArmorProfile::new(ArmorType::Large, 7),
            classifications: UnitClassifications {
                spell_immune: false,
                ..Default::default()
            },
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::SpellResistance(
                crate::components::SpellResistanceEffectProfile {
                    ability: AbilityId(620),
                    damage_taken_per_10k: 5000,
                },
            )),
            ..Default::default()
        },
    );
    let recipient = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::RangedInstant),
        UnitGameplayProperties {
            armor: ArmorProfile::new(ArmorType::Large, 10),
            ..Default::default()
        },
    );
    let first = donor(&mut sim, 110);
    let second = donor(&mut sim, 111);
    sim.world
        .get_mut::<TargetState>(entity(&sim, attacker))
        .unwrap()
        .current = Some(recipient);
    sim.step();
    assert_eq!(sim.unit(recipient).unwrap().status.armor_modifier_count, 1);
    let mut restored = wire_restored(&sim, 4);
    assert_eq!(sim.step().checksum, restored.step().checksum);
    let rules = DamageRules::default();
    let incoming = rules.apply_attack(
        100,
        DamageType::Normal,
        ArmorProfile::new(ArmorType::Large, 10),
    );
    let returned = (incoming * 2700 + 5000) / 10_000;
    let returned = rules.apply_spell(returned, ArmorType::Large);
    assert_eq!(sim.unit(recipient).unwrap().health, 1000 - incoming);
    assert_eq!(
        sim.unit(attacker).unwrap().health,
        1000 - scale_damage_per_10k(returned, 5000)
    );
    for simulation in [&mut sim, &mut restored] {
        simulation.world.despawn(entity(simulation, first));
        simulation.world.despawn(entity(simulation, second));
    }
    assert_eq!(sim.step().checksum, restored.step().checksum);
    assert_eq!(sim.unit(recipient).unwrap().status.armor_modifier_count, 0);
}

#[test]
fn thorns_accepts_allied_air_mechanical_and_invulnerable_units_and_resamples_allegiance() {
    let mut sim = simulation(1);
    let source = donor(&mut sim, 110);
    let mut allies = Vec::new();
    for (x, air, mechanical, invulnerable) in [
        (140, true, false, false),
        (150, false, true, false),
        (165, false, false, true),
    ] {
        allies.push(sim.spawn_unit_with_properties(
            unit(1, x, 0, AttackDelivery::RangedInstant),
            UnitGameplayProperties {
                collision_radius: Some(CollisionRadius(0)),
                movement_class: if air {
                    MovementClass::Air
                } else {
                    MovementClass::Ground
                },
                mechanical,
                build_time_ticks: mechanical.then_some(1),
                repair_time_ticks: mechanical.then_some(1),
                classifications: UnitClassifications {
                    invulnerable,
                    ..Default::default()
                },
                ..Default::default()
            },
        ));
    }
    let outside = sim.spawn_unit(unit(1, 166, 0, AttackDelivery::Melee));
    let enemy = sim.spawn_unit(unit(0, 130, 0, AttackDelivery::Melee));
    let building = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(108, 28, 4, 4),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: None,
    });
    sim.step();
    for id in allies.iter().copied().chain([source]) {
        assert_eq!(
            sim.unit(id).unwrap().status.armor_modifiers[0].melee_damage_return_per_10k,
            2700
        );
    }
    for id in [outside, enemy] {
        assert_eq!(sim.unit(id).unwrap().status.armor_modifier_count, 0);
    }
    assert_eq!(
        sim.building(building).unwrap().status.armor_modifier_count,
        0
    );
    sim.world.entity_mut(entity(&sim, source)).insert(Team(0));
    sim.step();
    for id in &allies {
        assert_eq!(sim.unit(*id).unwrap().status.armor_modifier_count, 0);
    }
    sim.world
        .get_mut::<UnitClassifications>(entity(&sim, source))
        .unwrap()
        .invisible = true;
    sim.step();
    assert_eq!(sim.unit(enemy).unwrap().status.armor_modifier_count, 0);
    sim.world
        .get_mut::<UnitClassifications>(entity(&sim, source))
        .unwrap()
        .invisible = false;
    sim.step();
    assert_eq!(sim.unit(enemy).unwrap().status.armor_modifier_count, 1);
    sim.world
        .get_mut::<Health>(entity(&sim, source))
        .unwrap()
        .current = 0;
    sim.step();
    assert_eq!(sim.unit(enemy).unwrap().status.armor_modifier_count, 0);
}

#[test]
fn thorns_returns_only_landed_melee_packets_and_cannot_recursively_return() {
    for kind in [
        WeaponDamageKind::Melee,
        WeaponDamageKind::Ranged,
        WeaponDamageKind::Cleave,
        WeaponDamageKind::Defensive,
        WeaponDamageKind::Thorns,
    ] {
        let mut sim = simulation(1);
        let attacker = sim.spawn_unit_with_properties(
            unit(0, 20, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: aura(),
                ..Default::default()
            },
        );
        let recipient = sim.spawn_unit_with_properties(
            unit(1, 60, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: aura(),
                ..Default::default()
            },
        );
        sim.step();
        let mut units = sim.snapshot_units();
        let positions = units.iter().map(|u| u.position).collect::<Vec<_>>();
        let mut health = vec![1000, 1000];
        let mut attackers = vec![None; 2];
        let mut alerts = Vec::new();
        sim.apply_damage_to_target(
            TargetIndex::Unit(1),
            WeaponDamageEvent {
                source_id: attacker,
                damage: 100,
                damage_type: DamageType::Normal,
                completed_tick: 1,
                kind,
            },
            false,
            DamageTargetState {
                match_seed: 0,
                damage_rules: DamageRules::default(),
                units: &mut units,
                buildings: &[],
                unit_positions: &positions,
                unit_health: &mut health,
                building_health: &mut [],
                attackers_this_tick: &mut attackers,
                next_defense_alerts: &mut alerts,
                navigation_cell_size: sim.config.navigation_cell_size,
            },
        );
        assert_eq!(
            health[1], 900,
            "Thorns never reduces incoming damage: {kind:?}"
        );
        assert_eq!(
            health[0],
            if kind == WeaponDamageKind::Melee {
                973
            } else {
                1000
            }
        );
        assert_eq!(units[1].id, recipient);
    }
}

#[test]
fn thorns_is_blocked_by_spell_immunity_but_a_fatal_hit_still_returns_damage() {
    for immune in [false, true] {
        let mut sim = simulation(1);
        let attacker = sim.spawn_unit_with_properties(
            unit(0, 20, 2000, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: UnitClassifications {
                    spell_immune: immune,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let recipient = sim.spawn_unit_with_properties(
            unit(1, 60, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: aura(),
                ..Default::default()
            },
        );
        sim.step();
        let mut restored = wire_restored(&sim, 4);
        assert_eq!(sim.step().checksum, restored.step().checksum);
        assert!(sim.unit(recipient).is_none());
        assert_eq!(
            sim.unit(attacker).unwrap().health,
            if immune { 1000 } else { 460 }
        );
    }
}

#[test]
fn zero_speed_projectile_weapons_commit_attacks_without_inventing_impact_or_hit_effects() {
    for delivery in [
        AttackDelivery::RangedGuaranteedHit { speed_per_tick: 0 },
        AttackDelivery::RangedBallistic {
            speed_per_tick: 0,
            impact_radius: 0,
        },
        AttackDelivery::Bounce {
            speed_per_tick: 0,
            bounce_range: 100 * SUBUNITS_PER_WORLD_UNIT,
            max_bounces: 1,
            damage_percent_per_bounce: 60,
            allow_repeat_targets: false,
        },
    ] {
        let mut sim = simulation(1);
        let mut spawn = unit(0, 20, 100, delivery);
        spawn.attack.cooldown_ticks = 3;
        sim.spawn_unit_with_properties(
            spawn,
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Bash(
                    crate::BashEffectProfile {
                        ability: AbilityId(796),
                        chance_per_10k: 10_000,
                        bonus_damage: 17,
                        stun_duration_ticks: 8,
                        hero_stun_duration_ticks: 3,
                        targets: AttackTargetMask::AIR_AND_GROUND,
                    },
                )),
                ..Default::default()
            },
        );
        let victim = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
        let collateral = sim.spawn_unit(unit(1, 80, 0, AttackDelivery::Melee));
        let mut restored = wire_restored(&sim, 4);
        let mut attacks = 0;
        for _ in 0..12 {
            let report = sim.step();
            assert_eq!(report.checksum, restored.step().checksum);
            attacks += report.attacks_resolved;
            assert_eq!(sim.projectile_count(), 0);
        }
        assert!(attacks > 0);
        assert_eq!(sim.unit(victim).unwrap().health, 1000);
        assert_eq!(sim.unit(collateral).unwrap().health, 1000);
        assert_eq!(sim.unit(victim).unwrap().status.stunned_until_tick, 0);
    }
}
