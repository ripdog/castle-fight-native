use bevy::{
    ecs::system::SystemParam,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::{MouseScrollUnit, MouseWheel},
    },
    prelude::*,
    ui::FocusPolicy,
};
use castle_fight_sim::{
    CastleFightBuilderRace, CastleFightContentBundle, DebugStartingBuildings, MatchRules, PlayerId,
    TimeLimitOutcome,
};

use crate::{
    AuthoritativeSimulation, ClientOptions, SelectedMatch, SimulationPlayback,
    bridge::{PresentationSamples, PresentationSnapshot},
    client_match_config, create_demo_world_for_match_config,
    debug_menu::DebugMenuState,
    default_worker_count,
    presentation::CameraFocusRequest,
};

const BACKGROUND: Color = Color::srgba(0.025, 0.035, 0.055, 0.99);
const PANEL: Color = Color::srgb(0.075, 0.095, 0.13);
const CARD: Color = Color::srgb(0.095, 0.12, 0.16);
const CARD_BORDER: Color = Color::srgb(0.20, 0.26, 0.33);
const BUTTON: Color = Color::srgb(0.13, 0.17, 0.22);
const BUTTON_HOVER: Color = Color::srgb(0.22, 0.29, 0.37);
const BUTTON_SELECTED: Color = Color::srgb(0.22, 0.36, 0.46);
const BUTTON_DISABLED: Color = Color::srgb(0.10, 0.13, 0.17);
const BUTTON_BORDER: Color = Color::srgb(0.32, 0.43, 0.52);
const BUTTON_BORDER_DISABLED: Color = Color::srgb(0.19, 0.23, 0.27);
const MODAL_SCRIM: Color = Color::srgba(0.0, 0.0, 0.0, 0.72);
const TEXT: Color = Color::srgb(0.92, 0.94, 0.97);
const MUTED: Color = Color::srgb(0.60, 0.68, 0.74);
const ACCENT: Color = Color::srgb(0.45, 0.79, 0.89);
const ERROR: Color = Color::srgb(1.0, 0.45, 0.4);

const FONT_TITLE: f32 = 40.0;
const FONT_HEADING: f32 = 22.0;
const FONT_BODY: f32 = 18.0;
const FONT_LARGE: f32 = 24.0;

const MAX_TIME_LIMIT_MINUTES: u16 = 999;
const TIME_LIMIT_DIGITS: usize = 3;
const SCROLL_LINE_PIXELS: f32 = 56.0;
const RACE_PICKER_COLUMNS: u16 = 4;

pub(crate) struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<KeyboardInput>()
            .add_message::<MouseWheel>()
            .add_systems(
                Update,
                (
                    ensure_lobby_visible,
                    handle_time_limit_keys,
                    scroll_lobby,
                    handle_lobby_buttons,
                )
                    .chain(),
            );
    }
}

#[derive(Resource)]
pub(crate) struct LobbyState {
    options: ClientOptions,
    team_size: usize,
    local_player: PlayerId,
    networked: bool,
    connected_players: Vec<PlayerId>,
    host_player: Option<PlayerId>,
    required_players: usize,
    open_race: Option<u8>,
    /// Digits typed into the time-limit field while it has keyboard focus.
    time_input: Option<String>,
    scroll_y: f32,
    redraw_requested: bool,
    active: bool,
    error: Option<String>,
}

impl LobbyState {
    pub(crate) fn new(options: ClientOptions, local_player: PlayerId, networked: bool) -> Self {
        Self {
            team_size: options.team_size,
            local_player,
            required_players: options.team_size * 2,
            connected_players: vec![local_player],
            host_player: None,
            options,
            networked,
            open_race: None,
            time_input: None,
            scroll_y: 0.0,
            redraw_requested: false,
            active: true,
            error: None,
        }
    }

    fn local_player(&self) -> PlayerId {
        self.local_player
    }

    fn set_team_size(&mut self, size: usize) {
        self.team_size = size;
        let side_offset = if self.local_player.0 < 6 { 0 } else { 6 };
        if usize::from(self.local_player.0 - side_offset) >= size {
            self.local_player = PlayerId(side_offset);
        }
    }

    fn content(&self) -> &'static CastleFightContentBundle {
        castle_fight_sim::castle_fight_content_bundle(self.options.map_version)
            .expect("lobby content was resolved before display")
    }

    fn race(&self, player: u8) -> CastleFightBuilderRace {
        match self.options.builder_rawcodes[usize::from(player)] {
            Some(rawcode) => self
                .content()
                .builder_race_for_rawcode(rawcode)
                .expect("selected rawcode was validated before lobby display"),
            None => CastleFightBuilderRace::Human,
        }
    }

    fn selected(&self, action: LobbyAction) -> bool {
        match action {
            LobbyAction::TeamSize(size) => size == self.team_size,
            LobbyAction::Position(player) => player == self.local_player,
            LobbyAction::Race(player, race) => self.race(player) == race,
            LobbyAction::Start => true,
            LobbyAction::RoundTarget(wins) => self.options.rules.rounds_to_win == wins,
            LobbyAction::RescueStrikes(enabled) => {
                self.options.rules.rescue_strikes_enabled == enabled
            }
            LobbyAction::DebugStartingBuildings(mode) => {
                self.options.rules.debug_starting_buildings == mode
            }
            LobbyAction::TimeOutcome(mode) => self.options.rules.time_limit_outcome == mode,
            LobbyAction::TimeInput => self.time_input.is_some(),
            LobbyAction::TimeDelta(_)
            | LobbyAction::RaceDropdown(_)
            | LobbyAction::CloseRaceMenu
            | LobbyAction::DismissRaceMenu => false,
        }
    }

    fn player_label(&self, player: PlayerId) -> String {
        if player != self.local_player && !self.connected_players.contains(&player) {
            return "Empty".to_owned();
        }
        let number = if player.0 < 6 {
            usize::from(player.0) + 1
        } else {
            usize::from(player.0 - 6) + self.team_size + 1
        };
        if player == self.local_player {
            format!("Player {number} (You)")
        } else {
            format!("Player {number}")
        }
    }

    fn action_disabled(&self, action: LobbyAction) -> bool {
        if !self.networked {
            return false;
        }
        match action {
            LobbyAction::TeamSize(_) | LobbyAction::Position(_) => true,
            LobbyAction::TimeDelta(_)
            | LobbyAction::TimeInput
            | LobbyAction::RoundTarget(_)
            | LobbyAction::RescueStrikes(_)
            | LobbyAction::DebugStartingBuildings(_)
            | LobbyAction::TimeOutcome(_) => self.host_player != Some(self.local_player),
            LobbyAction::Start => {
                self.host_player != Some(self.local_player)
                    || self.connected_players.len() < self.required_players
            }
            LobbyAction::RaceDropdown(player) | LobbyAction::Race(player, _) => {
                player != self.local_player.0
            }
            LobbyAction::CloseRaceMenu | LobbyAction::DismissRaceMenu => false,
        }
    }

    pub(crate) fn active(&self) -> bool {
        self.active
    }

    /// Text shown in the time-limit field: the pending keystrokes with a caret
    /// while editing, otherwise the committed value.
    fn time_limit_text(&self) -> String {
        match &self.time_input {
            Some(buffer) => format!("{buffer}|"),
            None => match self.options.rules.time_limit_minutes {
                0 => "Off".to_owned(),
                minutes => format!("{minutes} min"),
            },
        }
    }
}

#[derive(Component)]
struct LobbyRoot;

/// The scrollable region between the fixed header and footer.
#[derive(Component)]
struct LobbyScroll;

#[derive(Component, Clone, Copy)]
enum LobbyAction {
    TeamSize(usize),
    Position(PlayerId),
    RaceDropdown(u8),
    Race(u8, CastleFightBuilderRace),
    /// Cancel button of the race picker.
    CloseRaceMenu,
    /// Click on the dimmed area around the race picker; carries no button styling.
    DismissRaceMenu,
    TimeDelta(i16),
    TimeInput,
    RoundTarget(u8),
    RescueStrikes(bool),
    DebugStartingBuildings(DebugStartingBuildings),
    TimeOutcome(TimeLimitOutcome),
    Start,
}

/// Applies `rules` locally, or requests them from the host when networked.
fn apply_rules(
    lobby: &mut LobbyState,
    authoritative: &mut AuthoritativeSimulation,
    rules: MatchRules,
) {
    if lobby.networked {
        if let Err(error) = authoritative.request_network_match_rules(rules) {
            lobby.error = Some(error);
        }
    } else {
        lobby.options.rules = rules;
    }
}

/// Ends time-limit editing, applying the typed value. An empty field keeps the
/// current limit.
fn commit_time_input(lobby: &mut LobbyState, authoritative: &mut AuthoritativeSimulation) {
    let Some(buffer) = lobby.time_input.take() else {
        return;
    };
    if let Ok(minutes) = buffer.parse::<u16>() {
        let mut rules = lobby.options.rules;
        rules.time_limit_minutes = minutes.min(MAX_TIME_LIMIT_MINUTES);
        apply_rules(lobby, authoritative, rules);
    }
    lobby.redraw_requested = true;
}

fn handle_time_limit_keys(
    mut keyboard: MessageReader<KeyboardInput>,
    lobby: Option<ResMut<LobbyState>>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
) {
    let Some(mut lobby) = lobby else {
        return;
    };
    if !lobby.active || lobby.time_input.is_none() {
        keyboard.clear();
        return;
    }
    for event in keyboard.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        let Some(buffer) = lobby.time_input.as_mut() else {
            break;
        };
        match &event.logical_key {
            Key::Backspace => {
                buffer.pop();
            }
            Key::Escape => lobby.time_input = None,
            Key::Enter => commit_time_input(&mut lobby, &mut authoritative),
            Key::Character(characters) => {
                for digit in characters.chars().filter(char::is_ascii_digit) {
                    if buffer.len() < TIME_LIMIT_DIGITS {
                        buffer.push(digit);
                    }
                }
            }
            _ => continue,
        }
        lobby.redraw_requested = true;
    }
}

fn scroll_lobby(
    mut wheel: MessageReader<MouseWheel>,
    lobby: Option<ResMut<LobbyState>>,
    mut areas: Query<(&mut ScrollPosition, &ComputedNode), With<LobbyScroll>>,
) {
    let Some(mut lobby) = lobby else {
        return;
    };
    let Ok((mut position, computed)) = areas.single_mut() else {
        wheel.clear();
        return;
    };
    let delta: f32 = wheel
        .read()
        .map(|event| match event.unit {
            MouseScrollUnit::Line => event.y * SCROLL_LINE_PIXELS,
            MouseScrollUnit::Pixel => event.y,
        })
        .sum();
    let max_scroll =
        ((computed.content_size.y - computed.size.y) * computed.inverse_scale_factor).max(0.0);
    let scroll_y = (position.y - delta).clamp(0.0, max_scroll);
    position.y = scroll_y;
    lobby.scroll_y = scroll_y;
}

fn ensure_lobby_visible(
    mut commands: Commands,
    lobby: Option<Res<LobbyState>>,
    roots: Query<Entity, With<LobbyRoot>>,
) {
    let Some(lobby) = lobby else {
        return;
    };
    if lobby.active && roots.is_empty() {
        spawn_lobby(&mut commands, &lobby);
    }
}

fn card_node(flex_basis: f32, flex_grow: f32, padding: f32, row_gap: f32) -> Node {
    Node {
        flex_basis: px(flex_basis),
        flex_grow,
        min_width: px(0.0),
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(px(padding)),
        row_gap: px(row_gap),
        border: UiRect::all(px(1.0)),
        border_radius: BorderRadius::all(px(8.0)),
        ..default()
    }
}

fn spawn_lobby(commands: &mut Commands, lobby: &LobbyState) {
    commands
        .spawn((
            LobbyRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100.0),
                height: percent(100.0),
                padding: UiRect::all(px(24.0)),
                ..default()
            },
            BackgroundColor(BACKGROUND),
            ZIndex(10_000),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: percent(100.0),
                    height: percent(100.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(32.0)),
                    row_gap: px(24.0),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(10.0)),
                    ..default()
                },
                BackgroundColor(PANEL),
                BorderColor::all(Color::srgb(0.28, 0.36, 0.45)),
            ))
            .with_children(|panel| {
                spawn_header(panel, lobby);
                panel
                    .spawn((
                        LobbyScroll,
                        Node {
                            flex_grow: 1.0,
                            flex_basis: px(0.0),
                            min_height: px(0.0),
                            flex_wrap: FlexWrap::Wrap,
                            align_content: AlignContent::FlexStart,
                            column_gap: px(28.0),
                            row_gap: px(28.0),
                            overflow: Overflow::scroll_y(),
                            ..default()
                        },
                        ScrollPosition(Vec2::new(0.0, lobby.scroll_y)),
                    ))
                    .with_children(|content| {
                        spawn_settings(content, lobby);
                        spawn_teams(content, lobby);
                    });
                spawn_footer(panel, lobby);
            });
            if let Some(player) = lobby.open_race {
                spawn_race_picker(root, lobby, player);
            }
        });
}

fn spawn_header(panel: &mut ChildSpawnerCommands, lobby: &LobbyState) {
    panel
        .spawn(Node {
            width: percent(100.0),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(24.0),
            row_gap: px(16.0),
            ..default()
        })
        .with_children(|header| {
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6.0),
                    ..default()
                })
                .with_children(|title| {
                    label(title, "Castle Fight", FONT_TITLE, TEXT);
                    label(
                        title,
                        format!(
                            "All Pick  |  Map {} / {}",
                            lobby.options.map_version, lobby.options.release_revision
                        ),
                        FONT_BODY,
                        MUTED,
                    );
                });
            header
                .spawn(Node {
                    align_items: AlignItems::Center,
                    column_gap: px(8.0),
                    ..default()
                })
                .with_children(|sizes| {
                    spawn_label_with_margin(sizes, "Players per team", FONT_BODY, MUTED);
                    for size in 1..=3 {
                        styled_button(
                            sizes,
                            format!("{size}v{size}"),
                            LobbyAction::TeamSize(size),
                            size == lobby.team_size,
                            lobby.networked,
                            ButtonStyle::fixed(96.0),
                        );
                    }
                });
        });
}

fn spawn_label_with_margin(
    parent: &mut ChildSpawnerCommands,
    value: &str,
    size: f32,
    color: Color,
) {
    parent.spawn((
        Text::new(value),
        TextFont::from_font_size(size),
        TextColor(color),
        Node {
            margin: UiRect::right(px(8.0)),
            ..default()
        },
    ));
}

/// A titled group of controls inside the settings card.
fn setting_block(
    parent: &mut ChildSpawnerCommands,
    title: &str,
    direction: FlexDirection,
    body: impl FnOnce(&mut ChildSpawnerCommands),
) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(10.0),
            ..default()
        })
        .with_children(|block| {
            label(block, title, FONT_BODY, MUTED);
            block
                .spawn(Node {
                    flex_direction: direction,
                    column_gap: px(8.0),
                    row_gap: px(8.0),
                    ..default()
                })
                .with_children(body);
        });
}

fn spawn_settings(content: &mut ChildSpawnerCommands, lobby: &LobbyState) {
    content
        .spawn((
            card_node(440.0, 1.0, 24.0, 26.0),
            BackgroundColor(CARD),
            BorderColor::all(CARD_BORDER),
        ))
        .with_children(|card| {
            label(card, "MATCH SETTINGS", FONT_HEADING, ACCENT);
            setting_block(
                card,
                "Time limit in minutes (click the box and type; 0 = off)",
                FlexDirection::Row,
                |row| {
                    for (caption, delta) in [("-10", -10), ("-1", -1)] {
                        spawn_time_delta(row, lobby, caption, delta);
                    }
                    let input = LobbyAction::TimeInput;
                    styled_button(
                        row,
                        lobby.time_limit_text(),
                        input,
                        lobby.selected(input),
                        lobby.action_disabled(input),
                        ButtonStyle::row().grow(2.0).font_size(FONT_LARGE),
                    );
                    for (caption, delta) in [("+1", 1), ("+10", 10)] {
                        spawn_time_delta(row, lobby, caption, delta);
                    }
                },
            );
            setting_block(card, "At time limit", FlexDirection::Column, |column| {
                for (caption, outcome) in [
                    (
                        "More strikes wins, then income",
                        TimeLimitOutcome::MoreStrikesThenIncome,
                    ),
                    ("Draw", TimeLimitOutcome::Draw),
                ] {
                    let action = LobbyAction::TimeOutcome(outcome);
                    button(
                        column,
                        caption,
                        action,
                        lobby.selected(action),
                        lobby.action_disabled(action),
                    );
                }
            });
            setting_block(card, "First to", FlexDirection::Row, |row| {
                for wins in 1..=3 {
                    let action = LobbyAction::RoundTarget(wins);
                    row_button(
                        row,
                        format!("{wins} round{}", if wins == 1 { "" } else { "s" }),
                        action,
                        lobby.selected(action),
                        lobby.action_disabled(action),
                    );
                }
            });
            setting_block(card, "Rescue strikes", FlexDirection::Row, |row| {
                for (caption, enabled) in [("On", true), ("Off", false)] {
                    let action = LobbyAction::RescueStrikes(enabled);
                    row_button(
                        row,
                        caption,
                        action,
                        lobby.selected(action),
                        lobby.action_disabled(action),
                    );
                }
            });
            setting_block(
                card,
                "Starting buildings (debug; click again to turn off)",
                FlexDirection::Row,
                |row| {
                    for (caption, mode) in [
                        ("Race buildings", DebugStartingBuildings::OwnRace),
                        ("All buildings", DebugStartingBuildings::All),
                    ] {
                        let action = LobbyAction::DebugStartingBuildings(mode);
                        row_button(
                            row,
                            caption,
                            action,
                            lobby.selected(action),
                            lobby.action_disabled(action),
                        );
                    }
                },
            );
        });
}

fn spawn_time_delta(row: &mut ChildSpawnerCommands, lobby: &LobbyState, caption: &str, delta: i16) {
    let action = LobbyAction::TimeDelta(delta);
    row_button(row, caption, action, false, lobby.action_disabled(action));
}

fn spawn_teams(content: &mut ChildSpawnerCommands, lobby: &LobbyState) {
    content
        .spawn(Node {
            flex_basis: px(640.0),
            flex_grow: 3.0,
            min_width: px(0.0),
            flex_wrap: FlexWrap::Wrap,
            align_content: AlignContent::FlexStart,
            column_gap: px(24.0),
            row_gap: px(24.0),
            ..default()
        })
        .with_children(|sides| {
            for team in 0..=1 {
                sides
                    .spawn((
                        card_node(300.0, 1.0, 24.0, 18.0),
                        BackgroundColor(CARD),
                        BorderColor::all(CARD_BORDER),
                    ))
                    .with_children(|column| {
                        label(
                            column,
                            if team == 0 { "WEST TEAM" } else { "EAST TEAM" },
                            FONT_HEADING,
                            ACCENT,
                        );
                        for slot in 0..lobby.team_size {
                            let player = PlayerId(slot as u8 + if team == 0 { 0 } else { 6 });
                            spawn_position_row(column, lobby, player, slot);
                        }
                    });
            }
        });
}

fn spawn_footer(panel: &mut ChildSpawnerCommands, lobby: &LobbyState) {
    panel
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: px(12.0),
            ..default()
        })
        .with_children(|footer| {
            if lobby.networked {
                label(
                    footer,
                    format!(
                        "{} / {} players connected  |  Choose your race",
                        lobby.connected_players.len(),
                        lobby.required_players
                    ),
                    FONT_BODY,
                    MUTED,
                );
            } else {
                label(
                    footer,
                    "Choose your position and each player's race. You control all players.",
                    FONT_BODY,
                    MUTED,
                );
            }
            if let Some(error) = &lobby.error {
                label(footer, error.clone(), FONT_BODY, ERROR);
            }
            let start_disabled = lobby.action_disabled(LobbyAction::Start);
            let start_label = if lobby.networked && lobby.host_player != Some(lobby.local_player())
            {
                "Waiting for host"
            } else if start_disabled {
                "Waiting for players"
            } else {
                "Start match"
            };
            styled_button(
                footer,
                start_label,
                LobbyAction::Start,
                true,
                start_disabled,
                ButtonStyle::column().font_size(FONT_LARGE).min_height(64.0),
            );
        });
}

fn spawn_position_row(
    parent: &mut ChildSpawnerCommands,
    lobby: &LobbyState,
    player: PlayerId,
    slot: usize,
) {
    parent
        .spawn(Node {
            width: percent(100.0),
            min_width: px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: px(8.0),
            ..default()
        })
        .with_children(|row| {
            if lobby.networked {
                row.spawn(Node {
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::SpaceBetween,
                    column_gap: px(8.0),
                    ..default()
                })
                .with_children(|identity| {
                    label(
                        identity,
                        lobby.player_label(player),
                        FONT_BODY,
                        if player == lobby.local_player() {
                            TEXT
                        } else {
                            MUTED
                        },
                    );
                    if lobby.host_player == Some(player) {
                        label(identity, "Host", FONT_BODY, ACCENT);
                    }
                });
            } else {
                button(
                    row,
                    format!("Slot {} / {}", slot + 1, lobby.player_label(player)),
                    LobbyAction::Position(player),
                    player == lobby.local_player(),
                    false,
                );
            }
            let builder = lobby
                .content()
                .builder(lobby.race(player.0))
                .expect("selected builder");
            let action = LobbyAction::RaceDropdown(player.0);
            let disabled = lobby.action_disabled(action);
            let name = builder.name.trim_end_matches(" Builder");
            button(
                row,
                if disabled {
                    format!("Race: {name}")
                } else {
                    format!("Race: {name}   v")
                },
                action,
                lobby.open_race == Some(player.0),
                disabled,
            );
        });
}

/// Centered modal listing every supported race. Always fits on screen, unlike an
/// anchored dropdown, and clicking the dimmed area dismisses it.
fn spawn_race_picker(root: &mut ChildSpawnerCommands, lobby: &LobbyState, player: u8) {
    root.spawn((
        Button,
        LobbyAction::DismissRaceMenu,
        Node {
            position_type: PositionType::Absolute,
            left: px(0.0),
            top: px(0.0),
            width: percent(100.0),
            height: percent(100.0),
            padding: UiRect::all(px(24.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            ..default()
        },
        BackgroundColor(MODAL_SCRIM),
    ))
    .with_children(|scrim| {
        scrim
            .spawn((
                FocusPolicy::Block,
                Node {
                    width: percent(100.0),
                    max_width: px(960.0),
                    max_height: percent(100.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(28.0)),
                    row_gap: px(20.0),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(10.0)),
                    ..default()
                },
                BackgroundColor(PANEL),
                BorderColor::all(BUTTON_BORDER),
            ))
            .with_children(|menu| {
                label(
                    menu,
                    format!("Choose race for {}", lobby.player_label(PlayerId(player))),
                    FONT_HEADING,
                    TEXT,
                );
                menu.spawn(Node {
                    display: Display::Grid,
                    width: percent(100.0),
                    grid_template_columns: RepeatedGridTrack::flex(RACE_PICKER_COLUMNS, 1.0),
                    column_gap: px(12.0),
                    row_gap: px(12.0),
                    ..default()
                })
                .with_children(|grid| {
                    for &race in lobby.content().supported_builder_races() {
                        let builder = lobby.content().builder(race).expect("supported builder");
                        let action = LobbyAction::Race(player, race);
                        button(
                            grid,
                            builder.name.trim_end_matches(" Builder"),
                            action,
                            lobby.race(player) == race,
                            lobby.action_disabled(action),
                        );
                    }
                });
                button(menu, "Cancel", LobbyAction::CloseRaceMenu, false, false);
            });
    });
}

fn label(parent: &mut ChildSpawnerCommands, value: impl Into<String>, size: f32, color: Color) {
    parent.spawn((
        Text::new(value),
        TextFont::from_font_size(size),
        TextColor(color),
    ));
}

fn button_color(action: LobbyAction, selected: bool, disabled: bool) -> Color {
    if disabled && !matches!(action, LobbyAction::TeamSize(_)) {
        BUTTON_DISABLED
    } else if selected {
        BUTTON_SELECTED
    } else {
        BUTTON
    }
}

#[derive(Clone, Copy)]
enum ButtonWidth {
    /// Fills the width of a column container.
    Full,
    /// Shares the width of a row container with its siblings.
    Share,
    Fixed(f32),
}

#[derive(Clone, Copy)]
struct ButtonStyle {
    width: ButtonWidth,
    grow: f32,
    font_size: f32,
    min_height: f32,
}

impl ButtonStyle {
    const fn new(width: ButtonWidth) -> Self {
        Self {
            width,
            grow: 1.0,
            font_size: FONT_BODY,
            min_height: 52.0,
        }
    }

    const fn column() -> Self {
        Self::new(ButtonWidth::Full)
    }

    const fn row() -> Self {
        Self::new(ButtonWidth::Share)
    }

    const fn fixed(width: f32) -> Self {
        Self::new(ButtonWidth::Fixed(width))
    }

    const fn grow(mut self, grow: f32) -> Self {
        self.grow = grow;
        self
    }

    const fn font_size(mut self, font_size: f32) -> Self {
        self.font_size = font_size;
        self
    }

    const fn min_height(mut self, min_height: f32) -> Self {
        self.min_height = min_height;
        self
    }
}

fn button(
    parent: &mut ChildSpawnerCommands,
    value: impl Into<String>,
    action: LobbyAction,
    selected: bool,
    disabled: bool,
) {
    styled_button(
        parent,
        value,
        action,
        selected,
        disabled,
        ButtonStyle::column(),
    );
}

fn row_button(
    parent: &mut ChildSpawnerCommands,
    value: impl Into<String>,
    action: LobbyAction,
    selected: bool,
    disabled: bool,
) {
    styled_button(
        parent,
        value,
        action,
        selected,
        disabled,
        ButtonStyle::row(),
    );
}

fn styled_button(
    parent: &mut ChildSpawnerCommands,
    value: impl Into<String>,
    action: LobbyAction,
    selected: bool,
    disabled: bool,
    style: ButtonStyle,
) {
    let (width, flex_grow, flex_basis) = match style.width {
        ButtonWidth::Full => (percent(100.0), 0.0, Val::Auto),
        ButtonWidth::Share => (Val::Auto, style.grow, px(0.0)),
        ButtonWidth::Fixed(width) => (px(width), 0.0, Val::Auto),
    };
    let editing_time = matches!(action, LobbyAction::TimeInput) && selected;
    parent
        .spawn((
            Button,
            action,
            Node {
                width,
                flex_grow,
                flex_basis,
                min_width: px(0.0),
                min_height: px(style.min_height),
                padding: UiRect::axes(px(12.0), px(8.0)),
                border_radius: BorderRadius::all(px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(button_color(action, selected, disabled)),
            BorderColor::all(if disabled {
                BUTTON_BORDER_DISABLED
            } else if editing_time {
                ACCENT
            } else {
                BUTTON_BORDER
            }),
        ))
        .with_child((
            Text::new(value),
            TextFont::from_font_size(style.font_size),
            TextLayout::justify(Justify::Center),
            TextColor(if disabled { MUTED } else { TEXT }),
        ));
}

type LobbyButtonQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        &'static LobbyAction,
        &'static mut BackgroundColor,
    ),
    (Changed<Interaction>, With<Button>),
>;

#[derive(SystemParam)]
struct LobbyGame<'w> {
    authoritative: ResMut<'w, AuthoritativeSimulation>,
    presentation: ResMut<'w, PresentationSamples>,
    selected_match: ResMut<'w, SelectedMatch>,
    playback: ResMut<'w, SimulationPlayback>,
    camera_focus: Option<ResMut<'w, CameraFocusRequest>>,
    debug_menu: ResMut<'w, DebugMenuState>,
}

impl LobbyGame<'_> {
    fn focus_local_builder(&mut self) {
        if let Some(camera_focus) = self.camera_focus.as_deref_mut() {
            camera_focus.0 = self
                .authoritative
                .simulation
                .builder_for_player(self.selected_match.local_player)
                .map(|builder| builder.id);
        }
    }
}

fn handle_lobby_buttons(
    mut commands: Commands,
    lobby: Option<ResMut<LobbyState>>,
    mut buttons: LobbyButtonQuery<'_, '_>,
    roots: Query<Entity, With<LobbyRoot>>,
    mut game: LobbyGame<'_>,
) {
    let Some(mut lobby) = lobby else {
        return;
    };
    if !lobby.active {
        return;
    }
    let Ok(root) = roots.single() else {
        return;
    };
    let mut redraw = std::mem::take(&mut lobby.redraw_requested);
    if lobby.networked {
        if let Some(status) = game.authoritative.network_lobby_status.clone() {
            for participant in &status.participants {
                let selection =
                    &mut lobby.options.builder_rawcodes[usize::from(participant.player_id)];
                if *selection != Some(participant.builder_rawcode) {
                    *selection = Some(participant.builder_rawcode);
                    redraw = true;
                }
            }
            if lobby.options.rules != status.rules {
                lobby.options.rules = status.rules;
                redraw = true;
            }
            let connected_players = status
                .connected_player_ids
                .into_iter()
                .map(PlayerId)
                .collect::<Vec<_>>();
            let host_player = Some(PlayerId(status.host_player_id));
            let required_players = usize::from(status.required_players);
            if lobby.connected_players != connected_players
                || lobby.host_player != host_player
                || lobby.required_players != required_players
            {
                lobby.connected_players = connected_players;
                lobby.host_player = host_player;
                lobby.required_players = required_players;
                redraw = true;
            }
            if status.started {
                game.focus_local_builder();
                lobby.active = false;
                game.authoritative.commands_enabled = true;
                game.playback.paused = false;
                commands.entity(root).despawn();
                return;
            }
        }
        if lobby.error != game.authoritative.network_lobby_error {
            lobby.error = game.authoritative.network_lobby_error.clone();
            redraw = true;
        }
    }
    for (interaction, action, mut background) in &mut buttons {
        // The scrim behind the race picker is a click target, not a visible button.
        if matches!(action, LobbyAction::DismissRaceMenu) {
            if *interaction == Interaction::Pressed {
                lobby.open_race = None;
                redraw = true;
            }
            continue;
        }
        let disabled = lobby.action_disabled(*action);
        match interaction {
            Interaction::Hovered => {
                if !disabled {
                    background.0 = BUTTON_HOVER;
                }
            }
            Interaction::None => {
                background.0 = button_color(*action, lobby.selected(*action), disabled);
            }
            Interaction::Pressed if disabled => {}
            Interaction::Pressed => {
                // Clicking away from the time-limit field applies what was typed.
                if lobby.time_input.is_some() && !matches!(action, LobbyAction::TimeInput) {
                    commit_time_input(&mut lobby, &mut game.authoritative);
                    redraw = true;
                }
                match *action {
                    LobbyAction::TeamSize(size) if !lobby.networked => {
                        lobby.set_team_size(size);
                        redraw = true;
                    }
                    LobbyAction::Position(player) if !lobby.networked => {
                        lobby.local_player = player;
                        redraw = true;
                    }
                    LobbyAction::TimeInput => {
                        if lobby.time_input.is_none() {
                            lobby.time_input = Some(String::new());
                            redraw = true;
                        }
                    }
                    LobbyAction::TimeDelta(delta) => {
                        let current = i32::from(lobby.options.rules.time_limit_minutes);
                        let mut rules = lobby.options.rules;
                        rules.time_limit_minutes = (current + i32::from(delta))
                            .clamp(0, i32::from(MAX_TIME_LIMIT_MINUTES))
                            as u16;
                        apply_rules(&mut lobby, &mut game.authoritative, rules);
                        redraw = true;
                    }
                    LobbyAction::RoundTarget(wins) => {
                        let mut rules = lobby.options.rules;
                        rules.rounds_to_win = wins;
                        apply_rules(&mut lobby, &mut game.authoritative, rules);
                        redraw = true;
                    }
                    LobbyAction::RescueStrikes(enabled) => {
                        let mut rules = lobby.options.rules;
                        rules.rescue_strikes_enabled = enabled;
                        apply_rules(&mut lobby, &mut game.authoritative, rules);
                        redraw = true;
                    }
                    LobbyAction::DebugStartingBuildings(mode) => {
                        let mut rules = lobby.options.rules;
                        // Selecting the active mode again turns the developer option off.
                        rules.debug_starting_buildings = if rules.debug_starting_buildings == mode {
                            DebugStartingBuildings::Off
                        } else {
                            mode
                        };
                        apply_rules(&mut lobby, &mut game.authoritative, rules);
                        redraw = true;
                    }
                    LobbyAction::TimeOutcome(outcome) => {
                        let mut rules = lobby.options.rules;
                        rules.time_limit_outcome = outcome;
                        apply_rules(&mut lobby, &mut game.authoritative, rules);
                        redraw = true;
                    }
                    LobbyAction::RaceDropdown(player) => {
                        lobby.open_race = Some(player);
                        redraw = true;
                    }
                    LobbyAction::CloseRaceMenu => {
                        lobby.open_race = None;
                        redraw = true;
                    }
                    LobbyAction::Race(player, race) => {
                        if lobby.content().supports_builder_race(race) {
                            let rawcode = lobby
                                .content()
                                .builder(race)
                                .expect("supported builder")
                                .rawcode;
                            if lobby.networked {
                                if let Err(error) = game.authoritative.request_network_race(rawcode)
                                {
                                    lobby.error = Some(error);
                                }
                            } else {
                                lobby.options.builder_rawcodes[usize::from(player)] = Some(rawcode);
                            }
                            lobby.open_race = None;
                            redraw = true;
                        }
                    }
                    LobbyAction::Start => {
                        if lobby.networked {
                            if let Err(error) = game.authoritative.request_network_start() {
                                lobby.error = Some(error);
                                redraw = true;
                            }
                            continue;
                        }
                        lobby.options.team_size = lobby.team_size;
                        let result = client_match_config(&lobby.options).and_then(|config| {
                            create_demo_world_for_match_config(default_worker_count(), None, config)
                        });
                        match result {
                            Ok(demo) => {
                                game.debug_menu.set_single_player(true);
                                *game.authoritative =
                                    AuthoritativeSimulation::new(demo.simulation, demo.content);
                                *game.presentation = PresentationSamples::new(
                                    PresentationSnapshot::capture(&game.authoritative.simulation),
                                )
                                .with_player_control(
                                    game.authoritative
                                        .simulation
                                        .player(lobby.local_player())
                                        .map(|player| player.team),
                                    true,
                                );
                                game.selected_match.content = demo.content;
                                game.selected_match.direct_buildings = demo.direct_buildings;
                                game.selected_match.local_player = lobby.local_player();
                                game.focus_local_builder();
                            }
                            Err(error) => {
                                lobby.error = Some(error.to_string());
                                redraw = true;
                                continue;
                            }
                        }
                        lobby.active = false;
                        game.authoritative.commands_enabled = true;
                        game.playback.paused = false;
                        commands.entity(root).despawn();
                        return;
                    }
                    _ => {}
                }
            }
        }
    }
    if redraw {
        commands.entity(root).despawn();
        spawn_lobby(&mut commands, &lobby);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use castle_fight_sim::MapVersion;
    use std::time::Duration;

    fn options() -> ClientOptions {
        ClientOptions {
            stress_units: None,
            stress_visual: None,
            health_bars: true,
            perf_log: false,
            profile_quicksave: None,
            profile_screenshot: None,
            profile_warmup: Duration::ZERO,
            profile_duration: Duration::from_secs(1),
            profile_paused: false,
            profile: false,
            benchmark: false,
            render_experiment: crate::render_audit::RenderExperiment::Baseline,
            map_version: MapVersion::CASTLE_FIGHT_9_27,
            release_revision: "r1".to_owned(),
            match_seed: 1,
            rules: castle_fight_sim::MatchRules::for_version(MapVersion::CASTLE_FIGHT_9_27),
            team_size: 1,
            builder_rawcodes: [None; 9],
            server: None,
            list_map_versions: false,
        }
    }

    #[test]
    fn source_builder_selection_obeys_promotion_gate_without_human_fallback() {
        let mut options = options();
        let content = castle_fight_sim::castle_fight_content_bundle(options.map_version).unwrap();
        for builder in content.builder_definitions() {
            options.builder_rawcodes[6] = Some(builder.rawcode);
            let result = client_match_config(&options);
            if content.supports_builder_race(builder.race) {
                assert_eq!(result.unwrap().participants[1].builder_race, builder.race);
            } else {
                assert!(matches!(result,
                    Err(crate::CastleFightMatchSetupError::UnsupportedBuilderRace(race))
                    if race == builder.race));
            }
        }
        options.builder_rawcodes[6] = Some(0);
        assert!(client_match_config(&options).is_err());
        let network = LobbyState::new(options, PlayerId(0), true);
        assert!(!network.action_disabled(LobbyAction::RaceDropdown(0)));
        assert!(!network.action_disabled(LobbyAction::Race(0, CastleFightBuilderRace::Elf)));
        assert!(network.action_disabled(LobbyAction::RaceDropdown(6)));
        assert!(network.action_disabled(LobbyAction::Race(6, CastleFightBuilderRace::Elf)));
    }

    #[test]
    fn position_choice_uses_authored_slot_and_network_choice_preserves_assignment() {
        let mut local = LobbyState::new(options(), PlayerId(0), false);
        local.local_player = PlayerId(7);
        assert_eq!(local.local_player(), PlayerId(7));

        let network = LobbyState::new(options(), PlayerId(7), true);
        assert_eq!(network.local_player(), PlayerId(7));
    }

    #[test]
    fn shrinking_team_size_moves_player_to_first_position_on_same_side() {
        let mut lobby = LobbyState::new(options(), PlayerId(0), false);
        lobby.set_team_size(3);
        lobby.local_player = PlayerId(8);
        lobby.set_team_size(1);
        assert_eq!(lobby.local_player(), PlayerId(6));
    }

    #[test]
    fn clicking_empty_position_moves_local_player() {
        let mut options = options();
        options.team_size = 2;
        let config = client_match_config(&options).unwrap();
        let demo = create_demo_world_for_match_config(1, None, config).unwrap();
        let snapshot = PresentationSnapshot::capture(&demo.simulation);

        let mut app = App::new();
        app.insert_resource(LobbyState::new(options, PlayerId(0), false))
            .insert_resource(AuthoritativeSimulation::new(demo.simulation, demo.content))
            .insert_resource(PresentationSamples::new(snapshot))
            .insert_resource(SelectedMatch {
                content: demo.content,
                direct_buildings: demo.direct_buildings,
                local_player: PlayerId(0),
            })
            .insert_resource(SimulationPlayback { paused: true })
            .init_resource::<DebugMenuState>()
            .add_plugins(LobbyPlugin);
        app.update();

        let target = app
            .world_mut()
            .query::<(Entity, &LobbyAction)>()
            .iter(app.world())
            .find_map(|(entity, action)| {
                matches!(action, LobbyAction::Position(PlayerId(7))).then_some(entity)
            })
            .unwrap();
        *app.world_mut()
            .entity_mut(target)
            .get_mut::<Interaction>()
            .unwrap() = Interaction::Pressed;
        app.update();
        assert_eq!(
            app.world().resource::<LobbyState>().local_player(),
            PlayerId(7)
        );
    }

    #[test]
    fn team_size_selection_creates_balanced_authored_roster() {
        let mut options = options();
        for size in 1..=3 {
            options.team_size = size;
            let config = client_match_config(&options).unwrap();
            let expected = (0..size)
                .map(|slot| PlayerId(slot as u8))
                .chain((0..size).map(|slot| PlayerId(6 + slot as u8)))
                .collect::<Vec<_>>();
            assert_eq!(
                config
                    .participants
                    .iter()
                    .map(|player| player.id)
                    .collect::<Vec<_>>(),
                expected
            );
            assert!(config.participants.iter().all(|player| {
                player.builder_race == castle_fight_sim::CastleFightBuilderRace::Human
            }));
        }
    }

    #[test]
    fn network_start_focuses_assigned_builder_once() {
        let options = options();
        let demo =
            create_demo_world_for_match_config(1, None, client_match_config(&options).unwrap())
                .unwrap();
        let snapshot = PresentationSnapshot::capture(&demo.simulation);
        let player = PlayerId(6);
        let target = demo.simulation.builder_for_player(player).unwrap().id;
        let mut authoritative = AuthoritativeSimulation::new(demo.simulation, demo.content);
        authoritative.commands_enabled = false;
        authoritative.network_lobby_status = Some(castle_fight_protocol::LobbyStatus {
            rules: authoritative.simulation.match_rules(),
            host_player_id: 0,
            debug_paused: false,
            debug_speed: castle_fight_protocol::DebugSpeed::Normal,
            connected_player_ids: vec![0, 6],
            required_players: 2,
            participants: Vec::new(),
            started: false,
        });
        let mut app = App::new();
        app.insert_resource(LobbyState::new(options, player, true))
            .insert_resource(authoritative)
            .insert_resource(PresentationSamples::new(snapshot))
            .insert_resource(SelectedMatch {
                content: demo.content,
                direct_buildings: demo.direct_buildings,
                local_player: player,
            })
            .insert_resource(SimulationPlayback { paused: true })
            .insert_resource(CameraFocusRequest::default())
            .init_resource::<DebugMenuState>()
            .add_plugins(LobbyPlugin);
        app.update();
        assert!(app.world().resource::<LobbyState>().active());
        app.world_mut()
            .resource_mut::<AuthoritativeSimulation>()
            .network_lobby_status
            .as_mut()
            .unwrap()
            .started = true;
        app.update();
        assert_eq!(app.world().resource::<CameraFocusRequest>().0, Some(target));
        assert!(!app.world().resource::<SimulationPlayback>().paused);
        app.world_mut().resource_mut::<CameraFocusRequest>().0 = None;
        app.update();
        assert_eq!(app.world().resource::<CameraFocusRequest>().0, None);
    }

    #[test]
    fn start_button_applies_lobby_roster_before_first_tick() {
        let options = options();
        let config = client_match_config(&options).unwrap();
        let demo = create_demo_world_for_match_config(1, None, config).unwrap();
        let snapshot = PresentationSnapshot::capture(&demo.simulation);
        let mut lobby = LobbyState::new(options, PlayerId(0), false);
        lobby.team_size = 3;
        lobby.local_player = PlayerId(8);

        let mut app = App::new();
        app.insert_resource(lobby)
            .insert_resource(AuthoritativeSimulation::new(demo.simulation, demo.content))
            .insert_resource(PresentationSamples::new(snapshot))
            .insert_resource(SelectedMatch {
                content: demo.content,
                direct_buildings: demo.direct_buildings,
                local_player: PlayerId(0),
            })
            .insert_resource(SimulationPlayback { paused: true })
            .insert_resource(CameraFocusRequest::default())
            .init_resource::<DebugMenuState>()
            .add_plugins(LobbyPlugin);
        app.update();

        let start = app
            .world_mut()
            .query::<(Entity, &LobbyAction)>()
            .iter(app.world())
            .find_map(|(entity, action)| matches!(action, LobbyAction::Start).then_some(entity))
            .unwrap();
        *app.world_mut()
            .entity_mut(start)
            .get_mut::<Interaction>()
            .unwrap() = Interaction::Pressed;
        app.update();

        assert!(!app.world().resource::<LobbyState>().active());
        assert!(!app.world().resource::<SimulationPlayback>().paused);
        assert_eq!(
            app.world().resource::<SelectedMatch>().local_player,
            PlayerId(8)
        );
        let simulation = &app.world().resource::<AuthoritativeSimulation>().simulation;
        assert_eq!(simulation.tick(), 0);
        assert_eq!(simulation.players().len(), 6);
        let controls = app.world().resource::<DebugMenuState>();
        let presentation = app.world().resource::<PresentationSamples>();
        assert!(controls.controls_all_players());
        for builder in simulation.builders() {
            assert!(controls.can_control_builder(simulation, PlayerId(8), builder.id));
            assert_eq!(
                controls.controller_for_actor(simulation, PlayerId(8), builder.id),
                builder.owner
            );
            assert!(presentation.current.builders.contains_key(&builder.id));
        }
        assert_eq!(
            app.world().resource::<CameraFocusRequest>().0,
            simulation
                .builder_for_player(PlayerId(8))
                .map(|builder| builder.id)
        );
    }

    fn lobby_app(options: ClientOptions) -> App {
        let config = client_match_config(&options).unwrap();
        let demo = create_demo_world_for_match_config(1, None, config).unwrap();
        let snapshot = PresentationSnapshot::capture(&demo.simulation);
        let mut app = App::new();
        app.insert_resource(LobbyState::new(options, PlayerId(0), false))
            .insert_resource(AuthoritativeSimulation::new(demo.simulation, demo.content))
            .insert_resource(PresentationSamples::new(snapshot))
            .insert_resource(SelectedMatch {
                content: demo.content,
                direct_buildings: demo.direct_buildings,
                local_player: PlayerId(0),
            })
            .insert_resource(SimulationPlayback { paused: true })
            .insert_resource(CameraFocusRequest::default())
            .init_resource::<DebugMenuState>()
            .add_plugins(LobbyPlugin);
        app.update();
        app
    }

    fn press(app: &mut App, matches_action: impl Fn(&LobbyAction) -> bool) {
        let target = app
            .world_mut()
            .query::<(Entity, &LobbyAction)>()
            .iter(app.world())
            .find_map(|(entity, action)| matches_action(action).then_some(entity))
            .expect("lobby action is displayed");
        *app.world_mut()
            .entity_mut(target)
            .get_mut::<Interaction>()
            .unwrap() = Interaction::Pressed;
        app.update();
        // Redraws respawn the lobby; let it settle so the next press finds fresh buttons.
        app.update();
    }

    fn type_key(app: &mut App, logical_key: Key) {
        app.world_mut().write_message(KeyboardInput {
            key_code: KeyCode::Unidentified(bevy::input::keyboard::NativeKeyCode::Unidentified),
            logical_key,
            state: ButtonState::Pressed,
            text: None,
            repeat: false,
            window: Entity::PLACEHOLDER,
        });
        app.update();
    }

    fn time_limit(app: &App) -> u16 {
        app.world()
            .resource::<LobbyState>()
            .options
            .rules
            .time_limit_minutes
    }

    #[test]
    fn typed_time_limit_applies_on_enter_ignoring_non_digits_and_clamping_length() {
        let mut app = lobby_app(options());
        press(&mut app, |action| matches!(action, LobbyAction::TimeInput));
        assert!(app.world().resource::<LobbyState>().time_input.is_some());

        for key in ["4", "x", "5", "1", "2"] {
            type_key(&mut app, Key::Character(key.into()));
        }
        // Only three digits fit, and nothing applies until committed.
        assert_eq!(
            app.world().resource::<LobbyState>().time_input.as_deref(),
            Some("451")
        );
        assert_eq!(time_limit(&app), 0);

        type_key(&mut app, Key::Enter);
        assert_eq!(time_limit(&app), 451);
        assert!(app.world().resource::<LobbyState>().time_input.is_none());
    }

    #[test]
    fn time_limit_edit_commits_when_another_control_is_pressed_and_cancels_on_escape() {
        let mut app = lobby_app(options());
        press(&mut app, |action| matches!(action, LobbyAction::TimeInput));
        type_key(&mut app, Key::Character("2".into()));
        type_key(&mut app, Key::Character("0".into()));
        press(&mut app, |action| {
            matches!(action, LobbyAction::RoundTarget(3))
        });
        assert_eq!(time_limit(&app), 20);
        assert_eq!(
            app.world()
                .resource::<LobbyState>()
                .options
                .rules
                .rounds_to_win,
            3
        );

        press(&mut app, |action| matches!(action, LobbyAction::TimeInput));
        type_key(&mut app, Key::Character("9".into()));
        type_key(&mut app, Key::Escape);
        assert_eq!(time_limit(&app), 20);
        assert!(app.world().resource::<LobbyState>().time_input.is_none());
    }

    #[test]
    fn race_picker_opens_selects_and_dismisses() {
        let mut app = lobby_app(options());
        let last_race = *app
            .world()
            .resource::<LobbyState>()
            .content()
            .supported_builder_races()
            .last()
            .unwrap();

        press(&mut app, |action| {
            matches!(action, LobbyAction::RaceDropdown(0))
        });
        assert_eq!(app.world().resource::<LobbyState>().open_race, Some(0));
        press(&mut app, |action| {
            matches!(action, LobbyAction::DismissRaceMenu)
        });
        assert_eq!(app.world().resource::<LobbyState>().open_race, None);

        press(&mut app, |action| {
            matches!(action, LobbyAction::RaceDropdown(0))
        });
        press(
            &mut app,
            |action| matches!(action, LobbyAction::Race(0, race) if *race == last_race),
        );
        let lobby = app.world().resource::<LobbyState>();
        assert_eq!(lobby.open_race, None);
        assert_eq!(lobby.race(0), last_race);
    }
}
