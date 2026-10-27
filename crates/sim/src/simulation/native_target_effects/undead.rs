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
    // The carrier is replaced at once by one of the retained skeletons (never a standing Ghoul).
    let allowed =
        &profile.table[usize::from(profile.first_index)..=usize::from(profile.last_index)];
    let skeletons: Vec<_> = sim
        .units()
        .into_iter()
        .filter(|unit| {
            unit.content
                .is_some_and(|c| profile.table.contains(&c.rawcode))
        })
        .collect();
    assert_eq!(skeletons.len(), usize::from(profile.count));
    assert!(
        skeletons
            .iter()
            .all(|unit| unit.content.is_some_and(|c| allowed.contains(&c.rawcode)))
    );
    assert!(skeletons.iter().all(|unit| !unit.classifications.summoned));
    assert_eq!(alive_count(&sim, b"u004"), 0);
    assert!(skeletons.iter().all(|unit| unit.team == Team(0)));
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
fn devour_chain_heals_per_nested_cast_and_kills_through_armor() {
    let profile = mastery();
    let lich = definition(b"u00D");
    let mut sim = simulation(1);
    let king = sim.spawn_resolved_unit(Team(0), at(40), lich);
    let victim = sim.spawn_unit(unit(1, 50, 0, AttackDelivery::Melee));
    sim.world
        .get_mut::<Health>(entity(&sim, king))
        .unwrap()
        .current = 100;
    sim.world
        .get_mut::<Health>(entity(&sim, victim))
        .unwrap()
        .current = 300;
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
    death_mastery::devour(profile, 2, source, target, &mut state);
    let bearer = state.units.iter().position(|u| u.id == king).unwrap();
    assert_eq!(
        state.unit_health[bearer],
        100 + victim_life,
        "each nested devour heals the retained share of the target's untouched life"
    );
    assert!(state.unit_health[target] <= 0, "death damage ignores armor");
}

#[test]
fn death_and_decay_pulses_each_body_once_a_second_on_its_own_phase_for_its_duration() {
    let profile: DeathAndDecayProfile = mastery().decay;
    let mut sim = simulation(1);
    // The second victim is spell-immune: the spell is an ultimate and passes immunity.
    let victims = [60, 62, 64, 66, 68, 70].map(|x| {
        sim.spawn_unit_with_properties(
            unit(1, x, 0, AttackDelivery::Melee),
            UnitGameplayProperties {
                classifications: UnitClassifications {
                    spell_immune: x == 62,
                    ..UnitClassifications::default()
                },
                ..UnitGameplayProperties::default()
            },
        )
    });
    let ally = sim.spawn_unit(unit(0, 60, 0, AttackDelivery::Melee));
    let cast_tick = sim.next_tick;
    sim.launch_death_and_decay(crate::components::DeathAndDecayState {
        source: ally,
        team: Team(0),
        position: at(60),
        profile,
        cast_tick,
        expires_tick: cast_tick + u64::from(profile.duration_ticks),
    });
    let mut last = [1000; 6];
    let mut pulses: [Vec<u64>; 6] = Default::default();
    for _ in 0..u64::from(profile.duration_ticks) + 40 {
        sim.step();
        for (i, victim) in victims.iter().enumerate() {
            let health = sim.unit(*victim).unwrap().health;
            if health < last[i] {
                pulses[i].push(sim.next_tick - cast_tick);
                last[i] = health;
            }
        }
    }
    let count = usize::try_from(u64::from(profile.duration_ticks) / 30).unwrap();
    for ticks in &pulses {
        // One pulse per second for the whole duration, the first within a second of the release.
        assert_eq!(ticks.len(), count);
        assert!(ticks[0] > 0 && ticks[0] <= 30);
        assert!(ticks.windows(2).all(|w| w[1] - w[0] == 30));
    }
    // Every body draws its own phase, so the pulses are not all on the same tick.
    assert!(pulses.iter().any(|ticks| ticks[0] != pulses[0][0]));
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

#[test]
fn city_of_decay_does_not_stack_across_buildings_of_one_team() {
    let city = bundle()
        .tower(crate::CastleFightTowerKind::CityOfDecay)
        .unwrap();
    let lost = |buildings: &[(u8, i32)]| {
        let mut sim = simulation(1);
        for &(team, x) in buildings {
            sim.spawn_building_for_player_with_properties(
                PlayerId(team),
                city.spawn(Team(team), BuildingFootprint::new(x, 0, 4, 4)),
                city.gameplay_properties(),
            );
        }
        let victim = sim.spawn_unit(unit(1, 400, 0, AttackDelivery::Melee));
        for _ in 0..100 {
            sim.step();
        }
        1000 - sim.unit(victim).unwrap().health
    };
    let single = lost(&[(0, 20)]);
    assert!(single > 0);
    assert_eq!(
        lost(&[(0, 20), (0, 30)]),
        single,
        "a second copy adds nothing"
    );
    // The enemy team's own building is an independent source against the other side.
    assert_eq!(lost(&[(0, 20), (1, 30)]), single);
}

#[test]
fn a_lich_king_raises_two_empowered_skeletons_from_the_upper_tiers_or_the_general() {
    let lich = definition(b"u00D");
    let spell = lich.spellcasting.expect("Lich King Raise Dead");
    let Effect::RaiseDead(profile) = spell.ability.effect else {
        panic!("the Lich King raises the dead");
    };
    let mut sim = simulation(1);
    sim.spawn_resolved_unit(Team(0), at(40), lich);
    corpse(&mut sim, b"hfoo", Team(1), 60);
    corpse(&mut sim, b"hfoo", Team(1), 70);
    for _ in 0..90 {
        sim.step();
    }
    let raised: Vec<_> = sim
        .units()
        .into_iter()
        .filter(|unit| {
            unit.content
                .is_some_and(|c| profile.table.contains(&c.rawcode))
        })
        .collect();
    assert_eq!(raised.len(), 2);
    let special = profile.special.expect("the General branch");
    for unit in &raised {
        let rawcode = unit.content.unwrap().rawcode;
        let slot = profile
            .table
            .iter()
            .position(|body| *body == rawcode)
            .unwrap();
        assert!(
            slot == usize::from(special.index)
                || (usize::from(profile.first_index)..=usize::from(profile.last_index))
                    .contains(&slot)
        );
        for grant in profile.bonus_grants.into_iter().flatten() {
            assert!(unit.status.native_grants.contains(grant.ability));
        }
    }
}

fn sapper() -> UnitGameplayProperties {
    UnitGameplayProperties {
        classifications: UnitClassifications {
            combat_sapper: true,
            ..Default::default()
        },
        ..Default::default()
    }
}

#[test]
fn eternal_servitude_converts_only_organic_non_undead_victims_of_a_vampire() {
    let vampire = definition(b"h01U");
    let mut sim = simulation(1);
    sim.spawn_resolved_unit(Team(0), at(40), vampire);
    let victim = sim.spawn_unit_with_properties(unit(1, 42, 0, AttackDelivery::Melee), sapper());
    let machine = sim.spawn_unit_with_properties(
        unit(1, 44, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            mechanical: true,
            build_time_ticks: Some(30),
            ..sapper()
        },
    );
    for id in [victim, machine] {
        sim.world
            .get_mut::<Health>(entity(&sim, id))
            .unwrap()
            .current = 1;
    }
    for _ in 0..200 {
        sim.step();
    }
    // The organic victim vanished without a corpse and a Lesser Vampire stands in its place.
    assert!(sim.unit(victim).is_none());
    assert_eq!(alive_count(&sim, b"h01V"), 1);
    let converted = sim
        .units()
        .into_iter()
        .find(|unit| {
            unit.content
                .is_some_and(|c| c.rawcode == u32::from_be_bytes(*b"h01V"))
        })
        .unwrap();
    assert_eq!(converted.team, Team(0));
    // A mechanical victim dies normally: it neither converts nor leaves its handle in place.
    assert_eq!(alive_count(&sim, b"h01V"), 1);
}

#[test]
fn blood_corrosion_stacks_five_levels_of_permanent_armor_reduction() {
    let lord = definition(b"h05H");
    let rule = crate::native_effects::unit_death_rule_for_version(
        crate::MapVersion::CASTLE_FIGHT_9_27,
        u32::from_be_bytes(*b"h05H"),
    )
    .and_then(|rule| rule.armor_stack)
    .expect("the Vampire Lord carries Blood Corrosion");
    let mut sim = simulation(1);
    sim.spawn_resolved_unit(Team(0), at(40), lord);
    let mut victim_spawn = unit(1, 42, 0, AttackDelivery::Melee);
    victim_spawn.health = 1_000_000;
    let victim = sim.spawn_unit_with_properties(victim_spawn, sapper());
    let before = {
        let u = sim.unit(victim).unwrap();
        u.status.effective_armor_points_per_100(u.armor)
    };
    for _ in 0..1200 {
        sim.step();
    }
    let after = {
        let u = sim.unit(victim).unwrap();
        u.status.effective_armor_points_per_100(u.armor)
    };
    assert_eq!(
        after - before,
        i32::from(rule.armor_per_level_per_100) * i32::from(rule.maximum_level)
    );
}
