//! The Ancient Keeper's Treant Sprout (`keeperSpell`): clear the ground, raise a ring of wall trees,
//! and turn the surviving trees into short-lived Treants after the transform delay.
use super::master_projectiles::{DIRECTION_SCALE, offset, rotate};
use super::*;
use crate::components::{MAX_SPROUT_TREES, NativeAction, TreantSproutProfile, TreantSproutState};

const RANDOM_PURPOSE_SPROUT_DISPLACEMENT: u64 = 0x5350_524f_5554_0001;
const ROLL_SCALE: u64 = 1_000_000;

/// The map's `Cos`/`Sin` bearing as a unit direction.
fn bearing(millidegrees: i32) -> SimPoint {
    rotate(SimPoint::new(DIRECTION_SCALE as i32, 0), millidegrees)
}

impl Simulation {
    pub(super) fn cast_treant_sprout(
        &mut self,
        source: AbilitySourceSnapshot,
        p: TreantSproutProfile,
        units: &mut [UnitSnapshot],
    ) -> usize {
        let caster = find_unit_index(units, source.id).expect("Treant Sprout caster");
        let center = units[caster].position;
        let owner = units[caster].owner;
        let team = units[caster].team;
        // The displacement only runs while the caster stands on the battlefield rectangle.
        let bounds =
            crate::building_mechanics::snowveil_for_version(p.map_version).battlefield_bounds;
        if center.x >= bounds[0]
            && center.x <= bounds[2]
            && center.y >= bounds[1]
            && center.y <= bounds[3]
        {
            let limit = square_i32(p.clearance_radius);
            for index in 0..units.len() {
                let unit = units[index];
                if index == caster
                    || unit.health <= 0
                    || !unit.classifications.combat_sapper
                    || unit.movement_class != MovementClass::Ground
                    || unit.classifications.invulnerable
                    || center.distance_sq(unit.position) > limit
                {
                    continue;
                }
                let roll = |salt: u64| {
                    deterministic_random(
                        self.config.match_seed,
                        self.next_tick,
                        source.id,
                        RANDOM_PURPOSE_SPROUT_DISPLACEMENT,
                        unit.id.0.wrapping_mul(2).wrapping_add(salt),
                    ) % ROLL_SCALE
                };
                let span = i64::from(p.displacement_maximum - p.displacement_minimum);
                let distance = p.displacement_minimum
                    + i32::try_from(span * roll(0) as i64 / ROLL_SCALE as i64)
                        .expect("sprout displacement fits");
                let window = i64::from(p.bearing_maximum_degrees - p.bearing_minimum_degrees);
                // Each side is thrown toward its own base: half a turn for the second team.
                let millidegrees = i64::from(p.bearing_minimum_degrees) * 1000
                    + window * 1000 * roll(1) as i64 / ROLL_SCALE as i64
                    + 180_000 * i64::from(unit.team.0);
                units[index].position = offset(
                    center,
                    bearing(i32::try_from(millidegrees).expect("sprout bearing fits")),
                    distance,
                );
            }
        }
        let cell = self.config.navigation_cell_size;
        let half = SimPoint::new(cell / 2, cell / 2);
        let side = i32::from(p.tree_cells);
        let mut footprints = [BuildingFootprint::new(0, 0, 0, 0); MAX_SPROUT_TREES];
        for (slot, angle) in footprints
            .iter_mut()
            .zip(&p.tree_angles_degrees[..usize::from(p.tree_count)])
        {
            let tree = offset(center, bearing(i32::from(*angle) * 1000), p.tree_radius);
            // An even-sized footprint is centred on the nearest cell corner.
            let corner = self
                .topology
                .cell_of_point(SimPoint::new(tree.x + half.x, tree.y + half.y));
            *slot = BuildingFootprint::new(
                corner.x - side / 2,
                corner.y - side / 2,
                p.tree_cells,
                p.tree_cells,
            );
        }
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::TreantSprout(TreantSproutState {
                profile: p,
                source: source.id,
                owner,
                team,
                center,
                due_tick: self.next_tick + u64::from(p.transform_delay_ticks),
                footprints,
            }),
        ));
        self.topology_dirty = true;
        1
    }

    pub(super) fn advance_treant_sprout(
        &mut self,
        entity: Entity,
        state: TreantSproutState,
        units: &mut [UnitSnapshot],
    ) {
        if self.next_tick < state.due_tick {
            return;
        }
        // Killing a tree releases its pathing whether or not a Treant follows.
        self.world.despawn(entity);
        self.topology_dirty = true;
        let p = state.profile;
        let alive =
            find_unit_index(units, state.source).is_some_and(|index| units[index].health > 0);
        if !alive {
            return;
        }
        let kind =
            crate::CastleFightUnitKind::from_rawcode_for_version(p.treant_rawcode, p.map_version)
                .unwrap()
                .expect("promoted Treant Sprout body");
        let definition = kind
            .definition_for_version(p.map_version)
            .unwrap()
            .resolved();
        for angle in &p.tree_angles_degrees[..usize::from(p.tree_count)] {
            let position = offset(
                state.center,
                bearing(i32::from(*angle) * 1000),
                p.treant_radius,
            );
            // `createUnit` plus a timed life: an ordinary combat body, not a summoned one.
            let child = self.spawn_resolved_unit_unchecked(
                Some(state.owner),
                UnitSpawn::from_template(state.team, position, definition.template),
                definition,
            );
            let id = self.allocate_id();
            self.world.spawn((
                id,
                NativeAction::TimedLife {
                    child,
                    expires_tick: self.next_tick + u64::from(p.treant_life_ticks),
                },
            ));
        }
        // `orderCodeAttack(caster)` after the transformation.
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::ResumeAttack {
                source: state.source,
                due_tick: self.next_tick,
            },
        ));
    }
}
