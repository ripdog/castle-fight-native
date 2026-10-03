use castle_fight_sim::{
    AbilityEffect, AbilityId, AbilityTargetPolicy, ArmorProfile, AttackDelivery, AttackProfile,
    AutomaticAbilityProfile, BuildingFootprint, BuildingSpawn, CastleFightProductionKind,
    CastleFightUnitKind, CombatRules, ManaProfile, MapVersion, MovementProfile, PassiveUnitEffect,
    SUBUNITS_PER_WORLD_UNIT, SimPoint, Simulation, SimulationConfig, SpellcastingProfile, Team,
    UnitSpawn, castle_fight_content_bundle,
};

const WORLD: i32 = SUBUNITS_PER_WORLD_UNIT;

fn passive_target(team: Team, x: i32) -> UnitSpawn {
    UnitSpawn {
        team,
        position: SimPoint::new(x * WORLD, 0),
        health: 10_000,
        attack: AttackProfile {
            damage: 0,
            range: 0,
            acquisition_range: 0,
            cooldown_ticks: 1_000,
            delivery: AttackDelivery::Melee,
        },
        movement: MovementProfile { speed_per_tick: 0 },
    }
}

#[test]
fn elven_native_lines_use_versioned_production_and_upgrade_data() {
    for (building, unit, rawcode) in [
        (
            CastleFightProductionKind::ArcheryRange,
            CastleFightUnitKind::Archer,
            *b"n022",
        ),
        (
            CastleFightProductionKind::ArcheryTower,
            CastleFightUnitKind::MasterArcher,
            *b"n023",
        ),
        (
            CastleFightProductionKind::HallOfHonor,
            CastleFightUnitKind::Blademaster,
            *b"n006",
        ),
    ] {
        let definition = unit
            .definition_for_version(MapVersion::CASTLE_FIGHT_9_27)
            .unwrap();
        assert_eq!(definition.rawcode, u32::from_be_bytes(rawcode));
        let production = building
            .definition_for_version(MapVersion::CASTLE_FIGHT_9_27)
            .unwrap();
        assert_eq!(
            production.gameplay_properties().production_unit,
            definition.gameplay_properties()
        );
        assert!(unit.definition_for_version(MapVersion::new(9, 32)).is_err());
    }
    assert_eq!(
        CastleFightProductionKind::ArcheryRange.upgrade_targets(),
        vec![CastleFightProductionKind::ArcheryTower]
    );
    assert_eq!(
        CastleFightProductionKind::ArcheryTower.upgrade_from(),
        Some(CastleFightProductionKind::ArcheryRange)
    );
    // The lobby/match still uses the Human-only development catalog until Elven is complete.
    let bundle = castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    assert!(
        !bundle
            .playable_human_direct_building_kinds()
            .iter()
            .any(|kind| { kind.rawcode(bundle) == Some(u32::from_be_bytes(*b"h08X")) })
    );
}

#[test]
fn elven_fixtures_preserve_protected_stats_and_complete_passive_inventories() {
    for (kind, health, armor, evasion, effect_count) in [
        (CastleFightUnitKind::Archer, 320, 2, 1_500, 2),
        (CastleFightUnitKind::MasterArcher, 525, 4, 2_500, 2),
        (CastleFightUnitKind::Blademaster, 400, 7, 1_500, 3),
    ] {
        let definition = kind
            .definition_for_version(MapVersion::CASTLE_FIGHT_9_27)
            .unwrap();
        assert_eq!(definition.health, health);
        assert_eq!(definition.armor.armor_points, armor);
        assert_eq!(definition.health_regen_per_second_per_10k, 10_000);
        assert!(!definition.mechanical);
        assert!(definition.corpse.is_some());
        assert_eq!(definition.passive_effects.iter().count(), effect_count);
        assert!(definition.passive_effects.iter().any(|effect| matches!(
            effect, PassiveUnitEffect::Evasion(profile) if profile.chance_per_10k == evasion
        )));
    }
    assert!(CastleFightUnitKind::Blademaster.definition().passive_effects.iter().any(|effect| matches!(
        effect, PassiveUnitEffect::CriticalStrike(profile)
            if profile.chance_per_10k == 2_500 && profile.damage_multiplier_per_10k == 25_000
    )));
}

#[test]
fn imported_headshot_proc_is_delivered_at_impact_and_restores_across_workers() {
    for (kind, bonus, chance) in [
        (CastleFightUnitKind::Archer, 50, 1_000),
        (CastleFightUnitKind::MasterArcher, 125, 2_000),
    ] {
        let definition = kind.definition();
        let bash = definition
            .passive_effects
            .iter()
            .find_map(|effect| match effect {
                PassiveUnitEffect::Bash(profile) => Some(profile),
                _ => None,
            })
            .unwrap();
        assert_eq!(bash.chance_per_10k, chance);
        assert_eq!(bash.bonus_damage, bonus);
        assert_eq!(bash.stun_duration_ticks, 15);
        let mut saw_proc = false;
        let mut saw_non_proc = false;
        for seed in 0..64 {
            let config = SimulationConfig {
                match_seed: seed,
                ..SimulationConfig::default()
            };
            let mut sim = Simulation::new(config.clone(), 1);
            let mut attack = definition.attack;
            attack.cooldown_ticks = 1_000;
            sim.spawn_unit_with_properties(
                UnitSpawn {
                    team: Team(0),
                    position: SimPoint::new(40 * WORLD, 0),
                    health: definition.health,
                    attack,
                    movement: MovementProfile { speed_per_tick: 0 },
                },
                definition.gameplay_properties(),
            );
            let target = sim.spawn_unit(passive_target(Team(1), 90));
            sim.step();
            sim.step();
            assert_eq!(
                sim.unit(target).unwrap().health,
                10_000,
                "damage must wait for projectile impact"
            );
            assert!(!sim.projectiles().is_empty());
            let snapshot = sim.capture_snapshot();
            let mut restored = Simulation::new(config, 3);
            restored.restore_snapshot(&snapshot).unwrap();
            for _ in 0..30 {
                sim.step();
                restored.step();
                assert_eq!(sim.checksum(), restored.checksum());
                let view = sim.unit(target).unwrap();
                if view.health != 10_000 {
                    let damage = 10_000 - view.health;
                    let rules = CombatRules::default().damage_rules;
                    let expected_normal = rules.apply_attack(
                        attack.damage,
                        definition.damage_type,
                        ArmorProfile::UNARMORED,
                    );
                    let expected_proc = rules.apply_attack(
                        attack.damage + bonus,
                        definition.damage_type,
                        ArmorProfile::UNARMORED,
                    );
                    if damage == expected_proc {
                        saw_proc = true;
                        let tick = sim.capture_snapshot().completed_tick().unwrap();
                        assert_eq!(view.stunned_until_tick, tick + 15);
                    } else {
                        saw_non_proc = true;
                        assert_eq!(damage, expected_normal);
                        assert_eq!(view.stunned_until_tick, 0);
                    }
                    break;
                }
            }
        }
        assert!(
            saw_proc && saw_non_proc,
            "imported deterministic chance must allow both outcomes"
        );
    }
}

#[test]
fn headshot_never_procs_against_a_building() {
    let definition = CastleFightUnitKind::MasterArcher.definition();
    for seed in 0..32 {
        let mut sim = Simulation::new(
            SimulationConfig {
                match_seed: seed,
                ..SimulationConfig::default()
            },
            1,
        );
        let mut attack = definition.attack;
        attack.cooldown_ticks = 1_000;
        sim.spawn_unit_with_properties(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(40 * WORLD, 0),
                health: definition.health,
                attack,
                movement: MovementProfile { speed_per_tick: 0 },
            },
            definition.gameplay_properties(),
        );
        let target = sim.spawn_building(BuildingSpawn {
            team: Team(1),
            footprint: BuildingFootprint::new(90, 0, 1, 1),
            health: 10_000,
            attack: None,
            production: None,
            spellcasting: None,
        });
        for _ in 0..32 {
            sim.step();
        }
        let expected_damage = CombatRules::default().damage_rules.apply_attack(
            attack.damage,
            definition.damage_type,
            ArmorProfile::UNARMORED,
        );
        assert_eq!(
            sim.building(target).unwrap().health,
            10_000 - expected_damage
        );
    }
}

#[test]
fn blademaster_spell_resistance_applies_to_spell_not_ordinary_damage() {
    let definition = CastleFightUnitKind::Blademaster.definition();
    let resistance = definition
        .passive_effects
        .iter()
        .find_map(|effect| match effect {
            PassiveUnitEffect::SpellResistance(profile) => Some(profile.damage_taken_per_10k),
            _ => None,
        })
        .unwrap();
    assert_eq!(resistance, 8_500);
    let mut sim = Simulation::new(SimulationConfig::default(), 2);
    let target = sim.spawn_unit_with_properties(
        passive_target(Team(1), 90),
        definition.gameplay_properties(),
    );
    sim.spawn_unit_with_spellcasting(
        passive_target(Team(0), 40),
        SpellcastingProfile {
            mana: ManaProfile {
                maximum: 1,
                starting: 1,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(u32::from_be_bytes(*b"TEST")),
                mana_cost: 1,
                cooldown_ticks: 1_000,
                range: 200 * WORLD,
                target_policy: AbilityTargetPolicy::RandomEnemyUnit,
                effect: AbilityEffect::Damage { amount: 100 },
            },
        },
    );
    sim.step();
    sim.step();
    assert_eq!(sim.unit(target).unwrap().health, 9_915);

    let mut saw_hit = false;
    for seed in 0..32 {
        let mut sim = Simulation::new(
            SimulationConfig {
                match_seed: seed,
                ..SimulationConfig::default()
            },
            1,
        );
        let mut properties = definition.gameplay_properties();
        properties.armor = Default::default();
        let target = sim.spawn_unit_with_properties(passive_target(Team(1), 90), properties);
        let mut source = passive_target(Team(0), 40);
        source.attack.damage = 100;
        source.attack.range = 200 * WORLD;
        source.attack.acquisition_range = 200 * WORLD;
        sim.spawn_unit(source);
        sim.step();
        sim.step();
        let health = sim.unit(target).unwrap().health;
        assert!(
            health == 10_000 || health == 9_900,
            "Evasion may miss, but spell resistance must not scale an ordinary hit"
        );
        saw_hit |= health == 9_900;
    }
    assert!(saw_hit);
}
