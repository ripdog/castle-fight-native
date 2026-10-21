use super::*;
use crate::components::{
    AbilityEffect as Effect, DeathExplosionProfile, LandMineProfile, LandMineState, NativeAction,
    ParasiteChild,
};

fn bundle() -> &'static crate::CastleFightContentBundle {
    crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap()
}

fn definition(rawcode: &[u8; 4]) -> ResolvedUnitDefinition {
    let rawcode = u32::from_be_bytes(*rawcode);
    bundle()
        .unit_definitions()
        .find(|definition| definition.rawcode == rawcode)
        .expect("promoted Mechanical body")
        .resolved()
}

fn land_mine(armed_in: u64) -> LandMineProfile {
    LandMineProfile {
        ability: AbilityId(970),
        minimum_distance: 5 * SUBUNITS_PER_WORLD_UNIT,
        maximum_distance: 20 * SUBUNITS_PER_WORLD_UNIT,
        carrier_speed_per_second: 100 * SUBUNITS_PER_WORLD_UNIT,
        activation_ticks: u16::try_from(armed_in).unwrap(),
        detection_radius: 30 * SUBUNITS_PER_WORLD_UNIT,
        trigger_targets: AttackTargetMask::GROUND_UNITS,
        explosion: DeathExplosionProfile {
            ability: AbilityId(971),
            full_radius: 20 * SUBUNITS_PER_WORLD_UNIT,
            full_damage: 200,
            partial_radius: 60 * SUBUNITS_PER_WORLD_UNIT,
            partial_damage: 80,
            delay_ticks: 4,
            targets: AttackTargetMask::GROUND_UNITS,
        },
        initial_mana_minimum_10k: 2_000,
        initial_mana_maximum_10k: 300_000,
    }
}

fn plant_mine(sim: &mut Simulation, source: SimId, profile: LandMineProfile, x: i32) {
    let id = sim.allocate_id();
    sim.world.spawn((
        id,
        NativeAction::LandMine(LandMineState {
            profile,
            source,
            owner: PlayerId(0),
            team: Team(0),
            position: SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT),
            placed_tick: sim.next_tick,
            armed_tick: sim.next_tick + u64::from(profile.activation_ticks),
        }),
    ));
}

fn mine_count(sim: &mut Simulation) -> usize {
    sim.world
        .query::<&NativeAction>()
        .iter(&sim.world)
        .filter(|action| matches!(action, NativeAction::LandMine(_)))
        .count()
}

#[test]
fn land_mine_is_inert_until_armed_then_hits_only_enemy_ground_units_after_the_death_delay() {
    let mut sim = simulation(1);
    let planter = sim.spawn_unit(unit(0, 0, 0, AttackDelivery::Melee));
    // The trigger boundary is the retained detection radius around the mine.
    let profile = land_mine(6);
    plant_mine(&mut sim, planter, profile, 100);
    let ally = sim.spawn_unit(unit(0, 101, 0, AttackDelivery::Melee));
    let flyer = sim.spawn_unit_with_properties(
        unit(1, 102, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            movement_class: MovementClass::Air,
            ..Default::default()
        },
    );
    let inner = sim.spawn_unit(unit(1, 110, 0, AttackDelivery::Melee));
    let outer = sim.spawn_unit(unit(1, 135, 0, AttackDelivery::Melee));
    // Enemies and allies stand on the mine from the start, but nothing happens before arming.
    for _ in 0..profile.activation_ticks {
        sim.step();
        assert_eq!(mine_count(&mut sim), 1);
    }
    assert!(
        [ally, flyer, inner, outer]
            .iter()
            .all(|id| sim.unit(*id).unwrap().health == 1000)
    );
    // Armed: the next evaluation detonates the mine; the explosion lands after its own delay.
    sim.step();
    sim.step();
    assert_eq!(mine_count(&mut sim), 0);
    let explosion_due = sim
        .world
        .query::<&NativeAction>()
        .iter(&sim.world)
        .find_map(|action| match action {
            NativeAction::DeathExplosion(state) => Some(state.due_tick),
            _ => None,
        })
        .expect("the detonated mine leaves its native death explosion");
    assert!(explosion_due > sim.next_tick - 1);
    while sim.next_tick <= explosion_due {
        assert_eq!(sim.unit(inner).unwrap().health, 1000);
        sim.step();
    }
    let loss = |id| 1000 - sim.unit(id).unwrap().health;
    // The nearer enemy is inside the full ring, the farther one only inside the partial ring.
    assert!(loss(inner) > loss(outer) && loss(outer) > 0);
    assert_eq!((loss(ally), loss(flyer)), (0, 0));
}

#[test]
fn a_land_mine_survives_when_no_enemy_ground_unit_is_in_range() {
    let mut sim = simulation(1);
    let planter = sim.spawn_unit(unit(0, 0, 0, AttackDelivery::Melee));
    let profile = land_mine(1);
    plant_mine(&mut sim, planter, profile, 100);
    // Just outside the retained detection radius, and an invulnerable body inside it.
    sim.spawn_unit(unit(1, 100 + 31, 0, AttackDelivery::Melee));
    sim.spawn_unit_with_properties(
        unit(1, 110, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            classifications: UnitClassifications {
                invulnerable: true,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    for _ in 0..10 {
        sim.step();
    }
    assert_eq!(mine_count(&mut sim), 1);
}

#[test]
fn mine_placement_is_uniform_in_angle_and_has_the_sampled_length() {
    let mut sim = simulation(1);
    let planter = sim.spawn_unit(unit(0, 0, 0, AttackDelivery::Melee));
    let distance = 120 * i64::from(SUBUNITS_PER_WORLD_UNIT);
    let mut quadrants = [false; 4];
    for sequence in 0..64 {
        let (dx, dy) = sim.land_mine_direction(planter, sequence, distance);
        let length = ((dx * dx + dy * dy) as f64).sqrt();
        assert!((length - distance as f64).abs() < 2.0, "{dx},{dy}");
        quadrants[usize::from(dx >= 0) * 2 + usize::from(dy >= 0)] = true;
    }
    assert!(quadrants.iter().all(|seen| *seen));
}

#[test]
fn a_trained_mine_layer_rolls_its_initial_mana_inside_the_retained_range() {
    let definition = definition(b"h06U");
    let spellcasting = definition.spellcasting.expect("Mine Layer spell");
    let Effect::ParasiteScript(script) = spellcasting.ability.effect else {
        panic!("Mine Layer is a Parasite script");
    };
    let ParasiteChild::LandMine(mine) = script.child else {
        panic!("Mine Layer plants land mines");
    };
    let mut sim = simulation(1);
    let mut seen = std::collections::BTreeSet::new();
    for raw in 1..200 {
        let mana = sim
            .trained_land_mine_mana(&spellcasting, SimId(raw))
            .expect("a Mine Layer rolls its mana");
        let total =
            u64::try_from(mana.current).unwrap() * 10_000 + u64::from(mana.regen_remainder_per_10k);
        assert!(u64::from(mine.initial_mana_minimum_10k) <= total);
        assert!(total <= u64::from(mine.initial_mana_maximum_10k));
        seen.insert(total);
    }
    assert!(seen.len() > 100, "{} distinct rolls", seen.len());
    sim.step();
}

#[test]
fn a_gobbo_repairs_a_wounded_mechanical_ally_and_every_gobbo_expires() {
    let gobbo = definition(b"n01U");
    let tank = definition(b"h06T");
    let Some(PassiveUnitEffect::GobboRepair(profile)) = gobbo
        .properties
        .passive_effects
        .iter()
        .find(|effect| matches!(effect, PassiveUnitEffect::GobboRepair(_)))
    else {
        panic!("the Gobbo carries its repair controller");
    };
    let mut sim = simulation(1);
    let helper = sim.spawn_resolved_unit(
        Team(0),
        SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT),
        gobbo,
    );
    let patient = sim.spawn_resolved_unit(
        Team(0),
        SimPoint::new(100 * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT),
        tank,
    );
    let organic = sim.spawn_unit(unit(0, 140, 0, AttackDelivery::Melee));
    let wounded = sim.unit(patient).unwrap().health / 2;
    for id in [patient, organic] {
        sim.world
            .get_mut::<Health>(entity(&sim, id))
            .unwrap()
            .current = wounded;
    }
    let mut last = wounded;
    // One shared queue pass fires within the first interval, so the Gobbo picks and repairs.
    for _ in 0..u64::from(profile.rules.interval_ticks) * 40 {
        sim.step();
        let health = sim.unit(patient).unwrap().health;
        assert!(health >= last);
        last = health;
    }
    assert!(last > wounded, "the mechanical ally was repaired");
    // Only mechanical bodies and structures are repair targets: the wounded organic ally is not.
    assert_eq!(sim.unit(organic).unwrap().health, wounded);
    while sim.unit(helper).is_some() && sim.next_tick <= u64::from(profile.timed_life_ticks) + 5 {
        sim.step();
    }
    assert!(
        sim.unit(helper).is_none_or(|unit| unit.health <= 0),
        "the Gobbo lives only for its retained timed life"
    );
}

#[test]
fn a_contact_self_destruct_walks_to_its_target_and_spares_allies() {
    let lunatic = definition(b"n01V");
    let mut sim = simulation(1);
    let bomber = sim.spawn_resolved_unit(
        Team(0),
        SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT),
        lunatic,
    );
    let victim = sim.spawn_unit(unit(1, 160, 0, AttackDelivery::Melee));
    let bystander = sim.spawn_unit(unit(0, 40, 0, AttackDelivery::Melee));
    // It runs to its target rather than exploding in place.
    for _ in 0..3 {
        sim.step();
    }
    assert!(sim.unit(bomber).unwrap().health > 0);
    assert_eq!(sim.unit(victim).unwrap().health, 1000);
    for _ in 0..600 {
        sim.step();
    }
    let alive = |id| sim.unit(id).is_some_and(|unit| unit.health > 0);
    assert!(!alive(bomber), "the cast kills its caster");
    assert!(
        sim.unit(victim).is_none_or(|unit| unit.health < 1000),
        "the explosion hurt its target"
    );
    // Its own side takes nothing: the bystander shared the bomber's start position.
    assert_eq!(sim.unit(bystander).unwrap().health, 1000);
}

#[test]
fn a_power_plant_covers_nearby_allied_buildings_and_augments_their_trainees() {
    let content = bundle();
    let version = crate::MapVersion::CASTLE_FIGHT_9_27;
    let plant = content
        .tower(crate::CastleFightTowerKind::PowerPlant)
        .unwrap();
    let barracks = content
        .production_building(crate::CastleFightProductionKind::Barracks)
        .unwrap();
    let spawn = &crate::building_mechanics::power_plant_for_version(version)
        .unwrap()
        .spawn;
    let mut sim = simulation(1);
    sim.spawn_building_for_player_with_properties(
        PlayerId(0),
        plant.spawn(Team(0), BuildingFootprint::new(20, 0, 4, 4)),
        plant.gameplay_properties(),
    );
    // Two otherwise identical producers, one inside the plant's area and one far outside it.
    for x in [25, 400] {
        let mut building = barracks.spawn(Team(0), BuildingFootprint::new(x, 0, 4, 4));
        building.production.as_mut().unwrap().initial_delay_ticks = 3;
        sim.spawn_building_for_player_with_properties(
            PlayerId(0),
            building,
            barracks.gameplay_properties(),
        );
    }
    // The aura is refreshed after production in a tick, so training starts a few ticks later.
    for _ in 0..4 {
        sim.step();
    }
    let mut trained: Vec<_> = sim.units().into_iter().collect();
    trained.sort_by_key(|unit| unit.position.x);
    let [near, far] = trained.as_slice() else {
        panic!("each producer trained one unit, got {}", trained.len());
    };
    // Covered production earns the retained maximum-life bonus and nothing else changes it.
    assert_eq!(near.health, far.health + spawn.max_life_bonus);
    // A body that already carries a spell defense gains no second reduction.
    let footman = definition(b"hfoo");
    let golem = definition(b"n01X");
    for (body, extra) in [(footman, 1), (golem, 0)] {
        let mut augmented = body;
        Simulation::augment_power_armor_definition(spawn, &mut augmented, version);
        assert_eq!(
            augmented.properties.passive_effects.iter().count(),
            body.properties.passive_effects.iter().count() + extra
        );
    }
}
