//! Reproducible visibility-only profile on extracted Castle Fight geometry and sight.
//! Run with tools/cargo-interactive run -p castle-fight-sim --example profile_fog.
use std::{hint::black_box, time::Instant};

use castle_fight_sim::{
    AttackDelivery, AttackProfile, CastleFightMatchConfig, CastleFightUnitKind, MapVersion,
    MovementProfile, SimPoint, Team, UnitSpawn, create_castle_fight_match,
};

fn main() {
    for count in [1_000, 5_000, 10_000] {
        let config =
            CastleFightMatchConfig::development_subset(MapVersion::CASTLE_FIGHT_9_27, "r1", 0)
                .expect("retained release");
        let mut game = create_castle_fight_match(config, 1).expect("registered match");
        let unit = game
            .content
            .unit(CastleFightUnitKind::Footman)
            .expect("catalog vision source");
        // Vision-only sources have no authored collision body or movement.
        let properties = castle_fight_sim::UnitGameplayProperties {
            collision_radius: None,
            ..unit.gameplay_properties()
        };
        let config = game.simulation.config();
        let minimum = SimPoint::new(
            config.navigation_min.x * config.navigation_cell_size,
            config.navigation_min.y * config.navigation_cell_size,
        );
        let width =
            (config.navigation_max.x - config.navigation_min.x) * config.navigation_cell_size;
        let height =
            (config.navigation_max.y - config.navigation_min.y) * config.navigation_cell_size;
        for index in 0..count {
            // Spread synthetic stationary sources over the retained playable bounds.
            let x = minimum.x + width * (index % 100) / 100 + width / 200;
            let y = minimum.y + height * ((index / 100) % 100) / 100 + height / 200;
            game.simulation.spawn_unit_with_properties(
                UnitSpawn {
                    team: Team((index % 2) as u8),
                    position: SimPoint::new(x, y),
                    health: 100,
                    attack: AttackProfile {
                        damage: 0,
                        range: 0,
                        acquisition_range: 0,
                        cooldown_ticks: 1,
                        delivery: AttackDelivery::Melee,
                    },
                    movement: MovementProfile { speed_per_tick: 0 },
                },
                properties,
            );
        }
        black_box(game.simulation.fog_of_war());
        let checksum = game.simulation.checksum();
        let start = Instant::now();
        for _ in 0..50 {
            black_box(game.simulation.fog_of_war());
        }
        println!(
            "sources={count} readback_ms={:.3}",
            start.elapsed().as_secs_f64() * 1_000.0 / 50.0
        );
        assert_eq!(
            game.simulation.checksum(),
            checksum,
            "visibility readback must not mutate state"
        );
    }
}
