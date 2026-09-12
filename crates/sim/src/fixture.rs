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
    let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
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
    let columns = 50usize;
    let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
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
            let y = (row - per_team.div_ceil(columns) as i32 / 2) * spacing;
            let x = if team == 0 {
                center_x - 4 * SUBUNITS_PER_WORLD_UNIT - column * spacing
            } else {
                center_x + 4 * SUBUNITS_PER_WORLD_UNIT + column * spacing
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
        BuildingFootprint::new(55, -58, 1, 117),
        BuildingFootprint::new(112, -58, 1, 117),
        BuildingFootprint::new(56, -58, 56, 1),
        BuildingFootprint::new(56, 58, 56, 1),
    ] {
        simulation.spawn_building(BuildingSpawn {
            team: cage_team,
            footprint,
            health: 1_000_000_000,
            production: None,
            attack: None,
            spellcasting: None,
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

    let spacing = 3 * SUBUNITS_PER_WORLD_UNIT / 4;
    let outside_columns = 48usize;
    for index in 0..per_team {
        let column = (index % outside_columns) as i32;
        let row = (index / outside_columns) as i32;
        simulation.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(
                54 * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 2 - column * spacing,
                (row - per_team.div_ceil(outside_columns) as i32 / 2) * spacing,
            ),
            health: 10_000,
            attack,
            movement,
        });
    }

    let inside_columns = 64usize;
    for index in 0..per_team {
        let column = (index % inside_columns) as i32;
        let row = (index / inside_columns) as i32;
        simulation.spawn_unit(UnitSpawn {
            team: cage_team,
            position: SimPoint::new(
                56 * SUBUNITS_PER_WORLD_UNIT + SUBUNITS_PER_WORLD_UNIT / 2 + column * spacing,
                (row - per_team.div_ceil(inside_columns) as i32 / 2) * spacing,
            ),
            health: 10_000,
            attack,
            movement,
        });
    }
}
