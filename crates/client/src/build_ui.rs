use bevy::{prelude::*, window::PrimaryWindow};
use castle_fight_sim::{BuildingFootprint, PlayerResources, Team};

use crate::{
    AuthoritativeSimulation,
    bridge::PresentationSamples,
    demo::{BuildKind, ProductionKind},
    inspection::InspectionSelection,
    presentation::{WorldMetrics, draw_footprint_outline, viewport_ground_point},
    resource_ui::TOP_BAR_HEIGHT,
    terrain::TerrainSurface,
};

const PANEL_LEFT: f32 = 12.0;
const PANEL_BOTTOM: f32 = 12.0;
const PANEL_PADDING: f32 = 8.0;
const GRID_GAP: f32 = 4.0;
const CELL_SIZE: f32 = 66.0;
const GRID_COLUMNS: usize = 4;
const GRID_ROWS: usize = 3;
const PANEL_WIDTH: f32 =
    PANEL_PADDING * 2.0 + CELL_SIZE * GRID_COLUMNS as f32 + GRID_GAP * (GRID_COLUMNS as f32 - 1.0);
const PANEL_HEIGHT: f32 =
    PANEL_PADDING * 2.0 + CELL_SIZE * GRID_ROWS as f32 + GRID_GAP * (GRID_ROWS as f32 - 1.0);

const PANEL_BACKGROUND: Color = Color::srgba(0.105, 0.070, 0.040, 0.97);
const PANEL_BORDER: Color = Color::srgb(0.28, 0.25, 0.20);
const SLOT_BACKGROUND: Color = Color::srgb(0.018, 0.016, 0.014);
const SLOT_BORDER: Color = Color::srgb(0.34, 0.34, 0.32);
const BUTTON_NORMAL: Color = Color::srgb(0.095, 0.075, 0.055);
const BUTTON_HOVERED: Color = Color::srgb(0.18, 0.14, 0.095);
const BUTTON_SELECTED: Color = Color::srgb(0.18, 0.27, 0.14);
const BUTTON_SELECTED_BORDER: Color = Color::srgb(0.86, 0.72, 0.28);
const BUTTON_DISABLED: Color = Color::srgb(0.045, 0.043, 0.040);
const BUTTON_DISABLED_BORDER: Color = Color::srgb(0.18, 0.17, 0.16);
const BUTTON_TEXT: Color = Color::srgb(0.92, 0.90, 0.84);
const BUTTON_TEXT_DISABLED: Color = Color::srgb(0.42, 0.40, 0.37);

const BUILD_GRID: [Option<BuildKind>; GRID_COLUMNS * GRID_ROWS] = [
    Some(BuildKind::Production(ProductionKind::Barracks)),
    Some(BuildKind::Production(ProductionKind::RangersHall)),
    Some(BuildKind::Production(ProductionKind::OrcishSiegeFactory)),
    Some(BuildKind::Production(ProductionKind::IceTrollHut)),
    Some(BuildKind::Production(ProductionKind::GryphonRock)),
    Some(BuildKind::Tower(
        castle_fight_sim::CastleFightTowerKind::WatchTower,
    )),
    Some(BuildKind::Tower(
        castle_fight_sim::CastleFightTowerKind::PoofTower,
    )),
    None,
    None,
    None,
    None,
    None,
];

#[derive(Debug, Clone, Copy)]
pub(crate) struct BuildRequest {
    pub(crate) team: Team,
    pub(crate) kind: BuildKind,
    pub(crate) footprint: BuildingFootprint,
}

#[derive(Resource, Default)]
pub(crate) struct PendingBuildPlacements(pub(crate) Vec<BuildRequest>);

#[derive(Resource)]
pub(crate) struct BuildSelection {
    pub(crate) team: Team,
    pub(crate) kind: Option<BuildKind>,
    pub(crate) status: String,
}

impl Default for BuildSelection {
    fn default() -> Self {
        Self {
            team: Team(0),
            kind: None,
            status: "Select a building, then left-click the battlefield to place it.".into(),
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum BuildUiAction {
    Building(BuildKind),
}

#[derive(Component)]
struct BuildPanel;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct BuildButtonText(BuildKind);

type BuildActionInteractions<'w, 's> = Query<
    'w,
    's,
    (&'static Interaction, &'static BuildUiAction),
    (Changed<Interaction>, With<Button>),
>;

pub(crate) struct BuildUiPlugin;

impl Plugin for BuildUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BuildSelection>()
            .init_resource::<PendingBuildPlacements>()
            .add_systems(Startup, setup_build_ui)
            .add_systems(
                Update,
                (
                    sync_build_panel_to_selection,
                    sync_build_selection_affordability,
                    handle_build_ui_actions,
                    style_build_ui_buttons,
                    queue_world_placement,
                    draw_build_preview,
                )
                    .chain(),
            );
    }
}

fn setup_build_ui(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(PANEL_LEFT),
                bottom: px(PANEL_BOTTOM),
                width: px(PANEL_WIDTH),
                height: px(PANEL_HEIGHT),
                padding: UiRect::all(px(PANEL_PADDING)),
                border: UiRect::all(px(3.0)),
                flex_direction: FlexDirection::Column,
                row_gap: px(GRID_GAP),
                ..default()
            },
            BackgroundColor(PANEL_BACKGROUND),
            BorderColor::all(PANEL_BORDER),
            Visibility::Hidden,
            BuildPanel,
        ))
        .with_children(|panel| {
            for row_index in 0..GRID_ROWS {
                panel
                    .spawn((Node {
                        width: percent(100.0),
                        height: px(CELL_SIZE),
                        column_gap: px(GRID_GAP),
                        ..default()
                    },))
                    .with_children(|row| {
                        let row_start = row_index * GRID_COLUMNS;
                        for slot in &BUILD_GRID[row_start..row_start + GRID_COLUMNS] {
                            match slot {
                                Some(kind) => spawn_build_button(row, *kind),
                                None => spawn_empty_slot(row),
                            }
                        }
                    });
            }
        });
}

fn spawn_build_button(parent: &mut ChildSpawnerCommands, kind: BuildKind) {
    parent
        .spawn((
            Button,
            Node {
                width: px(CELL_SIZE),
                height: px(CELL_SIZE),
                border: UiRect::all(px(2.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_shrink: 0.0,
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(SLOT_BORDER),
            BuildUiAction::Building(kind),
        ))
        .with_child((
            Text::new(build_button_label(kind)),
            TextFont::from_font_size(10.0),
            TextColor(BUTTON_TEXT),
            TextLayout::justify(Justify::Center),
            BuildButtonText(kind),
        ));
}

fn spawn_empty_slot(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        Node {
            width: px(CELL_SIZE),
            height: px(CELL_SIZE),
            border: UiRect::all(px(2.0)),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(SLOT_BACKGROUND),
        BorderColor::all(SLOT_BORDER),
    ));
}

fn build_button_label(kind: BuildKind) -> String {
    let name = match kind {
        BuildKind::Production(ProductionKind::Barracks) => "Barracks",
        BuildKind::Production(ProductionKind::RangersHall) => "Rngrs Hall",
        BuildKind::Production(ProductionKind::OrcishSiegeFactory) => "Siege Fac.",
        BuildKind::Production(ProductionKind::IceTrollHut) => "Ice Hut",
        BuildKind::Production(ProductionKind::GryphonRock) => "Gryph Rock",
        BuildKind::Tower(castle_fight_sim::CastleFightTowerKind::WatchTower) => "Watch Tower",
        BuildKind::Tower(castle_fight_sim::CastleFightTowerKind::PoofTower) => "Poof Tower",
    };
    let economy = kind.economy();
    if economy.lumber_cost == 0 {
        format!("{name}\n{}g", economy.gold_cost)
    } else {
        format!("{name}\n{}g {}w", economy.gold_cost, economy.lumber_cost)
    }
}

fn sync_build_panel_to_selection(
    inspection: Res<InspectionSelection>,
    samples: Res<PresentationSamples>,
    mut selection: ResMut<BuildSelection>,
    mut panel: Single<&mut Visibility, With<BuildPanel>>,
) {
    let builder_team = inspection
        .selected
        .and_then(|selected| samples.current.builders.get(&selected))
        .map(|builder| builder.team);

    match builder_team {
        Some(team) => {
            selection.team = team;
            **panel = Visibility::Visible;
        }
        None => {
            selection.kind = None;
            **panel = Visibility::Hidden;
        }
    }
}

fn sync_build_selection_affordability(
    authoritative: Res<AuthoritativeSimulation>,
    pending: Res<PendingBuildPlacements>,
    mut selection: ResMut<BuildSelection>,
) {
    let Some(kind) = selection.kind else {
        return;
    };
    if can_afford_build_kind(&authoritative, &pending, selection.team, kind) {
        return;
    }
    selection.kind = None;
    selection.status =
        insufficient_resources_status(&authoritative, &pending, selection.team, kind);
}

fn handle_build_ui_actions(
    keys: Res<ButtonInput<KeyCode>>,
    authoritative: Res<AuthoritativeSimulation>,
    pending: Res<PendingBuildPlacements>,
    mut selection: ResMut<BuildSelection>,
    actions: BuildActionInteractions,
) {
    if keys.just_pressed(KeyCode::Escape) {
        selection.kind = None;
        selection.status = "Placement cancelled.".into();
    }

    for (interaction, action) in &actions {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let BuildUiAction::Building(kind) = *action;
        if !can_afford_build_kind(&authoritative, &pending, selection.team, kind) {
            selection.kind = None;
            selection.status =
                insufficient_resources_status(&authoritative, &pending, selection.team, kind);
            continue;
        }
        selection.kind = Some(kind);
        selection.status = format!(
            "{} {} selected — {} gold / {} lumber. Left-click to place.",
            team_label(selection.team),
            kind.label(),
            kind.gold_cost(),
            kind.lumber_cost(),
        );
    }
}

fn style_build_ui_buttons(
    selection: Res<BuildSelection>,
    authoritative: Res<AuthoritativeSimulation>,
    pending: Res<PendingBuildPlacements>,
    mut buttons: Query<(
        &BuildUiAction,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut labels: Query<(&BuildButtonText, &mut TextColor)>,
) {
    for (action, interaction, mut background, mut border) in &mut buttons {
        let BuildUiAction::Building(kind) = *action;
        let affordable = can_afford_build_kind(&authoritative, &pending, selection.team, kind);
        let selected = affordable && selection.kind == Some(kind);
        *background = BackgroundColor(if !affordable {
            BUTTON_DISABLED
        } else if selected {
            BUTTON_SELECTED
        } else if *interaction == Interaction::Hovered {
            BUTTON_HOVERED
        } else {
            BUTTON_NORMAL
        });
        *border = BorderColor::all(if !affordable {
            BUTTON_DISABLED_BORDER
        } else if selected {
            BUTTON_SELECTED_BORDER
        } else {
            SLOT_BORDER
        });
    }
    for (label, mut color) in &mut labels {
        color.0 = if can_afford_build_kind(&authoritative, &pending, selection.team, label.0) {
            BUTTON_TEXT
        } else {
            BUTTON_TEXT_DISABLED
        };
    }
}

fn queue_world_placement(
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    world: (Res<WorldMetrics>, Res<TerrainSurface>),
    authoritative: Res<AuthoritativeSimulation>,
    mut selection: ResMut<BuildSelection>,
    mut pending: ResMut<PendingBuildPlacements>,
) {
    let (metrics, terrain) = world;
    if !mouse_buttons.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(kind) = selection.kind else {
        return;
    };
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor_over_build_panel(cursor, window.height(), true) {
        return;
    }
    let (camera, camera_transform) = *camera;
    let Some(world) = viewport_ground_point(camera, camera_transform, cursor, &terrain) else {
        selection.status = "Placement rejected: cursor does not intersect the battlefield.".into();
        return;
    };
    let footprint = placement_footprint(&metrics, world, kind);
    if !authoritative
        .simulation
        .can_place_building_for_team(selection.team, footprint)
    {
        selection.status =
            "Placement rejected: outside this side's build region, blocked, or occupied.".into();
        return;
    }
    if !can_afford_build_kind(&authoritative, &pending, selection.team, kind) {
        selection.kind = None;
        selection.status =
            insufficient_resources_status(&authoritative, &pending, selection.team, kind);
        return;
    }

    pending.0.push(BuildRequest {
        team: selection.team,
        kind,
        footprint,
    });
    selection.status = format!("Queued {} {}.", team_label(selection.team), kind.label());
}

fn draw_build_preview(
    window: Single<&Window, With<PrimaryWindow>>,
    camera: Single<(&Camera, &GlobalTransform), With<Camera3d>>,
    metrics: Res<WorldMetrics>,
    terrain: Res<TerrainSurface>,
    authoritative: Res<AuthoritativeSimulation>,
    pending: Res<PendingBuildPlacements>,
    selection: Res<BuildSelection>,
    mut gizmos: Gizmos,
) {
    if selection.kind.is_none() {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor_over_build_panel(cursor, window.height(), true) {
        return;
    }
    let (camera, camera_transform) = *camera;
    let Some(world) = viewport_ground_point(camera, camera_transform, cursor, &terrain) else {
        return;
    };
    let kind = selection.kind.expect("checked selected build kind");
    let footprint = placement_footprint(&metrics, world, kind);
    let valid = authoritative
        .simulation
        .can_place_building_for_team(selection.team, footprint)
        && can_afford_build_kind(&authoritative, &pending, selection.team, kind);
    let color = if valid {
        team_ui_color(selection.team)
    } else {
        Color::srgb(1.0, 0.18, 0.15)
    };
    draw_footprint_outline(&mut gizmos, &metrics, &terrain, footprint, color);
}

fn placement_footprint(metrics: &WorldMetrics, world: Vec3, kind: BuildKind) -> BuildingFootprint {
    let size = kind.footprint_size();
    metrics.footprint_at_world(world, size, size)
}

fn resources_after_pending_builds(
    authoritative: &AuthoritativeSimulation,
    pending: &PendingBuildPlacements,
    team: Team,
) -> Option<PlayerResources> {
    let mut resources = authoritative.simulation.player_resources(team)?;
    for request in pending.0.iter().filter(|request| request.team == team) {
        let economy = request.kind.economy();
        resources.gold = resources.gold.saturating_sub(economy.gold_cost);
        resources.lumber = resources.lumber.saturating_sub(economy.lumber_cost);
    }
    Some(resources)
}

fn can_afford_build_kind(
    authoritative: &AuthoritativeSimulation,
    pending: &PendingBuildPlacements,
    team: Team,
    kind: BuildKind,
) -> bool {
    let Some(resources) = resources_after_pending_builds(authoritative, pending, team) else {
        return false;
    };
    let economy = kind.economy();
    resources.gold >= economy.gold_cost && resources.lumber >= economy.lumber_cost
}

fn insufficient_resources_status(
    authoritative: &AuthoritativeSimulation,
    pending: &PendingBuildPlacements,
    team: Team,
    kind: BuildKind,
) -> String {
    let resources = resources_after_pending_builds(authoritative, pending, team)
        .expect("build UI supports the two Castle Fight players");
    format!(
        "Cannot afford {}: need {} gold / {} lumber; available {} / {}.",
        kind.label(),
        kind.gold_cost(),
        kind.lumber_cost(),
        resources.gold,
        resources.lumber,
    )
}

pub(crate) fn cursor_over_build_panel(
    cursor: Vec2,
    window_height: f32,
    panel_visible: bool,
) -> bool {
    if cursor.y <= TOP_BAR_HEIGHT {
        return true;
    }
    if !panel_visible {
        return false;
    }
    let panel_top = window_height - PANEL_BOTTOM - PANEL_HEIGHT;
    cursor.x >= PANEL_LEFT
        && cursor.x <= PANEL_LEFT + PANEL_WIDTH
        && cursor.y >= panel_top
        && cursor.y <= panel_top + PANEL_HEIGHT
}

fn team_label(team: Team) -> &'static str {
    match team.0 {
        0 => "Blue",
        1 => "Red",
        _ => "Unknown",
    }
}

fn team_ui_color(team: Team) -> Color {
    match team.0 {
        0 => Color::srgb(0.20, 0.58, 1.0),
        1 => Color::srgb(1.0, 0.28, 0.22),
        _ => Color::WHITE,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use castle_fight_sim::{NavCell, SUBUNITS_PER_WORLD_UNIT, SimPoint, SimulationConfig};

    use crate::demo::create_demo_world;

    #[test]
    fn placement_preview_uses_authoritative_navigation_cell_scale() {
        let config = SimulationConfig {
            navigation_cell_size: 10 * SUBUNITS_PER_WORLD_UNIT,
            navigation_min: NavCell::new(0, 0),
            navigation_max: NavCell::new(20, 20),
            team_objective: [SimPoint::new(0, 0), SimPoint::new(0, 0)],
            ..SimulationConfig::default()
        };
        let metrics = WorldMetrics::from_simulation_config(&config);
        let footprint = placement_footprint(
            &metrics,
            Vec3::new(105.0, 0.0, 75.0),
            BuildKind::Production(ProductionKind::Barracks),
        );
        assert_eq!(footprint, BuildingFootprint::new(8, 5, 4, 4));

        let tower = placement_footprint(
            &metrics,
            Vec3::new(105.0, 0.0, 75.0),
            BuildKind::Tower(castle_fight_sim::CastleFightTowerKind::WatchTower),
        );
        assert_eq!(tower, BuildingFootprint::new(8, 5, 4, 4));
    }

    #[test]
    fn panel_capture_matches_visible_bottom_left_panel_bounds() {
        let window_height = 720.0;
        let panel_top = window_height - PANEL_BOTTOM - PANEL_HEIGHT;
        assert!(cursor_over_build_panel(
            Vec2::new(PANEL_LEFT, panel_top),
            window_height,
            true,
        ));
        assert!(cursor_over_build_panel(
            Vec2::new(PANEL_LEFT + PANEL_WIDTH, panel_top + PANEL_HEIGHT),
            window_height,
            true,
        ));
        assert!(!cursor_over_build_panel(
            Vec2::new(PANEL_LEFT + PANEL_WIDTH + 1.0, panel_top),
            window_height,
            true,
        ));
        assert!(!cursor_over_build_panel(
            Vec2::new(PANEL_LEFT, panel_top),
            window_height,
            false,
        ));
        assert!(cursor_over_build_panel(
            Vec2::new(900.0, TOP_BAR_HEIGHT / 2.0),
            window_height,
            false,
        ));
    }

    #[test]
    fn starting_resources_disable_unaffordable_build_commands() {
        let demo = create_demo_world(1, Some(0));
        let authoritative = AuthoritativeSimulation {
            simulation: demo.simulation,
        };
        let mut pending = PendingBuildPlacements::default();

        assert!(can_afford_build_kind(
            &authoritative,
            &pending,
            Team(0),
            BuildKind::Production(ProductionKind::Barracks),
        ));
        assert!(can_afford_build_kind(
            &authoritative,
            &pending,
            Team(0),
            BuildKind::Production(ProductionKind::RangersHall),
        ));
        assert!(can_afford_build_kind(
            &authoritative,
            &pending,
            Team(0),
            BuildKind::Production(ProductionKind::GryphonRock),
        ));
        assert!(!can_afford_build_kind(
            &authoritative,
            &pending,
            Team(0),
            BuildKind::Production(ProductionKind::OrcishSiegeFactory),
        ));
        assert!(!can_afford_build_kind(
            &authoritative,
            &pending,
            Team(0),
            BuildKind::Tower(castle_fight_sim::CastleFightTowerKind::WatchTower),
        ));

        pending.0.push(BuildRequest {
            team: Team(0),
            kind: BuildKind::Production(ProductionKind::Barracks),
            footprint: BuildingFootprint::new(0, 0, 4, 4),
        });
        assert!(!can_afford_build_kind(
            &authoritative,
            &pending,
            Team(0),
            BuildKind::Production(ProductionKind::RangersHall),
        ));
        assert!(can_afford_build_kind(
            &authoritative,
            &pending,
            Team(0),
            BuildKind::Production(ProductionKind::Barracks),
        ));
    }

    #[test]
    fn build_grid_contains_every_currently_implemented_demo_building() {
        let populated: Vec<_> = BUILD_GRID.iter().flatten().copied().collect();
        assert_eq!(populated.len(), 7);
        for production in ProductionKind::ALL {
            assert!(populated.contains(&BuildKind::Production(production)));
        }
        for tower in castle_fight_sim::CastleFightTowerKind::ALL {
            assert!(populated.contains(&BuildKind::Tower(tower)));
        }
    }
}
