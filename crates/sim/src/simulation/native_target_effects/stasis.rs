use super::*;
use crate::{
    building_mechanics::BuildingStasisProfile,
    components::{NativeAction, StasisTrapState},
};

fn profile() -> BuildingStasisProfile {
    BuildingStasisProfile {
        map_version: crate::MapVersion::CASTLE_FIGHT_9_27,
        ability: AbilityId(101),
        ward_rawcode: 102,
        trigger_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
        trigger_invulnerable: true,
        trigger_spell_immune: false,
        targets: AttackTargetMask::AIR_AND_GROUND,
        detection_radius: 40 * SUBUNITS_PER_WORLD_UNIT,
        detonation_radius: 70 * SUBUNITS_PER_WORLD_UNIT,
        activation_ticks: 3,
        detonation_ticks: 4,
        lifetime_ticks: 30,
        stun_ticks: 11,
        hero_stun_ticks: 5,
        stand_ticks: 8,
    }
}

fn trap(sim: &mut Simulation, team: Team, x: i32, profile: BuildingStasisProfile) -> SimId {
    let id = sim.allocate_id();
    sim.world.spawn((
        id,
        NativeAction::StasisTrap(StasisTrapState {
            source: SimId(999),
            team,
            position: unit(team.0, x, 0, AttackDelivery::Melee).position,
            profile,
            armed_tick: sim.tick() + u64::from(profile.activation_ticks),
            detonation_tick: None,
            expires_tick: sim.tick() + u64::from(profile.lifetime_ticks),
        }),
    ));
    id
}

#[test]
fn trap_arms_detects_then_samples_live_effect_recipients_at_its_own_origin() {
    let mut sim = simulation(1);
    trap(&mut sim, Team(0), 50, profile());
    let trigger = sim.spawn_unit(unit(1, 90, 0, AttackDelivery::Melee));
    let outer = sim.spawn_unit(unit(1, 120, 0, AttackDelivery::Melee));
    let outside = sim.spawn_unit(unit(1, 121, 0, AttackDelivery::Melee));
    let ally = sim.spawn_unit(unit(0, 70, 0, AttackDelivery::Melee));
    let hero = sim.spawn_unit_with_properties(
        unit(1, 80, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            mechanical: true,
            build_time_ticks: Some(7),
            movement_class: MovementClass::Air,
            classifications: UnitClassifications {
                hero: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let protected: Vec<_> = [true, false]
        .into_iter()
        .map(|immune| {
            sim.spawn_unit_with_properties(
                unit(1, 70, 0, AttackDelivery::Melee),
                UnitGameplayProperties {
                    classifications: UnitClassifications {
                        spell_immune: immune,
                        invulnerable: !immune,
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )
        })
        .collect();
    let mut restored = wire_restored(&sim, 4);
    while sim.tick() <= 6 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    assert_eq!(sim.unit(trigger).unwrap().status.native_stun_until_tick, 0);
    // The original detection recipient leaves before detonation; the trap still detonates.
    for simulation in [&mut sim, &mut restored] {
        let e = entity(simulation, trigger);
        simulation
            .world
            .entity_mut(e)
            .insert(Position(unit(1, 130, 0, AttackDelivery::Melee).position));
    }
    assert_eq!(sim.step().checksum, restored.step().checksum);
    assert_eq!(sim.unit(outer).unwrap().status.native_stun_until_tick, 18);
    assert_eq!(sim.unit(hero).unwrap().status.native_stun_until_tick, 12);
    assert_eq!(
        sim.unit(hero).unwrap().status.native_stun_ability,
        Some(profile().ability)
    );
    for id in [trigger, outside, ally].into_iter().chain(protected) {
        assert_eq!(sim.unit(id).unwrap().status.native_stun_until_tick, 0);
    }
    let mut restored = wire_restored(&sim, 2);
    while sim.tick() <= 18 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    assert_eq!(sim.unit(outer).unwrap().health, 1000);
}

#[test]
fn trap_waits_for_eligible_detection_and_expires_without_stunning() {
    for (x, immune, invulnerable) in [(91, false, false), (80, true, false), (80, false, true)] {
        let mut sim = simulation(1);
        let trap_id = trap(&mut sim, Team(0), 50, profile());
        let target = sim.spawn_unit_with_properties(
            unit(1, x, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: UnitClassifications {
                    spell_immune: immune,
                    invulnerable,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let mut restored = wire_restored(&sim, 3);
        while sim.tick() <= 30 {
            assert_eq!(sim.step().checksum, restored.step().checksum);
        }
        assert_eq!(sim.unit(target).unwrap().status.native_stun_until_tick, 0);
        assert!(
            !sim.world
                .iter_entities()
                .any(|e| e.get::<SimId>() == Some(&trap_id))
        );
    }
}

#[test]
fn trap_detonation_removes_nearby_unarmed_traps_before_their_pending_actions() {
    let mut sim = simulation(1);
    let first = trap(&mut sim, Team(0), 50, profile());
    let nearby = trap(
        &mut sim,
        Team(1),
        120,
        BuildingStasisProfile {
            activation_ticks: 15,
            ..profile()
        },
    );
    let outside = trap(
        &mut sim,
        Team(1),
        121,
        BuildingStasisProfile {
            activation_ticks: 15,
            ..profile()
        },
    );
    sim.spawn_unit(unit(1, 90, 0, AttackDelivery::Melee));
    for _ in 0..8 {
        sim.step();
    }
    let exists = |id| {
        sim.world
            .iter_entities()
            .any(|e| e.get::<SimId>() == Some(&id))
    };
    assert!(!exists(first));
    assert!(!exists(nearby));
    assert!(exists(outside));
}

#[test]
fn hostile_structure_trigger_spends_parent_resources_and_trap_survives_source_removal() {
    let mut sim = simulation(1);
    let caster = sim.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(1, 1, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: Some(SpellcastingProfile {
            mana: ManaProfile::per_second(10, 10, 0),
            ability: AutomaticAbilityProfile {
                id: AbilityId(100),
                mana_cost: 3,
                cooldown_ticks: 100,
                range: 500 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::BuildingStasis(profile()),
            },
        }),
    });
    let hostile = sim.spawn_building_with_properties(
        BuildingSpawn {
            team: Team(1),
            footprint: BuildingFootprint::new(5, 1, 1, 1),
            health: 1000,
            attack: None,
            production: None,
            spellcasting: None,
        },
        BuildingGameplayProperties {
            classifications: UnitClassifications {
                invulnerable: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    let origin = footprint_center_point(
        sim.building(caster).unwrap().footprint,
        sim.config.navigation_cell_size,
    );
    let target = sim.spawn_unit_with_properties(
        UnitSpawn {
            position: SimPoint::new(origin.x + profile().detection_radius, origin.y),
            ..unit(1, 60, 0, AttackDelivery::Melee)
        },
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    assert!(sim.set_negative_building_shield(target, 2, None));
    sim.step();
    let view = sim.building(caster).unwrap();
    assert_eq!(view.mana_current, Some(7));
    assert_eq!(view.ability_ready_tick, Some(100));
    assert_eq!(view.status.action_animation.unwrap().until_tick, 8);
    assert_eq!(sim.building(hostile).unwrap().health, 1000);
    sim.world.despawn(entity(&sim, caster));
    let mut restored = wire_restored(&sim, 4);
    while sim.tick() <= 7 {
        assert_eq!(sim.step().checksum, restored.step().checksum);
    }
    assert_eq!(sim.unit(target).unwrap().status.native_stun_until_tick, 18);
    assert_eq!(sim.unit(target).unwrap().negative_building_shield_level, 2);
}
