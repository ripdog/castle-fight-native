use crate::{
    components::{
        AttackDelivery, AttackProfile, BuildingFootprint, BuildingSpawn, MovementProfile, Team,
        UnitSpawn,
    },
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
        delivery: AttackDelivery::Melee,
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

pub fn populate_crossing_crowd(simulation: &mut Simulation, total_units: usize) {
    assert!(total_units >= 2 && total_units.is_multiple_of(2));

    let per_team = total_units / 2;
    let columns = 100usize;
    let spacing = SUBUNITS_PER_WORLD_UNIT / 3;
    let center_x = 60 * SUBUNITS_PER_WORLD_UNIT;
    let attack = AttackProfile {
        delivery: AttackDelivery::Melee,
        damage: 0,
        range: SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 4 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 30,
    };
    let movement = MovementProfile {
        speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
    };

    for team in 0..2u8 {
        for index in 0..per_team {
            let column = (index % columns) as i32;
            let row = (index / columns) as i32;
            let x_offset = (column - columns as i32 / 2) * spacing;
            let y = (row - per_team.div_ceil(columns) as i32 / 2) * spacing;
            let x = if team == 0 {
                center_x - 2 * SUBUNITS_PER_WORLD_UNIT + x_offset
            } else {
                center_x + 2 * SUBUNITS_PER_WORLD_UNIT - x_offset
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

pub fn populate_dense_cage_battle(simulation: &mut Simulation, total_units: usize) {
    assert!(total_units >= 2);
    assert!(
        total_units.is_multiple_of(2),
        "fixture requires an even unit count"
    );

    let cage_team = Team(1);
    for footprint in [
        BuildingFootprint::new(59, -9, 1, 19),
        BuildingFootprint::new(68, -9, 1, 19),
        BuildingFootprint::new(60, -9, 8, 1),
        BuildingFootprint::new(60, 9, 8, 1),
    ] {
        simulation.spawn_building(BuildingSpawn {
            team: cage_team,
            footprint,
            health: 1_000_000_000,
            production: None,
        });
    }

    let attack = AttackProfile {
        delivery: AttackDelivery::Melee,
        damage: 5,
        range: 2 * SUBUNITS_PER_WORLD_UNIT,
        acquisition_range: 12 * SUBUNITS_PER_WORLD_UNIT,
        cooldown_ticks: 10,
    };
    let movement = MovementProfile {
        speed_per_tick: SUBUNITS_PER_WORLD_UNIT / 8,
    };
    let per_team = total_units / 2;

    for index in 0..per_team {
        let column = (index % 16) as i32;
        let row = (index / 16) as i32;
        simulation.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(
                (58 * SUBUNITS_PER_WORLD_UNIT) - column * (SUBUNITS_PER_WORLD_UNIT / 3),
                ((row % 48) - 24) * (SUBUNITS_PER_WORLD_UNIT / 3),
            ),
            health: 10_000,
            attack,
            movement,
        });
    }

    for index in 0..per_team {
        let cell_x = 60 + (index % 8) as i32;
        let cell_y = -8 + ((index / 8) % 17) as i32;
        let subcell = (index / (8 * 17)) as i32;
        let jitter_x = (subcell % 8) * (SUBUNITS_PER_WORLD_UNIT / 16);
        let jitter_y = ((subcell / 8) % 8) * (SUBUNITS_PER_WORLD_UNIT / 16);
        simulation.spawn_unit(UnitSpawn {
            team: cage_team,
            position: SimPoint::new(
                cell_x * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 4 + jitter_x,
                cell_y * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 4 + jitter_y,
            ),
            health: 10_000,
            attack,
            movement,
        });
    }
}
