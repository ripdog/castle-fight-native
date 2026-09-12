mod components;
mod fixture;
mod math;
mod simulation;
mod spatial;

pub use components::{AttackProfile, MovementProfile, SimId, Team, UnitSpawn};
pub use fixture::populate_lane_battle;
pub use math::{SUBUNITS_PER_WORLD_UNIT, SimPoint};
pub use simulation::{Simulation, SimulationConfig, TickResult, UnitView};

#[cfg(test)]
mod tests {
    use super::*;

    fn duel_unit(team: u8, x: i32, damage: i32, health: i32) -> UnitSpawn {
        UnitSpawn {
            team: Team(team),
            position: SimPoint::new(x, 0),
            health,
            attack: AttackProfile {
                damage,
                range: 4 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 10,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        }
    }

    #[test]
    fn units_do_not_attack_on_spawn_tick() {
        let mut sim = Simulation::new(SimulationConfig::default(), 1);
        let a = sim.spawn_unit(duel_unit(0, 0, 10, 100));
        let b = sim.spawn_unit(duel_unit(1, SUBUNITS_PER_WORLD_UNIT, 10, 100));

        let first = sim.step();
        assert_eq!(first.attacks_resolved, 0);
        assert_eq!(sim.unit(a).unwrap().health, 100);
        assert_eq!(sim.unit(b).unwrap().health, 100);

        let second = sim.step();
        assert_eq!(second.attacks_resolved, 2);
        assert_eq!(sim.unit(a).unwrap().health, 90);
        assert_eq!(sim.unit(b).unwrap().health, 90);
    }

    #[test]
    fn death_cancels_later_attack_in_canonical_order() {
        let mut sim = Simulation::new(SimulationConfig::default(), 2);
        let first = sim.spawn_unit(duel_unit(0, 0, 100, 100));
        let second = sim.spawn_unit(duel_unit(1, SUBUNITS_PER_WORLD_UNIT, 100, 100));

        sim.step();
        let result = sim.step();

        assert_eq!(result.attacks_resolved, 1);
        assert_eq!(result.deaths, 1);
        assert_eq!(sim.unit(first).unwrap().health, 100);
        assert!(sim.unit(second).is_none());
    }

    #[test]
    fn exact_target_tie_uses_lowest_sim_id() {
        let mut sim = Simulation::new(SimulationConfig::default(), 3);
        let source = sim.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(0, 0),
            health: 100,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let lower = sim.spawn_unit(duel_unit(1, -SUBUNITS_PER_WORLD_UNIT, 0, 100));
        let _higher = sim.spawn_unit(duel_unit(1, SUBUNITS_PER_WORLD_UNIT, 0, 100));

        sim.step();
        assert_eq!(sim.unit(source).unwrap().target, Some(lower));
    }

    #[test]
    fn worker_count_does_not_change_battle_checksum() {
        let mut expected = None;
        for workers in [1, 2, 4] {
            let mut sim = Simulation::new(SimulationConfig::default(), workers);
            populate_lane_battle(&mut sim, 512);
            for _ in 0..200 {
                sim.step();
            }

            match expected {
                Some(checksum) => assert_eq!(sim.checksum(), checksum, "workers={workers}"),
                None => expected = Some(sim.checksum()),
            }
        }
    }
}
