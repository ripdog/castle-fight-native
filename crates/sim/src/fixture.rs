use crate::{
    components::{AttackProfile, MovementProfile, Team, UnitSpawn},
    math::{SUBUNITS_PER_WORLD_UNIT, SimPoint},
    simulation::Simulation,
};

pub fn populate_lane_battle(simulation: &mut Simulation, total_units: usize) {
    assert!(total_units >= 2);
    assert!(
        total_units.is_multiple_of(2),
        "fixture requires an even unit count"
    );

    let per_team = total_units / 2;
    let columns = 32usize;
    let spacing = SUBUNITS_PER_WORLD_UNIT / 2;
    let front_left = 48 * SUBUNITS_PER_WORLD_UNIT;
    let front_right = 72 * SUBUNITS_PER_WORLD_UNIT;

    let attack = AttackProfile {
        damage: 5,
        range: 2 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 8 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 10,
    };
    let movement = MovementProfile {
        speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
    };

    for team in 0..2u8 {
        for index in 0..per_team {
            let column = (index % columns) as i32;
            let row = (index / columns) as i32;
            let y = (row - (per_team.div_ceil(columns) as i32 / 2)) * spacing;
            let x = if team == 0 {
                front_left - column * spacing
            } else {
                front_right + column * spacing
            };

            simulation.spawn_unit(UnitSpawn {
                team: Team(team),
                position: SimPoint::new(x, y),
                health: 10_000,
                attack,
                movement,
            });
        }
    }
}
