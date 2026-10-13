use super::*;
use crate::{EtherealEffectProfile, NativeDebuffProfile, TriggeredSpellProcProfile};

fn profile(ethereal: bool) -> NativeDebuffProfile {
    NativeDebuffProfile {
        ability: AbilityId(971),
        buff: 972,
        targets: AttackTargetMask::AIR_AND_GROUND,
        organic_only: false,
        duration_ticks: 30,
        hero_duration_ticks: 3,
        movement_percent_delta: -40,
        attack_speed_percent_delta: -20,
        damage_bonus_per_10k: 0,
        ethereal: ethereal.then_some(EtherealEffectProfile {
            magic_damage_per_10k: 20_000,
            healing_per_10k: 15_000,
            allied_damage_per_10k: 10_000,
        }),
    }
}
fn apply(sim: &mut Simulation, target: SimId, p: NativeDebuffProfile) -> bool {
    let mut units = sim.snapshot_units();
    let i = find_unit_index(&units, target).unwrap();
    let result = apply_ability_effect_to_unit(
        &mut units[i],
        AbilityEffect::NativeDebuff(p),
        Team(0),
        sim.next_tick,
        sim.combat_rules.damage_rules,
    );
    *sim.world
        .entity_mut(units[i].entity)
        .get_mut::<StatusState>()
        .unwrap() = units[i].status;
    result
}

fn cripple() -> NativeDebuffProfile {
    NativeDebuffProfile {
        damage_bonus_per_10k: -5000,
        movement_percent_delta: 0,
        attack_speed_percent_delta: 0,
        ..profile(false)
    }
}

fn orb(p: NativeDebuffProfile, targets: AttackTargetMask) -> PassiveUnitEffects {
    PassiveUnitEffects::from_slice(&[PassiveUnitEffect::TriggeredSpellProc(
        TriggeredSpellProcProfile {
            hero_chance_per_10k: None,
            summoned_chance_per_10k: None,
            ability: AbilityId(973),
            chance_per_10k: 10_000,
            targets,
            effect: TriggeredAttackEffect::NativeDebuff {
                parent: AbilityId(973),
                profile: p,
            },
        },
    )])
}

#[test]
fn cripple_reduces_weapon_damage_and_refreshes_its_family_without_affecting_spells() {
    let mut sim = simulation(1);
    let mut spawn = unit(1, 40, 20, AttackDelivery::RangedInstant);
    spawn.attack.cooldown_ticks = 1;
    let attacker = sim.spawn_unit_with_spellcasting(
        spawn,
        SpellcastingProfile {
            mana: ManaProfile::per_second(0, 0, 0),
            ability: AutomaticAbilityProfile {
                id: AbilityId(975),
                mana_cost: 0,
                cooldown_ticks: 1000,
                range: 200 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::RandomEnemyUnit,
                effect: AbilityEffect::Damage { amount: 7 },
            },
        },
    );
    let target = sim.spawn_unit(unit(0, 20, 0, AttackDelivery::Melee));
    let mut p = cripple();
    p.duration_ticks = 3;
    assert!(apply(&mut sim, attacker, p));
    p.ability = AbilityId(974); // Another native ability uses the same buff family.
    assert!(apply(&mut sim, attacker, p));
    assert_eq!(sim.unit(attacker).unwrap().status.armor_modifier_count, 1);
    sim.step();
    assert_eq!(sim.unit(target).unwrap().health, 993);
    for expected in [983, 973, 953] {
        sim.step();
        assert_eq!(sim.unit(target).unwrap().health, expected);
    }
    assert!(
        !sim.unit(attacker)
            .unwrap()
            .status
            .attacks_disabled(sim.next_tick)
    );
}

#[test]
fn zero_damage_landed_orb_applies_only_when_parent_and_child_both_admit_the_target() {
    for (parent, child, immune, evades, fatal, expected) in [
        (
            AttackTargetMask::AIR_AND_GROUND,
            AttackTargetMask::AIR_AND_GROUND,
            false,
            false,
            false,
            true,
        ),
        (
            AttackTargetMask::AIR_UNITS,
            AttackTargetMask::AIR_AND_GROUND,
            false,
            false,
            false,
            false,
        ),
        (
            AttackTargetMask::AIR_AND_GROUND,
            AttackTargetMask::AIR_UNITS,
            false,
            false,
            false,
            false,
        ),
        (
            AttackTargetMask::AIR_AND_GROUND,
            AttackTargetMask::AIR_AND_GROUND,
            true,
            false,
            false,
            false,
        ),
        (
            AttackTargetMask::AIR_AND_GROUND,
            AttackTargetMask::AIR_AND_GROUND,
            false,
            true,
            false,
            false,
        ),
        (
            AttackTargetMask::AIR_AND_GROUND,
            AttackTargetMask::AIR_AND_GROUND,
            false,
            false,
            true,
            false,
        ),
    ] {
        let mut sim = simulation(1);
        let mut p = cripple();
        p.targets = child;
        sim.spawn_unit_with_properties(
            unit(0, 20, if fatal { 1001 } else { 0 }, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: orb(p, parent),
                ..Default::default()
            },
        );
        let target = sim.spawn_unit_with_properties(
            unit(1, 40, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: UnitClassifications {
                    spell_immune: immune,
                    ..Default::default()
                },
                passive_effects: if evades {
                    PassiveUnitEffects::from_slice(&[PassiveUnitEffect::Evasion(
                        EvasionEffectProfile {
                            ability: AbilityId(976),
                            chance_per_10k: 10_000,
                        },
                    )])
                } else {
                    PassiveUnitEffects::default()
                },
                ..Default::default()
            },
        );
        sim.step();
        sim.step();
        assert_eq!(
            sim.unit(target)
                .is_some_and(|u| u.status.armor_modifier_count > 0),
            expected
        );
        if expected {
            let casts = sim.ability_casts_last_tick();
            assert_eq!(casts.len(), 2);
            assert_eq!(casts[0].ability, AbilityId(973));
            assert_eq!(casts[1].ability, p.ability);
            assert!(
                casts
                    .iter()
                    .all(|cast| cast.target == AbilityCastTarget::Unit(target))
            );
        }
    }
}

#[test]
fn native_debuff_orb_payload_survives_source_death_and_wire_restore_until_impact() {
    let mut sim = simulation(1);
    let source = sim.spawn_unit_with_properties(
        unit(
            0,
            20,
            0,
            AttackDelivery::RangedGuaranteedHit {
                speed_per_tick: 2 * SUBUNITS_PER_WORLD_UNIT,
            },
        ),
        UnitGameplayProperties {
            passive_effects: orb(cripple(), AttackTargetMask::AIR_AND_GROUND),
            ..Default::default()
        },
    );
    let target = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 0);
    sim.world
        .get_mut::<Health>(entity(&sim, source))
        .unwrap()
        .current = 0;
    let mut other = wire_restored(&sim, 4);
    for _ in 0..14 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    assert_eq!(
        sim.unit(target).unwrap().status.armor_modifiers[0].damage_bonus_per_10k,
        -5000
    );
}

#[test]
fn complete_cripple_damage_suppression_keeps_landed_attack_procs_enabled() {
    let mut sim = simulation(1);
    let attacker = sim.spawn_unit_with_properties(
        unit(1, 40, 17, AttackDelivery::Melee),
        UnitGameplayProperties {
            passive_effects: orb(cripple(), AttackTargetMask::AIR_AND_GROUND),
            ..Default::default()
        },
    );
    let target = sim.spawn_unit(unit(0, 20, 0, AttackDelivery::Melee));
    let mut suppression = cripple();
    suppression.damage_bonus_per_10k = -10_000;
    assert!(apply(&mut sim, attacker, suppression));
    sim.step();
    sim.step();
    let view = sim.unit(target).unwrap();
    assert_eq!(view.health, 1000);
    assert_eq!(view.status.armor_modifiers[0].damage_bonus_per_10k, -5000);
    assert!(
        !sim.unit(attacker)
            .unwrap()
            .status
            .attacks_disabled(sim.next_tick)
    );
}

#[test]
fn native_debuff_admits_mechanical_air_and_heroes_and_rejects_protection_and_allies() {
    for (team, mechanical, air, hero, immune, invulnerable, eligible) in [
        (1, false, false, false, false, false, true),
        (1, true, true, true, false, false, true),
        (0, false, false, false, false, false, false),
        (1, false, false, false, true, false, false),
        (1, false, false, false, false, true, false),
    ] {
        let mut sim = simulation(1);
        let target = sim.spawn_unit_with_properties(
            unit(team, 40, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                mechanical,
                build_time_ticks: mechanical.then_some(1),
                movement_class: if air {
                    MovementClass::Air
                } else {
                    MovementClass::Ground
                },
                classifications: UnitClassifications {
                    hero,
                    spell_immune: immune,
                    invulnerable,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        assert_eq!(apply(&mut sim, target, profile(true)), eligible);
        let status = sim.unit(target).unwrap().status;
        if eligible {
            assert_eq!(
                status.armor_modifiers[0].expires_tick,
                if hero { 3 } else { 30 }
            );
            assert!(status.attacks_disabled(0));
            assert!(status.ethereal_at(0).is_some());
            assert_eq!(status.movement_modifiers[0].percent_delta, -40);
        } else {
            assert_eq!(status.armor_modifier_count, 0);
        }
    }
}

#[test]
fn slow_family_refreshes_without_becoming_ethereal_or_disabling_attacks() {
    let mut sim = simulation(1);
    let target = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
    assert!(apply(&mut sim, target, profile(false)));
    assert!(apply(&mut sim, target, profile(false)));
    let status = sim.unit(target).unwrap().status;
    assert_eq!(status.armor_modifier_count, 1);
    assert_eq!(status.movement_modifier_count, 1);
    assert_eq!(status.attack_speed_modifier_count, 1);
    assert!(status.ethereal_at(0).is_none());
    assert!(!status.attacks_disabled(0));
}

#[test]
fn ethereal_target_blocks_physical_weapons_but_amplifies_magic_and_healing() {
    for (kind, expected) in [(DamageType::Normal, 1000), (DamageType::Magic, 980)] {
        let mut sim = simulation(1);
        sim.spawn_unit_with_properties(
            unit(0, 20, 10, AttackDelivery::RangedInstant),
            UnitGameplayProperties {
                damage_type: kind,
                ..Default::default()
            },
        );
        let target = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
        apply(&mut sim, target, profile(true));
        sim.step();
        sim.step();
        assert_eq!(sim.unit(target).unwrap().health, expected);
        let mut units = sim.snapshot_units();
        let i = find_unit_index(&units, target).unwrap();
        units[i].health = 900;
        assert!(apply_ability_effect_to_unit(
            &mut units[i],
            AbilityEffect::Heal(crate::components::NativeHealProfile {
                amount: 20,
                targets: AttackTargetMask::AIR_AND_GROUND,
                organic_only: false,
                nonancient_only: false,
                affects_invulnerable: false,
                resume_attack_ticks: 0,
            }),
            Team(1),
            sim.next_tick,
            sim.combat_rules.damage_rules
        ));
        assert_eq!(units[i].health, 930);
    }
}

#[test]
fn ethereal_buff_and_exclusive_expiry_restore_across_worker_counts() {
    let mut sim = simulation(1);
    let target = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
    let mut p = profile(true);
    p.duration_ticks = 6;
    apply(&mut sim, target, p);
    let mut other = wire_restored(&sim, 4);
    for _ in 0..8 {
        sim.step();
        other.step();
        assert_eq!(sim.checksum(), other.checksum());
    }
    assert!(
        sim.unit(target)
            .unwrap()
            .status
            .ethereal_at(sim.next_tick)
            .is_none()
    );
    assert!(
        !sim.unit(target)
            .unwrap()
            .status
            .attacks_disabled(sim.next_tick)
    );
}

#[test]
fn slow_autocast_commits_once_for_an_engaged_target_and_native_buff_family() {
    let mut sim = simulation(1);
    let mut casters = Vec::new();
    for x in [20, 21] {
        casters.push(sim.spawn_unit_with_spellcasting(
            unit(0, x, 0, AttackDelivery::Melee),
            SpellcastingProfile {
                mana: ManaProfile::per_second(9, 9, 0),
                ability: AutomaticAbilityProfile {
                    id: profile(false).ability,
                    mana_cost: 3,
                    cooldown_ticks: 1000,
                    range: 60 * SUBUNITS_PER_WORLD_UNIT,
                    target_policy: AbilityTargetPolicy::NearestEnemyInCombat,
                    effect: AbilityEffect::NativeDebuff(profile(false)),
                },
            },
        ));
    }
    let target = sim.spawn_unit(unit(1, 30, 11, AttackDelivery::RangedInstant));
    sim.world
        .get_mut::<TargetState>(entity(&sim, target))
        .unwrap()
        .current = Some(casters[0]);
    sim.step();
    assert_eq!(sim.unit(casters[0]).unwrap().mana_current, Some(6));
    assert_eq!(sim.unit(casters[1]).unwrap().mana_current, Some(9));
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 1);
}

#[test]
fn physical_missiles_released_before_banish_resolve_against_live_ethereal_state() {
    let mut sim = simulation(1);
    sim.spawn_unit(unit(
        0,
        20,
        19,
        AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: 2 * SUBUNITS_PER_WORLD_UNIT,
        },
    ));
    let target = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
    sim.step();
    sim.step();
    assert!(sim.projectile_count() > 0);
    apply(&mut sim, target, profile(true));
    let mut other = wire_restored(&sim, 4);
    for _ in 0..15 {
        sim.step();
        other.step();
        assert_eq!(sim.checksum(), other.checksum());
    }
    assert_eq!(sim.unit(target).unwrap().health, 1000);
}

#[test]
fn native_healing_wave_applies_recipient_ethereal_factors_without_changing_bounce_falloff() {
    let mut sim = simulation(1);
    let first = sim.spawn_unit(unit(1, 40, 0, AttackDelivery::Melee));
    let second = sim.spawn_unit(unit(1, 50, 0, AttackDelivery::Melee));
    for id in [first, second] {
        apply(&mut sim, id, profile(true));
        sim.world
            .get_mut::<Health>(entity(&sim, id))
            .unwrap()
            .current = 800;
    }
    let mut units = sim.snapshot_units();
    let index = find_unit_index(&units, first).unwrap();
    sim.start_healing_wave(
        SimId(981),
        Team(1),
        SimPoint::new(0, 0),
        index,
        crate::HealingWaveProfile {
            ability: AbilityId(982),
            healing: 50,
            trigger_healing: 10,
            maximum_targets: 2,
            jump_radius: 100 * SUBUNITS_PER_WORLD_UNIT,
            retention_per_10k: 5000,
            recovery_ticks: 0,
        },
        &mut units,
    );
    assert_eq!(units[index].health, 890);
    for u in &units {
        sim.world.get_mut::<Health>(u.entity).unwrap().current = u.health;
    }
    for _ in 0..9 {
        sim.step();
    }
    assert_eq!(sim.unit(second).unwrap().health, 838);
}
