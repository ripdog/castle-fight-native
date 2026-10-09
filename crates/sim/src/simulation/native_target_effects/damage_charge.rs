use super::*;
use crate::{DamageChargeProfile, SelfDestructProfile};

pub(super) fn profile() -> DamageChargeProfile {
    DamageChargeProfile {
        marker: AbilityId(109),
        mana_maximum: 100,
        mana_starting: 0,
        mana_regen_per_second_per_10k: -10_000,
        threshold: 30,
        armed_speed_per_tick: 5 * SUBUNITS_PER_WORLD_UNIT,
        explosion: SelfDestructProfile {
            ability: AbilityId(110),
            full_radius: 40 * SUBUNITS_PER_WORLD_UNIT,
            full_damage: 100,
            partial_radius: 70 * SUBUNITS_PER_WORLD_UNIT,
            partial_damage: 40,
            building_damage_per_10k: 5000,
            targets: AttackTargetMask::AIR_UNITS.union(AttackTargetMask::BUILDINGS),
        },
    }
}

fn charged_unit(sim: &mut Simulation, x: i32) -> SimId {
    sim.spawn_unit_with_properties(
        unit(0, x, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            collision_radius: Some(CollisionRadius(2 * SUBUNITS_PER_WORLD_UNIT)),
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::DamageCharge(profile())),
            action_timing: ActionTimingProfile {
                cast_point_ticks: 2,
                cast_ticks: 3,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}

fn arm(sim: &mut Simulation, source: SimId) {
    let e = entity(sim, source);
    sim.world
        .entity_mut(e)
        .get_mut::<StatusState>()
        .unwrap()
        .damage_charge
        .as_mut()
        .unwrap()
        .armed = true;
}

#[test]
fn damage_charge_uses_reduced_events_without_absorption_and_keeps_fractional_drain() {
    let mut sim = simulation(1);
    let source = charged_unit(&mut sim, 20);
    let mut units = sim.snapshot_units();
    let target = &mut units[0];
    let state = target.status.damage_charge.as_mut().unwrap();
    super::super::damage_charge::restore_mana(state, 10);
    for _ in 0..15 {
        super::super::damage_charge::advance_regeneration(state, 0);
    }
    assert_eq!(
        state.mana_per_10k_hz,
        95_000 * crate::CASTLE_FIGHT_SIMULATION_HZ as i64
    );
    let health = target.health;
    super::super::status::apply_ability_effect_to_unit(
        target,
        AbilityEffect::Damage { amount: 10 },
        Team(1),
        1,
        DamageRules::warcraft_frozen_throne(),
    );
    assert_eq!(target.health, health - 10);
    assert_eq!(
        super::super::damage_charge::mana_current(target.status.damage_charge.unwrap()),
        19
    );
    assert!(!target.status.damage_charge.unwrap().armed);
    // The threshold branch enables autocast but does not store the triggering addition.
    let previous = target.status.damage_charge.unwrap().mana_per_10k_hz;
    super::super::damage_thresholds::observe_damage_event(target, target.health, 11);
    assert!(target.status.damage_charge.unwrap().armed);
    assert_eq!(
        target.status.damage_charge.unwrap().mana_per_10k_hz,
        previous
    );
    super::super::damage_thresholds::observe_damage_event(target, target.health, 90);
    assert_eq!(
        target.status.damage_charge.unwrap().mana_per_10k_hz,
        previous
    );
    assert!(sim.unit(source).is_some());
}

#[test]
fn damage_charge_contact_autocast_waits_for_cast_point_and_recovers_in_progress() {
    let mut sim = simulation(1);
    let source = charged_unit(&mut sim, 20);
    let target = sim.spawn_unit_with_properties(
        unit(1, 50, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            collision_radius: Some(CollisionRadius(2 * SUBUNITS_PER_WORLD_UNIT)),
            ..Default::default()
        },
    );
    arm(&mut sim, source);
    sim.step();
    assert!(
        sim.unit(source)
            .unwrap()
            .status
            .damage_charge
            .unwrap()
            .detonation_tick
            .is_none(),
        "explosion radius must not become autocast range"
    );
    assert_eq!(sim.unit(target).unwrap().health, 1000);
    for _ in 0..20 {
        sim.step();
        if sim
            .unit(source)
            .unwrap()
            .status
            .damage_charge
            .unwrap()
            .detonation_tick
            .is_some()
        {
            break;
        }
    }
    let deadline = sim
        .unit(source)
        .unwrap()
        .status
        .damage_charge
        .unwrap()
        .detonation_tick
        .unwrap();
    assert!(deadline >= sim.tick());
    let mut restored = wire_restored(&sim, 4);
    while sim.tick() <= deadline {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    assert!(sim.unit(source).is_none());
    assert_eq!(sim.unit(target).unwrap().health, 900);
    assert_eq!(sim.last_ability_casts[0].ability, AbilityId(110));
}

#[test]
fn native_self_destruct_uses_caster_center_falloff_armor_points_and_air_mask() {
    let mut sim = simulation(1);
    let source = charged_unit(&mut sim, 20);
    arm(&mut sim, source);
    let mut targets = Vec::new();
    for (team, x, movement_class, invulnerable, spell_immune, armor) in [
        (1, 20, MovementClass::Air, false, false, ArmorType::Large),
        (1, 60, MovementClass::Air, false, true, ArmorType::Fortified),
        (1, 61, MovementClass::Air, false, false, ArmorType::Medium),
        (1, 90, MovementClass::Air, false, false, ArmorType::Small),
        (1, 91, MovementClass::Air, false, false, ArmorType::Small),
        (1, 30, MovementClass::Ground, false, false, ArmorType::Small),
        (0, 30, MovementClass::Air, false, false, ArmorType::Small),
        (1, 30, MovementClass::Air, true, false, ArmorType::Small),
    ] {
        targets.push(sim.spawn_unit_with_properties(
            unit(team, x, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                movement_class,
                armor: ArmorProfile::new(armor, 10),
                classifications: UnitClassifications {
                    invulnerable,
                    spell_immune,
                    ..Default::default()
                },
                ..Default::default()
            },
        ));
    }
    let mut units = sim.snapshot_units();
    units[0]
        .status
        .damage_charge
        .as_mut()
        .unwrap()
        .detonation_tick = Some(sim.tick());
    sim.resolve_damage_charge(&mut units, &mut []);
    assert_eq!(units[0].health, 0);
    let healths: Vec<_> = targets
        .iter()
        .map(|id| units.iter().find(|unit| unit.id == *id).unwrap().health)
        .collect();
    assert_eq!(healths, [937, 937, 975, 975, 1000, 1000, 1000, 1000]);
}

#[test]
fn fatal_charge_event_and_ordinary_death_do_not_trigger_a_damaging_explosion() {
    let mut sim = simulation(1);
    let attacker = sim.spawn_unit_with_properties(
        unit(1, 50, 1200, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    let source = charged_unit(&mut sim, 20);
    let e = entity(&sim, attacker);
    sim.world
        .entity_mut(e)
        .get_mut::<TargetState>()
        .unwrap()
        .current = Some(source);
    sim.step();
    sim.step();
    assert!(sim.unit(source).is_none());
    assert_eq!(sim.unit(attacker).unwrap().health, 1000);
    assert!(
        !sim.last_ability_casts
            .iter()
            .any(|cast| cast.ability == AbilityId(110))
    );
}

#[test]
fn native_charge_survives_disabled_passives_but_cannot_cast_while_disabled() {
    let mut sim = simulation(1);
    charged_unit(&mut sim, 20);
    sim.spawn_unit_with_properties(
        unit(1, 20, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    let mut units = sim.snapshot_units();
    units[0].abilities_disabled = true;
    units[0].passive_effects = PassiveUnitEffects::EMPTY;
    super::super::damage_thresholds::observe_damage_event(&mut units[0], 1000, 40);
    assert!(units[0].status.damage_charge.unwrap().armed);
    sim.resolve_damage_charge(&mut units, &mut []);
    assert_eq!(units[0].health, 1000);
    assert!(
        units[0]
            .status
            .damage_charge
            .unwrap()
            .detonation_tick
            .is_none()
    );
    units[0].abilities_disabled = false;
    sim.resolve_damage_charge(&mut units, &mut []);
    assert_eq!(
        units[0].status.damage_charge.unwrap().detonation_tick,
        Some(2)
    );
    let animation = units[0].status.action_animation;
    super::super::damage_thresholds::observe_damage_event(&mut units[0], 1000, 40);
    assert_eq!(
        units[0].status.action_animation, animation,
        "further damage must not restart an armed native cast"
    );
}

#[test]
fn native_self_destruct_applies_building_factor_at_footprint_distance_and_skips_protection() {
    let mut sim = Simulation::new(
        SimulationConfig {
            navigation_cell_size: 10 * SUBUNITS_PER_WORLD_UNIT,
            navigation_max: NavCell::new(500, 64),
            ..Default::default()
        },
        1,
    );
    charged_unit(&mut sim, 20);
    // The footprint edge is in the partial ring; its center is outside the entire AoE.
    let footprint = BuildingFootprint::new(7, 1, 8, 2);
    sim.spawn_building(BuildingSpawn {
        team: Team(1),
        footprint,
        health: 1000,
        production: None,
        attack: None,
        spellcasting: None,
    });
    let mut units = sim.snapshot_units();
    let mut buildings = sim.snapshot_buildings();
    units[0].status.damage_charge.as_mut().unwrap().armed = true;
    units[0]
        .status
        .damage_charge
        .as_mut()
        .unwrap()
        .detonation_tick = Some(0);
    buildings[0].snow_damage_taken_per_10k = 5000;
    sim.resolve_damage_charge(&mut units, &mut buildings);
    assert_eq!(buildings[0].health, 990);
    units[0].health = 1000;
    buildings[0].classifications.invulnerable = true;
    sim.resolve_damage_charge(&mut units, &mut buildings);
    assert_eq!(buildings[0].health, 990);
}
