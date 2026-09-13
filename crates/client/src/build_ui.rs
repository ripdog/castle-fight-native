use bevy::{prelude::*, window::PrimaryWindow};
use castle_fight_sim::{BuildingFootprint, Team};

use crate::{
    AuthoritativeSimulation,
    demo::{BuildKind, ProductionKind},
    presentation::{WorldMetrics, draw_footprint_outline, viewport_ground_point},
    terrain::TerrainSurface,
};

const PANEL_LEFT: f32 = 16.0;
const PANEL_TOP: f32 = 16.0;
const PANEL_WIDTH: f32 = 360.0;
const PANEL_HEIGHT: f32 = 500.0;

const PANEL_BACKGROUND: Color = Color::srgba(0.035, 0.045, 0.060, 0.94);
const BUTTON_NORMAL: Color = Color::srgb(0.10, 0.12, 0.15);
const BUTTON_HOVERED: Color = Color::srgb(0.17, 0.20, 0.24);
const BUTTON_SELECTED: Color = Color::srgb(0.20, 0.42, 0.30);
const BUTTON_BORDER: Color = Color::srgb(0.32, 0.36, 0.42);

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
    Team(u8),
    Building(BuildKind),
    Cancel,
}

#[derive(Component)]
struct BuildSelectionText;

#[derive(Component)]
struct BuildStatusText;

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
                    handle_build_ui_actions,
                    style_build_ui_buttons,
                    update_build_ui_text,
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
                top: px(PANEL_TOP),
                width: px(PANEL_WIDTH),
                height: px(PANEL_HEIGHT),
                padding: UiRect::all(px(14.0)),
                flex_direction: FlexDirection::Column,
                row_gap: px(8.0),
                border_radius: BorderRadius::all(px(8.0)),
                ..default()
            },
            BackgroundColor(PANEL_BACKGROUND),
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("BUILDINGS"),
                TextFont::from_font_size(22.0),
                TextColor(Color::WHITE),
            ));
            panel.spawn((
                Text::new("Side"),
                TextFont::from_font_size(15.0),
                TextColor(Color::srgb(0.72, 0.76, 0.82)),
            ));
            panel
                .spawn((Node {
                    width: percent(100.0),
                    column_gap: px(8.0),
                    ..default()
                },))
                .with_children(|row| {
                    spawn_button(row, "Blue", BuildUiAction::Team(0));
                    spawn_button(row, "Red", BuildUiAction::Team(1));
                });
            panel.spawn((
                Text::new("Building"),
                TextFont::from_font_size(15.0),
                TextColor(Color::srgb(0.72, 0.76, 0.82)),
            ));
            spawn_building_button_row(
                panel,
                BuildKind::Production(ProductionKind::Footman),
                Some(BuildKind::Production(ProductionKind::Ranger)),
            );
            spawn_building_button_row(
                panel,
                BuildKind::Production(ProductionKind::Catapult),
                Some(BuildKind::Production(ProductionKind::IceTrollPriest)),
            );
            spawn_building_button_row(
                panel,
                BuildKind::Production(ProductionKind::GryphonRider),
                None,
            );
            spawn_building_button_row(
                panel,
                BuildKind::GuaranteedTower,
                Some(BuildKind::ProjectileTower),
            );
            spawn_building_button_row(panel, BuildKind::GlobalAreaSpell, None);
            spawn_button(panel, "Cancel placement (Esc)", BuildUiAction::Cancel);
            panel.spawn((
                Text::new("No building selected"),
                TextFont::from_font_size(16.0),
                TextColor(Color::srgb(0.90, 0.92, 0.96)),
                BuildSelectionText,
            ));
            panel.spawn((
                Text::new(""),
                TextFont::from_font_size(14.0),
                TextColor(Color::srgb(0.70, 0.78, 0.86)),
                BuildStatusText,
            ));
        });
}

fn spawn_building_button_row(
    parent: &mut ChildSpawnerCommands,
    first: BuildKind,
    second: Option<BuildKind>,
) {
    parent
        .spawn((Node {
            width: percent(100.0),
            column_gap: px(8.0),
            ..default()
        },))
        .with_children(|row| {
            spawn_button(row, first.label(), BuildUiAction::Building(first));
            if let Some(second) = second {
                spawn_button(row, second.label(), BuildUiAction::Building(second));
            }
        });
}

fn spawn_button(parent: &mut ChildSpawnerCommands, label: &'static str, action: BuildUiAction) {
    parent
        .spawn((
            Button,
            Node {
                min_width: px(120.0),
                height: px(38.0),
                padding: UiRect::horizontal(px(12.0)),
                border: UiRect::all(px(1.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                flex_grow: 1.0,
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(BUTTON_BORDER),
            action,
        ))
        .with_child((
            Text::new(label),
            TextFont::from_font_size(15.0),
            TextColor(Color::WHITE),
        ));
}

fn handle_build_ui_actions(
    keys: Res<ButtonInput<KeyCode>>,
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
        match *action {
            BuildUiAction::Team(team) => {
                selection.team = Team(team);
                selection.status = format!("{} side selected.", team_label(selection.team));
            }
            BuildUiAction::Building(kind) => {
                selection.kind = Some(kind);
                selection.status = format!(
                    "{} {} selected — left-click the battlefield to place.",
                    team_label(selection.team),
                    kind.label()
                );
            }
            BuildUiAction::Cancel => {
                selection.kind = None;
                selection.status = "Placement cancelled.".into();
            }
        }
    }
}

fn style_build_ui_buttons(
    selection: Res<BuildSelection>,
    mut buttons: Query<(
        &BuildUiAction,
        &Interaction,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
) {
    for (action, interaction, mut background, mut border) in &mut buttons {
        let selected = match *action {
            BuildUiAction::Team(team) => selection.team.0 == team,
            BuildUiAction::Building(kind) => selection.kind == Some(kind),
            BuildUiAction::Cancel => false,
        };
        *background = BackgroundColor(if selected {
            BUTTON_SELECTED
        } else if *interaction == Interaction::Hovered {
            BUTTON_HOVERED
        } else {
            BUTTON_NORMAL
        });
        *border = BorderColor::all(if selected {
            match *action {
                BuildUiAction::Team(team) => team_ui_color(Team(team)),
                BuildUiAction::Building(_) => Color::srgb(0.48, 0.88, 0.56),
                BuildUiAction::Cancel => BUTTON_BORDER,
            }
        } else {
            BUTTON_BORDER
        });
    }
}

fn update_build_ui_text(
    selection: Res<BuildSelection>,
    mut selection_text: Query<&mut Text, With<BuildSelectionText>>,
    mut status_text: Query<&mut Text, (With<BuildStatusText>, Without<BuildSelectionText>)>,
) {
    let selected = selection.kind.map_or_else(
        || {
            format!(
                "Side: {} | No building selected",
                team_label(selection.team)
            )
        },
        |kind| format!("Side: {} | {}", team_label(selection.team), kind.label()),
    );
    for mut text in &mut selection_text {
        **text = selected.clone();
    }
    for mut text in &mut status_text {
        **text = selection.status.clone();
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
    if cursor_over_build_panel(cursor) {
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
    selection: Res<BuildSelection>,
    mut gizmos: Gizmos,
) {
    if selection.kind.is_none() {
        return;
    }
    let Some(cursor) = window.cursor_position() else {
        return;
    };
    if cursor_over_build_panel(cursor) {
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
        .can_place_building_for_team(selection.team, footprint);
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

pub(crate) fn cursor_over_build_panel(cursor: Vec2) -> bool {
    cursor.x >= PANEL_LEFT
        && cursor.x <= PANEL_LEFT + PANEL_WIDTH
        && cursor.y >= PANEL_TOP
        && cursor.y <= PANEL_TOP + PANEL_HEIGHT
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
            BuildKind::Production(ProductionKind::Footman),
        );
        assert_eq!(footprint, BuildingFootprint::new(8, 5, 4, 4));

        let tower = placement_footprint(
            &metrics,
            Vec3::new(105.0, 0.0, 75.0),
            BuildKind::GuaranteedTower,
        );
        assert_eq!(tower, BuildingFootprint::new(9, 6, 3, 3));
    }

    #[test]
    fn panel_capture_matches_visible_panel_bounds() {
        assert!(cursor_over_build_panel(Vec2::new(16.0, 16.0)));
        assert!(cursor_over_build_panel(Vec2::new(376.0, 516.0)));
        assert!(!cursor_over_build_panel(Vec2::new(377.0, 200.0)));
        assert!(!cursor_over_build_panel(Vec2::new(200.0, 517.0)));
    }
}
