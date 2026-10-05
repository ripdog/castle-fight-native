use super::*;
use crate::components::{HealingWaveProfile, ManaProfile, NativeBoltProfile};

mod building_immunity;
mod recovery;
mod visibility;

fn unit(sim: &mut Simulation, team: u8, x: i32, movement: MovementClass, wounded: bool) -> SimId {
    let id = sim.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(team),
            position: SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        UnitGameplayProperties {
            movement_class: movement,
            classifications: UnitClassifications {
                combat_sapper: true,
                ..UnitClassifications::default()
            },
            ..UnitGameplayProperties::default()
        },
    );
    if wounded {
        let entity = sim
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&id))
            .unwrap()
            .id();
        sim.world
            .entity_mut(entity)
            .get_mut::<Health>()
            .unwrap()
            .current = 10;
    }
    id
}
fn sim(workers: usize) -> Simulation {
    Simulation::new(
        SimulationConfig {
            navigation_min: NavCell::new(-100, -100),
            navigation_max: NavCell::new(1000, 100),
            ..SimulationConfig::default()
        },
        workers,
    )
}
fn wave() -> HealingWaveProfile {
    HealingWaveProfile {
        ability: AbilityId(10),
        healing: 20,
        trigger_healing: 1,
        maximum_targets: 3,
        jump_radius: 80 * SUBUNITS_PER_WORLD_UNIT,
        retention_per_10k: 5_000,
        recovery_ticks: 24,
    }
}
fn caster(sim: &mut Simulation, profile: SpellcastingProfile) -> SimId {
    sim.spawn_unit_with_spellcasting(
        UnitSpawn {
            team: Team(0),
            position: SimPoint::new(20 * SUBUNITS_PER_WORLD_UNIT, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 30,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        profile,
    )
}
#[test]
fn healing_wave_trigger_is_ground_but_secondary_targets_include_air_and_wait_for_exact_hop() {
    let mut s = sim(1);
    let c = caster(
        &mut s,
        SpellcastingProfile {
            mana: ManaProfile {
                maximum: 10,
                starting: 10,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(1),
                mana_cost: 3,
                cooldown_ticks: 200,
                range: 100 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::WoundedFriendlyUnit,
                effect: AbilityEffect::HealingWave(wave()),
            },
        },
    );
    let ground = unit(&mut s, 0, 30, MovementClass::Ground, true);
    let air = unit(&mut s, 0, 50, MovementClass::Air, true);
    let enemy = unit(&mut s, 1, 40, MovementClass::Ground, true);
    s.step();
    assert_eq!(s.unit(ground).unwrap().health, 31);
    assert_eq!(s.unit(air).unwrap().health, 10);
    assert_eq!(s.unit(c).unwrap().mana_current, Some(7));
    for _ in 1..8 {
        s.step();
    }
    assert_eq!(s.unit(air).unwrap().health, 10);
    s.step();
    assert_eq!(s.unit(air).unwrap().health, 20);
    assert_eq!(s.unit(enemy).unwrap().health, 10);
    assert_eq!(s.chain_lightnings_last_tick()[0].bounce_index, 1);
}
#[test]
fn healing_wave_restoration_selects_lowest_live_health_not_nearest_and_keeps_resource_history() {
    let mut s = sim(1);
    caster(
        &mut s,
        SpellcastingProfile {
            mana: ManaProfile {
                maximum: 10,
                starting: 10,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(1),
                mana_cost: 3,
                cooldown_ticks: 200,
                range: 40 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::WoundedFriendlyUnit,
                effect: AbilityEffect::HealingWave(wave()),
            },
        },
    );
    unit(&mut s, 0, 30, MovementClass::Ground, true);
    let near = unit(&mut s, 0, 50, MovementClass::Air, true);
    let far = unit(&mut s, 0, 90, MovementClass::Air, true);
    let entity = s
        .world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&near))
        .unwrap()
        .id();
    s.world
        .entity_mut(entity)
        .get_mut::<Health>()
        .unwrap()
        .current = 50;
    s.step();
    let snapshot = s.capture_snapshot();
    let mut restored = sim(4);
    restored.restore_snapshot(&snapshot).unwrap();
    for _ in 1..24 {
        assert_eq!(s.step().checksum, restored.step().checksum);
    }
    assert_eq!(s.unit(far).unwrap().health, 20);
    assert_eq!(s.unit(near).unwrap().health, 55);
}
fn bolt() -> NativeBoltProfile {
    NativeBoltProfile {
        ability: AbilityId(20),
        damage: 7,
        stun_ticks: 6,
        hero_stun_ticks: 2,
        damage_per_second: 0,
        duration_ticks: 6,
        speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
        cleanse: false,
        targets: AttackTargetMask::AIR_UNITS,
    }
}
#[test]
fn solar_search_is_target_centered_but_bolts_launch_at_caster_and_retain_native_delivery() {
    let mut s = sim(1);
    let c = caster(
        &mut s,
        SpellcastingProfile {
            mana: ManaProfile {
                maximum: 10,
                starting: 10,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(2),
                mana_cost: 3,
                cooldown_ticks: 200,
                range: 120 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::FlyingEnemyUnit,
                effect: AbilityEffect::SolarStrike {
                    profile: bolt(),
                    radius: 10 * SUBUNITS_PER_WORLD_UNIT,
                    maximum_targets: 2,
                },
            },
        },
    );
    let target = unit(&mut s, 1, 100, MovementClass::Air, false);
    let nearby = unit(&mut s, 1, 108, MovementClass::Air, false);
    let wrong_origin = unit(&mut s, 1, 15, MovementClass::Air, false);
    // Limit trigger candidates with Avul, not magic immunity (the script does not check the latter).
    let entity = s
        .world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&wrong_origin))
        .unwrap()
        .id();
    s.world.entity_mut(entity).insert(UnitClassifications {
        combat_sapper: true,
        invulnerable: true,
        ..UnitClassifications::default()
    });
    s.step();
    assert_eq!(s.unit(c).unwrap().mana_current, Some(7));
    assert_eq!(s.unit(target).unwrap().health, 100);
    let actions = s
        .world
        .iter_entities()
        .filter_map(|e| e.get::<crate::components::NativeAction>())
        .count();
    assert_eq!(actions, 2);
    for _ in 0..12 {
        s.step();
    }
    assert_eq!(s.unit(target).unwrap().health, 93);
    assert_eq!(s.unit(nearby).unwrap().health, 93);
    assert_eq!(s.unit(wrong_origin).unwrap().health, 100);
}

fn entity(s: &Simulation, id: SimId) -> Entity {
    s.world
        .iter_entities()
        .find(|e| e.get::<SimId>() == Some(&id))
        .unwrap()
        .id()
}

fn fire() -> SpellcastingProfile {
    SpellcastingProfile {
        mana: ManaProfile {
            maximum: 10,
            starting: 10,
            regen_per_tick_per_10k: 0,
        },
        ability: AutomaticAbilityProfile {
            id: AbilityId(20),
            mana_cost: 0,
            cooldown_ticks: 90,
            range: 200 * SUBUNITS_PER_WORLD_UNIT,
            target_policy: AbilityTargetPolicy::RandomEnemyUnitOrBuilding,
            effect: AbilityEffect::PhoenixFire(NativeBoltProfile {
                damage_per_second: 3,
                duration_ticks: 120,
                stun_ticks: 0,
                hero_stun_ticks: 0,
                targets: AttackTargetMask::ALL,
                ..bolt()
            }),
        },
    }
}

#[test]
fn native_fire_ignores_stun_and_recovery_has_exact_final_pulse_and_buff_exclusion() {
    for movement in [MovementClass::Ground, MovementClass::Air] {
        let mut s = sim(1);
        let c = caster(&mut s, fire());
        s.world
            .entity_mut(entity(&s, c))
            .get_mut::<StatusState>()
            .unwrap()
            .stunned_until_tick = 1000;
        let target = unit(&mut s, 1, 25, movement, false);
        s.step(); // passive fires even while ordinary/active orders are disabled
        s.step(); // impact at tick 1; buff expires at tick 121
        assert_eq!(s.unit(target).unwrap().health, 93);
        assert_eq!(s.unit(target).unwrap().status.damage_over_time_count, 1);
        let snapshot = s.capture_snapshot();
        let mut restored = sim(4);
        restored.restore_snapshot(&snapshot).unwrap();
        for tick in 2..=121 {
            assert_eq!(s.step().checksum, restored.step().checksum);
            let pulses = ((tick - 1) / 30).min(4);
            assert_eq!(
                s.unit(target).unwrap().health,
                93 - pulses * 3,
                "tick {tick}"
            );
            if tick == 90 {
                assert!(
                    s.projectiles().is_empty(),
                    "cooldown ready, but native buff excludes re-hit"
                );
            }
        }
        assert_eq!(s.unit(target).unwrap().status.damage_over_time_count, 0);
        assert_eq!(
            s.projectiles().len(),
            1,
            "reacquires on the exact expiry tick"
        );
        s.step();
        assert_eq!(s.unit(target).unwrap().health, 74);
    }
}

#[test]
fn native_fire_buildings_receive_all_pulses_and_are_excluded_until_buff_expiry() {
    let mut s = sim(1);
    caster(&mut s, fire());
    let target = s.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(1, 0, 1, 1),
        health: 100,
        production: None,
        attack: None,
        spellcasting: None,
    });
    s.step();
    let impact = s.projectiles()[0].impact_tick;
    for _ in 1..=impact {
        s.step();
    }
    assert_eq!(s.building(target).unwrap().health, 93);
    let snapshot = s.capture_snapshot();
    let mut restored = sim(4);
    restored.restore_snapshot(&snapshot).unwrap();
    for tick in impact + 1..=impact + 120 {
        assert_eq!(s.step().checksum, restored.step().checksum);
        assert_eq!(
            s.building(target).unwrap().health,
            93 - ((tick - impact) / 30) as i32 * 3
        );
        if tick == 90 {
            assert!(s.projectiles().is_empty());
        }
    }
    assert_eq!(s.projectiles().len(), 1);
}

#[test]
fn native_bolts_home_on_live_targets_restore_midflight_and_drop_dead_or_immune_targets() {
    let mut s = sim(1);
    caster(&mut s, fire());
    let target = unit(&mut s, 1, 100, MovementClass::Air, false);
    s.step();
    let initial_impact = s.projectiles()[0].impact_tick;
    s.world
        .entity_mut(entity(&s, target))
        .get_mut::<Position>()
        .unwrap()
        .0 = SimPoint::new(180 * SUBUNITS_PER_WORLD_UNIT, 0);
    s.step();
    assert!(s.projectiles()[0].impact_tick > initial_impact);
    let snapshot = s.capture_snapshot();
    let mut restored = sim(4);
    restored.restore_snapshot(&snapshot).unwrap();
    for _ in 0..20 {
        assert_eq!(s.step().checksum, restored.step().checksum);
    }
    assert_eq!(s.unit(target).unwrap().health, 93);
    for immune in [true, false] {
        let mut s = sim(1);
        caster(&mut s, fire());
        let target = unit(&mut s, 1, 40, MovementClass::Air, false);
        s.step();
        if immune {
            s.world
                .entity_mut(entity(&s, target))
                .insert(UnitClassifications {
                    spell_immune: true,
                    ..UnitClassifications::default()
                });
        } else {
            s.world
                .entity_mut(entity(&s, target))
                .get_mut::<Health>()
                .unwrap()
                .current = 0;
        }
        for _ in 0..5 {
            s.step();
        }
        assert!(s.projectiles().is_empty());
        if immune {
            let target = s.unit(target).unwrap();
            assert_eq!(target.health, 100);
            assert_eq!(target.status.damage_over_time_count, 0);
            assert!(s.ability_casts_last_tick().is_empty());
        }
    }
}

#[test]
fn generated_per_second_mana_encoding_preserves_arbitrary_phase_and_snapshot_remainders() {
    for rate in [1, 10_000, 35_000, 999_999] {
        for phase in 0..30 {
            assert_eq!(
                (phase..phase + 300)
                    .map(|tick| ManaProfile::per_second(100, 0, rate).regeneration_at_tick(tick, 30))
                    .sum::<u32>(),
                rate * 10
            );
        }
    }
    let legacy = ManaProfile {
        maximum: 100,
        starting: 0,
        regen_per_tick_per_10k: 333,
    };
    assert_eq!(legacy.regeneration_at_tick(29, 30), 333);
    let mut s = sim(1);
    let mut profile = fire();
    profile.mana = ManaProfile::per_second(100, 0, 35_000);
    let c = caster(&mut s, profile);
    for _ in 0..17 {
        s.step();
    }
    let snapshot = s.capture_snapshot();
    let mut restored = sim(4);
    restored.restore_snapshot(&snapshot).unwrap();
    for _ in 17..300 {
        assert_eq!(s.step().checksum, restored.step().checksum);
    }
    assert_eq!(s.unit(c).unwrap().mana_current, Some(35));
}

#[test]
fn healing_wave_cumulative_deadlines_do_not_drift_and_never_revisit_or_heal_mechanical_enemies() {
    let mut s = sim(1);
    let mut profile = fire();
    let mut wave = wave();
    wave.maximum_targets = 4;
    profile.ability.effect = AbilityEffect::HealingWave(wave);
    profile.ability.target_policy = AbilityTargetPolicy::WoundedFriendlyUnit;
    profile.ability.cooldown_ticks = 200;
    let c = caster(&mut s, profile);
    let first = unit(&mut s, 0, 25, MovementClass::Ground, true);
    let second = unit(&mut s, 0, 30, MovementClass::Air, true);
    let third = unit(&mut s, 0, 35, MovementClass::Air, true);
    s.world
        .entity_mut(entity(&s, c))
        .get_mut::<Health>()
        .unwrap()
        .current = 15;
    let mechanical = unit(&mut s, 0, 25, MovementClass::Air, true);
    s.world
        .entity_mut(entity(&s, mechanical))
        .insert(MechanicalUnit);
    let enemy = unit(&mut s, 1, 25, MovementClass::Air, true);
    for tick in 0..=24 {
        s.step();
        if [0, 8, 15, 23].contains(&tick) {
            assert_eq!(s.chain_lightnings_last_tick().len(), 1);
        } else {
            assert!(s.chain_lightnings_last_tick().is_empty());
        }
    }
    assert_eq!(s.unit(first).unwrap().health, 31);
    assert_eq!(s.unit(second).unwrap().health, 20);
    assert_eq!(s.unit(third).unwrap().health, 15);
    assert_eq!(
        s.unit(c).unwrap().health,
        17,
        "self is a valid secondary target"
    );
    assert_eq!(s.unit(mechanical).unwrap().health, 10);
    assert_eq!(s.unit(enemy).unwrap().health, 10);
}

#[test]
fn proxy_search_counts_attempted_native_casts_and_distinguishes_trigger_from_effect_eligibility() {
    for immune in [false, true] {
        let mut s = sim(1);
        let mut profile = fire();
        profile.ability.id = AbilityId(2);
        profile.ability.mana_cost = 3;
        profile.ability.target_policy = AbilityTargetPolicy::FlyingEnemyUnit;
        profile.ability.effect = AbilityEffect::SolarStrike {
            profile: bolt(),
            radius: 10 * SUBUNITS_PER_WORLD_UNIT,
            maximum_targets: 2,
        };
        let c = caster(&mut s, profile);
        let target = unit(&mut s, 1, 25, MovementClass::Air, false);
        let secondary = unit(&mut s, 1, 26, MovementClass::Air, false);
        s.world
            .entity_mut(entity(&s, secondary))
            .insert(UnitClassifications {
                hero: true,
                combat_sapper: true,
                spell_immune: immune,
                ..UnitClassifications::default()
            });
        let capped = unit(&mut s, 1, 27, MovementClass::Air, false);
        let ground = unit(&mut s, 1, 25, MovementClass::Ground, false);
        let not_sapper = unit(&mut s, 1, 25, MovementClass::Air, false);
        s.world
            .entity_mut(entity(&s, not_sapper))
            .insert(UnitClassifications::default());
        let ally = unit(&mut s, 0, 25, MovementClass::Air, false);
        s.step();
        assert_eq!(s.unit(c).unwrap().mana_current, Some(7));
        assert_eq!(s.projectiles().len(), 2);
        s.step();
        assert_eq!(s.unit(target).unwrap().health, 93);
        let second = s.unit(secondary).unwrap();
        assert_eq!(second.health, if immune { 100 } else { 93 });
        assert_eq!(second.stunned_until_tick, if immune { 0 } else { 3 });
        for id in [capped, ground, not_sapper, ally] {
            assert_eq!(s.unit(id).unwrap().health, 100);
        }
    }
}

#[test]
fn native_healing_trigger_never_spends_mana_for_air_only_full_health_mechanical_or_enemy_targets() {
    let mut s = sim(1);
    let mut profile = fire();
    profile.ability.effect = AbilityEffect::HealingWave(wave());
    profile.ability.mana_cost = 3;
    profile.ability.target_policy = AbilityTargetPolicy::WoundedFriendlyUnit;
    let c = caster(&mut s, profile);
    unit(&mut s, 0, 25, MovementClass::Air, true);
    unit(&mut s, 0, 30, MovementClass::Ground, false);
    let mechanical = unit(&mut s, 0, 35, MovementClass::Ground, true);
    s.world
        .entity_mut(entity(&s, mechanical))
        .insert(MechanicalUnit);
    unit(&mut s, 1, 40, MovementClass::Ground, true);
    s.step();
    assert_eq!(s.unit(c).unwrap().mana_current, Some(10));
    assert!(s.ability_casts_last_tick().is_empty());
    assert!(s.chain_lightnings_last_tick().is_empty());
}
