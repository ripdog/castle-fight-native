//! Text chat for networked games, in the lobby and during the match.
//!
//! `Enter` opens the input line; in a match `Enter` talks to allies and `Shift+Enter` to everyone
//! (as in Warcraft III), in the lobby everything goes to everyone. `Enter` sends and `Esc`
//! cancels. Chat is presentation only and never touches the simulation.
use std::collections::VecDeque;

use crate::console::wc3;
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
};
use castle_fight_protocol::MAX_CHAT_CHARS;
use castle_fight_sim::PlayerId;

use crate::{
    AuthoritativeSimulation, AuthorityMode,
    game_menu::{GameMenuState, MatchSessionReset},
    lobby::LobbyState,
    main_menu::MainMenuState,
    settings_menu::SettingsMenuState,
};

const LINE_SECONDS: f32 = 14.0;
const VISIBLE_LINES: usize = 8;
const STORED_LINES: usize = 40;
/// Distance of the log above the bottom edge, clearing the match console.
/// In a match the chat floats above the console's tallest tile (0.192 layout units).
const MATCH_BOTTOM_UNITS: f32 = 0.2;
const LOBBY_BOTTOM: f32 = 200.0;
const ALL_COLOR: Color = Color::srgb(0.94, 0.94, 0.92);
const ALLY_COLOR: Color = Color::srgb(0.55, 0.88, 0.55);
const INPUT_COLOR: Color = Color::srgb(1.0, 0.92, 0.55);

pub(crate) struct ChatPlugin;

impl Plugin for ChatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ChatState>()
            .add_message::<KeyboardInput>()
            .add_message::<MatchSessionReset>()
            .add_systems(Startup, setup_chat)
            .add_systems(
                PreUpdate,
                swallow_game_keys_while_typing.after(bevy::ui::UiSystems::Focus),
            )
            .add_systems(
                Update,
                (
                    clear_chat_for_new_match,
                    receive_chat,
                    handle_chat_keys,
                    expire_chat_lines,
                    redraw_chat,
                )
                    .chain(),
            );
    }
}

/// A line received from the server, waiting to be shown.
pub(crate) struct IncomingChat {
    pub(crate) player_id: u8,
    pub(crate) text: String,
    pub(crate) team_only: bool,
}

struct ChatLine {
    text: String,
    team_only: bool,
    remaining: f32,
}

#[derive(Resource, Default)]
struct ChatState {
    open: bool,
    input: String,
    team_only: bool,
    lines: VecDeque<ChatLine>,
    dirty: bool,
}

#[derive(Component)]
struct ChatRoot;

fn setup_chat(mut commands: Commands) {
    commands.spawn((
        ChatRoot,
        Node {
            position_type: PositionType::Absolute,
            left: px(16.0),
            bottom: wc3(MATCH_BOTTOM_UNITS),
            width: px(520.0),
            flex_direction: FlexDirection::Column,
            row_gap: px(2.0),
            padding: UiRect::all(px(6.0)),
            display: Display::None,
            ..default()
        },
        BackgroundColor(Color::srgba(0.02, 0.02, 0.03, 0.55)),
        GlobalZIndex(10_200),
        Pickable::IGNORE,
    ));
}

fn clear_chat_for_new_match(
    mut resets: MessageReader<MatchSessionReset>,
    mut chat: ResMut<ChatState>,
) {
    if resets.read().count() > 0 {
        *chat = ChatState {
            dirty: true,
            ..default()
        };
    }
}

fn receive_chat(mut authoritative: ResMut<AuthoritativeSimulation>, mut chat: ResMut<ChatState>) {
    if authoritative.pending_chat.is_empty() {
        return;
    }
    for incoming in std::mem::take(&mut authoritative.pending_chat) {
        let name = authoritative
            .player_name(PlayerId(incoming.player_id))
            .map_or_else(
                || format!("Player {}", u16::from(incoming.player_id) + 1),
                str::to_owned,
            );
        let tag = if incoming.team_only { "[Allies] " } else { "" };
        chat.lines.push_back(ChatLine {
            text: format!("{tag}{name}: {}", incoming.text),
            team_only: incoming.team_only,
            remaining: LINE_SECONDS,
        });
        while chat.lines.len() > STORED_LINES {
            chat.lines.pop_front();
        }
    }
    chat.dirty = true;
}

/// Keyboard input must not also drive the game (hotkeys, camera) while a line is being typed.
fn swallow_game_keys_while_typing(chat: Res<ChatState>, mut keys: ResMut<ButtonInput<KeyCode>>) {
    if chat.open {
        keys.reset_all();
    }
}

/// The other screens that can own the keyboard.
#[derive(bevy::ecs::system::SystemParam)]
struct Modals<'w> {
    lobby: Option<Res<'w, LobbyState>>,
    main_menu: Option<Res<'w, MainMenuState>>,
    game_menu: Option<Res<'w, GameMenuState>>,
    settings_menu: Option<Res<'w, SettingsMenuState>>,
}

impl Modals<'_> {
    fn in_lobby(&self) -> bool {
        self.lobby.as_ref().is_some_and(|lobby| lobby.active())
    }

    fn chat_allowed(&self, authoritative: &AuthoritativeSimulation) -> bool {
        matches!(
            authoritative.authority,
            AuthorityMode::Network {
                connected: true,
                ..
            }
        ) && !self.main_menu.as_ref().is_some_and(|menu| menu.active())
            && !self.game_menu.as_ref().is_some_and(|menu| menu.is_open())
            && !self
                .settings_menu
                .as_ref()
                .is_some_and(|menu| menu.is_open())
            && !self.lobby.as_ref().is_some_and(|lobby| lobby.is_typing())
    }
}

fn handle_chat_keys(
    mut keyboard: MessageReader<KeyboardInput>,
    keys: Res<ButtonInput<KeyCode>>,
    mut chat: ResMut<ChatState>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
    modals: Modals,
) {
    let in_lobby = modals.in_lobby();
    for event in keyboard.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        if !chat.open {
            let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
            if matches!(event.logical_key, Key::Enter)
                && !alt
                && modals.chat_allowed(&authoritative)
            {
                let shift = keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight);
                chat.open = true;
                chat.input.clear();
                chat.team_only = !in_lobby && !shift;
                chat.dirty = true;
            }
            continue;
        }
        match &event.logical_key {
            Key::Escape => chat.open = false,
            Key::Enter => {
                let text = castle_fight_protocol::sanitize_chat_text(&chat.input);
                if !text.is_empty() {
                    let team_only = chat.team_only;
                    if let Err(error) = authoritative.request_chat(text, team_only) {
                        authoritative.pending_status.push(error);
                    }
                }
                chat.open = false;
            }
            Key::Backspace => {
                chat.input.pop();
            }
            Key::Character(characters) => {
                for ch in characters.chars().filter(|ch| !ch.is_control()) {
                    if chat.input.chars().count() < MAX_CHAT_CHARS {
                        chat.input.push(ch);
                    }
                }
            }
            Key::Space if chat.input.chars().count() < MAX_CHAT_CHARS => chat.input.push(' '),
            _ => continue,
        }
        chat.dirty = true;
    }
}

fn expire_chat_lines(time: Res<Time>, mut chat: ResMut<ChatState>) {
    if chat.lines.is_empty() {
        return;
    }
    let delta = time.delta_secs();
    let was_visible = chat
        .lines
        .iter()
        .filter(|line| line.remaining > 0.0)
        .count();
    for line in &mut chat.lines {
        line.remaining -= delta;
    }
    if chat
        .lines
        .iter()
        .filter(|line| line.remaining > 0.0)
        .count()
        != was_visible
    {
        chat.dirty = true;
    }
}

fn redraw_chat(
    mut commands: Commands,
    mut chat: ResMut<ChatState>,
    lobby: Option<Res<LobbyState>>,
    root: Single<(Entity, &mut Node, Option<&Children>), With<ChatRoot>>,
) {
    let (root, mut node, children) = root.into_inner();
    let bottom = if lobby.as_ref().is_some_and(|lobby| lobby.active()) {
        px(LOBBY_BOTTOM)
    } else {
        wc3(MATCH_BOTTOM_UNITS)
    };
    if node.bottom != bottom {
        node.bottom = bottom;
    }
    if !std::mem::take(&mut chat.dirty) {
        return;
    }
    for child in children.into_iter().flatten() {
        commands.entity(*child).despawn();
    }
    // While typing, recent history stays up; otherwise only lines that have not yet faded.
    let visible = chat
        .lines
        .iter()
        .filter(|line| chat.open || line.remaining > 0.0)
        .collect::<Vec<_>>();
    let shown = &visible[visible.len().saturating_sub(VISIBLE_LINES)..];
    node.display = if chat.open || !shown.is_empty() {
        Display::Flex
    } else {
        Display::None
    };
    commands.entity(root).with_children(|list| {
        for line in shown {
            list.spawn((
                Text::new(line.text.clone()),
                TextFont::from_font_size(15.0),
                TextColor(if line.team_only {
                    ALLY_COLOR
                } else {
                    ALL_COLOR
                }),
            ));
        }
        if chat.open {
            let target = if chat.team_only { "Allies" } else { "All" };
            list.spawn((
                Text::new(format!("[{target}] {}_", chat.input)),
                TextFont::from_font_size(15.0),
                TextColor(INPUT_COLOR),
            ));
        }
    });
}
