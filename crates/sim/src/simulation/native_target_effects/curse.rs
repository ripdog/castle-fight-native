use super::*;

fn profile() -> crate::CurseProfile {
    crate::CurseProfile {
        ability: AbilityId(921),
        buff: crate::NativeBuffIdentity {
            rawcode: 922,
            positive: false,
            stealable: true,
            organic_only: false,
        },
        chance_to_miss_per_10k: 10_000,
        targets: AttackTargetMask::AIR_AND_GROUND,
        duration_ticks: 8,
        hero_duration_ticks: 4,
    }
}

fn caster(sim: &mut Simulation, x: i32) -> SimId {
    sim.spawn_unit_with_spellcasting(
        unit(0, x, 0, AttackDelivery::Melee),
        SpellcastingProfile {
            mana: ManaProfile::per_second(9, 9, 0),
            ability: AutomaticAbilityProfile {
                id: profile().ability,
                mana_cost: 3,
                cooldown_ticks: 1000,
                range: 60 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::NearestEnemyInCombat,
                effect: AbilityEffect::Curse(profile()),
            },
        },
    )
}

fn engage(sim: &mut Simulation, source: SimId, target: SimId) {
    sim.world
        .get_mut::<TargetState>(entity(sim, source))
        .unwrap()
        .current = Some(target);
}

#[test]
fn curse_autocast_admits_mechanical_air_and_heroes_but_commits_once_per_buff_family() {
    let mut sim = simulation(1);
    let first = caster(&mut sim, 20);
    let second = caster(&mut sim, 21);
    let target = sim.spawn_unit_with_properties(
        unit(1, 30, 11, AttackDelivery::RangedInstant),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            mechanical: true,
            build_time_ticks: Some(1),
            classifications: UnitClassifications {
                hero: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    engage(&mut sim, target, first);
    sim.step();
    assert_eq!(sim.unit(first).unwrap().mana_current, Some(6));
    assert_eq!(sim.unit(second).unwrap().mana_current, Some(9));
    let status = sim.unit(target).unwrap().status;
    assert_eq!(status.armor_modifier_count, 1);
    assert_eq!(status.armor_modifiers[0].expires_tick, 4);
    assert_eq!(status.armor_modifiers[0].native_buff, Some(profile().buff));
    assert_eq!(sim.unit(first).unwrap().health, 1000);
    sim.step();
    assert!(
        sim.attacks_last_tick()
            .iter()
            .any(|a| a.source == target && a.missed)
    );
    sim.world.despawn(entity(&sim, first));
    sim.world.despawn(entity(&sim, second));
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..7 {
        sim.step();
        restored.step();
        assert_eq!(sim.checksum(), restored.checksum());
    }
    assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 0);
}

#[test]
fn curse_autocast_excludes_idle_protected_and_immune_targets_without_resource_commit() {
    for (flags, damage) in [
        (UnitClassifications::default(), 0),
        (
            UnitClassifications {
                invulnerable: true,
                ..Default::default()
            },
            11,
        ),
        (
            UnitClassifications {
                spell_immune: true,
                ..Default::default()
            },
            11,
        ),
        (
            UnitClassifications {
                locust: true,
                ..Default::default()
            },
            11,
        ),
    ] {
        let mut sim = simulation(1);
        let source = caster(&mut sim, 20);
        let target = sim.spawn_unit_with_properties(
            unit(1, 30, damage, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: flags,
                ..Default::default()
            },
        );
        engage(&mut sim, target, source);
        sim.step();
        assert_eq!(sim.unit(source).unwrap().mana_current, Some(9));
        assert_eq!(sim.unit(target).unwrap().status.armor_modifier_count, 0);
        assert!(sim.ability_casts_last_tick().is_empty());
    }
}

#[test]
fn curse_misses_melee_and_projectile_releases_without_on_hit_procs_then_expires() {
    for delivery in [
        AttackDelivery::Melee,
        AttackDelivery::RangedInstant,
        AttackDelivery::RangedGuaranteedHit {
            speed_per_tick: 40 * SUBUNITS_PER_WORLD_UNIT,
        },
    ] {
        let mut sim = simulation(1);
        let mut spawn = unit(0, 20, 11, delivery);
        spawn.attack.cooldown_ticks = 1;
        let source = sim.spawn_unit_with_properties(
            spawn,
            UnitGameplayProperties {
                passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::CriticalStrike(
                    crate::CriticalStrikeEffectProfile {
                        ability: AbilityId(924),
                        chance_per_10k: 10_000,
                        damage_multiplier_per_10k: 20_000,
                        targets: AttackTargetMask::AIR_AND_GROUND,
                    },
                )),
                ..Default::default()
            },
        );
        let target = sim.spawn_unit(unit(1, 30, 0, AttackDelivery::Melee));
        let mut state = sim
            .world
            .get_mut::<StatusState>(entity(&sim, source))
            .unwrap();
        apply_timed_armor_modifier(
            &mut state,
            TimedArmorModifier {
                id: ModifierId(921),
                native_buff: Some(profile().buff),
                attack_miss_chance_per_10k: 10_000,
                expires_tick: 2,
                ..Default::default()
            },
        );
        for _ in 0..2 {
            sim.step();
            assert_eq!(sim.unit(target).unwrap().health, 1000);
            assert!(
                sim.attacks_last_tick()
                    .iter()
                    .filter(|a| a.source == source)
                    .all(|a| a.missed && !a.critical)
            );
        }
        let mut restored = wire_restored(&sim, 3);
        for _ in 0..3 {
            sim.step();
            restored.step();
            assert_eq!(sim.checksum(), restored.checksum());
        }
        assert!(sim.unit(target).unwrap().health < 1000);
    }
}

#[test]
fn curse_and_evasion_keep_independent_rolls_across_workers() {
    fn setup(curse: bool, evasion: bool, workers: usize) -> (Simulation, SimId) {
        let mut sim = simulation(workers);
        let mut attacker = unit(0, 20, 1, AttackDelivery::Melee);
        attacker.attack.cooldown_ticks = 1;
        let id = sim.spawn_unit(attacker);
        sim.spawn_unit_with_properties(
            unit(1, 30, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                passive_effects: if evasion {
                    PassiveUnitEffects::single(PassiveUnitEffect::Evasion(EvasionEffectProfile {
                        ability: AbilityId(925),
                        chance_per_10k: 5000,
                    }))
                } else {
                    PassiveUnitEffects::default()
                },
                ..Default::default()
            },
        );
        if curse {
            let mut status = sim.world.get_mut::<StatusState>(entity(&sim, id)).unwrap();
            apply_timed_armor_modifier(
                &mut status,
                TimedArmorModifier {
                    id: ModifierId(921),
                    attack_miss_chance_per_10k: 3000,
                    expires_tick: 100,
                    ..Default::default()
                },
            );
        }
        (sim, id)
    }
    let (mut curse, source) = setup(true, false, 1);
    let (mut evasion, _) = setup(false, true, 2);
    let (mut both, _) = setup(true, true, 4);
    let mut separate_outcomes = [false; 2];
    for _ in 0..80 {
        curse.step();
        evasion.step();
        both.step();
        let miss = |sim: &Simulation| {
            sim.attacks_last_tick()
                .iter()
                .find(|a| a.source == source)
                .map(|a| a.missed)
        };
        match (miss(&curse), miss(&evasion), miss(&both)) {
            (Some(a), Some(b), Some(c)) => {
                assert_eq!(c, a || b);
                separate_outcomes[0] |= a && !b;
                separate_outcomes[1] |= b && !a;
            }
            (None, None, None) => {}
            state => panic!("attack scheduling changed with miss effects: {state:?}"),
        }
    }
    assert_eq!(separate_outcomes, [true, true]);
}

#[test]
fn curse_autocast_recognizes_an_enemy_attacking_a_structure() {
    let mut sim = simulation(1);
    let source = caster(&mut sim, 20);
    let building = sim.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(6, 1, 1, 1),
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    let enemy = sim.spawn_unit(unit(1, 40, 11, AttackDelivery::Melee));
    engage(&mut sim, enemy, building);
    sim.step();
    assert_eq!(sim.unit(source).unwrap().mana_current, Some(6));
    assert_eq!(sim.unit(enemy).unwrap().status.armor_modifier_count, 1);
}
