//! F10 in-game menu, following Warcraft III's Game Menu: resume, return to the main menu, exit.
//!
//! The menu deliberately lives on F10 rather than Escape. Escape is spammed to clear selection and
//! cancel targeting, so a menu on that key would constantly interrupt play. Escape only closes the
//! menu (or steps back from a confirmation) while it is already open.
use bevy::{
    app::AppExit,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
        mouse::{MouseButton, MouseWheel},
    },
    prelude::*,
    ui::{FocusPolicy, UiSystems},
};
use castle_fight_sim::MatchLifecycle;

use crate::{
    AuthoritativeSimulation, SimulationPlayback,
    lobby::LobbyState,
    main_menu::{HostedServer, MainMenuState},
};

const SCRIM: Color = Color::srgba(0.0, 0.0, 0.0, 0.62);
const PANEL: Color = Color::srgb(0.075, 0.095, 0.13);
const PANEL_BORDER: Color = Color::srgb(0.28, 0.36, 0.45);
const BUTTON: Color = Color::srgb(0.13, 0.17, 0.22);
const BUTTON_HOVER: Color = Color::srgb(0.22, 0.29, 0.37);
const BUTTON_BORDER: Color = Color::srgb(0.32, 0.43, 0.52);
const TEXT: Color = Color::srgb(0.92, 0.94, 0.97);
const MUTED: Color = Color::srgb(0.60, 0.68, 0.74);
const WARNING: Color = Color::srgb(1.0, 0.78, 0.35);

pub(crate) struct GameMenuPlugin;

impl Plugin for GameMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<GameMenuState>()
            .add_message::<LeaveMatchRequest>()
            .add_message::<MatchSessionReset>()
            .add_message::<KeyboardInput>()
            .add_message::<MouseWheel>()
            .add_systems(
                PreUpdate,
                block_gameplay_input
                    .after(UiSystems::Focus)
                    .run_if(game_menu_is_open),
            )
            .add_systems(
                Update,
                (handle_menu_keys, handle_menu_buttons, redraw_menu).chain(),
            );
    }
}

/// Asks the main menu to abandon the current match and show itself again.
#[derive(Message)]
pub(crate) struct LeaveMatchRequest;

/// Broadcast after the authoritative world has been replaced so client-local state that refers to
/// the old match (selection, action panel, tooltips) can be dropped instead of aliasing the new
/// match's reused `SimId`s.
#[derive(Message)]
pub(crate) struct MatchSessionReset;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GameMenuScreen {
    Main,
    ConfirmLeave,
    ConfirmExit,
}

#[derive(Resource, Default)]
pub(crate) struct GameMenuState {
    screen: Option<GameMenuScreen>,
    /// The screen currently spawned, used to redraw only on change.
    drawn: Option<GameMenuScreen>,
    /// Offline matches stop while the menu is open; this remembers that we paused them so
    /// closing the menu does not resume a match the player had already paused.
    paused_by_menu: bool,
}

impl GameMenuState {
    pub(crate) fn is_open(&self) -> bool {
        self.screen.is_some()
    }

    fn open(&mut self, playback: &mut SimulationPlayback, networked: bool) {
        self.screen = Some(GameMenuScreen::Main);
        if !networked && !playback.paused {
            playback.paused = true;
            self.paused_by_menu = true;
        }
    }

    fn close(&mut self, playback: &mut SimulationPlayback) {
        self.screen = None;
        if std::mem::take(&mut self.paused_by_menu) {
            playback.paused = false;
        }
    }

    /// Closes without touching playback, for when the match is being torn down anyway.
    fn dismiss(&mut self) {
        self.screen = None;
        self.paused_by_menu = false;
    }
}

#[derive(Component)]
struct GameMenuRoot;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
enum GameMenuAction {
    Resume,
    LeaveMatch,
    ExitGame,
    ConfirmLeave,
    ConfirmExit,
    Back,
}

fn game_menu_is_open(menu: Res<GameMenuState>) -> bool {
    menu.is_open()
}

/// Hides keyboard and mouse input from gameplay systems while the menu is open. This runs after UI
/// focus, so the menu's own buttons still receive this frame's click, and the menu reads key
/// presses from `KeyboardInput` messages rather than `ButtonInput`.
fn block_gameplay_input(
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut wheel: ResMut<Messages<MouseWheel>>,
) {
    keys.reset_all();
    mouse.reset_all();
    wheel.clear();
}

fn match_screen_active(lobby: Option<&LobbyState>, main_menu: Option<&MainMenuState>) -> bool {
    !lobby.is_some_and(LobbyState::active) && !main_menu.is_some_and(MainMenuState::active)
}

fn handle_menu_keys(
    mut keyboard: MessageReader<KeyboardInput>,
    mut menu: ResMut<GameMenuState>,
    mut playback: ResMut<SimulationPlayback>,
    authoritative: Res<AuthoritativeSimulation>,
    lobby: Option<Res<LobbyState>>,
    main_menu: Option<Res<MainMenuState>>,
) {
    if !match_screen_active(lobby.as_deref(), main_menu.as_deref()) {
        keyboard.clear();
        if menu.is_open() {
            menu.dismiss();
        }
        return;
    }
    for event in keyboard.read() {
        if event.state != ButtonState::Pressed || event.repeat {
            continue;
        }
        match (event.key_code, &event.logical_key) {
            (KeyCode::F10, _) => {
                if menu.is_open() {
                    menu.close(&mut playback);
                } else {
                    menu.open(&mut playback, authoritative.is_networked());
                }
            }
            (_, Key::Escape) => match menu.screen {
                Some(GameMenuScreen::Main) => menu.close(&mut playback),
                Some(_) => menu.screen = Some(GameMenuScreen::Main),
                None => {}
            },
            _ => {}
        }
    }
}

type MenuButtonQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        &'static GameMenuAction,
        &'static mut BackgroundColor,
    ),
    (Changed<Interaction>, With<Button>),
>;

fn handle_menu_buttons(
    mut buttons: MenuButtonQuery<'_, '_>,
    mut menu: ResMut<GameMenuState>,
    mut playback: ResMut<SimulationPlayback>,
    authoritative: Res<AuthoritativeSimulation>,
    main_menu: Option<Res<MainMenuState>>,
    mut leave: MessageWriter<LeaveMatchRequest>,
    mut exit: MessageWriter<AppExit>,
) {
    let match_over = matches!(
        authoritative.simulation.lifecycle(),
        MatchLifecycle::Finished { .. }
    );
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
    match action {
        GameMenuAction::Resume => menu.close(&mut playback),
        GameMenuAction::Back => menu.screen = Some(GameMenuScreen::Main),
        // A finished match has nothing left to lose, so skip the confirmation.
        GameMenuAction::LeaveMatch if !match_over => {
            menu.screen = Some(GameMenuScreen::ConfirmLeave);
        }
        GameMenuAction::LeaveMatch | GameMenuAction::ConfirmLeave => {
            if main_menu.is_some() {
                menu.dismiss();
                leave.write(LeaveMatchRequest);
            }
        }
        GameMenuAction::ExitGame if !match_over => {
            menu.screen = Some(GameMenuScreen::ConfirmExit);
        }
        GameMenuAction::ExitGame | GameMenuAction::ConfirmExit => {
            exit.write(AppExit::Success);
        }
    }
}

fn redraw_menu(
    mut commands: Commands,
    mut menu: ResMut<GameMenuState>,
    roots: Query<Entity, With<GameMenuRoot>>,
    authoritative: Res<AuthoritativeSimulation>,
    main_menu: Option<Res<MainMenuState>>,
    hosted: Option<Res<HostedServer>>,
) {
    if menu.drawn == menu.screen {
        return;
    }
    for root in &roots {
        commands.entity(root).despawn();
    }
    menu.drawn = menu.screen;
    let Some(screen) = menu.screen else {
        return;
    };
    let context = MenuContext {
        can_return_to_main_menu: main_menu.is_some(),
        networked: authoritative.is_networked(),
        hosting: hosted.is_some(),
        paused: menu.paused_by_menu,
    };
    spawn_menu(&mut commands, screen, context);
}

#[derive(Clone, Copy)]
struct MenuContext {
    can_return_to_main_menu: bool,
    networked: bool,
    hosting: bool,
    paused: bool,
}

fn spawn_menu(commands: &mut Commands, screen: GameMenuScreen, context: MenuContext) {
    commands
        .spawn((
            GameMenuRoot,
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
            GlobalZIndex(30_000),
        ))
        .with_children(|scrim| {
            scrim
                .spawn((
                    Node {
                        width: px(360.0),
                        max_width: percent(92.0),
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
                .with_children(|panel| match screen {
                    GameMenuScreen::Main => {
                        label(panel, "GAME MENU", 26.0, TEXT);
                        if context.paused {
                            label(
                                panel,
                                "The game is paused while this menu is open.",
                                13.0,
                                MUTED,
                            );
                        }
                        button(panel, "RESUME GAME", GameMenuAction::Resume);
                        if context.can_return_to_main_menu {
                            button(panel, "MAIN MENU", GameMenuAction::LeaveMatch);
                        }
                        button(panel, "EXIT GAME", GameMenuAction::ExitGame);
                        label(panel, "F10 or Esc closes this menu.", 12.0, MUTED);
                    }
                    GameMenuScreen::ConfirmLeave => {
                        label(panel, "RETURN TO MAIN MENU?", 22.0, TEXT);
                        label(panel, leave_warning(context), 14.0, WARNING);
                        button(panel, "LEAVE MATCH", GameMenuAction::ConfirmLeave);
                        button(panel, "CANCEL", GameMenuAction::Back);
                    }
                    GameMenuScreen::ConfirmExit => {
                        label(panel, "EXIT CASTLE FIGHT?", 22.0, TEXT);
                        label(panel, exit_warning(context), 14.0, WARNING);
                        button(panel, "EXIT GAME", GameMenuAction::ConfirmExit);
                        button(panel, "CANCEL", GameMenuAction::Back);
                    }
                });
        });
}

fn leave_warning(context: MenuContext) -> &'static str {
    if context.hosting {
        "You are hosting this game. Leaving ends it for every player."
    } else if context.networked {
        "You will disconnect from this match."
    } else {
        "This match will be abandoned."
    }
}

fn exit_warning(context: MenuContext) -> &'static str {
    if context.hosting {
        "You are hosting this game. Exiting ends it for every player."
    } else if context.networked {
        "You will disconnect from this match and close the game."
    } else {
        "The current match will be lost."
    }
}

fn label(parent: &mut ChildSpawnerCommands, value: impl Into<String>, size: f32, color: Color) {
    parent.spawn((
        Text::new(value),
        TextFont::from_font_size(size),
        TextColor(color),
    ));
}

fn button(parent: &mut ChildSpawnerCommands, value: impl Into<String>, action: GameMenuAction) {
    parent
        .spawn((
            Button,
            action,
            Node {
                width: percent(100.0),
                min_height: px(38.0),
                padding: UiRect::axes(px(14.0), px(7.0)),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_restores_only_a_pause_the_menu_itself_applied() {
        let mut menu = GameMenuState::default();
        let mut playback = SimulationPlayback { paused: false };
        menu.open(&mut playback, false);
        menu.close(&mut playback);
        assert!(!playback.paused);

        // A match the player already paused stays paused after the menu closes.
        let mut playback = SimulationPlayback { paused: true };
        menu.open(&mut playback, false);
        menu.close(&mut playback);
        assert!(playback.paused);
    }
}
