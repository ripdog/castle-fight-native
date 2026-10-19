use super::*;
use crate::{
    ManaProfile, MapVersion, NativeAbilityGrant, NativeAbilityGrantEffect, NativeAbilityGrants,
};

fn profile() -> ForgottenOneProfile {
    ForgottenOneProfile {
        parent: crate::NativeParasiteProfile {
            buff_rawcode: 506,
            map_version: MapVersion::CASTLE_FIGHT_9_27,
            speed_per_tick: 1000 * SUBUNITS_PER_WORLD_UNIT,
            affects_invulnerable: true,
            duration_ticks: 1,
            hero_duration_ticks: 0,
            body_rawcode: u32::from_be_bytes(*b"nfbr"),
            count: 1,
            body_duration_ticks: 1,
        },
        map_version: MapVersion::CASTLE_FIGHT_9_27,
        trigger_targets: AttackTargetMask::GROUND_AND_BUILDINGS,
        trigger_invulnerable: true,
        trigger_spell_immune: false,
        trigger_mechanical_only: true,
        selector_exclusions: [AbilityId(504), AbilityId(505)],
        artillery_body: u32::from_be_bytes(*b"n01C"),
        plain_body: u32::from_be_bytes(*b"n021"),
        count: 6,
        angle_step_degrees: 60,
        ring_radius: 64 * SUBUNITS_PER_WORLD_UNIT,
        lifetime_ticks: 270,
    }
}
fn simulation(artillery: bool) -> Simulation {
    let mut config = SimulationConfig {
        unit_separation_distance: 0,
        navigation_cell_size: 16 * SUBUNITS_PER_WORLD_UNIT,
        navigation_max: NavCell::new(500, 64),
        ..Default::default()
    };
    config.match_rules.artillery_enabled = artillery;
    Simulation::new(config, 1)
}
fn source(sim: &mut Simulation) -> AbilitySourceSnapshot {
    sim.spawn_building(BuildingSpawn {
        team: Team(0),
        footprint: BuildingFootprint::new(1, 1, 1, 1),
        health: 1000,
        attack: None,
        production: None,
        spellcasting: Some(SpellcastingProfile {
            mana: ManaProfile {
                maximum: 9,
                starting: 9,
                regen_per_tick_per_10k: 0,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(501),
                mana_cost: 5,
                cooldown_ticks: 30,
                range: 100 * SUBUNITS_PER_WORLD_UNIT,
                target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
                effect: AbilityEffect::ForgottenOne(profile()),
            },
        }),
    });
    let b = sim.snapshot_buildings()[0];
    AbilitySourceSnapshot {
        map_version: b.map_version,
        source: AbilitySourceIndex::Building(0),
        id: b.id,
        team: b.team,
        origin: AbilitySourceOrigin::Building(b.footprint),
        health: b.health,
        stunned_until_tick: 0,
        spellcasting: b.spellcasting,
        mana_current: b.mana_current,
        ability_state: b.ability_state,
    }
}
fn enemy(sim: &mut Simulation, x: i32, air: bool) -> SimId {
    sim.spawn_unit_with_properties(
        UnitSpawn {
            team: Team(1),
            position: SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT),
            health: 1000,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1000,
                delivery: AttackDelivery::Melee,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        },
        UnitGameplayProperties {
            movement_class: if air {
                MovementClass::Air
            } else {
                MovementClass::Ground
            },
            classifications: UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}
fn restored(sim: &Simulation) -> Simulation {
    let content = crate::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27).unwrap();
    let snapshot =
        SimulationSnapshot::decode_wire(&sim.capture_snapshot().encode_wire().unwrap(), content)
            .unwrap();
    let mut other = Simulation::new(sim.config.clone(), 4);
    other.restore_snapshot(&snapshot).unwrap();
    other
}
fn bodies(sim: &mut Simulation, rawcode: u32) -> Vec<UnitSnapshot> {
    sim.snapshot_units()
        .into_iter()
        .filter(|u| u.rawcode == rawcode && u.team == Team(0))
        .collect()
}

#[test]
fn ring_has_six_friendly_bodies_around_the_ground_target_and_follows_the_artillery_mode_body() {
    for artillery in [true, false] {
        let mut sim = simulation(artillery);
        let src = source(&mut sim);
        let target = enemy(&mut sim, 300, false);
        enemy(&mut sim, 301, true); // Air units are never selected.
        let units = sim.snapshot_units();
        let centre = units.iter().find(|u| u.id == target).unwrap().position;
        assert!(sim.cast_forgotten_one(src, AbilityId(501), target, profile(), 0, &units));
        let (used, other) = if artillery {
            (profile().artillery_body, profile().plain_body)
        } else {
            (profile().plain_body, profile().artillery_body)
        };
        let ring = bodies(&mut sim, used);
        assert_eq!((ring.len(), bodies(&mut sim, other).len()), (6, 0));
        let radius_sq = i64::from(profile().ring_radius).pow(2);
        for body in &ring {
            let dx = i64::from(body.position.x - centre.x);
            let dy = i64::from(body.position.y - centre.y);
            assert!(
                (dx * dx + dy * dy - radius_sq).abs() < radius_sq / 1_000,
                "{dx} {dy} {radius_sq}"
            );
        }
        let mut positions: Vec<_> = ring.iter().map(|b| (b.position.x, b.position.y)).collect();
        positions.sort_unstable();
        positions.dedup();
        assert_eq!(positions.len(), 6);
        // Timed life expires every body together.
        let mut other_sim = restored(&sim);
        for _ in 0..=profile().lifetime_ticks {
            assert_eq!(
                {
                    sim.step();
                    sim.checksum()
                },
                {
                    other_sim.step();
                    other_sim.checksum()
                }
            );
        }
        assert!(bodies(&mut sim, used).is_empty());
    }
}

#[test]
fn only_living_ground_non_excluded_enemies_are_eligible_and_failure_creates_no_bodies() {
    let mut sim = simulation(true);
    let src = source(&mut sim);
    let only_air = enemy(&mut sim, 300, true);
    let units = sim.snapshot_units();
    assert!(!sim.cast_forgotten_one(src, AbilityId(501), only_air, profile(), 0, &units));
    let ground = enemy(&mut sim, 302, false);
    let mut units = sim.snapshot_units();
    let index = units.iter().position(|u| u.id == ground).unwrap();
    for exclusion in profile().selector_exclusions {
        units[index].status.native_grants =
            NativeAbilityGrants::from_slice(&[NativeAbilityGrant {
                ability: exclusion,
                effect: NativeAbilityGrantEffect::AuraMarker,
            }]);
        assert!(!sim.cast_forgotten_one(src, AbilityId(501), ground, profile(), 0, &units));
    }
    assert!(bodies(&mut sim, profile().artillery_body).is_empty());
}
