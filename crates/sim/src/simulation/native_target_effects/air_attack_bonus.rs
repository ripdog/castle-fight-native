use super::super::source_damage_runtime::{RetiredDamageSource, attack_damage_for_recipient};
use super::*;

fn retired(bonus: u16) -> RetiredDamageSource {
    RetiredDamageSource {
        team: Team(0),
        hits_ground: true,
        hits_air: true,
        grants: crate::NativeAbilityGrants::default(),
        air_attack_bonus_per_10k: bonus,
    }
}

#[test]
fn air_bonus_requires_positive_attack_event_flying_recipient_and_bound_source() {
    let mut sim = simulation(1);
    let victim = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    let mut units = sim.snapshot_units();
    let index = find_unit_index(&units, victim).unwrap();
    let source = SimId(900);
    let mut retained = BTreeMap::from([(source, retired(4000))]);
    let invoke = |units: &[UnitSnapshot], damage, kind, retained: &BTreeMap<_, _>| {
        attack_damage_for_recipient(source, &units[index], damage, kind, units, retained)
    };
    assert_eq!(invoke(&units, 37, DamageType::Magic, &retained), 51);
    assert_eq!(invoke(&units, 37, DamageType::Spells, &retained), 37);
    assert_eq!(invoke(&units, 0, DamageType::Magic, &retained), 0);
    assert_eq!(invoke(&units, -5, DamageType::Magic, &retained), -5);
    units[index].movement_class = MovementClass::Ground;
    assert_eq!(invoke(&units, 37, DamageType::Magic, &retained), 37);
    units[index].movement_class = MovementClass::Air;
    retained.clear();
    assert_eq!(invoke(&units, 37, DamageType::Magic, &retained), 37);
    retained.insert(source, retired(0));
    assert_eq!(invoke(&units, 37, DamageType::Magic, &retained), 37);
}

#[test]
fn air_bonus_rewrites_reduced_damage_without_combat_sapper_or_immunity_gate() {
    let mut sim = simulation(1);
    let victim = sim.spawn_unit_with_properties(
        unit(1, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            armor: ArmorProfile {
                armor_type: ArmorType::Medium,
                armor_points: 11,
            },
            classifications: UnitClassifications {
                spell_immune: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let source = SimId(900);
    sim.retired_damage_sources.insert(source, retired(5000));
    let mut units = sim.snapshot_units();
    let index = find_unit_index(&units, victim).unwrap();
    units[index].snow_damage_taken_per_10k = 5000;
    let positions: Vec<_> = units.iter().map(|u| u.position).collect();
    let mut health: Vec<_> = units.iter().map(|u| u.health).collect();
    let mut attackers = vec![None; units.len()];
    let mut alerts = Vec::new();
    let rules = sim.combat_rules.damage_rules;
    let reduced = scale_damage_per_10k(
        rules.apply_attack_with_armor_per_100(173, DamageType::Magic, ArmorType::Medium, 1100),
        5000,
    );
    sim.apply_damage_to_target(
        TargetIndex::Unit(index),
        WeaponDamageEvent {
            kind: WeaponDamageKind::Ranged,
            source_id: source,
            damage: 173,
            damage_type: DamageType::Magic,
            completed_tick: 1,
        },
        false,
        DamageTargetState {
            match_seed: 0,
            damage_rules: rules,
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
    assert_eq!(health[index], 1000 - reduced * 15_000 / 10_000);
    assert!(!units[index].classifications.combat_sapper);
}

#[test]
fn promoted_air_bonus_applies_to_later_bounces_after_source_death_and_wire_restore() {
    let version = crate::MapVersion::CASTLE_FIGHT_9_27;
    let mut matched = 0;
    for kind in crate::CastleFightUnitKind::ALL {
        let definition = kind.definition_for_version(version).unwrap();
        let bonus =
            crate::content::native_air_attack_bonus_for_version(version, definition.rawcode);
        if bonus == 0 {
            continue;
        }
        matched += 1;
        let mut sim = simulation(1);
        let source = sim.spawn_unit_with_properties(
            unit(
                0,
                20,
                160,
                AttackDelivery::Bounce {
                    speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
                    bounce_range: 80 * SUBUNITS_PER_WORLD_UNIT,
                    max_bounces: 2,
                    damage_percent_per_bounce: 50,
                    allow_repeat_targets: false,
                },
            ),
            UnitGameplayProperties {
                content: definition.resolved().properties.content,
                damage_type: DamageType::Magic,
                ..Default::default()
            },
        );
        let primary = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
        let mut air = Vec::new();
        for x in [80, 90] {
            air.push(sim.spawn_unit_with_properties(
                unit(1, x, 0, AttackDelivery::Melee),
                UnitGameplayProperties {
                    movement_class: MovementClass::Air,
                    ..Default::default()
                },
            ));
        }
        sim.world
            .get_mut::<TargetState>(entity(&sim, source))
            .unwrap()
            .current = Some(primary);
        sim.step();
        sim.step();
        assert_eq!(sim.projectile_count(), 1);
        sim.world
            .get_mut::<Health>(entity(&sim, source))
            .unwrap()
            .current = 1;
        sim.debug_damage_all_units(1);
        assert!(sim.unit(source).is_none());
        assert_eq!(
            sim.retired_damage_sources[&source].air_attack_bonus_per_10k,
            bonus
        );
        let mut copy = wire_restored(&sim, 4);
        let mut expected = BTreeMap::from([(primary, 160)]);
        for _ in 0..30 {
            let Some(view) = sim.projectiles().into_iter().next() else {
                break;
            };
            let ProjectileViewKind::Bounce {
                source_rawcode,
                target,
                bounce_index,
                ..
            } = view.kind
            else {
                unreachable!()
            };
            assert_eq!(source_rawcode, Some(definition.rawcode));
            let raw = 160 / (1 << bounce_index);
            expected
                .entry(target)
                .or_insert_with(|| raw * (10_000 + i32::from(bonus)) / 10_000);
            assert_eq!(
                {
                    sim.step();
                    sim.checksum()
                },
                {
                    copy.step();
                    copy.checksum()
                }
            );
        }
        assert_eq!(sim.projectile_count(), 0);
        assert_eq!(expected.len(), 3);
        for target in [primary, air[0], air[1]] {
            assert_eq!(sim.unit(target).unwrap().health, 999 - expected[&target]);
        }
        assert!(!sim.retired_damage_sources.contains_key(&source));
        assert_eq!(
            {
                sim.step();
                sim.checksum()
            },
            {
                copy.step();
                copy.checksum()
            }
        );
    }
    assert!(matched > 0);
}
