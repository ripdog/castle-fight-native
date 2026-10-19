use super::*;
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
