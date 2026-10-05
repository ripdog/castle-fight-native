//! Deterministic developer actions, applied through the canonical match stream.
use crate::{
    BuildingFootprint, CastleFightContentBundle, CastleFightProductionKind, CastleFightTowerKind,
    PlayerCommand, PlayerId, Simulation, Team,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugCommand {
    GrantResources,
    KillAllUnits,
    SetBuildingsInvulnerable {
        enabled: bool,
    },
    PopulateBuildings,
    PlayerCommand {
        player: PlayerId,
        command: PlayerCommand,
    },
}

impl DebugCommand {
    pub(crate) fn apply(self, simulation: &mut Simulation, content: &CastleFightContentBundle) {
        match self {
            Self::GrantResources => {
                for player in simulation.players() {
                    simulation.debug_grant_player_resources_for(player.id, 1_000_000, 1_000_000);
                }
            }
            Self::KillAllUnits => {
                simulation.debug_damage_all_units(9_999);
            }
            Self::SetBuildingsInvulnerable { enabled } => {
                simulation.debug_set_buildings_invulnerable(enabled)
            }
            Self::PopulateBuildings => {
                populate_debug_building_lines(simulation, content);
            }
            Self::PlayerCommand { player, command } => {
                // Use the actor owner's ordinary admission and execution rules. This override
                // grants the host control; it does not bypass placement/cost/actor validation.
                if crate::admit_player_command(simulation, content, player, command).is_ok() {
                    crate::commands::execute_player_command(simulation, content, player, command);
                }
            }
        }
    }
}

const DEBUG_BUILDING_LINE_MARGIN_CELLS: i32 = 4;
const DEBUG_BUILDING_LINE_GAP_CELLS: i32 = 2;

#[derive(Debug, Clone, Copy)]
enum DebugBuildingKind {
    Production(CastleFightProductionKind),
    Tower(CastleFightTowerKind),
}

impl DebugBuildingKind {
    fn footprint_size(self, content: &CastleFightContentBundle) -> u16 {
        match self {
            Self::Production(kind) => {
                content
                    .production_building(kind)
                    .expect("debug production kind must belong to selected content")
                    .footprint_size_cells
            }
            Self::Tower(kind) => {
                content
                    .tower(kind)
                    .expect("debug tower kind must belong to selected content")
                    .footprint_size_cells
            }
        }
    }
}

pub fn populate_debug_building_lines(
    simulation: &mut Simulation,
    content: &CastleFightContentBundle,
) -> (usize, usize) {
    let definitions = CastleFightProductionKind::ALL
        .into_iter()
        .filter(|kind| content.production_building(*kind).is_some())
        .map(DebugBuildingKind::Production)
        .chain(
            CastleFightTowerKind::ALL
                .into_iter()
                .filter(|kind| content.tower(*kind).is_some())
                .map(DebugBuildingKind::Tower),
        )
        .collect::<Vec<_>>();
    if definitions.is_empty() {
        return (0, 0);
    }

    let sizes = definitions
        .iter()
        .map(|definition| definition.footprint_size(content))
        .collect::<Vec<_>>();
    let mut spawned = 0;
    let mut skipped = 0;

    for team in [Team(0), Team(1)] {
        let Some(castle) = simulation
            .team_objective(team)
            .and_then(|objective| simulation.building(objective))
        else {
            skipped += definitions.len();
            continue;
        };
        let owner = castle.owner.or_else(|| {
            simulation
                .players()
                .into_iter()
                .filter(|player| player.team == team)
                .map(|player| player.id)
                .min()
        });
        let Some(owner) = owner else {
            skipped += definitions.len();
            continue;
        };

        let region = simulation
            .team_build_regions(team)
            .iter()
            .copied()
            .find(|region| footprint_contains(*region, castle.footprint))
            .or_else(|| {
                simulation
                    .team_build_regions(team)
                    .iter()
                    .copied()
                    .max_by_key(|region| u32::from(region.width) * u32::from(region.height))
            });
        let Some(region) = region else {
            skipped += definitions.len();
            continue;
        };

        let Some(footprints) = plan_debug_building_footprints(region, castle.footprint, &sizes)
        else {
            skipped += definitions.len();
            continue;
        };

        for (&definition, footprint) in definitions.iter().zip(footprints) {
            match definition {
                DebugBuildingKind::Production(kind) => {
                    let definition = content
                        .production_building(kind)
                        .expect("debug production kind must belong to selected content");
                    simulation.debug_spawn_building_for_player_with_properties(
                        owner,
                        definition.spawn(team, footprint),
                        definition.gameplay_properties(),
                    );
                }
                DebugBuildingKind::Tower(kind) => {
                    let definition = content
                        .tower(kind)
                        .expect("debug tower kind must belong to selected content");
                    simulation.debug_spawn_building_for_player_with_properties(
                        owner,
                        definition.spawn(team, footprint),
                        definition.gameplay_properties(),
                    );
                }
            }
            spawned += 1;
        }
    }

    (spawned, skipped)
}

/// Keep the complete catalog behind the castle, adding columns rather than
/// silently abandoning it once one vertical line fills. Prefer gaps, then use a
/// dense grid; reject insufficient space before spawning any partial roster.
fn plan_debug_building_footprints(
    region: BuildingFootprint,
    castle: BuildingFootprint,
    sizes: &[u16],
) -> Option<Vec<BuildingFootprint>> {
    let Some(&maximum) = sizes.iter().max() else {
        return Some(Vec::new());
    };
    if sizes.contains(&0) {
        return None;
    }
    let maximum = i32::from(maximum);
    let left_side = castle.min_x * 2 + i32::from(castle.width) - 1
        < region.min_x * 2 + i32::from(region.width) - 1;
    let mut min_x = region.min_x + DEBUG_BUILDING_LINE_MARGIN_CELLS;
    let mut max_x = region.max_x() - DEBUG_BUILDING_LINE_MARGIN_CELLS;
    if left_side {
        max_x = max_x.min(castle.min_x - 1);
    } else {
        min_x = min_x.max(castle.max_x() + 1);
    }
    let min_y = region.min_y + DEBUG_BUILDING_LINE_MARGIN_CELLS;
    let max_y = region.max_y() - DEBUG_BUILDING_LINE_MARGIN_CELLS;
    let width = max_x - min_x + 1;
    let height = max_y - min_y + 1;
    if width < maximum || height < maximum {
        return None;
    }
    for gap in [DEBUG_BUILDING_LINE_GAP_CELLS, 0] {
        let pitch = maximum + gap;
        let rows = usize::try_from((height + gap) / pitch).ok()?;
        let columns = usize::try_from((width + gap) / pitch).ok()?;
        if sizes.len().div_ceil(rows) > columns {
            continue;
        }
        return sizes
            .iter()
            .enumerate()
            .map(|(index, &size)| {
                let column = i32::try_from(index / rows).ok()?;
                let row = i32::try_from(index % rows).ok()?;
                let slot_min_x = if left_side {
                    min_x + column * pitch
                } else {
                    max_x - maximum + 1 - column * pitch
                };
                Some(BuildingFootprint::new(
                    slot_min_x + (maximum - i32::from(size)) / 2,
                    max_y - row * pitch - i32::from(size) + 1,
                    size,
                    size,
                ))
            })
            .collect();
    }
    None
}

const fn footprint_contains(region: BuildingFootprint, footprint: BuildingFootprint) -> bool {
    footprint.min_x >= region.min_x
        && footprint.max_x() <= region.max_x()
        && footprint.min_y >= region.min_y
        && footprint.max_y() <= region.max_y()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn footprints_overlap(a: BuildingFootprint, b: BuildingFootprint) -> bool {
        a.min_x <= b.max_x() && b.min_x <= a.max_x() && a.min_y <= b.max_y() && b.min_y <= a.max_y()
    }

    #[test]
    fn debug_layout_adds_columns_deterministically_without_crossing_castle_or_region() {
        let region = BuildingFootprint::new(0, 0, 64, 64);
        let sizes = [3, 5, 6, 4, 6, 3, 4, 5, 6, 3, 5, 4];
        for (castle_x, left_side) in [(20, true), (40, false)] {
            let castle = BuildingFootprint::new(castle_x, 30, 4, 4);
            let plan = plan_debug_building_footprints(region, castle, &sizes).unwrap();
            assert_eq!(
                plan,
                plan_debug_building_footprints(region, castle, &sizes).unwrap()
            );
            assert_eq!(plan.len(), sizes.len());
            assert_ne!(plan.first().unwrap().min_x, plan.last().unwrap().min_x);
            for (index, (&size, footprint)) in sizes.iter().zip(&plan).enumerate() {
                assert_eq!(footprint.width, size);
                assert_eq!(footprint.height, size);
                assert!(footprint_contains(region, *footprint));
                assert!(if left_side {
                    footprint.max_x() < castle.min_x
                } else {
                    footprint.min_x > castle.max_x()
                });
                assert!(
                    plan[index + 1..]
                        .iter()
                        .all(|other| !footprints_overlap(*footprint, *other))
                );
            }
        }
    }

    #[test]
    fn debug_layout_uses_dense_fallback_and_rejects_an_unplaceable_complete_catalog() {
        let region = BuildingFootprint::new(0, 0, 40, 24);
        let castle = BuildingFootprint::new(16, 10, 4, 4);
        // Behind the castle: 12 x 16 cells. Gapped 6-cell slots hold 2,
        // dense slots hold 4; do not skip all definitions just to preserve gaps.
        let plan = plan_debug_building_footprints(region, castle, &[6; 4]).unwrap();
        assert_eq!(plan.len(), 4);
        assert!(plan_debug_building_footprints(region, castle, &[6; 5]).is_none());
        assert!(plan_debug_building_footprints(region, castle, &[0]).is_none());
        assert_eq!(
            plan_debug_building_footprints(region, castle, &[]),
            Some(Vec::new())
        );
    }
}
