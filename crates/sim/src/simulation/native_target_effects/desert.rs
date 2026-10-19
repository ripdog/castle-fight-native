use super::*;
use crate::MapVersion;
use crate::components::{ParasiteChild, PendingParasiteScript};

fn fan_caster(sim: &mut Simulation) -> SimId {
    sim.spawn_unit_with_properties_and_spellcasting(
        unit(0, 20, 0, AttackDelivery::Melee),
        UnitGameplayProperties::default(),
        SpellcastingProfile {
            mana: ManaProfile::per_second(150, 75, 0),
            ability: AutomaticAbilityProfile {
                id: AbilityId(940),
                mana_cost: 40,
                cooldown_ticks: 30,
                range: 0,
                target_policy: AbilityTargetPolicy::AttackDamageTrigger,
                effect: AbilityEffect::FanOfKnives {
                    ability: AbilityId(940),
                    damage: 45,
                    maximum_total_damage: 300,
                    maximum_targets: 0,
                    radius: 50 * SUBUNITS_PER_WORLD_UNIT,
                    targets: AttackTargetMask::AIR_AND_GROUND,
                    organic_only: true,
                },
            },
        },
    )
}

#[test]
fn fan_of_knives_divides_its_total_cap_skips_mechanical_and_distant_units_and_spends_mana() {
    let mut sim = simulation(1);
    let caster = fan_caster(&mut sim);
    // One enemy actually strikes the caster; the rest only stand in the area.
    let striker = sim.spawn_unit(unit(1, 21, 5, AttackDelivery::Melee));
    let mut bystanders = vec![striker];
    for x in 22..30 {
        bystanders.push(sim.spawn_unit(unit(1, x, 0, AttackDelivery::Melee)));
    }
    let mechanical = sim.spawn_unit_with_properties(
        unit(1, 30, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            mechanical: true,
            build_time_ticks: Some(1),
            ..Default::default()
        },
    );
    let distant = sim.spawn_unit(unit(1, 200, 0, AttackDelivery::Melee));
    for _ in 0..6 {
        sim.step();
    }
    let losses: Vec<_> = bystanders
        .iter()
        .map(|id| 1000 - sim.unit(*id).unwrap().health)
        .collect();
    // The striker also took no retaliation (the caster's weapon is empty), so every loss is the fan.
    assert!(losses.iter().all(|loss| *loss == losses[0]), "{losses:?}");
    assert!(
        losses[0] > 0 && losses.iter().sum::<i32>() <= 300,
        "{losses:?}"
    );
    assert_eq!(sim.unit(mechanical).unwrap().health, 1000);
    assert_eq!(sim.unit(distant).unwrap().health, 1000);
    assert_eq!(sim.unit(caster).unwrap().mana_current, Some(35));
}

#[test]
fn parasite_life_steal_heals_the_requested_amount_even_when_armor_reduces_the_damage() {
    let mut sim = simulation(1);
    let caster = sim.spawn_unit(unit(0, 20, 0, AttackDelivery::Melee));
    let target = sim.spawn_unit_with_properties(
        unit(1, 21, 0, AttackDelivery::Melee),
        UnitGameplayProperties {
            armor: ArmorProfile::new(ArmorType::Large, 10),
            ..Default::default()
        },
    );
    sim.world
        .get_mut::<Health>(entity(&sim, caster))
        .unwrap()
        .current = 500;
    sim.world
        .get_mut::<Health>(entity(&sim, target))
        .unwrap()
        .current = 60;
    let mut units = sim.snapshot_units();
    let c = find_unit_index(&units, caster).unwrap();
    units[c].status.pending_parasite_script = Some(PendingParasiteScript {
        ability: AbilityId(941),
        child: ParasiteChild::LifeSteal {
            maximum_life: 75,
            minimum_delay_ticks: 3,
            maximum_delay_ticks: 18,
        },
        target,
        due_tick: sim.next_tick,
    });
    assert_eq!(sim.resolve_pending_parasite_scripts(&mut units), 1);
    let t = find_unit_index(&units, target).unwrap();
    // The steal is clamped to the target's life (60), but armor reduces what it actually loses.
    assert!(units[t].health > 0 && units[t].health < 60);
    assert_eq!(units[c].health, 560);
    assert!(units[c].status.pending_parasite_script.is_none());
}

#[test]
fn an_orb_childs_own_cooldown_gates_later_procs_and_starts_when_one_fires() {
    let mut sim = simulation(1);
    let mut spawn = unit(0, 20, 1, AttackDelivery::Melee);
    spawn.attack.cooldown_ticks = 10;
    let attacker = sim.spawn_unit_with_properties(
        spawn,
        UnitGameplayProperties {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::TriggeredSpellProc(
                crate::TriggeredSpellProcProfile {
                    cooldown_ticks: 100,
                    hero_chance_per_10k: None,
                    summoned_chance_per_10k: None,
                    ability: AbilityId(960),
                    chance_per_10k: 10_000,
                    targets: AttackTargetMask::AIR_AND_GROUND,
                    effect: TriggeredAttackEffect::NativeDebuff {
                        parent: AbilityId(960),
                        profile: crate::NativeDebuffProfile {
                            ability: AbilityId(961),
                            buff: 962,
                            targets: AttackTargetMask::AIR_AND_GROUND,
                            organic_only: false,
                            duration_ticks: 30,
                            hero_duration_ticks: 30,
                            movement_percent_delta: -10,
                            attack_speed_percent_delta: 0,
                            damage_bonus_per_10k: 0,
                            ethereal: None,
                            miss_chance_per_10k: 0,
                        },
                    },
                },
            )),
            ..Default::default()
        },
    );
    sim.spawn_unit(unit(1, 21, 0, AttackDelivery::Melee));
    let mut procs = Vec::new();
    for _ in 0..130 {
        sim.step();
        procs.push(
            sim.ability_casts_last_tick()
                .iter()
                .filter(|cast| cast.source == attacker && cast.ability == AbilityId(960))
                .count(),
        );
    }
    let first = procs
        .iter()
        .position(|count| *count > 0)
        .expect("a proc fires");
    // Attacks land every ten ticks, yet the child cannot fire again until its cooldown elapses.
    assert_eq!(procs.iter().sum::<usize>(), 2);
    let second = procs.iter().rposition(|count| *count > 0).unwrap();
    assert!((100..110).contains(&(second - first)), "{first} {second}");
}

#[test]
fn locusts_are_real_flying_bodies_that_fly_out_strike_on_contact_and_return() {
    let version = MapVersion::CASTLE_FIGHT_9_27;
    let AbilityEffect::ParasiteScript(script) = crate::CastleFightUnitKind::HarpiesMatriarch
        .definition_for_version(version)
        .unwrap()
        .spellcasting
        .unwrap()
        .ability
        .effect
    else {
        panic!("Matriarch swarm");
    };
    let crate::ParasiteChild::LocustSwarm(profile) = script.child else {
        panic!("Matriarch swarm child");
    };
    let mut sim = simulation(1);
    let caster = sim.spawn_unit(unit(0, 20, 0, AttackDelivery::Melee));
    let victim = sim.spawn_unit(unit(1, 400, 0, AttackDelivery::Melee));
    let center = SimPoint::new(20 * SUBUNITS_PER_WORLD_UNIT, 20 * SUBUNITS_PER_WORLD_UNIT);
    let source = AbilitySourceSnapshot {
        map_version: Some(version),
        source: AbilitySourceIndex::Unit(0),
        id: caster,
        team: Team(0),
        origin: AbilitySourceOrigin::Unit(center),
        health: 1000,
        stunned_until_tick: 0,
        spellcasting: None,
        mana_current: None,
        ability_state: None,
    };
    sim.launch_locust_swarm(source, PlayerId(0), center, profile);
    let bodies = |sim: &mut Simulation| {
        sim.snapshot_units()
            .into_iter()
            .filter(|unit| unit.rawcode == profile.body_rawcode)
            .collect::<Vec<_>>()
    };
    for _ in 0..8 {
        sim.step();
    }
    let early = bodies(&mut sim);
    assert!(
        !early.is_empty() && early.len() < usize::from(profile.count),
        "released over time"
    );
    assert_eq!(
        sim.unit(victim).unwrap().health,
        1000,
        "nothing has touched the victim yet"
    );
    let mut struck_at = None;
    let mut farthest = 0u64;
    for tick in 0..400 {
        sim.step();
        for body in bodies(&mut sim) {
            farthest = farthest.max(body.position.distance_sq(center));
        }
        if struck_at.is_none() && sim.unit(victim).unwrap().health < 1000 {
            struck_at = Some(tick);
        }
    }
    assert_eq!(bodies(&mut sim).len(), usize::from(profile.count));
    let struck_at = struck_at.expect("locusts reach and strike the victim");
    // The victim is about 380 world units away; a body flying at its retained speed needs time.
    assert!(struck_at > 10, "struck after {struck_at} ticks");
    assert!(
        farthest > square_i32(200 * SUBUNITS_PER_WORLD_UNIT),
        "bodies actually left the center"
    );
}
