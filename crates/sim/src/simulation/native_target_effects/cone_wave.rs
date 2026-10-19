use super::*;
use crate::components::{ConeWaveProfile, NativeAction};
fn profile() -> ConeWaveProfile {
    ConeWaveProfile {
        ability: AbilityId(33),
        damage: 30,
        maximum_damage: 45,
        distance: 100 * SUBUNITS_PER_WORLD_UNIT,
        initial_radius: 5 * SUBUNITS_PER_WORLD_UNIT,
        final_radius: 25 * SUBUNITS_PER_WORLD_UNIT,
        speed_per_tick: 10 * SUBUNITS_PER_WORLD_UNIT,
        targets: AttackTargetMask::AIR_AND_GROUND,
    }
}
fn point(x: i32, y: i32) -> SimPoint {
    SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, y * SUBUNITS_PER_WORLD_UNIT)
}
fn victim(sim: &mut Simulation, team: u8, x: i32, y: i32, air: bool, immune: bool) -> SimId {
    let mut spawn = unit(team, x, 0, AttackDelivery::Melee);
    spawn.position = point(x, y);
    sim.spawn_unit_with_properties(
        spawn,
        UnitGameplayProperties {
            movement_class: if air {
                MovementClass::Air
            } else {
                MovementClass::Ground
            },
            classifications: UnitClassifications {
                spell_immune: immune,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}
#[test]
fn cone_wave_is_fixed_heading_expands_hits_once_and_retains_aggregate_damage_budget() {
    let mut sim = simulation(1);
    let near = victim(&mut sim, 1, 30, 20, false, false);
    let far = victim(&mut sim, 1, 80, 37, true, false);
    let outside = victim(&mut sim, 1, 30, 30, false, false);
    let behind = victim(&mut sim, 1, 5, 20, false, false);
    let ally = victim(&mut sim, 0, 40, 20, false, false);
    let immune = victim(&mut sim, 1, 25, 20, false, true);
    let state = super::super::native_actions::cone_wave::new_state(
        SimId(777),
        Team(0),
        point(10, 20),
        point(60, 20),
        profile(),
        0,
    );
    let id = sim.allocate_id();
    sim.world.spawn((id, NativeAction::ConeWave(state)));
    sim.step();
    sim.step();
    assert_eq!(sim.unit(near).unwrap().health, 1000);
    let mut restored = wire_restored(&sim, 4);
    for _ in 0..12 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    assert_eq!(sim.unit(near).unwrap().health, 970);
    assert_eq!(sim.unit(far).unwrap().health, 985);
    for target in [outside, behind, ally, immune] {
        assert_eq!(sim.unit(target).unwrap().health, 1000);
    }
    assert_eq!(sim.projectile_count(), 0);
}
#[test]
fn cone_proc_uses_caster_origin_and_preserves_wave_when_trigger_victim_dies() {
    let mut sim = simulation(1);
    let mut spawn = unit(0, 10, 2000, AttackDelivery::RangedInstant);
    spawn.attack.range = 200 * SUBUNITS_PER_WORLD_UNIT;
    sim.spawn_unit_with_properties(
        spawn,
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::TriggeredSpellProc(
                crate::TriggeredSpellProcProfile {
                    cooldown_ticks: 0,
                    hero_chance_per_10k: None,
                    summoned_chance_per_10k: None,
                    ability: AbilityId(99),
                    chance_per_10k: 10_000,
                    targets: AttackTargetMask::AIR_AND_GROUND,
                    effect: TriggeredAttackEffect::ConeWave(profile()),
                },
            )),
            ..Default::default()
        },
    );
    let trigger = victim(&mut sim, 1, 40, 20, false, false);
    let behind_target = victim(&mut sim, 1, 60, 20, false, false);
    sim.step();
    sim.step();
    assert!(sim.unit(trigger).is_none());
    assert_eq!(sim.projectile_count(), 1);
    for _ in 0..7 {
        sim.step();
    }
    assert_eq!(sim.unit(behind_target).unwrap().health, 970);
}

#[test]
fn native_ground_wave_reaches_structures_with_shared_budget_and_preserves_primary_building_proc_origin()
 {
    let mut sim = simulation(1);
    let mut p = profile();
    p.targets = AttackTargetMask::GROUND_AND_BUILDINGS;
    p.maximum_damage = 55;
    let source = sim.spawn_unit_with_properties(
        unit(0, 10, 0, AttackDelivery::RangedInstant),
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::TriggeredSpellProc(
                crate::TriggeredSpellProcProfile {
                    cooldown_ticks: 0,
                    hero_chance_per_10k: None,
                    summoned_chance_per_10k: None,
                    ability: AbilityId(99),
                    chance_per_10k: 10000,
                    targets: AttackTargetMask::GROUND_AND_BUILDINGS,
                    effect: TriggeredAttackEffect::ConeWave(p),
                },
            )),
            ..Default::default()
        },
    );
    let building = sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint: BuildingFootprint::new(40, 20, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: None,
    });
    sim.world
        .get_mut::<TargetState>(entity(&sim, source))
        .unwrap()
        .current = Some(building);
    let farther = victim(&mut sim, 1, 60, 20, false, false);
    let air = victim(&mut sim, 1, 30, 20, true, false);
    sim.step();
    sim.step();
    let wave = sim
        .world
        .iter_entities()
        .find_map(|e| match e.get::<NativeAction>() {
            Some(NativeAction::ConeWave(w)) => Some(w.clone()),
            _ => None,
        })
        .expect("building primary launches native child");
    assert_eq!(wave.origin, point(10, 20));
    let mut other = wire_restored(&sim, 4);
    for _ in 0..10 {
        assert_eq!(sim.step().checksum, other.step().checksum);
    }
    assert!(sim.building(building).unwrap().health < 1000);
    assert_eq!(sim.unit(farther).unwrap().health, 975);
    assert_eq!(sim.unit(air).unwrap().health, 1000);
}

#[test]
fn native_orb_wave_launch_state_enforces_shared_one_tick_child_cooldown_and_recovers_on_next_boundary()
 {
    let mut sim = simulation(1);
    let first = super::super::native_actions::cone_wave::new_state(
        SimId(777),
        Team(0),
        point(10, 20),
        point(60, 20),
        profile(),
        0,
    );
    sim.launch_attack_cone_wave(first.clone());
    sim.launch_attack_cone_wave(first);
    assert_eq!(sim.projectile_count(), 1);
    let mut other = wire_restored(&sim, 4);
    assert_eq!(sim.step().checksum, other.step().checksum);
    let second = super::super::native_actions::cone_wave::new_state(
        SimId(777),
        Team(0),
        point(10, 20),
        point(60, 20),
        profile(),
        1,
    );
    sim.launch_attack_cone_wave(second.clone());
    other.launch_attack_cone_wave(second);
    assert_eq!(sim.projectile_count(), 2);
    assert_eq!(sim.step().checksum, other.step().checksum);
}

#[test]
fn cone_orb_origin_and_aim_use_post_movement_combat_positions() {
    let mut sim = simulation(1);
    let source = sim.spawn_unit(unit(0, 10, 0, AttackDelivery::Melee));
    let target = sim.spawn_unit(unit(1, 50, 0, AttackDelivery::Melee));
    let mut units = sim.snapshot_units();
    let source_index = find_unit_index(&units, source).unwrap();
    let target_index = find_unit_index(&units, target).unwrap();
    units[source_index].death_position = point(20, 20);
    units[target_index].death_position = point(70, 40);
    let mut health = units.iter().map(|u| u.health).collect::<Vec<_>>();
    let result = apply_pending_attack_effects(
        TargetIndex::Unit(target_index),
        PendingAttackEffects {
            triggered_spell: Some(TriggeredAttackEffect::ConeWave(profile())),
            ..Default::default()
        },
        PendingAttackEffectSource {
            id: source,
            position: point(10, 20),
            team: Team(0),
        },
        PendingAttackEffectState {
            navigation_cell_size: sim.config.navigation_cell_size,
            completed_tick: 1,
            units: &mut units,
            unit_health: &mut health,
            buildings: &mut [],
            building_health: &mut [],
            retired_sources: &BTreeMap::new(),
            damage_rules: sim.combat_rules.damage_rules,
        },
    );
    let state = result.cone_wave.unwrap();
    assert_eq!(state.origin, point(20, 20));
    assert!(state.destination.y > point(70, 40).y);
}
