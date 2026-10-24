//! Settings and controls-reference overlays, reachable from the main menu and the F10 menu.
use bevy::{
    audio::{GlobalVolume, Volume},
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    ui::FocusPolicy,
    window::{MonitorSelection, PrimaryWindow, WindowMode},
};
use castle_fight_protocol::{MAX_PLAYER_NAME_CHARS, sanitize_player_name};

use crate::{
    client_settings::{ClientSettings, MAX_SPEED_MULTIPLIER, MIN_SPEED_MULTIPLIER},
    menu_focus::MenuOverlay,
    view_state::{ViewStatePersistence, toggle_window_mode},
};

const SCRIM: Color = Color::srgba(0.0, 0.0, 0.0, 0.7);
const PANEL: Color = Color::srgb(0.075, 0.095, 0.13);
const PANEL_BORDER: Color = Color::srgb(0.28, 0.36, 0.45);
const BUTTON: Color = Color::srgb(0.13, 0.17, 0.22);
const BUTTON_HOVER: Color = Color::srgb(0.22, 0.29, 0.37);
const BUTTON_BORDER: Color = Color::srgb(0.32, 0.43, 0.52);
const TEXT: Color = Color::srgb(0.92, 0.94, 0.97);
const MUTED: Color = Color::srgb(0.60, 0.68, 0.74);
const ACCENT: Color = Color::srgb(0.45, 0.79, 0.89);

const VOLUME_STEP: f32 = 0.1;
const SPEED_STEP: f32 = 0.25;

pub(crate) struct SettingsMenuPlugin;

impl Plugin for SettingsMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SettingsMenuState>()
            .init_resource::<ClientSettings>()
            .add_message::<KeyboardInput>()
            .add_systems(
                Update,
                (
                    apply_audio_settings,
                    handle_settings_keys.after(crate::game_menu::handle_menu_keys),
                    handle_settings_buttons,
                    redraw_settings,
                )
                    .chain(),
            );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Panel {
    Settings,
    Controls,
}

#[derive(Resource, Default)]
pub(crate) struct SettingsMenuState {
    panel: Option<Panel>,
    drawn: Option<(Panel, bool)>,
    /// The name field has keyboard focus.
    editing_name: bool,
    /// Draw again next frame (a value changed).
    dirty: bool,
}

impl SettingsMenuState {
    pub(crate) fn is_open(&self) -> bool {
        self.panel.is_some()
    }

    pub(crate) fn open(&mut self, panel: Panel) {
        self.panel = Some(panel);
        self.editing_name = false;
    }

    fn close(&mut self) {
        self.panel = None;
        self.editing_name = false;
    }
}

#[derive(Component)]
struct SettingsRoot;

#[derive(Component, Debug, Clone, Copy, PartialEq)]
enum SettingsAction {
    VolumeDelta(f32),
    ToggleFullscreen,
    ToggleEdgeScroll,
    PanDelta(f32),
    ZoomDelta(f32),
    EditName,
    Close,
}

/// Mirrors the saved master volume into Bevy's global volume. Sounds already playing keep their
/// level, which is fine for the short combat sounds the game plays.
fn apply_audio_settings(
    settings: Res<ClientSettings>,
    global_volume: Option<ResMut<GlobalVolume>>,
    mut commands: Commands,
) {
    if !settings.is_changed() {
        return;
    }
    let volume = GlobalVolume::new(Volume::Linear(settings.master_volume));
    match global_volume {
        Some(mut global) => *global = volume,
        None => commands.insert_resource(volume),
    }
}

fn handle_settings_keys(
    mut keyboard: MessageReader<KeyboardInput>,
    mut state: ResMut<SettingsMenuState>,
    mut settings: ResMut<ClientSettings>,
) {
    if !state.is_open() {
        keyboard.clear();
        return;
    }
    for event in keyboard.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        if !state.editing_name {
            if event.logical_key == Key::Escape {
                state.close();
            }
            continue;
        }
        match &event.logical_key {
            Key::Backspace => {
                settings.player_name.pop();
            }
            Key::Enter | Key::Escape => {
                settings.player_name = sanitize_player_name(&settings.player_name);
                settings.save();
                state.editing_name = false;
            }
            Key::Character(characters) => {
                for ch in characters.chars().filter(|ch| !ch.is_control()) {
                    if settings.player_name.chars().count() < MAX_PLAYER_NAME_CHARS {
                        settings.player_name.push(ch);
                    }
                }
            }
            _ => continue,
        }
        state.dirty = true;
    }
}

type SettingsButtonQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        &'static SettingsAction,
        &'static mut BackgroundColor,
    ),
    (Changed<Interaction>, With<Button>),
>;

fn handle_settings_buttons(
    mut buttons: SettingsButtonQuery<'_, '_>,
    mut state: ResMut<SettingsMenuState>,
    mut settings: ResMut<ClientSettings>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    persistence: Option<ResMut<ViewStatePersistence>>,
) {
    let mut chosen = None;
    for (interaction, action, mut background) in &mut buttons {
        match interaction {
            Interaction::Hovered => background.0 = BUTTON_HOVER,
            Interaction::None => background.0 = BUTTON,
            Interaction::Pressed => chosen = Some(*action),
        }
    }
    let Some(action) = chosen else {
        return;
    };
    // Clicking anywhere else ends name editing, keeping what was typed.
    if state.editing_name && action != SettingsAction::EditName {
        settings.player_name = sanitize_player_name(&settings.player_name);
        state.editing_name = false;
    }
    let mut changed = true;
    match action {
        SettingsAction::VolumeDelta(delta) => {
            settings.master_volume = round_to_step(settings.master_volume + delta, 0.0, 1.0);
        }
        SettingsAction::ToggleFullscreen => {
            if let Some(mut persistence) = persistence {
                toggle_window_mode(&mut window, &mut persistence);
            } else {
                window.mode = if window.mode == WindowMode::Windowed {
                    WindowMode::BorderlessFullscreen(MonitorSelection::Current)
                } else {
                    WindowMode::Windowed
                };
            }
            changed = false;
        }
        SettingsAction::ToggleEdgeScroll => settings.edge_scroll = !settings.edge_scroll,
        SettingsAction::PanDelta(delta) => {
            settings.pan_speed = round_to_step(
                settings.pan_speed + delta,
                MIN_SPEED_MULTIPLIER,
                MAX_SPEED_MULTIPLIER,
            );
        }
        SettingsAction::ZoomDelta(delta) => {
            settings.zoom_speed = round_to_step(
                settings.zoom_speed + delta,
                MIN_SPEED_MULTIPLIER,
                MAX_SPEED_MULTIPLIER,
            );
        }
        SettingsAction::EditName => {
            state.editing_name = true;
            changed = false;
        }
        SettingsAction::Close => {
            state.close();
            changed = false;
        }
    }
    if changed {
        settings.save();
    }
    state.dirty = true;
}

/// Keeps stepped values on tidy multiples so repeated clicks never accumulate float error.
fn round_to_step(value: f32, min: f32, max: f32) -> f32 {
    ((value * 100.0).round() / 100.0).clamp(min, max)
}

fn redraw_settings(
    mut commands: Commands,
    mut state: ResMut<SettingsMenuState>,
    settings: Res<ClientSettings>,
    roots: Query<Entity, With<SettingsRoot>>,
    window: Single<&Window, With<PrimaryWindow>>,
) {
    let wanted = state.panel.map(|panel| (panel, state.editing_name));
    if !std::mem::take(&mut state.dirty) && state.drawn == wanted {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    state.drawn = wanted;
    let Some((panel, editing_name)) = wanted else {
        return;
    };
    let fullscreen = window.mode != WindowMode::Windowed;
    commands
        .spawn((
            SettingsRoot,
            MenuOverlay(4),
            Node {
                position_type: PositionType::Absolute,
                left: percent(0.0),
                top: percent(0.0),
                width: percent(100.0),
                height: percent(100.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(SCRIM),
            FocusPolicy::Block,
            GlobalZIndex(31_000),
        ))
        .with_children(|scrim| {
            scrim
                .spawn((
                    Node {
                        width: px(560.0),
                        max_width: percent(94.0),
                        max_height: percent(94.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(px(22.0)),
                        row_gap: px(10.0),
                        border: UiRect::all(px(1.0)),
                        ..default()
                    },
                    BackgroundColor(PANEL),
                    BorderColor::all(PANEL_BORDER),
                    FocusPolicy::Block,
                ))
                .with_children(|body| match panel {
                    Panel::Settings => {
                        spawn_settings_rows(body, &settings, fullscreen, editing_name);
                    }
                    Panel::Controls => spawn_controls(body),
                });
        });
}

fn spawn_settings_rows(
    body: &mut ChildSpawnerCommands,
    settings: &ClientSettings,
    fullscreen: bool,
    editing_name: bool,
) {
    label(body, "SETTINGS", 26.0, TEXT);
    stepper_row(
        body,
        "Volume",
        format!("{:.0}%", settings.master_volume * 100.0),
        SettingsAction::VolumeDelta(-VOLUME_STEP),
        SettingsAction::VolumeDelta(VOLUME_STEP),
    );
    toggle_row(
        body,
        "Fullscreen",
        if fullscreen { "On" } else { "Off" },
        SettingsAction::ToggleFullscreen,
    );
    toggle_row(
        body,
        "Edge scrolling",
        if settings.edge_scroll { "On" } else { "Off" },
        SettingsAction::ToggleEdgeScroll,
    );
    stepper_row(
        body,
        "Camera speed",
        format!("x{:.2}", settings.pan_speed),
        SettingsAction::PanDelta(-SPEED_STEP),
        SettingsAction::PanDelta(SPEED_STEP),
    );
    stepper_row(
        body,
        "Zoom speed",
        format!("x{:.2}", settings.zoom_speed),
        SettingsAction::ZoomDelta(-SPEED_STEP),
        SettingsAction::ZoomDelta(SPEED_STEP),
    );
    let name = if editing_name {
        format!("{}_", settings.player_name)
    } else if settings.player_name.is_empty() {
        format!("{} (account name)", crate::network::default_player_name())
    } else {
        settings.player_name.clone()
    };
    toggle_row(body, "Player name", &name, SettingsAction::EditName);
    label(
        body,
        "Your name is shown to other players. Click it, type, then press Enter.",
        12.0,
        MUTED,
    );
    button(body, "CLOSE", SettingsAction::Close);
}

fn spawn_controls(body: &mut ChildSpawnerCommands) {
    label(body, "CONTROLS", 26.0, TEXT);
    for (heading, lines) in CONTROLS {
        label(body, *heading, 15.0, ACCENT);
        for (keys, action) in *lines {
            body.spawn(Node {
                column_gap: px(12.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Node {
                        width: px(170.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    Text::new(*keys),
                    TextFont::from_font_size(13.0),
                    TextColor(TEXT),
                ));
                row.spawn((
                    Text::new(*action),
                    TextFont::from_font_size(13.0),
                    TextColor(MUTED),
                ));
            });
        }
    }
    button(body, "CLOSE", SettingsAction::Close);
}

type ControlGroup = (&'static str, &'static [(&'static str, &'static str)]);

const CONTROLS: &[ControlGroup] = &[
    (
        "Camera",
        &[
            ("WASD / arrow keys", "Pan"),
            (
                "Pointer at screen edge",
                "Pan (can be turned off in Settings)",
            ),
            ("Middle mouse drag", "Grab and drag the map"),
            ("Mouse wheel", "Zoom"),
            ("Q / E", "Rotate"),
            ("Home", "Reset the camera"),
            ("Backspace", "Jump to your castle"),
            (". (period)", "Jump to the last attacked building"),
        ],
    ),
    (
        "Selection",
        &[
            ("Click / drag a box", "Select"),
            ("Shift+click", "Add or remove from the selection"),
            (
                "Ctrl+click, double-click",
                "Select everything of the same type",
            ),
            (
                "Ctrl+0-9 / 0-9",
                "Save / recall a selection group (twice to centre)",
            ),
            ("Tab / Shift+Tab", "Cycle the type within a mixed selection"),
            ("` (grave)", "Select the next idle builder"),
        ],
    ),
    (
        "Orders",
        &[
            ("Right-click", "Smart order: move, repair, follow or attack"),
            (
                "Command-card hotkeys",
                "Shown on each button (B opens the build menu)",
            ),
            ("Shift", "Queue orders; keep placing the same building"),
            ("Esc", "Cancel the current command"),
        ],
    ),
    (
        "Game",
        &[
            (
                "Enter / Shift+Enter",
                "Chat to allies / to everyone (online games)",
            ),
            ("F10", "Game menu: resume, settings, leave, exit"),
            ("F9", "Scoreboard"),
            ("H", "Show or hide health bars"),
            ("Space / P", "Pause (offline games only)"),
            (
                "Ctrl+S / Ctrl+O",
                "Quicksave / quickload (offline games only)",
            ),
            ("Alt+Enter", "Toggle fullscreen"),
            ("F1 / F8", "Debug overlays / debug menu"),
            (
                "Tab, arrows, Enter",
                "Move between and press buttons in any menu",
            ),
        ],
    ),
];

fn label(parent: &mut ChildSpawnerCommands, value: impl Into<String>, size: f32, color: Color) {
    parent.spawn((
        Text::new(value),
        TextFont::from_font_size(size),
        TextColor(color),
    ));
}

fn button(parent: &mut ChildSpawnerCommands, value: &str, action: SettingsAction) {
    parent
        .spawn((
            Button,
            action,
            Node {
                min_width: px(44.0),
                min_height: px(36.0),
                padding: UiRect::axes(px(14.0), px(6.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(BUTTON),
            BorderColor::all(BUTTON_BORDER),
        ))
        .with_child((
            Text::new(value),
            TextFont::from_font_size(16.0),
            TextColor(TEXT),
        ));
}

fn row(
    parent: &mut ChildSpawnerCommands,
    name: &str,
    controls: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            column_gap: px(12.0),
            ..default()
        })
        .with_children(|row| {
            label(row, name, 17.0, TEXT);
            row.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: px(8.0),
                ..default()
            })
            .with_children(controls);
        });
}

fn stepper_row(
    parent: &mut ChildSpawnerCommands,
    name: &str,
    value: String,
    down: SettingsAction,
    up: SettingsAction,
) {
    row(parent, name, |controls| {
        button(controls, "-", down);
        label(controls, value, 17.0, TEXT);
        button(controls, "+", up);
    });
}

fn toggle_row(parent: &mut ChildSpawnerCommands, name: &str, value: &str, action: SettingsAction) {
    row(parent, name, |controls| button(controls, value, action));
}
