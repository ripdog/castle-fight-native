use castle_fight_sim::{
    BuilderBuildError, BuildingEconomyProfile, BuildingFootprint, BuildingUpgradeError,
    CastleFightBuildingKind, CastleFightContentBundle, CastleFightMatchConfig,
    CastleFightMatchSetupError, CastleFightProductionKind, CastleFightTowerKind,
    CastleFightUnitKind, CommandCardPosition, MapVersion, SUBUNITS_PER_WORLD_UNIT, SimId, SimPoint,
    Simulation, Team, TerrainElevationMap, create_castle_fight_match,
};

use crate::presentation::WorldMetrics;

const CAMERA_MIN_X_WORLD: i32 = -5_888;
const CAMERA_MAX_X_WORLD: i32 = 5_888;
const CAMERA_MIN_Y_WORLD: i32 = -3_328;
const CAMERA_MAX_Y_WORLD: i32 = 3_328;
const DEVELOPMENT_MATCH_SEED: u64 = 0x4341_5354_4c45;

pub struct DemoWorld {
    pub simulation: Simulation,
    pub metrics: WorldMetrics,
    pub terrain_source_json: &'static str,
    pub terrain: TerrainElevationMap,
    pub content: &'static CastleFightContentBundle,
    pub direct_buildings: Vec<BuildKind>,
    pub match_config: CastleFightMatchConfig,
}

#[cfg(test)]
#[must_use]
pub fn create_demo_world(workers: usize, stress_units: Option<usize>) -> DemoWorld {
    create_demo_world_for_version(workers, stress_units, MapVersion::CASTLE_FIGHT_9_27, "r1")
        .expect("default development release must remain playable")
}

pub fn create_demo_world_for_version(
    workers: usize,
    stress_units: Option<usize>,
    map_version: MapVersion,
    release_revision: &str,
) -> Result<DemoWorld, CastleFightMatchSetupError> {
    let match_config = CastleFightMatchConfig::development_subset(
        map_version,
        release_revision,
        DEVELOPMENT_MATCH_SEED,
    )?;
    let mut game = create_castle_fight_match(match_config, workers)?;
    let metrics = WorldMetrics::from_simulation_config(&game.simulation_config)
        .with_camera_focus_bounds_world(
            CAMERA_MIN_X_WORLD as f32,
            CAMERA_MIN_Y_WORLD as f32,
            CAMERA_MAX_X_WORLD as f32,
            CAMERA_MAX_Y_WORLD as f32,
        );

    if let Some(unit_count) = stress_units {
        populate_render_stress_units(&mut game.simulation, game.content, unit_count);
    }

    Ok(DemoWorld {
        simulation: game.simulation,
        metrics,
        terrain_source_json: game.terrain_source_json,
        terrain: game.terrain,
        content: game.content,
        direct_buildings: game
            .direct_buildings
            .into_iter()
            .map(BuildKind::from)
            .collect(),
        match_config: game.match_config,
    })
}

fn populate_render_stress_units(
    simulation: &mut Simulation,
    content: &CastleFightContentBundle,
    unit_count: usize,
) {
    const COLUMNS: usize = 90;
    const ROWS: usize = 32;
    const SPACING_WORLD: i32 = 40;
    const START_X_WORLD: i32 = -1_800;
    const START_Y_WORLD: i32 = -640;

    for index in 0..unit_count {
        let column = index % COLUMNS;
        let row = (index / COLUMNS) % ROWS;
        let position = SimPoint::new(
            (START_X_WORLD + column as i32 * SPACING_WORLD) * SUBUNITS_PER_WORLD_UNIT,
            (START_Y_WORLD + row as i32 * SPACING_WORLD) * SUBUNITS_PER_WORLD_UNIT,
        );
        let kind = CastleFightUnitKind::ALL[index % CastleFightUnitKind::ALL.len()];
        let definition = content
            .unit(kind)
            .expect("stress roster must belong to selected development bundle");
        simulation.spawn_resolved_unit(Team((index & 1) as u8), position, definition.resolved());
    }
}

pub(crate) type ProductionKind = CastleFightProductionKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildKind {
    Production(ProductionKind),
    Tower(CastleFightTowerKind),
}

impl From<CastleFightBuildingKind> for BuildKind {
    fn from(kind: CastleFightBuildingKind) -> Self {
        match kind {
            CastleFightBuildingKind::Production(kind) => Self::Production(kind),
            CastleFightBuildingKind::Tower(kind) => Self::Tower(kind),
        }
    }
}

impl BuildKind {
    pub(crate) fn shared(self) -> CastleFightBuildingKind {
        match self {
            Self::Production(kind) => CastleFightBuildingKind::Production(kind),
            Self::Tower(kind) => CastleFightBuildingKind::Tower(kind),
        }
    }

    pub(crate) fn label(self, content: &CastleFightContentBundle) -> &'static str {
        self.shared()
            .name(content)
            .expect("build kind must belong to selected content bundle")
    }

    pub(crate) fn tooltips(
        self,
        content: &CastleFightContentBundle,
    ) -> (&'static str, &'static str) {
        self.shared()
            .tooltips(content)
            .expect("build kind must belong to selected content bundle")
    }

    pub(crate) fn footprint_size(self, content: &CastleFightContentBundle) -> u16 {
        self.shared()
            .footprint_size_cells(content)
            .expect("build kind must belong to selected content bundle")
    }

    pub(crate) fn economy(self, content: &CastleFightContentBundle) -> BuildingEconomyProfile {
        self.shared()
            .economy(content)
            .expect("build kind must belong to selected content bundle")
    }

    pub(crate) fn rawcode(self, content: &CastleFightContentBundle) -> u32 {
        self.shared()
            .rawcode(content)
            .expect("build kind must belong to selected content bundle")
    }

    pub(crate) fn command_card_position(
        self,
        content: &CastleFightContentBundle,
    ) -> CommandCardPosition {
        self.shared()
            .command_card_position(content)
            .expect("build kind must belong to selected content bundle")
    }

    pub(crate) fn gold_cost(self, content: &CastleFightContentBundle) -> u32 {
        self.economy(content).gold_cost
    }

    pub(crate) fn lumber_cost(self, content: &CastleFightContentBundle) -> u32 {
        self.economy(content).lumber_cost
    }
}

pub(crate) fn order_demo_building(
    simulation: &mut Simulation,
    content: &CastleFightContentBundle,
    team: Team,
    footprint: BuildingFootprint,
    kind: BuildKind,
) -> Result<(), BuilderBuildError> {
    let builder = simulation
        .builder_for_team(team)
        .ok_or(BuilderBuildError::Builder(
            castle_fight_sim::BuilderCommandError::BuilderNotFound,
        ))?;
    match kind {
        BuildKind::Production(kind) => {
            let definition = content
                .production_building(kind)
                .expect("build kind must belong to selected content bundle");
            simulation.order_builder_purchase_building_with_properties(
                builder.id,
                definition.spawn(team, footprint),
                definition.gameplay_properties(),
            )
        }
        BuildKind::Tower(kind) => {
            let definition = content
                .tower(kind)
                .expect("build kind must belong to selected content bundle");
            simulation.order_builder_purchase_building_with_properties(
                builder.id,
                definition.spawn(team, footprint),
                definition.gameplay_properties(),
            )
        }
    }
}

pub(crate) fn order_demo_production_upgrade(
    simulation: &mut Simulation,
    content: &CastleFightContentBundle,
    source_id: SimId,
    target: ProductionKind,
) -> Result<(), BuildingUpgradeError> {
    let source = simulation
        .building(source_id)
        .ok_or(BuildingUpgradeError::SourceNotFound)?;
    let source_kind = source
        .content
        .and_then(|identity| content.building_kind_for_rawcode(identity.rawcode))
        .and_then(|kind| match kind {
            CastleFightBuildingKind::Production(kind) => Some(kind),
            CastleFightBuildingKind::Tower(_) => None,
        })
        .ok_or(BuildingUpgradeError::SourceDefinitionMismatch)?;
    if !source_kind
        .upgrade_targets_for_version(content.map_version)
        .map_err(|_| BuildingUpgradeError::SourceDefinitionMismatch)?
        .contains(&target)
    {
        return Err(BuildingUpgradeError::SourceDefinitionMismatch);
    }

    let source_definition = content
        .production_building(source_kind)
        .ok_or(BuildingUpgradeError::SourceDefinitionMismatch)?;
    let target_definition = content
        .production_building(target)
        .ok_or(BuildingUpgradeError::SourceDefinitionMismatch)?;
    simulation.start_building_upgrade(
        source_id,
        source_definition.spawn(source.team, source.footprint),
        source_definition.gameplay_properties(),
        target_definition.spawn(source.team, source.footprint),
        target_definition.gameplay_properties(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_and_headless_bootstrap_share_the_same_authoritative_initial_state() {
        let demo = create_demo_world(1, None);
        let headless = create_castle_fight_match(demo.match_config, 1)
            .expect("selected development release must remain playable");
        assert_eq!(demo.simulation.checksum(), headless.simulation.checksum());
    }

    #[test]
    fn demo_builder_catalog_comes_from_selected_content_bundle() {
        let DemoWorld {
            simulation,
            content,
            direct_buildings,
            ..
        } = create_demo_world(1, None);
        let builder = simulation.builder_for_team(Team(0)).expect("blue builder");
        assert_eq!(direct_buildings.len(), 7);
        for kind in direct_buildings {
            assert!(builder.configuration.allows_building(kind.rawcode(content)));
        }
        let stronghold = content
            .production_building(ProductionKind::Stronghold)
            .expect("Stronghold in development bundle");
        assert!(!builder.configuration.allows_building(stronghold.rawcode));
    }

    #[test]
    fn build_selectors_read_selected_bundle() {
        let DemoWorld { content, .. } = create_demo_world(1, None);
        let barracks = BuildKind::Production(ProductionKind::Barracks);
        assert_eq!(barracks.label(content), "Barracks");
        assert_eq!(barracks.gold_cost(content), 100);
        assert_eq!(barracks.lumber_cost(content), 0);
        assert_eq!(barracks.footprint_size(content), 4);

        let watch = BuildKind::Tower(CastleFightTowerKind::WatchTower);
        assert_eq!(watch.label(content), "Watch Tower");
        assert_eq!(watch.gold_cost(content), 150);
        assert_eq!(watch.lumber_cost(content), 300);
    }

    #[test]
    fn normal_demo_starts_with_original_resources_and_no_free_production_buildings() {
        let DemoWorld { simulation, .. } = create_demo_world(1, None);
        for team in [Team(0), Team(1)] {
            let economy = simulation.player_economy(team).expect("player economy");
            assert_eq!(economy.resources.gold, 250);
            assert_eq!(economy.resources.lumber, 125);
            assert_eq!(economy.resources.legendary_points_used, 0);
            assert_eq!(economy.resources.legendary_points_cap, 1);
            assert_eq!(economy.income, 5);
        }
        assert_eq!(simulation.building_count(), 2);
    }

    #[test]
    fn stress_population_uses_selected_bundle_units() {
        let DemoWorld { simulation, .. } = create_demo_world(1, Some(2_000));
        assert_eq!(simulation.units().len(), 2_000);
    }

    #[test]
    fn shared_bootstrap_preserves_original_placement_edges() {
        let DemoWorld { simulation, .. } = create_demo_world(1, Some(0));
        assert!(
            simulation.can_place_building_for_team(Team(0), BuildingFootprint::new(-193, 20, 4, 4))
        );
        assert!(
            !simulation
                .can_place_building_for_team(Team(0), BuildingFootprint::new(-194, 20, 4, 4))
        );
        assert!(
            !simulation.can_place_building_for_team(Team(1), BuildingFootprint::new(60, 0, 4, 4))
        );
        assert!(
            simulation.can_place_building_for_team(Team(1), BuildingFootprint::new(65, 0, 4, 4))
        );
    }
}
