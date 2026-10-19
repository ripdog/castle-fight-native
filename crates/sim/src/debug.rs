//! Deterministic developer actions, applied through the canonical match stream.
use crate::{
    BuildingFootprint, CastleFightBuilderRace, CastleFightBuildingKind, CastleFightContentBundle,
    CastleFightMatchConfig, CastleFightMatchSetupError, CastleFightParticipantConfig,
    CastleFightProductionKind, CastleFightTowerKind, DebugStartingBuildings, MapVersion,
    PlayerCommand, PlayerId, Simulation, Team,
};

/// Fixed seed so the benchmark match is reproducible across runs.
pub const BENCHMARK_MATCH_SEED: u64 = 1;
/// Fixed seed for de-synchronizing spellcaster mana in the benchmark match.
pub const BENCHMARK_MANA_SEED: u64 = 0x5eed_0000_c0de_0001;
/// Largest team the benchmark match supports; Castle Fight maps have three slots per team.
const BENCHMARK_MAX_PLAYERS_PER_TEAM: usize = 3;

/// Developer-tool choices applied to a benchmark match after it is created.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BenchmarkSetupOptions {
    /// Suppresses building health loss so castles and production survive a sustained run.
    pub invulnerable_buildings: bool,
    /// Randomizes spellcaster mana so spell buildings do not cast in lockstep.
    pub randomize_mana: bool,
}

impl Default for BenchmarkSetupOptions {
    fn default() -> Self {
        Self {
            invulnerable_buildings: true,
            randomize_mana: true,
        }
    }
}

/// Match configuration for the all-buildings benchmark: team 0 takes ids `0..N` and team 1
/// takes ids `6..6+N`, every builder is Human, and every implemented building starts complete.
pub fn castle_fight_benchmark_match_config(
    version: MapVersion,
    players_per_team: usize,
    seed: u64,
) -> Result<CastleFightMatchConfig, CastleFightMatchSetupError> {
    if !(1..=BENCHMARK_MAX_PLAYERS_PER_TEAM).contains(&players_per_team) {
        return Err(CastleFightMatchSetupError::UnsupportedParticipants);
    }
    let participant = |id: u8, team: u8| CastleFightParticipantConfig {
        id: PlayerId(id),
        team: Team(team),
        builder_race: CastleFightBuilderRace::Human,
    };
    let participants =
        (0..players_per_team)
            .map(|slot| participant(u8::try_from(slot).expect("bounded slot fits in u8"), 0))
            .chain((0..players_per_team).map(|slot| {
                participant(6 + u8::try_from(slot).expect("bounded slot fits in u8"), 1)
            }))
            .collect();
    let mut config = CastleFightMatchConfig::development_subset_with_participants(
        version,
        "r1",
        seed,
        participants,
    )?;
    config.rules.debug_starting_buildings = DebugStartingBuildings::All;
    Ok(config)
}

/// Applies the benchmark developer setup to a freshly created match in a fixed order and
/// returns the number of spellcasters whose mana was randomized.
pub fn apply_benchmark_setup(simulation: &mut Simulation, options: BenchmarkSetupOptions) -> usize {
    if options.invulnerable_buildings {
        simulation.debug_set_buildings_invulnerable(true);
    }
    if options.randomize_mana {
        simulation.debug_randomize_spellcaster_mana(BENCHMARK_MANA_SEED)
    } else {
        0
    }
}

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

/// Which buildings each player receives from [`populate_debug_buildings`].
#[derive(Debug, Clone, Copy)]
pub enum DebugRoster<'a> {
    /// Every implemented building of every race.
    All,
    /// Each listed player's builder-race buildings, upgrade tiers included. Players without an
    /// entry receive nothing.
    OwnRace(&'a [(PlayerId, CastleFightBuilderRace)]),
}

impl DebugBuildingKind {
    fn from_building(kind: CastleFightBuildingKind) -> Self {
        match kind {
            CastleFightBuildingKind::Production(kind) => Self::Production(kind),
            CastleFightBuildingKind::Tower(kind) => Self::Tower(kind),
        }
    }

    fn is_implemented(self, content: &CastleFightContentBundle) -> bool {
        match self {
            Self::Production(kind) => content.production_building(kind).is_some(),
            Self::Tower(kind) => content.tower(kind).is_some(),
        }
    }
}

fn all_definitions(content: &CastleFightContentBundle) -> Vec<DebugBuildingKind> {
    CastleFightProductionKind::ALL
        .into_iter()
        .map(DebugBuildingKind::Production)
        .chain(
            CastleFightTowerKind::ALL
                .into_iter()
                .map(DebugBuildingKind::Tower),
        )
        .filter(|kind| kind.is_implemented(content))
        .collect()
}

/// The race's direct build menu plus everything reachable through upgrade links, in
/// breadth-first order so base buildings precede their upgrades.
fn own_race_definitions(
    content: &CastleFightContentBundle,
    race: CastleFightBuilderRace,
) -> Vec<DebugBuildingKind> {
    let mut reached = content.direct_building_kinds_for_race(race);
    let mut next = 0;
    while let Some(&kind) = reached.get(next) {
        next += 1;
        for target in kind
            .upgrade_targets_for_version(content.map_version)
            .unwrap_or_default()
        {
            if !reached.contains(&target) {
                reached.push(target);
            }
        }
    }
    reached
        .into_iter()
        .map(DebugBuildingKind::from_building)
        .filter(|kind| kind.is_implemented(content))
        .collect()
}

/// Every implemented building of every race, for every player. See [`populate_debug_buildings`].
pub fn populate_debug_building_lines(
    simulation: &mut Simulation,
    content: &CastleFightContentBundle,
) -> (usize, usize) {
    populate_debug_buildings(simulation, content, DebugRoster::All)
}

/// Spawns one completed copy of each building in the player's roster (upgraded and legendary
/// tiers included), packed from the outer edge of each team's build region. Returns
/// `(spawned, skipped)`.
pub fn populate_debug_buildings(
    simulation: &mut Simulation,
    content: &CastleFightContentBundle,
    roster: DebugRoster<'_>,
) -> (usize, usize) {
    let all = match roster {
        DebugRoster::All => all_definitions(content),
        DebugRoster::OwnRace(_) => Vec::new(),
    };
    let mut spawned = 0;
    let mut skipped = 0;

    for team in [Team(0), Team(1)] {
        let mut owners = simulation
            .players()
            .into_iter()
            .filter(|player| player.team == team)
            .map(|player| player.id)
            .collect::<Vec<_>>();
        owners.sort_unstable();
        let castle = simulation
            .team_objective(team)
            .and_then(|objective| simulation.building(objective))
            .map(|castle| castle.footprint);
        let region = castle
            .and_then(|castle| {
                simulation
                    .team_build_regions(team)
                    .iter()
                    .copied()
                    .find(|region| footprint_contains(*region, castle))
            })
            .or_else(|| {
                simulation
                    .team_build_regions(team)
                    .iter()
                    .copied()
                    .max_by_key(|region| u32::from(region.width) * u32::from(region.height))
            });
        let (Some(castle), Some(region)) = (castle, region) else {
            for owner in owners {
                skipped += match roster {
                    DebugRoster::All => all.len(),
                    DebugRoster::OwnRace(races) => races
                        .iter()
                        .find(|(id, _)| *id == owner)
                        .map_or(0, |&(_, race)| own_race_definitions(content, race).len()),
                };
            }
            continue;
        };

        let mut cursor = PackedColumnCursor::new(region, castle);
        for owner in owners {
            let own;
            let definitions = match roster {
                DebugRoster::All => &all,
                DebugRoster::OwnRace(races) => {
                    let Some(&(_, race)) = races.iter().find(|(id, _)| *id == owner) else {
                        continue;
                    };
                    own = own_race_definitions(content, race);
                    &own
                }
            };
            for &definition in definitions {
                let size = definition.footprint_size(content);
                let placed = cursor.place(size, |footprint| {
                    spawn_debug_building(simulation, content, owner, team, definition, footprint)
                });
                if placed.is_some() {
                    spawned += 1;
                } else {
                    skipped += 1;
                }
            }
        }
    }

    (spawned, skipped)
}

fn spawn_debug_building(
    simulation: &mut Simulation,
    content: &CastleFightContentBundle,
    owner: PlayerId,
    team: Team,
    definition: DebugBuildingKind,
    footprint: BuildingFootprint,
) -> bool {
    match definition {
        DebugBuildingKind::Production(kind) => {
            let definition = content
                .production_building(kind)
                .expect("debug production kind must belong to selected content");
            simulation.debug_try_spawn_building_for_player_with_properties(
                owner,
                definition.spawn(team, footprint),
                definition.gameplay_properties(),
            )
        }
        DebugBuildingKind::Tower(kind) => {
            let definition = content
                .tower(kind)
                .expect("debug tower kind must belong to selected content");
            simulation.debug_try_spawn_building_for_player_with_properties(
                owner,
                definition.spawn(team, footprint),
                definition.gameplay_properties(),
            )
        }
    }
    .is_some()
}

/// Packs buildings into columns running along the build region's outermost edge (the edge
/// furthest from the castle's side), with no gaps between neighbours. Candidate positions the
/// caller rejects are skipped one cell at a time; once a column is full the cursor moves inward
/// by the widest building placed in it and starts a new column.
#[derive(Debug, Clone, Copy)]
struct PackedColumnCursor {
    region: BuildingFootprint,
    outer_is_min_x: bool,
    column_offset: i32,
    column_width: i32,
    next_y: i32,
}

impl PackedColumnCursor {
    fn new(region: BuildingFootprint, castle: BuildingFootprint) -> Self {
        Self {
            region,
            outer_is_min_x: castle.min_x * 2 + i32::from(castle.width)
                < region.min_x * 2 + i32::from(region.width),
            column_offset: 0,
            column_width: 0,
            next_y: region.min_y,
        }
    }

    /// Places a `size` x `size` building at the first position the cursor reaches that
    /// `try_place` accepts. A failed search leaves the cursor untouched, so smaller buildings
    /// can still use space a larger one could not.
    fn place(
        &mut self,
        size: u16,
        mut try_place: impl FnMut(BuildingFootprint) -> bool,
    ) -> Option<BuildingFootprint> {
        if size == 0 {
            return None;
        }
        let size_cells = i32::from(size);
        let mut probe = *self;
        while probe.column_offset + size_cells <= i32::from(probe.region.width) {
            let min_x = if probe.outer_is_min_x {
                probe.region.min_x + probe.column_offset
            } else {
                probe.region.max_x() - probe.column_offset - size_cells + 1
            };
            for min_y in probe.next_y..=probe.region.max_y() - size_cells + 1 {
                let footprint = BuildingFootprint::new(min_x, min_y, size, size);
                if try_place(footprint) {
                    probe.next_y = min_y + size_cells;
                    probe.column_width = probe.column_width.max(size_cells);
                    *self = probe;
                    return Some(footprint);
                }
            }
            probe.column_offset += probe.column_width.max(1);
            probe.column_width = 0;
            probe.next_y = probe.region.min_y;
        }
        None
    }
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

    fn overlaps(a: BuildingFootprint, b: BuildingFootprint) -> bool {
        a.min_x <= b.max_x() && b.min_x <= a.max_x() && a.min_y <= b.max_y() && b.min_y <= a.max_y()
    }

    fn pack(
        cursor: &mut PackedColumnCursor,
        sizes: &[u16],
        blocked: &[BuildingFootprint],
    ) -> Vec<Option<BuildingFootprint>> {
        let mut placed: Vec<BuildingFootprint> = Vec::new();
        sizes
            .iter()
            .map(|&size| {
                let footprint = cursor.place(size, |candidate| {
                    !blocked
                        .iter()
                        .chain(&placed)
                        .any(|b| overlaps(*b, candidate))
                })?;
                placed.push(footprint);
                Some(footprint)
            })
            .collect()
    }

    #[test]
    fn packing_is_gapless_then_moves_inward_when_the_outer_column_is_full() {
        let region = BuildingFootprint::new(0, 0, 12, 8);
        let castle = BuildingFootprint::new(1, 2, 2, 2);
        let mut cursor = PackedColumnCursor::new(region, castle);
        let plan = pack(&mut cursor, &[4, 4, 3, 3, 3], &[]);
        let plan = plan.into_iter().map(Option::unwrap).collect::<Vec<_>>();
        // Outer edge is min_x; first column holds 4+4 cells, then a new column begins 4 inward.
        assert_eq!((plan[0].min_x, plan[0].min_y), (0, 0));
        assert_eq!((plan[1].min_x, plan[1].min_y), (0, 4));
        assert_eq!((plan[2].min_x, plan[2].min_y), (4, 0));
        assert_eq!((plan[3].min_x, plan[3].min_y), (4, 3));
        // The second column only has 2 free rows left, so the last building opens a third.
        assert_eq!((plan[4].min_x, plan[4].min_y), (7, 0));
    }

    #[test]
    fn packing_mirrors_for_the_east_side_and_skips_occupied_locations() {
        let region = BuildingFootprint::new(0, 0, 10, 8);
        let castle = BuildingFootprint::new(6, 2, 2, 2);
        let mut cursor = PackedColumnCursor::new(region, castle);
        assert!(!cursor.outer_is_min_x);
        let blocked = [BuildingFootprint::new(7, 0, 3, 2)];
        let plan = pack(&mut cursor, &[3, 3], &blocked);
        // The first candidate overlaps the blocker, so the cursor slides to the first free cell.
        assert_eq!(plan[0], Some(BuildingFootprint::new(7, 2, 3, 3)));
        assert_eq!(plan[1], Some(BuildingFootprint::new(7, 5, 3, 3)));
    }

    #[test]
    fn packing_reports_failure_without_consuming_space_for_smaller_buildings() {
        let region = BuildingFootprint::new(0, 0, 6, 6);
        let castle = BuildingFootprint::new(4, 2, 2, 2);
        let mut cursor = PackedColumnCursor::new(region, castle);
        let plan = pack(&mut cursor, &[6, 6, 2, 0], &[]);
        assert_eq!(plan[0], Some(BuildingFootprint::new(0, 0, 6, 6)));
        assert_eq!(plan[1], None);
        assert_eq!(plan[3], None);
        assert_eq!(plan[2], None, "region is full after the single 6x6");
        let mut cursor = PackedColumnCursor::new(region, castle);
        let plan = pack(&mut cursor, &[7, 2], &[]);
        assert_eq!(plan, vec![None, Some(BuildingFootprint::new(4, 0, 2, 2))]);
    }
}
