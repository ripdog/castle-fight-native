use super::*;
use crate::{
    ManaProfile, MapVersion, NativeAbilityGrant, NativeAbilityGrantEffect, NativeAbilityGrants,
};

fn profile() -> WellOfPainProfile {
    WellOfPainProfile {
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
        selector_excluded_rawcode: u32::from_be_bytes(*b"hxxx"),
        wounded_exclusion: AbilityId(507),
        damage: 160,
        ghost_rawcode: u32::from_be_bytes(*b"n03C"),
    }
}
fn simulation() -> Simulation {
    Simulation::new(
        SimulationConfig {
            unit_separation_distance: 0,
            navigation_cell_size: 16 * SUBUNITS_PER_WORLD_UNIT,
            navigation_max: NavCell::new(500, 64),
            ..Default::default()
        },
        1,
    )
}
fn well(sim: &mut Simulation) -> AbilitySourceSnapshot {
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
                effect: AbilityEffect::WellOfPain(profile()),
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
fn enemy(sim: &mut Simulation, x: i32) -> SimId {
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
            classifications: UnitClassifications {
                combat_sapper: true,
                ..Default::default()
            },
            ..Default::default()
        },
    )
}
fn ghosts(sim: &mut Simulation) -> usize {
    sim.snapshot_units()
        .iter()
        .filter(|u| u.rawcode == profile().ghost_rawcode && u.team == Team(0))
        .count()
}

#[test]
fn wounded_enemies_are_always_preferred_and_full_health_enemies_are_the_fallback() {
    for seed in 0..8 {
        let mut sim = simulation();
        let src = well(&mut sim);
        let full = enemy(&mut sim, 300);
        let hurt = enemy(&mut sim, 301);
        let mut units = sim.snapshot_units();
        let hurt_index = units.iter().position(|u| u.id == hurt).unwrap();
        units[hurt_index].health = 900;
        assert!(sim.cast_well_of_pain(src, AbilityId(501), full, profile(), seed, &mut units));
        assert_eq!(units[hurt_index].health, 740);
        assert!(
            units
                .iter()
                .filter(|u| u.id != hurt)
                .all(|u| u.health == 1000)
        );
        units[hurt_index].health = 1000;
        assert!(sim.cast_well_of_pain(
            src,
            AbilityId(501),
            full,
            profile(),
            seed + 100,
            &mut units
        ));
        assert_eq!(units.iter().filter(|u| u.health == 840).count(), 1);
    }
}

#[test]
fn lethal_strike_raises_one_friendly_body_but_a_surviving_or_immune_target_does_not() {
    let mut sim = simulation();
    let src = well(&mut sim);
    let victim = enemy(&mut sim, 300);
    let mut units = sim.snapshot_units();
    units[0].health = 160;
    units[0].classifications.spell_immune = true;
    assert!(sim.cast_well_of_pain(src, AbilityId(501), victim, profile(), 0, &mut units));
    assert_eq!((units[0].health, ghosts(&mut sim)), (160, 0));
    units[0].classifications.spell_immune = false;
    units[0].health = 161;
    assert!(sim.cast_well_of_pain(src, AbilityId(501), victim, profile(), 1, &mut units));
    assert_eq!((units[0].health, ghosts(&mut sim)), (1, 0));
    units[0].health = 160;
    assert!(sim.cast_well_of_pain(src, AbilityId(501), victim, profile(), 2, &mut units));
    assert_eq!((units[0].health <= 0, ghosts(&mut sim)), (true, 1));
}

#[test]
fn native_marker_exclusions_apply_to_both_selectors_and_the_extra_marker_only_to_wounded_selection()
{
    let mut sim = simulation();
    let src = well(&mut sim);
    let id = enemy(&mut sim, 300);
    let mut units = sim.snapshot_units();
    let mark = |ability| {
        NativeAbilityGrants::from_slice(&[NativeAbilityGrant {
            ability,
            effect: NativeAbilityGrantEffect::AuraMarker,
        }])
    };
    for exclusion in profile().selector_exclusions {
        units[0].status.native_grants = mark(exclusion);
        units[0].health = 500;
        assert!(!sim.cast_well_of_pain(src, AbilityId(501), id, profile(), 0, &mut units));
        assert_eq!(units[0].health, 500);
    }
    // The extra wounded-only marker removes the wounded candidate but not the ordinary fallback.
    units[0].status.native_grants = mark(profile().wounded_exclusion);
    assert!(sim.cast_well_of_pain(src, AbilityId(501), id, profile(), 0, &mut units));
    assert_eq!(units[0].health, 340);
}
