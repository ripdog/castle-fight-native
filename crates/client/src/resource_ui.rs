use bevy::{ecs::system::SystemParam, prelude::*};
use castle_fight_sim::{PlayerId, SimId, Simulation};

use crate::{
    AuthoritativeSimulation, SelectedMatch,
    bridge::PresentationSamples,
    build_ui::{ActionPanelMode, ActionPanelState},
    console::{
        ConsoleMetrics, ConsoleSlots, MENU_BUTTON_HEIGHT, MENU_BUTTON_WIDTH, RESOURCE_SOCKET_PITCH,
        RESOURCE_SOCKET_WIDTH, TOP_BAR_UNITS, wc3, wc3_font,
    },
    debug_menu::DebugMenuState,
    inspection::InspectionSelection,
    presentation::{
        BuildingGridSnapState, CameraFocusRequest, FpsDisplay, MapGridState, player_color,
    },
    ui_icons::{UiIconAssets, UiIconKey, UiResourceIcon},
};

/// Nominal 1080p height of the console's top bar. Fixed-pixel developer panels hang below it;
/// the console itself scales with the window (see `console::ConsoleMetrics`).
pub(crate) const TOP_BAR_HEIGHT: f32 = 58.0;

const GOLD_COLOR: Color = Color::srgb(0.96, 0.78, 0.16);
const LUMBER_COLOR: Color = Color::srgb(0.26, 0.78, 0.34);
const LEGENDARY_COLOR: Color = Color::srgb(0.72, 0.56, 0.96);
const PROGRESS_TRACK: Color = Color::srgb(0.11, 0.09, 0.055);
const BUILDER_SHORTCUT_LEFT: f32 = 10.0;
/// Gap between the bottom edge of the top bar and the builder shortcut column.
const BUILDER_SHORTCUT_TOP_GAP: f32 = 10.0;
const BUILDER_SHORTCUT_SIZE: f32 = 54.0;
const BUILDER_SHORTCUT_GAP: f32 = 6.0;
const BUILDER_SHORTCUT_BORDER: f32 = 3.0;
const BUILDER_SHORTCUT_BACKGROUND: Color = Color::srgba(0.025, 0.023, 0.020, 0.96);
const BUILDER_SHORTCUT_HOVERED: Color = Color::srgba(0.16, 0.14, 0.10, 0.98);
const BUILDER_SHORTCUT_SELECTED: Color = Color::srgba(0.28, 0.24, 0.12, 0.98);
const MENU_BUTTON_BACKGROUND: Color = Color::srgba(0.02, 0.02, 0.025, 0.55);
const MENU_BUTTON_HOVERED: Color = Color::srgba(0.22, 0.18, 0.08, 0.85);
const MENU_BUTTON_ENABLED: Color = Color::srgba(0.12, 0.30, 0.12, 0.7);
const MENU_BUTTON_TEXT_COLOR: Color = Color::srgb(0.95, 0.78, 0.30);
/// Inset of the first resource icon inside the top bar's resource frame, in layout units.
const RESOURCE_GROUP_INSET: f32 = 0.0165;
/// Width of the income readout's progress bar, centred in the fourth socket.
const INCOME_WIDTH: f32 = 0.06;
const DOCK_TEXT: f32 = 0.0092;
const DOCK_BUTTON_TEXT: f32 = 0.0078;
const MENU_BUTTON_TEXT: f32 = 0.0092;

#[derive(Component)]
struct PerformanceText;

#[derive(Component)]
struct SelectedPlayerText;

#[derive(Component)]
struct GoldText;

#[derive(Component)]
struct GoldIncomeText;

#[derive(Component)]
struct GoldIncomeProgress;

#[derive(Component)]
struct LumberText;

#[derive(Component)]
struct LegendaryText;

#[derive(Component)]
struct BuilderShortcutRoot;

#[derive(Component)]
struct MapGridToggleButton;

#[derive(Component)]
struct BuildingGridSnapToggleButton;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
struct BuilderShortcutButton(SimId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BuilderShortcutSpec {
    id: SimId,
    owner: PlayerId,
    rawcode: u32,
    name: &'static str,
}

#[derive(Resource, Default)]
pub(crate) struct BuilderShortcutState {
    entries: Vec<BuilderShortcutSpec>,
}

impl BuilderShortcutState {
    fn len(&self) -> usize {
        self.entries.len()
    }
}

pub(crate) struct ResourceUiPlugin;

impl Plugin for ResourceUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ConsoleSlots>()
            .init_resource::<BuilderShortcutState>()
            .init_resource::<MapGridState>()
            .init_resource::<BuildingGridSnapState>()
            .init_resource::<CameraFocusRequest>()
            .add_message::<crate::game_menu::MatchSessionReset>()
            .add_message::<crate::game_menu::OpenGameMenuRequest>()
            .add_message::<crate::scoreboard_ui::ToggleScoreboardRequest>()
            .add_systems(Update, reset_resource_ui_for_new_match)
            .add_systems(
                Startup,
                (
                    setup_resource_bar,
                    setup_builder_shortcuts,
                    setup_dock_header,
                    setup_menu_buttons,
                ),
            )
            .add_systems(
                Update,
                (
                    update_resource_bar.after(crate::builder_controls::BuilderCommandInput),
                    sync_builder_shortcuts,
                    handle_builder_shortcut_click,
                    update_builder_shortcut_visuals,
                    handle_scores_button,
                    handle_menu_button,
                    handle_map_grid_toggle_click,
                    update_map_grid_toggle_visual,
                    handle_building_grid_snap_toggle_click,
                    update_building_grid_snap_toggle_visual,
                )
                    .chain(),
            );
    }
}

fn reset_resource_ui_for_new_match(
    mut resets: MessageReader<crate::game_menu::MatchSessionReset>,
    mut shortcuts: ResMut<BuilderShortcutState>,
    mut focus: ResMut<CameraFocusRequest>,
) {
    if resets.read().count() > 0 {
        *shortcuts = BuilderShortcutState::default();
        focus.0 = None;
    }
}

fn setup_builder_shortcuts(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(BUILDER_SHORTCUT_LEFT),
            top: wc3(TOP_BAR_UNITS),
            margin: UiRect::top(px(BUILDER_SHORTCUT_TOP_GAP)),
            flex_direction: FlexDirection::Column,
            row_gap: px(BUILDER_SHORTCUT_GAP),
            ..default()
        },
        ZIndex(950),
        BuilderShortcutRoot,
    ));
}

#[derive(Component)]
struct ScoresButton;

fn menu_button(
    row: &mut ChildSpawnerCommands,
    label: &'static str,
    marker: impl Bundle,
    background: Color,
) {
    row.spawn((
        Button,
        Node {
            width: wc3(MENU_BUTTON_WIDTH),
            height: wc3(MENU_BUTTON_HEIGHT),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(background),
        marker,
    ))
    .with_child((
        Text::new(label),
        TextFont::from_font_size(wc3_font(MENU_BUTTON_TEXT)),
        TextColor(MENU_BUTTON_TEXT_COLOR),
        Pickable::IGNORE,
    ));
}

/// The four sockets in the top bar's left tile hold Warcraft's menu buttons: the game menu, the
/// scoreboard, the map grid and building-grid snapping.
fn setup_menu_buttons(mut commands: Commands, slots: Res<ConsoleSlots>) {
    commands.entity(slots.menu_bar).with_children(|row| {
        menu_button(row, "Menu (F10)", MenuButton, MENU_BUTTON_BACKGROUND);
        menu_button(row, "Scores (F9)", ScoresButton, MENU_BUTTON_BACKGROUND);
        menu_button(row, "Grid", MapGridToggleButton, MENU_BUTTON_BACKGROUND);
        menu_button(
            row,
            "Snap",
            BuildingGridSnapToggleButton,
            MENU_BUTTON_ENABLED,
        );
    });
}

#[derive(Component)]
struct MenuButton;

fn handle_menu_button(
    buttons: Query<&Interaction, (Changed<Interaction>, With<MenuButton>)>,
    mut requests: MessageWriter<crate::game_menu::OpenGameMenuRequest>,
) {
    for interaction in &buttons {
        if *interaction == Interaction::Pressed {
            requests.write(crate::game_menu::OpenGameMenuRequest);
        }
    }
}

type ScoresButtonQuery<'w, 's> = Query<
    'w,
    's,
    (&'static Interaction, &'static mut BackgroundColor),
    (Changed<Interaction>, With<ScoresButton>),
>;

fn handle_scores_button(
    mut buttons: ScoresButtonQuery<'_, '_>,
    mut requests: MessageWriter<crate::scoreboard_ui::ToggleScoreboardRequest>,
) {
    for (interaction, mut background) in &mut buttons {
        background.0 = match interaction {
            Interaction::Pressed => {
                requests.write(crate::scoreboard_ui::ToggleScoreboardRequest);
                MENU_BUTTON_HOVERED
            }
            Interaction::Hovered => MENU_BUTTON_HOVERED,
            Interaction::None => MENU_BUTTON_BACKGROUND,
        };
    }
}

/// Dock header: whose resources are shown, plus the developer FPS/tick readout.
fn setup_dock_header(mut commands: Commands, slots: Res<ConsoleSlots>) {
    commands.entity(slots.dock_header).with_children(|row| {
        row.spawn((
            Text::new("BLUE PLAYER"),
            TextFont::from_font_size(wc3_font(DOCK_TEXT)),
            TextColor(Color::srgb(0.35, 0.67, 1.0)),
            SelectedPlayerText,
        ));
        row.spawn((
            Text::new("FPS --\nTICK 0"),
            TextFont::from_font_size(wc3_font(DOCK_BUTTON_TEXT)),
            TextColor(Color::srgb(0.72, 0.76, 0.82)),
            PerformanceText,
        ));
    });
}

/// Gold, lumber and legendary points sit in the top bar's resource frame, one icon and one
/// number each, at Warcraft's own offsets (legendary points use the supply slot).
fn setup_resource_bar(
    mut commands: Commands,
    slots: Res<ConsoleSlots>,
    asset_server: Option<Res<AssetServer>>,
    mut icons: Option<ResMut<UiIconAssets>>,
) {
    let mut load = |resource: UiResourceIcon| -> Option<Handle<Image>> {
        let server = asset_server.as_deref()?;
        icons
            .as_deref_mut()?
            .image(UiIconKey::Resource(resource), server)
    };
    let groups = [
        (RESOURCE_GROUP_INSET, UiResourceIcon::Gold, GOLD_COLOR),
        (
            RESOURCE_GROUP_INSET + RESOURCE_SOCKET_PITCH,
            UiResourceIcon::Lumber,
            LUMBER_COLOR,
        ),
        (
            RESOURCE_GROUP_INSET + 2.0 * RESOURCE_SOCKET_PITCH,
            UiResourceIcon::Supply,
            LEGENDARY_COLOR,
        ),
    ];
    commands.entity(slots.resources).with_children(|bar| {
        for (index, (offset, resource, accent)) in groups.into_iter().enumerate() {
            let icon = load(resource);
            bar.spawn(Node {
                position_type: PositionType::Absolute,
                left: wc3(offset),
                top: px(0.0),
                height: percent(100.0),
                align_items: AlignItems::Center,
                column_gap: wc3(0.004),
                ..default()
            })
            .with_children(|group| {
                let icon_node = Node {
                    width: wc3(0.0164),
                    height: wc3(0.0164),
                    ..default()
                };
                match icon {
                    Some(icon) => {
                        group.spawn((ImageNode::new(icon), icon_node, Pickable::IGNORE));
                    }
                    None => {
                        group.spawn((
                            Text::new(["G", "W", "L"][index]),
                            TextFont::from_font_size(wc3_font(0.012)),
                            TextColor(accent),
                        ));
                    }
                }
                let value = group.spawn((
                    Text::new("0"),
                    TextFont::from_font_size(wc3_font(0.0105)),
                    TextColor(accent),
                    Pickable::IGNORE,
                ));
                let mut value = value;
                match index {
                    0 => value.insert(GoldText),
                    1 => value.insert(LumberText),
                    _ => value.insert(LegendaryText),
                };
            });
        }
        // The fourth socket (Warcraft's upkeep slot) shows gold income and the time to payout.
        bar.spawn(Node {
            position_type: PositionType::Absolute,
            left: wc3(3.0 * RESOURCE_SOCKET_PITCH + (RESOURCE_SOCKET_WIDTH - INCOME_WIDTH) / 2.0),
            top: px(0.0),
            height: percent(100.0),
            flex_direction: FlexDirection::Column,
            justify_content: JustifyContent::Center,
            row_gap: wc3(0.0015),
            ..default()
        })
        .with_children(|income| {
            income.spawn((
                Text::new("income +0"),
                TextFont::from_font_size(wc3_font(0.0092)),
                TextColor(GOLD_COLOR.with_alpha(0.9)),
                Pickable::IGNORE,
                GoldIncomeText,
            ));
            income
                .spawn((
                    Node {
                        width: wc3(INCOME_WIDTH),
                        height: wc3(0.0035),
                        ..default()
                    },
                    BackgroundColor(PROGRESS_TRACK),
                ))
                .with_child((
                    Node {
                        width: percent(0.0),
                        height: percent(100.0),
                        ..default()
                    },
                    BackgroundColor(GOLD_COLOR),
                    GoldIncomeProgress,
                ));
        });
    });
}

fn controllable_builder_shortcuts(
    simulation: &Simulation,
    local_player: PlayerId,
    presentation: &PresentationSamples,
    control_all_players: bool,
) -> Vec<BuilderShortcutSpec> {
    let mut shortcuts = presentation
        .current
        .builders
        .values()
        .filter(|builder| {
            control_all_players || simulation.can_player_control_builder(local_player, builder.id)
        })
        .map(|builder| BuilderShortcutSpec {
            id: builder.id,
            owner: builder.owner,
            rawcode: builder.appearance.rawcode,
            name: builder.appearance.name,
        })
        .collect::<Vec<_>>();
    // Keep the local player's own builder first, followed by takeover-controlled allied builders
    // in stable player/entity order. The UI therefore naturally grows from one to several buttons
    // without encoding any particular team size.
    shortcuts.sort_unstable_by_key(|shortcut| {
        (
            shortcut.owner != local_player,
            shortcut.owner.0,
            shortcut.id.0,
        )
    });
    shortcuts
}

#[derive(SystemParam)]
struct BuilderShortcutSyncResources<'w> {
    authoritative: Res<'w, AuthoritativeSimulation>,
    selected_match: Res<'w, SelectedMatch>,
    presentation: Res<'w, PresentationSamples>,
    debug_menu: Option<Res<'w, DebugMenuState>>,
    asset_server: Option<Res<'w, AssetServer>>,
    icon_assets: Option<ResMut<'w, UiIconAssets>>,
    state: ResMut<'w, BuilderShortcutState>,
}

fn sync_builder_shortcuts(
    mut commands: Commands,
    root: Single<Entity, With<BuilderShortcutRoot>>,
    mut resources: BuilderShortcutSyncResources<'_>,
) {
    let next = controllable_builder_shortcuts(
        &resources.authoritative.simulation,
        resources.selected_match.local_player,
        &resources.presentation,
        resources
            .debug_menu
            .as_deref()
            .is_some_and(DebugMenuState::controls_all_players),
    );
    if next == resources.state.entries {
        return;
    }

    let icon_handles = next
        .iter()
        .map(|shortcut| {
            let asset_server = resources.asset_server.as_deref()?;
            resources.icon_assets.as_deref_mut()?.image(
                UiIconKey::unit_game_interface(shortcut.rawcode),
                asset_server,
            )
        })
        .collect::<Vec<_>>();

    commands.entity(*root).despawn_children();
    commands.entity(*root).with_children(|root| {
        for (shortcut, icon) in next.iter().zip(icon_handles) {
            root.spawn((
                Button,
                Node {
                    width: px(BUILDER_SHORTCUT_SIZE),
                    height: px(BUILDER_SHORTCUT_SIZE),
                    border: UiRect::all(px(BUILDER_SHORTCUT_BORDER)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    ..default()
                },
                BackgroundColor(BUILDER_SHORTCUT_BACKGROUND),
                BorderColor::all(player_color(shortcut.owner)),
                BuilderShortcutButton(shortcut.id),
            ))
            .with_children(|button| {
                if let Some(icon) = icon {
                    button.spawn((
                        ImageNode::new(icon),
                        Node {
                            position_type: PositionType::Absolute,
                            left: px(2.0),
                            right: px(2.0),
                            top: px(2.0),
                            bottom: px(2.0),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                } else {
                    button.spawn((
                        Text::new(shortcut.name.chars().next().unwrap_or('?').to_string()),
                        TextFont::from_font_size(26.0),
                        TextColor(Color::WHITE),
                        Pickable::IGNORE,
                    ));
                }
            });
        }
    });
    resources.state.entries = next;
}

type BuilderShortcutClickQuery<'w, 's> = Query<
    'w,
    's,
    (&'static Interaction, &'static BuilderShortcutButton),
    (Changed<Interaction>, With<Button>),
>;

fn handle_builder_shortcut_click(
    buttons: BuilderShortcutClickQuery<'_, '_>,
    mut selection: ResMut<InspectionSelection>,
    mut action_panel: ResMut<ActionPanelState>,
    mut camera_focus: ResMut<CameraFocusRequest>,
) {
    for (interaction, shortcut) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        selection.replace([shortcut.0]);
        action_panel.mode = ActionPanelMode::Actions;
        action_panel.status = "Choose an action.".into();
        camera_focus.0 = Some(shortcut.0);
    }
}

fn update_builder_shortcut_visuals(
    selection: Res<InspectionSelection>,
    mut buttons: Query<(&Interaction, &BuilderShortcutButton, &mut BackgroundColor), With<Button>>,
) {
    for (interaction, shortcut, mut background) in &mut buttons {
        background.0 = if selection.selected == Some(shortcut.0) {
            BUILDER_SHORTCUT_SELECTED
        } else if *interaction == Interaction::Hovered || *interaction == Interaction::Pressed {
            BUILDER_SHORTCUT_HOVERED
        } else {
            BUILDER_SHORTCUT_BACKGROUND
        };
    }
}

fn handle_map_grid_toggle_click(
    buttons: Query<&Interaction, (Changed<Interaction>, With<MapGridToggleButton>)>,
    mut state: ResMut<MapGridState>,
) {
    for interaction in &buttons {
        if *interaction == Interaction::Pressed {
            state.enabled = !state.enabled;
        }
    }
}

fn update_map_grid_toggle_visual(
    state: Res<MapGridState>,
    mut button: Single<(&Interaction, &mut BackgroundColor), With<MapGridToggleButton>>,
) {
    let (interaction, background) = &mut *button;
    background.0 = if **interaction == Interaction::Hovered || **interaction == Interaction::Pressed
    {
        MENU_BUTTON_HOVERED
    } else if state.enabled {
        MENU_BUTTON_ENABLED
    } else {
        MENU_BUTTON_BACKGROUND
    };
}

fn handle_building_grid_snap_toggle_click(
    buttons: Query<&Interaction, (Changed<Interaction>, With<BuildingGridSnapToggleButton>)>,
    mut state: ResMut<BuildingGridSnapState>,
) {
    for interaction in &buttons {
        if *interaction == Interaction::Pressed {
            state.enabled = !state.enabled;
        }
    }
}

fn update_building_grid_snap_toggle_visual(
    state: Res<BuildingGridSnapState>,
    mut button: Single<(&Interaction, &mut BackgroundColor), With<BuildingGridSnapToggleButton>>,
) {
    let (interaction, background) = &mut *button;
    background.0 = if **interaction == Interaction::Hovered || **interaction == Interaction::Pressed
    {
        MENU_BUTTON_HOVERED
    } else if state.enabled {
        MENU_BUTTON_ENABLED
    } else {
        MENU_BUTTON_BACKGROUND
    };
}

/// The scoreboard dock, whose buttons and readouts sit over the map's top-right corner.
#[must_use]
pub(crate) fn cursor_over_map_controls(
    cursor: Vec2,
    window_width: f32,
    window_height: f32,
) -> bool {
    ConsoleMetrics::new(window_width, window_height).over_dock(cursor, window_width)
}

#[must_use]
pub(crate) fn cursor_over_builder_shortcuts(
    cursor: Vec2,
    state: &BuilderShortcutState,
    window_height: f32,
) -> bool {
    let count = state.len();
    if count == 0 {
        return false;
    }
    let top = builder_shortcut_top(window_height);
    let height = count as f32 * BUILDER_SHORTCUT_SIZE
        + count.saturating_sub(1) as f32 * BUILDER_SHORTCUT_GAP;
    cursor.x >= BUILDER_SHORTCUT_LEFT
        && cursor.x <= BUILDER_SHORTCUT_LEFT + BUILDER_SHORTCUT_SIZE
        && cursor.y >= top
        && cursor.y <= top + height
}

fn builder_shortcut_top(window_height: f32) -> f32 {
    ConsoleMetrics::new(0.0, window_height).top_bar_height() + BUILDER_SHORTCUT_TOP_GAP
}

type ResourceTextQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Text,
        Option<&'static PerformanceText>,
        Option<&'static SelectedPlayerText>,
        Option<&'static GoldText>,
        Option<&'static GoldIncomeText>,
        Option<&'static LumberText>,
        Option<&'static LegendaryText>,
        Option<&'static mut TextColor>,
    ),
>;

/// Inputs of the developer FPS/tick readout.
#[derive(bevy::ecs::system::SystemParam)]
struct PerformanceReadout<'w> {
    fps: Res<'w, FpsDisplay>,
    debug: Option<Res<'w, crate::presentation::DebugPresentation>>,
}

fn update_resource_bar(
    authoritative: Res<AuthoritativeSimulation>,
    selected_match: Res<SelectedMatch>,
    inspection: Res<InspectionSelection>,
    presentation: Res<PresentationSamples>,
    readout: PerformanceReadout,
    mut resource_texts: ResourceTextQuery<'_, '_>,
    mut progress: Single<&mut Node, With<GoldIncomeProgress>>,
) {
    let selected_player = inspection
        .selected
        .and_then(|selected| {
            presentation
                .current
                .builders
                .get(&selected)
                .map(|builder| builder.owner)
                .or_else(|| {
                    presentation
                        .current
                        .units
                        .get(&selected)
                        .map(|unit| unit.owner)
                })
                .or_else(|| {
                    presentation
                        .current
                        .buildings
                        .get(&selected)
                        .and_then(|building| building.owner)
                })
        })
        .unwrap_or(selected_match.local_player);
    let economy = *presentation
        .current
        .player_economy
        .get(&selected_player)
        .or_else(|| {
            presentation
                .current
                .player_economy
                .get(&selected_match.local_player)
        })
        .expect("local player must have presentation economy state");
    let resources = authoritative
        .pending_build_commands
        .project(&authoritative.simulation, selected_match.content)
        .resources
        .get(&selected_player)
        .copied()
        .unwrap_or(economy.resources);

    progress.width = percent(f32::from(economy.income_progress_per_10k) / 100.0);

    let local = presentation
        .current
        .players
        .get(&selected_match.local_player)
        .expect("local player must exist in presentation state");
    let selected = presentation
        .current
        .players
        .get(&selected_player)
        .expect("selected owner must exist in presentation state");
    let relation = if selected.id == local.id {
        "YOU"
    } else if selected.team == local.team {
        "ALLY"
    } else {
        "ENEMY"
    };
    let player_label = match authoritative.player_name(selected_player) {
        Some(name) => format!("{} - {relation}", name.to_uppercase()),
        None => format!("PLAYER {} - {relation}", u16::from(selected_player.0) + 1),
    };
    let player_text_color = player_color(selected_player);
    for (
        mut text,
        performance,
        selected_player,
        gold,
        gold_income,
        lumber,
        legendary,
        mut text_color,
    ) in &mut resource_texts
    {
        if performance.is_some() {
            let fps = readout
                .fps
                .fps()
                .map_or_else(|| "--".to_owned(), |fps| format!("{fps:.0}"));
            // Developer readout: shown only with the F1 debug overlays on.
            text.0 = if readout.debug.as_ref().is_none_or(|debug| debug.overlays) {
                format!("FPS {fps}\nTICK {}", presentation.current.tick)
            } else {
                String::new()
            };
        } else if selected_player.is_some() {
            text.0.clone_from(&player_label);
            if let Some(color) = text_color.as_mut() {
                color.0 = player_text_color;
            }
        } else if gold.is_some() {
            text.0 = resources.gold.to_string();
        } else if gold_income.is_some() {
            text.0 = format!("income +{}", economy.income);
        } else if lumber.is_some() {
            text.0 = resources.lumber.to_string();
        } else if legendary.is_some() {
            text.0 = format!(
                "{} / {}",
                resources.legendary_points_used, resources.legendary_points_cap
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use castle_fight_sim::{
        BuilderConfiguration, BuilderLocomotion, BuilderProfile, BuilderSpawn, ContentIdentity,
        SimPoint, SimulationConfig, Team,
    };

    use crate::{
        AuthoritativeSimulation,
        bridge::{PresentationSamples, PresentationSnapshot},
        demo::create_demo_world,
        inspection::InspectionSelection,
    };

    #[test]
    fn resource_bar_system_runs_without_conflicting_text_queries() {
        let demo = create_demo_world(1, Some(0));
        let snapshot = PresentationSnapshot::capture(&demo.simulation);
        let authoritative = AuthoritativeSimulation::new(demo.simulation, demo.content);
        let mut app = App::new();
        app.insert_resource(SelectedMatch {
            content: demo.content,
            direct_buildings: demo.direct_buildings,
            local_player: castle_fight_sim::PlayerId(0),
        })
        .insert_resource(authoritative)
        .insert_resource(InspectionSelection::default())
        .insert_resource(ActionPanelState::default())
        .insert_resource(PresentationSamples::new(snapshot))
        .insert_resource(FpsDisplay::default())
        .add_plugins(ResourceUiPlugin);

        app.update();

        let mut performance = app
            .world_mut()
            .query_filtered::<&Text, With<PerformanceText>>();
        let text = performance.single(app.world()).expect("performance text");
        assert_eq!(text.0, "FPS --\nTICK 0");

        let kind = castle_fight_sim::CastleFightBuildingKind::Production(
            castle_fight_sim::CastleFightProductionKind::Barracks,
        );
        let builder = app
            .world()
            .resource::<AuthoritativeSimulation>()
            .simulation
            .builder_for_player(PlayerId(0))
            .unwrap();
        let before = app
            .world()
            .resource::<AuthoritativeSimulation>()
            .simulation
            .player_resources_for(builder.owner)
            .unwrap();
        let position = crate::build_orders::tests::legal_position(
            &app.world().resource::<AuthoritativeSimulation>().simulation,
            demo.content,
            builder.team,
            kind,
        );
        let cost = kind.economy(demo.content).unwrap();
        app.world_mut()
            .resource_mut::<AuthoritativeSimulation>()
            .submit_local_command(
                builder.owner,
                castle_fight_sim::PlayerCommand::PlaceBuilding {
                    builder: builder.id,
                    building: kind.stable_id(),
                    position,
                },
            );
        app.update();
        let gold = app
            .world_mut()
            .query_filtered::<&Text, With<GoldText>>()
            .single(app.world())
            .unwrap()
            .0
            .clone();
        let lumber = app
            .world_mut()
            .query_filtered::<&Text, With<LumberText>>()
            .single(app.world())
            .unwrap()
            .0
            .clone();
        assert_eq!(gold, (before.gold - cost.gold_cost).to_string());
        assert_eq!(lumber, (before.lumber - cost.lumber_cost).to_string());
        app.world_mut()
            .resource_mut::<AuthoritativeSimulation>()
            .submit_local_command(
                builder.owner,
                castle_fight_sim::PlayerCommand::StopBuilder {
                    builder: builder.id,
                },
            );
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<&Text, With<GoldText>>()
                .single(app.world())
                .unwrap()
                .0,
            before.gold.to_string()
        );
        assert_eq!(
            app.world()
                .resource::<AuthoritativeSimulation>()
                .simulation
                .tick(),
            0
        );
    }

    #[test]
    fn builder_shortcuts_filter_by_authoritative_control_and_keep_authored_icon_identity() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 1);
        let profile = BuilderProfile {
            speed_per_tick: 0,
            order_queue_capacity: 4,
            build_range: 0,
            repair_range: 0,
            repair_autocast_range: 0,
            repair_time_ratio_numerator: 1,
            repair_time_ratio_denominator: 1,
            full_repair_duration_ticks: 1,
            blink_range: 0,
            blink_boundary_inset: 0,
        };
        let spawn = |team, position, rawcode, name| BuilderSpawn {
            team,
            position,
            profile,
            configuration: BuilderConfiguration {
                appearance: ContentIdentity {
                    map_version: castle_fight_sim::CASTLE_FIGHT_DEFAULT_MAP_VERSION,
                    rawcode,
                    name,
                },
                locomotion: BuilderLocomotion::Foot,
                build_catalog: Vec::new(),
            },
            repair_autocast_enabled: false,
        };
        let own = simulation.spawn_builder_for_player(
            PlayerId(0),
            spawn(
                Team(0),
                SimPoint::new(10, 0),
                u32::from_be_bytes(*b"X00C"),
                "Human Builder",
            ),
        );
        simulation.spawn_builder_for_player(
            PlayerId(1),
            spawn(
                Team(1),
                SimPoint::new(100, 0),
                u32::from_be_bytes(*b"X019"),
                "Orc Builder",
            ),
        );
        let presentation = PresentationSamples::new(PresentationSnapshot::capture(&simulation));

        let shortcuts =
            controllable_builder_shortcuts(&simulation, PlayerId(0), &presentation, false);
        assert_eq!(shortcuts.len(), 1);
        assert_eq!(shortcuts[0].id, own);
        assert_eq!(shortcuts[0].owner, PlayerId(0));
        assert_eq!(shortcuts[0].rawcode, u32::from_be_bytes(*b"X00C"));

        let all_shortcuts =
            controllable_builder_shortcuts(&simulation, PlayerId(0), &presentation, true);
        assert_eq!(all_shortcuts.len(), 2);
        assert_eq!(all_shortcuts[0].owner, PlayerId(0));
        assert_eq!(all_shortcuts[1].owner, PlayerId(1));
    }

    #[test]
    fn building_grid_snapping_is_enabled_by_default() {
        assert!(BuildingGridSnapState::default().enabled);
    }

    #[test]
    fn builder_shortcut_hit_box_grows_with_button_count() {
        let state = BuilderShortcutState {
            entries: vec![
                BuilderShortcutSpec {
                    id: SimId(1),
                    owner: PlayerId(0),
                    rawcode: u32::from_be_bytes(*b"X00C"),
                    name: "Human Builder",
                },
                BuilderShortcutSpec {
                    id: SimId(2),
                    owner: PlayerId(1),
                    rawcode: u32::from_be_bytes(*b"X019"),
                    name: "Orc Builder",
                },
            ],
        };
        let height = 1080.0;
        let top = builder_shortcut_top(height);
        assert!(cursor_over_builder_shortcuts(
            Vec2::new(BUILDER_SHORTCUT_LEFT + 20.0, top + 20.0),
            &state,
            height,
        ));
        assert!(cursor_over_builder_shortcuts(
            Vec2::new(
                BUILDER_SHORTCUT_LEFT + 20.0,
                top + BUILDER_SHORTCUT_SIZE + BUILDER_SHORTCUT_GAP + 20.0,
            ),
            &state,
            height,
        ));
        assert!(!cursor_over_builder_shortcuts(
            Vec2::new(BUILDER_SHORTCUT_LEFT + BUILDER_SHORTCUT_SIZE + 1.0, top),
            &state,
            height,
        ));
    }
}
