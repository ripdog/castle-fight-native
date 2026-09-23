use bevy::{ecs::system::SystemParam, prelude::*};
use castle_fight_sim::{PlayerId, SimId, Simulation};

use crate::{
    AuthoritativeSimulation, SelectedMatch,
    bridge::PresentationSamples,
    build_ui::{ActionPanelMode, ActionPanelState},
    debug_menu::DebugMenuState,
    inspection::InspectionSelection,
    presentation::{
        BuildingGridSnapState, CameraFocusRequest, FpsDisplay, MapGridState, player_color,
    },
    ui_icons::{UiIconAssets, UiIconKey},
};

pub(crate) const TOP_BAR_HEIGHT: f32 = 58.0;

const BAR_BACKGROUND: Color = Color::srgba(0.055, 0.048, 0.036, 0.98);
const BAR_BORDER: Color = Color::srgb(0.33, 0.27, 0.16);
const SLOT_BACKGROUND: Color = Color::srgba(0.025, 0.023, 0.020, 0.95);
const SLOT_BORDER: Color = Color::srgb(0.24, 0.21, 0.15);
const LABEL_COLOR: Color = Color::srgb(0.67, 0.63, 0.52);
const GOLD_COLOR: Color = Color::srgb(0.96, 0.78, 0.16);
const LUMBER_COLOR: Color = Color::srgb(0.26, 0.78, 0.34);
const LEGENDARY_COLOR: Color = Color::srgb(0.72, 0.56, 0.96);
const PROGRESS_TRACK: Color = Color::srgb(0.11, 0.09, 0.055);
const BUILDER_SHORTCUT_LEFT: f32 = 10.0;
const BUILDER_SHORTCUT_TOP: f32 = TOP_BAR_HEIGHT + 10.0;
const BUILDER_SHORTCUT_SIZE: f32 = 54.0;
const BUILDER_SHORTCUT_GAP: f32 = 6.0;
const BUILDER_SHORTCUT_BORDER: f32 = 3.0;
const BUILDER_SHORTCUT_BACKGROUND: Color = Color::srgba(0.025, 0.023, 0.020, 0.96);
const BUILDER_SHORTCUT_HOVERED: Color = Color::srgba(0.16, 0.14, 0.10, 0.98);
const BUILDER_SHORTCUT_SELECTED: Color = Color::srgba(0.28, 0.24, 0.12, 0.98);
const MAP_GRID_BUTTON_WIDTH: f32 = 52.0;
const MAP_GRID_BUTTON_HEIGHT: f32 = 22.0;
const MAP_GRID_BUTTON_TOP: f32 = TOP_BAR_HEIGHT + 4.0;
// The fixed right-side resource slots are GOLD, LUMBER, LEGENDARY. Keep this centred below GOLD.
const MAP_GRID_BUTTON_RIGHT: f32 = 385.0;
const MAP_GRID_BUTTON_BACKGROUND: Color = Color::srgba(0.025, 0.023, 0.020, 0.96);
const MAP_GRID_BUTTON_HOVERED: Color = Color::srgba(0.16, 0.14, 0.10, 0.98);
const MAP_GRID_BUTTON_ENABLED: Color = Color::srgba(0.18, 0.28, 0.16, 0.98);
const GRID_SNAP_BUTTON_WIDTH: f32 = 72.0;
const GRID_SNAP_BUTTON_RIGHT: f32 =
    MAP_GRID_BUTTON_RIGHT - MAP_GRID_BUTTON_WIDTH - BUILDER_SHORTCUT_GAP;

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

#[derive(Component)]
struct BuildingGridSnapCheckmark;

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
        app.init_resource::<BuilderShortcutState>()
            .init_resource::<MapGridState>()
            .init_resource::<BuildingGridSnapState>()
            .init_resource::<CameraFocusRequest>()
            .add_systems(
                Startup,
                (
                    setup_resource_bar,
                    setup_builder_shortcuts,
                    setup_map_grid_toggle,
                    setup_building_grid_snap_toggle,
                ),
            )
            .add_systems(
                Update,
                (
                    update_resource_bar,
                    sync_builder_shortcuts,
                    handle_builder_shortcut_click,
                    update_builder_shortcut_visuals,
                    handle_map_grid_toggle_click,
                    update_map_grid_toggle_visual,
                    handle_building_grid_snap_toggle_click,
                    update_building_grid_snap_toggle_visual,
                )
                    .chain(),
            );
    }
}

fn setup_builder_shortcuts(mut commands: Commands) {
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: px(BUILDER_SHORTCUT_LEFT),
            top: px(BUILDER_SHORTCUT_TOP),
            flex_direction: FlexDirection::Column,
            row_gap: px(BUILDER_SHORTCUT_GAP),
            ..default()
        },
        ZIndex(950),
        BuilderShortcutRoot,
    ));
}

fn setup_map_grid_toggle(mut commands: Commands) {
    commands
        .spawn((
            Button,
            Node {
                position_type: PositionType::Absolute,
                right: px(MAP_GRID_BUTTON_RIGHT),
                top: px(MAP_GRID_BUTTON_TOP),
                width: px(MAP_GRID_BUTTON_WIDTH),
                height: px(MAP_GRID_BUTTON_HEIGHT),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(MAP_GRID_BUTTON_BACKGROUND),
            BorderColor::all(SLOT_BORDER),
            ZIndex(1001),
            MapGridToggleButton,
        ))
        .with_child((
            Text::new("GRID"),
            TextFont::from_font_size(9.0),
            TextColor(LABEL_COLOR),
            Pickable::IGNORE,
        ));
}

fn setup_building_grid_snap_toggle(mut commands: Commands) {
    commands
        .spawn((
            Button,
            Node {
                position_type: PositionType::Absolute,
                right: px(GRID_SNAP_BUTTON_RIGHT),
                top: px(MAP_GRID_BUTTON_TOP),
                width: px(GRID_SNAP_BUTTON_WIDTH),
                height: px(MAP_GRID_BUTTON_HEIGHT),
                padding: UiRect::horizontal(px(5.0)),
                align_items: AlignItems::Center,
                column_gap: px(4.0),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(MAP_GRID_BUTTON_ENABLED),
            BorderColor::all(SLOT_BORDER),
            ZIndex(1001),
            BuildingGridSnapToggleButton,
        ))
        .with_children(|button| {
            button
                .spawn((
                    Node {
                        width: px(12.0),
                        height: px(12.0),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::Center,
                        border: UiRect::all(px(1.0)),
                        ..default()
                    },
                    BorderColor::all(LABEL_COLOR),
                ))
                .with_child((
                    Text::new("X"),
                    TextFont::from_font_size(9.0),
                    TextColor(LABEL_COLOR),
                    Pickable::IGNORE,
                    BuildingGridSnapCheckmark,
                ));
            button.spawn((
                Text::new("SNAP"),
                TextFont::from_font_size(9.0),
                TextColor(LABEL_COLOR),
                Pickable::IGNORE,
            ));
        });
}

fn setup_resource_bar(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(0.0),
                right: px(0.0),
                top: px(0.0),
                height: px(TOP_BAR_HEIGHT),
                padding: UiRect::horizontal(px(10.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::FlexStart,
                column_gap: px(8.0),
                border: UiRect::bottom(px(2.0)),
                ..default()
            },
            BackgroundColor(BAR_BACKGROUND),
            BorderColor::all(BAR_BORDER),
            ZIndex(1000),
        ))
        .with_children(|bar| {
            bar.spawn((
                Node {
                    width: px(132.0),
                    height: px(42.0),
                    padding: UiRect::horizontal(px(10.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::FlexStart,
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                BackgroundColor(SLOT_BACKGROUND),
                BorderColor::all(SLOT_BORDER),
            ))
            .with_child((
                Text::new("FPS --\nTICK 0"),
                TextFont::from_font_size(12.0),
                TextColor(Color::srgb(0.72, 0.76, 0.82)),
                PerformanceText,
            ));

            bar.spawn((Node {
                flex_grow: 1.0,
                ..default()
            },));

            bar.spawn((
                Node {
                    width: px(124.0),
                    height: px(42.0),
                    padding: UiRect::horizontal(px(10.0)),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                BackgroundColor(SLOT_BACKGROUND),
                BorderColor::all(SLOT_BORDER),
            ))
            .with_child((
                Text::new("BLUE PLAYER"),
                TextFont::from_font_size(13.0),
                TextColor(Color::srgb(0.35, 0.67, 1.0)),
                SelectedPlayerText,
            ));

            spawn_resource_slot(bar, "G", "GOLD", GOLD_COLOR, GoldText, true, true);
            spawn_resource_slot(bar, "W", "LUMBER", LUMBER_COLOR, LumberText, false, false);
            spawn_resource_slot(
                bar,
                "L",
                "LEGENDARY",
                LEGENDARY_COLOR,
                LegendaryText,
                false,
                false,
            );
        });
}

fn spawn_resource_slot<M: Component>(
    parent: &mut ChildSpawnerCommands,
    icon: &'static str,
    label: &'static str,
    accent: Color,
    value_marker: M,
    show_income: bool,
    has_progress: bool,
) {
    parent
        .spawn((
            Node {
                width: px(154.0),
                height: px(42.0),
                padding: UiRect::axes(px(6.0), px(4.0)),
                align_items: AlignItems::Center,
                column_gap: px(7.0),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(SLOT_BACKGROUND),
            BorderColor::all(SLOT_BORDER),
        ))
        .with_children(|slot| {
            slot.spawn((
                Node {
                    width: px(29.0),
                    height: px(29.0),
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::Center,
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                BackgroundColor(accent.with_alpha(0.18)),
                BorderColor::all(accent.with_alpha(0.72)),
            ))
            .with_child((
                Text::new(icon),
                TextFont::from_font_size(18.0),
                TextColor(accent),
            ));

            slot.spawn((Node {
                flex_direction: FlexDirection::Column,
                justify_content: JustifyContent::Center,
                flex_grow: 1.0,
                row_gap: px(1.0),
                ..default()
            },))
                .with_children(|column| {
                    column.spawn((
                        Text::new(label),
                        TextFont::from_font_size(9.0),
                        TextColor(LABEL_COLOR),
                    ));
                    column.spawn((
                        Text::new("0"),
                        TextFont::from_font_size(17.0),
                        TextColor(Color::WHITE),
                        value_marker,
                    ));
                    if has_progress {
                        column
                            .spawn((
                                Node {
                                    width: percent(100.0),
                                    height: px(4.0),
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
                    }
                    if show_income {
                        column.spawn((
                            Text::new("income +0"),
                            TextFont::from_font_size(8.0),
                            TextColor(GOLD_COLOR.with_alpha(0.82)),
                            GoldIncomeText,
                        ));
                    }
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
        MAP_GRID_BUTTON_HOVERED
    } else if state.enabled {
        MAP_GRID_BUTTON_ENABLED
    } else {
        MAP_GRID_BUTTON_BACKGROUND
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
    mut checkmark: Single<&mut Text, With<BuildingGridSnapCheckmark>>,
) {
    let (interaction, background) = &mut *button;
    background.0 = if **interaction == Interaction::Hovered || **interaction == Interaction::Pressed
    {
        MAP_GRID_BUTTON_HOVERED
    } else if state.enabled {
        MAP_GRID_BUTTON_ENABLED
    } else {
        MAP_GRID_BUTTON_BACKGROUND
    };
    checkmark.0 = if state.enabled { "X" } else { "" }.into();
}

#[must_use]
pub(crate) fn cursor_over_map_controls(cursor: Vec2, window_width: f32) -> bool {
    let left = window_width - MAP_GRID_BUTTON_RIGHT - MAP_GRID_BUTTON_WIDTH;
    let right = window_width - GRID_SNAP_BUTTON_RIGHT;
    cursor.x >= left
        && cursor.x <= right
        && cursor.y >= MAP_GRID_BUTTON_TOP
        && cursor.y <= MAP_GRID_BUTTON_TOP + MAP_GRID_BUTTON_HEIGHT
}

#[must_use]
pub(crate) fn cursor_over_builder_shortcuts(cursor: Vec2, state: &BuilderShortcutState) -> bool {
    let count = state.len();
    if count == 0 {
        return false;
    }
    let height = count as f32 * BUILDER_SHORTCUT_SIZE
        + count.saturating_sub(1) as f32 * BUILDER_SHORTCUT_GAP;
    cursor.x >= BUILDER_SHORTCUT_LEFT
        && cursor.x <= BUILDER_SHORTCUT_LEFT + BUILDER_SHORTCUT_SIZE
        && cursor.y >= BUILDER_SHORTCUT_TOP
        && cursor.y <= BUILDER_SHORTCUT_TOP + height
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

fn update_resource_bar(
    selected_match: Res<SelectedMatch>,
    inspection: Res<InspectionSelection>,
    presentation: Res<PresentationSamples>,
    fps_display: Res<FpsDisplay>,
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
    let resources = economy.resources;

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
    let player_label = format!("PLAYER {} · {relation}", u16::from(selected_player.0) + 1);
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
            let fps = fps_display
                .fps()
                .map_or_else(|| "--".to_owned(), |fps| format!("{fps:.0}"));
            text.0 = format!("FPS {fps}\nTICK {}", presentation.current.tick);
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
    }

    #[test]
    fn builder_shortcuts_filter_by_authoritative_control_and_keep_authored_icon_identity() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 1);
        let profile = BuilderProfile {
            speed_per_tick: 0,
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
                appearance: ContentIdentity { rawcode, name },
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
    fn map_control_hit_box_covers_grid_and_snap_buttons() {
        let window_width = 1_440.0;
        let left = window_width - MAP_GRID_BUTTON_RIGHT - MAP_GRID_BUTTON_WIDTH;
        let right = window_width - GRID_SNAP_BUTTON_RIGHT;
        assert!(cursor_over_map_controls(
            Vec2::new(left + 2.0, MAP_GRID_BUTTON_TOP + 5.0),
            window_width,
        ));
        assert!(cursor_over_map_controls(
            Vec2::new(right - 2.0, MAP_GRID_BUTTON_TOP + 5.0),
            window_width,
        ));
        assert!(!cursor_over_map_controls(
            Vec2::new(left - 1.0, MAP_GRID_BUTTON_TOP + 5.0),
            window_width,
        ));
        assert!(!cursor_over_map_controls(
            Vec2::new(
                (left + right) * 0.5,
                MAP_GRID_BUTTON_TOP + MAP_GRID_BUTTON_HEIGHT + 1.0
            ),
            window_width,
        ));
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
        assert!(cursor_over_builder_shortcuts(
            Vec2::new(BUILDER_SHORTCUT_LEFT + 20.0, BUILDER_SHORTCUT_TOP + 20.0),
            &state
        ));
        assert!(cursor_over_builder_shortcuts(
            Vec2::new(
                BUILDER_SHORTCUT_LEFT + 20.0,
                BUILDER_SHORTCUT_TOP + BUILDER_SHORTCUT_SIZE + BUILDER_SHORTCUT_GAP + 20.0,
            ),
            &state
        ));
        assert!(!cursor_over_builder_shortcuts(
            Vec2::new(
                BUILDER_SHORTCUT_LEFT + BUILDER_SHORTCUT_SIZE + 1.0,
                BUILDER_SHORTCUT_TOP
            ),
            &state
        ));
    }
}
