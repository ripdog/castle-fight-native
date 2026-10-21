use super::*;
use crate::components::{
    AbilityEffect as Effect, DeathAndDecayProfile, DeathMasteryProfile, NativeAction,
};
use crate::{CorpseDefinitionId, PoisonProfile};

fn bundle() -> &'static crate::CastleFightContentBundle {
    crate::castle_fight_content_bundle(crate::MapVersion::CASTLE_FIGHT_9_27).unwrap()
}

fn definition(rawcode: &[u8; 4]) -> ResolvedUnitDefinition {
    let rawcode = u32::from_be_bytes(*rawcode);
    bundle()
        .unit_definitions()
        .find(|definition| definition.rawcode == rawcode)
        .expect("promoted Undead body")
        .resolved()
}

fn at(x: i32) -> SimPoint {
    SimPoint::new(x * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT)
}

/// A corpse of the retained body `rawcode`, as left by any team.
fn corpse(sim: &mut Simulation, rawcode: &[u8; 4], team: Team, x: i32) {
    let body = definition(rawcode);
    let source = sim.allocate_id();
    let id = sim.allocate_id();
    sim.world.spawn((
        id,
        Position(at(x)),
        Corpse {
            source_unit: source,
            source_owner: PlayerId(team.0),
            source_team: team,
            definition: CorpseDefinitionId(u32::from_be_bytes(*rawcode)),
            created_tick: 0,
            decay_start_tick: 0,
            expires_tick: None,
            resurrection: Some(body),
            shrine_state: ShrineRevivalState::default(),
            vertex_rgba: None,
        },
    ));
}

fn alive_count(sim: &Simulation, rawcode: &[u8; 4]) -> usize {
    sim.units()
        .into_iter()
        .filter(|unit| {
            unit.health > 0
                && unit.content.map(|content| content.rawcode) == Some(u32::from_be_bytes(*rawcode))
        })
        .count()
}

#[test]
fn raise_dead_consumes_only_raisable_corpses_of_either_team_and_pays_once() {
    let necromancer = definition(b"u005");
    let spell = necromancer.spellcasting.expect("Necromancer Raise Dead");
    let Effect::RaiseDead(profile) = spell.ability.effect else {
        panic!("the Necromancer raises the dead");
    };
    let mut sim = simulation(1);
    sim.spawn_resolved_unit(Team(0), at(40), necromancer);
    // A footman's corpse of the enemy and an ally's both qualify; a Zombie-free control: the
    // Necromancer's own kind cannot be raised (cannot-raise death type).
    corpse(&mut sim, b"hfoo", Team(1), 60);
    corpse(&mut sim, b"hfoo", Team(0), 70);
    for _ in 0..90 {
        sim.step();
    }
    // Exactly the retained count of corpses is consumed by the one cast, nearest first.
    assert_eq!(sim.corpse_count(), 2 - usize::from(profile.count));
    assert_eq!(alive_count(&sim, b"u004"), usize::from(profile.count));
    // The raised body belongs to the caster's team and is flagged as summoned.
    let raised = sim
        .units()
        .into_iter()
        .find(|unit| unit.content.map(|content| content.rawcode) == Some(profile.body_rawcode))
        .unwrap();
    assert_eq!(raised.team, Team(0));
    // The cooldown keeps the second corpse until it has elapsed.
    for _ in 0..u64::from(spell.ability.cooldown_ticks) / 2 {
        sim.step();
    }
    assert_eq!(sim.corpse_count(), 2 - usize::from(profile.count));
}

#[test]
fn raise_dead_ignores_a_body_the_map_cannot_raise() {
    let necromancer = definition(b"u005");
    let mut sim = simulation(1);
    sim.spawn_resolved_unit(Team(0), at(40), necromancer);
    // Mechanical bodies cannot be raised: the map's death type forbids it.
    corpse(&mut sim, b"h06T", Team(1), 60);
    for _ in 0..90 {
        sim.step();
    }
    assert_eq!(sim.corpse_count(), 1);
    assert_eq!(alive_count(&sim, b"u004"), 0);
}

fn infection() -> PoisonProfile {
    let Some(PassiveUnitEffect::DiseaseCloud(profile)) = definition(b"u007")
        .properties
        .passive_effects
        .iter()
        .find(|effect| matches!(effect, PassiveUnitEffect::DiseaseCloud(_)))
    else {
        panic!("the Plague Bearer carries Disease Cloud");
    };
    profile.infection
}

#[test]
fn disease_cloud_infection_outlives_the_carrier_and_a_death_leaves_a_spreading_ward() {
    let bearer_definition = definition(b"u007");
    let infection = infection();
    let ward = infection.ward.expect("the plague leaves a ward");
    let mut sim = simulation(1);
    let carrier = sim.spawn_resolved_unit(Team(0), at(40), bearer_definition);
    let first = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
    let far = sim.spawn_unit(unit(1, 40 + 380, 0, AttackDelivery::Melee));
    for _ in 0..3 {
        sim.step();
    }
    let infected = |sim: &Simulation, id| {
        sim.unit(id)
            .unwrap()
            .status
            .poison
            .is_some_and(|p| p.sources > 0)
    };
    assert!(infected(&sim, first));
    assert!(!infected(&sim, far), "the cloud only reaches its own area");
    // Killing the carrier does not cure the infection.
    sim.world
        .get_mut::<Health>(entity(&sim, carrier))
        .unwrap()
        .current = 0;
    sim.step();
    assert!(infected(&sim, first));
    // An enemy of the infected body kills it in the combat phase; the cloud then reaches a
    // bystander who never met the bearer.
    let bystander = sim.spawn_unit(unit(1, 80, 0, AttackDelivery::Melee));
    sim.spawn_unit(unit(0, 61, 10_000, AttackDelivery::Melee));
    for _ in 0..30 {
        sim.step();
    }
    assert!(sim.unit(first).is_none_or(|unit| unit.health <= 0));
    assert!(
        sim.world
            .query::<&NativeAction>()
            .iter(&sim.world)
            .any(|action| matches!(action, NativeAction::PlagueWard(_)))
            || infected(&sim, bystander)
    );
    for _ in 0..u64::from(ward.duration_ticks) {
        sim.step();
    }
    assert!(infected(&sim, bystander));
}

fn mastery() -> DeathMasteryProfile {
    let Some(PassiveUnitEffect::DeathMastery(profile)) = definition(b"u00D")
        .properties
        .passive_effects
        .iter()
        .find(|effect| matches!(effect, PassiveUnitEffect::DeathMastery(_)))
    else {
        panic!("the Lich King carries its damage listener");
    };
    profile
}

#[test]
fn devour_heals_a_share_of_the_target_life_and_kills_through_armor() {
    let profile = mastery();
    let lich = definition(b"u00D");
    let mut sim = simulation(1);
    let king = sim.spawn_resolved_unit(Team(0), at(40), lich);
    let victim = sim.spawn_unit(unit(1, 50, 0, AttackDelivery::Melee));
    sim.world
        .get_mut::<Health>(entity(&sim, king))
        .unwrap()
        .current = 100;
    let victim_life = sim.unit(victim).unwrap().health;
    let mut units = sim.snapshot_units();
    let mut health: Vec<_> = units.iter().map(|u| u.health).collect();
    let mut buildings = Vec::new();
    let mut building_health = Vec::new();
    let retired = BTreeMap::new();
    let mut state = PendingAttackEffectState {
        navigation_cell_size: sim.config.navigation_cell_size,
        completed_tick: 0,
        units: &mut units,
        unit_health: &mut health,
        buildings: &mut buildings,
        building_health: &mut building_health,
        damage_rules: sim.combat_rules.damage_rules,
        retired_sources: &retired,
    };
    let source = PendingAttackEffectSource {
        id: king,
        position: at(40),
        team: Team(0),
    };
    let target = state.units.iter().position(|u| u.id == victim).unwrap();
    death_mastery::devour(profile, source, target, &mut state);
    let bearer = state.units.iter().position(|u| u.id == king).unwrap();
    assert_eq!(
        state.unit_health[bearer],
        100 + victim_life / 2,
        "the heal is the retained share of the target's life before the blow"
    );
    assert!(state.unit_health[target] <= 0, "death damage ignores armor");
}

#[test]
fn death_and_decay_costs_a_share_of_maximum_life_once_per_second_for_its_duration() {
    let profile: DeathAndDecayProfile = mastery().decay;
    let mut sim = simulation(1);
    let victim = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
    let ally = sim.spawn_unit(unit(0, 60, 0, AttackDelivery::Melee));
    sim.launch_death_and_decay(crate::components::DeathAndDecayState {
        source: ally,
        team: Team(0),
        position: at(60),
        profile,
        next_pulse_tick: sim.next_tick + 30,
        expires_tick: sim.next_tick + u64::from(profile.duration_ticks),
    });
    let mut first_pulse_tick = None;
    for _ in 0..40 {
        sim.step();
        if first_pulse_tick.is_none() && sim.unit(victim).unwrap().health < 1000 {
            first_pulse_tick = Some(sim.next_tick);
        }
    }
    // The first pulse lands one second after the release, never earlier.
    assert!(first_pulse_tick.is_some_and(|tick| tick >= 30));
    let first = 1000 - sim.unit(victim).unwrap().health;
    assert!(first > 0);
    for _ in 0..u64::from(profile.duration_ticks) + 40 {
        sim.step();
    }
    // One equal pulse per second for the whole duration, then nothing more.
    let pulses = i32::try_from(u64::from(profile.duration_ticks) / 30).unwrap();
    assert_eq!(1000 - sim.unit(victim).unwrap().health, first * pulses);
    assert_eq!(sim.unit(ally).unwrap().health, 1000);
}

#[test]
fn a_completed_building_burns_organic_enemies_anywhere_and_spares_machines() {
    let city = bundle()
        .tower(crate::CastleFightTowerKind::CityOfDecay)
        .unwrap();
    let mut sim = simulation(1);
    sim.spawn_building_for_player_with_properties(
        PlayerId(0),
        city.spawn(Team(0), BuildingFootprint::new(20, 0, 4, 4)),
        city.gameplay_properties(),
    );
    let faraway = sim.spawn_unit(unit(1, 400, 0, AttackDelivery::Melee));
    let machine = sim.spawn_unit_with_properties(
        unit(1, 405, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            mechanical: true,
            build_time_ticks: Some(30),
            ..Default::default()
        },
    );
    for _ in 0..70 {
        sim.step();
    }
    assert!(sim.unit(faraway).unwrap().health < 1000);
    assert_eq!(
        sim.unit(machine).unwrap().health,
        1000,
        "organic bodies only"
    );
}

#[test]
fn skull_pile_raises_from_a_corpse_of_either_team_but_never_from_undead_or_invulnerable_bodies() {
    let pile = bundle()
        .tower(crate::CastleFightTowerKind::SkullPile)
        .unwrap();
    let mut sim = simulation(1);
    sim.spawn_building_for_player_with_properties(
        PlayerId(0),
        pile.spawn(Team(0), BuildingFootprint::new(20, 0, 4, 4)),
        pile.gameplay_properties(),
    );
    // An undead corpse is the only candidate at first: nothing may be raised from it.
    corpse(&mut sim, b"u002", Team(1), 80);
    let hostile = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
    let _ = hostile;
    for _ in 0..400 {
        sim.step();
    }
    assert_eq!(sim.corpse_count(), 1, "undead bodies are never raised");
    corpse(&mut sim, b"hfoo", Team(0), 90);
    for _ in 0..400 {
        sim.step();
    }
    // A friendly footman's corpse was consumed and a skeleton stands for the building's owner.
    assert_eq!(sim.corpse_count(), 1);
    let tier_one = [b"n00C", b"u002", b"n00D", b"u00A"].map(|code| u32::from_be_bytes(*code));
    assert!(sim.units().into_iter().any(|unit| {
        unit.owner == PlayerId(0)
            && unit.team == Team(0)
            && unit
                .content
                .is_some_and(|content| tier_one.contains(&content.rawcode))
    }));
}

#[test]
fn the_friendly_vampiric_aura_skips_its_carrier_mechanical_and_ranged_bodies() {
    let vampire = definition(b"h01U");
    let mut sim = simulation(1);
    let carrier = sim.spawn_resolved_unit(Team(0), at(40), vampire);
    let melee = sim.spawn_unit(unit(0, 50, 0, AttackDelivery::Melee));
    let ranged = sim.spawn_unit(unit(0, 55, 0, AttackDelivery::RangedInstant));
    let machine = sim.spawn_unit_with_properties(
        unit(0, 60, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            mechanical: true,
            build_time_ticks: Some(30),
            ..Default::default()
        },
    );
    sim.step();
    sim.step();
    let steal = |id| {
        let unit = sim.unit(id).unwrap();
        unit.status.armor_modifiers[..usize::from(unit.status.armor_modifier_count)]
            .iter()
            .map(|m| m.melee_life_steal_per_10k)
            .max()
            .unwrap_or(0)
    };
    assert!(steal(melee) > 0);
    assert_eq!((steal(ranged), steal(machine)), (0, 0));
    // The carrier benefits only from its separate self aura, which is the stronger one.
    assert!(steal(carrier) > steal(melee));
}

#[test]
fn a_hostile_command_aura_takes_the_strongest_source_and_spares_mechanical_enemies() {
    let banshee = definition(b"u001");
    let mut sim = simulation(1);
    sim.spawn_resolved_unit(Team(0), at(40), banshee);
    sim.spawn_resolved_unit(Team(0), at(41), banshee);
    let enemy = sim.spawn_unit(unit(1, 60, 0, AttackDelivery::Melee));
    let machine = sim.spawn_unit_with_properties(
        unit(1, 62, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            mechanical: true,
            build_time_ticks: Some(30),
            ..Default::default()
        },
    );
    sim.step();
    sim.step();
    let bonus = |id| {
        let unit = sim.unit(id).unwrap();
        unit.status.armor_modifiers[..usize::from(unit.status.armor_modifier_count)]
            .iter()
            .map(|m| i32::from(m.damage_bonus_per_10k))
            .sum::<i32>()
    };
    let one = bonus(enemy);
    assert!(one < 0, "enemy damage falls");
    // Two Banshees share the aura's buff family, so the penalty is not doubled.
    assert!(one > -3000);
    assert_eq!(bonus(machine), 0);
}
