use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
};

use bevy::{
    clipboard::Clipboard,
    ecs::system::SystemParam,
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
};
use castle_fight_server::{
    AuthoritativeMatch, DEFAULT_GAME_PORT, ServerMatchOptions, tcp::TcpAuthoritativeServer,
};
use castle_fight_sim::PlayerId;

use crate::{
    AuthoritativeSimulation, ClientOptions, SelectedMatch, SimulationPlayback,
    bridge::{PresentationSamples, PresentationSnapshot},
    client_match_config,
    client_settings::ClientSettings,
    compatibility_identity_for_demo, create_demo_world_for_match_config,
    debug_menu::DebugMenuState,
    default_worker_count,
    game_menu::{LeaveMatchRequest, MatchSessionReset},
    lobby::LobbyState,
    menu_focus::{MenuFocus, MenuOverlay},
    network::NetworkClient,
    settings_menu::{Panel, SettingsMenuState},
};

const DEFAULT_MULTIPLAYER_PORT: u16 = DEFAULT_GAME_PORT;

const BACKGROUND: Color = Color::srgba(0.025, 0.035, 0.055, 0.995);
const PANEL: Color = Color::srgb(0.075, 0.095, 0.13);
const BUTTON: Color = Color::srgb(0.13, 0.17, 0.22);
const BUTTON_HOVER: Color = Color::srgb(0.22, 0.29, 0.37);
const TEXT: Color = Color::srgb(0.92, 0.94, 0.97);
const MUTED: Color = Color::srgb(0.60, 0.68, 0.74);
const ERROR: Color = Color::srgb(1.0, 0.45, 0.4);
const WARNING: Color = Color::srgb(1.0, 0.78, 0.35);

pub(crate) struct MainMenuPlugin;

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuNotice>()
            .add_message::<LeaveMatchRequest>()
            .add_message::<MatchSessionReset>()
            .add_systems(Startup, setup_main_menu)
            .add_systems(
                Update,
                (
                    handle_menu_text_input,
                    handle_main_menu_buttons,
                    finish_pending_session,
                    return_to_main_menu,
                )
                    .chain(),
            );
    }
}

/// A message for the main menu to show after the player was sent back to it (for example because
/// the host removed them from the lobby).
#[derive(Resource, Default)]
pub(crate) struct MenuNotice(pub(crate) Option<String>);

/// Handle to the server thread started by Host Game. Present only while that session is open.
#[derive(Resource)]
pub(crate) struct HostedServer {
    stop: Arc<AtomicBool>,
    port: u16,
    /// This machine's address on its local network, when it could be determined.
    lan_address: Option<IpAddr>,
}

impl HostedServer {
    /// The `address:port` other players on the same network type into Join Game.
    fn invite_address(&self) -> String {
        match self.lan_address {
            Some(address) => SocketAddr::new(address, self.port).to_string(),
            None => format!("<this computer's IP>:{}", self.port),
        }
    }
}

/// The local interface the operating system would use to reach the internet. Connecting a UDP
/// socket only selects a route; no packet is sent.
fn local_network_address() -> Option<IpAddr> {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    let address = socket.local_addr().ok()?.ip();
    (!address.is_unspecified() && !address.is_loopback()).then_some(address)
}

const MIN_HOST_PORT: u16 = 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainMenuScreen {
    Main,
    Join,
    Host,
}

/// A Host or Join attempt running on a worker thread. Dropping it abandons the attempt: the
/// worker's result is discarded, which drops the half-built session and closes its connection.
struct PendingSession {
    description: String,
    /// What to remember in the saved settings once the attempt succeeds.
    remember: Remember,
    result: Mutex<mpsc::Receiver<Result<NetworkSession, String>>>,
}

enum Remember {
    JoinAddress(String),
    HostPort(u16),
}

impl PendingSession {
    /// Runs `task` on its own thread so a slow or unreachable host cannot freeze the window.
    fn start(
        description: String,
        remember: Remember,
        task: impl FnOnce() -> Result<NetworkSession, String> + Send + 'static,
    ) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        thread::Builder::new()
            .name("cf-session-setup".to_owned())
            .spawn(move || {
                // The receiver is gone when the player cancelled; the session is dropped here.
                let _ = sender.send(task());
            })
            .map_err(|error| format!("Cannot start connection thread: {error}"))?;
        Ok(Self {
            description,
            remember,
            result: Mutex::new(receiver),
        })
    }

    fn poll(&self) -> Option<Result<NetworkSession, String>> {
        match self
            .result
            .lock()
            .expect("session setup receiver mutex poisoned")
            .try_recv()
        {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("The connection attempt ended unexpectedly.".to_owned()))
            }
        }
    }
}

#[derive(Resource)]
pub(crate) struct MainMenuState {
    options: ClientOptions,
    screen: MainMenuScreen,
    join_address: String,
    host_port_text: String,
    /// The converted Warcraft III art (`assets/wc3`) is not installed.
    wc3_art_missing: bool,
    error: Option<String>,
    active: bool,
    /// Action requested from the keyboard, run alongside button presses.
    requested: Option<MainMenuAction>,
    pending: Option<PendingSession>,
}

impl MainMenuState {
    pub(crate) fn new(options: ClientOptions) -> Self {
        Self::with_settings(options, ClientSettings::load())
    }

    fn with_settings(options: ClientOptions, settings: ClientSettings) -> Self {
        Self {
            options,
            screen: MainMenuScreen::Main,
            join_address: settings.last_join_address.clone(),
            host_port_text: settings.host_port.to_string(),
            wc3_art_missing: !crate::terrain::client_asset_root()
                .join("wc3")
                .join("manifest.json")
                .is_file(),
            error: None,
            active: true,
            requested: None,
            pending: None,
        }
    }

    pub(crate) fn active(&self) -> bool {
        self.active
    }

    /// The text the keyboard currently edits.
    fn field_mut(&mut self) -> &mut String {
        match self.screen {
            MainMenuScreen::Host => &mut self.host_port_text,
            MainMenuScreen::Main | MainMenuScreen::Join => &mut self.join_address,
        }
    }

    /// Appends typed or pasted text, dropping what the field cannot hold. Returns whether
    /// anything was added.
    fn insert_text(&mut self, text: &str) -> bool {
        let digits_only = self.screen == MainMenuScreen::Host;
        let limit = if digits_only { 5 } else { 255 };
        let field = self.field_mut();
        let before = field.len();
        for ch in text.chars() {
            let allowed = if digits_only {
                ch.is_ascii_digit()
            } else {
                !ch.is_control() && !ch.is_whitespace()
            };
            if allowed && field.len() < limit {
                field.push(ch);
            }
        }
        field.len() != before
    }
}

#[derive(Component)]
struct MainMenuRoot;

#[derive(Component)]
struct MenuTextField;

#[derive(Component, Clone, Copy)]
enum MainMenuAction {
    SinglePlayer,
    Settings,
    Controls,
    HostScreen,
    Host,
    JoinScreen,
    Connect,
    CancelConnect,
    Back,
    Exit,
}

fn setup_main_menu(mut commands: Commands, menu: Res<MainMenuState>) {
    if menu.active {
        spawn_main_menu(&mut commands, &menu);
    }
}

fn spawn_main_menu(commands: &mut Commands, menu: &MainMenuState) {
    commands
        .spawn((
            MainMenuRoot,
            MenuOverlay(2),
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
            BackgroundColor(BACKGROUND),
            GlobalZIndex(20_000),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: px(480.0),
                    max_width: percent(92.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(22.0)),
                    row_gap: px(10.0),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                BackgroundColor(PANEL),
                BorderColor::all(Color::srgb(0.28, 0.36, 0.45)),
            ))
            .with_children(|panel| {
                label(panel, "CASTLE FIGHT", 32.0, TEXT);
                label(
                    panel,
                    format!(
                        "Native  |  CF {} / {}",
                        menu.options.map_version, menu.options.release_revision
                    ),
                    14.0,
                    MUTED,
                );

                if let Some(pending) = &menu.pending {
                    panel.spawn(Node {
                        height: px(12.0),
                        ..default()
                    });
                    label(panel, pending.description.clone(), 16.0, TEXT);
                    button(panel, "CANCEL", MainMenuAction::CancelConnect);
                    return;
                }
                match menu.screen {
                    MainMenuScreen::Main => {
                        panel.spawn(Node {
                            height: px(12.0),
                            ..default()
                        });
                        button(panel, "SINGLE PLAYER", MainMenuAction::SinglePlayer);
                        button(panel, "HOST GAME", MainMenuAction::HostScreen);
                        button(panel, "JOIN GAME", MainMenuAction::JoinScreen);
                        button(panel, "SETTINGS", MainMenuAction::Settings);
                        button(panel, "CONTROLS", MainMenuAction::Controls);
                        button(panel, "EXIT", MainMenuAction::Exit);
                    }
                    MainMenuScreen::Host => {
                        label(panel, "PORT", 16.0, TEXT);
                        text_field(panel, field_label(&menu.host_port_text));
                        label(
                            panel,
                            "Other players join this computer at its IP address and this port. Forward the port on your router for internet play.",
                            12.0,
                            MUTED,
                        );
                        button(panel, "START HOSTING", MainMenuAction::Host);
                        button(panel, "BACK", MainMenuAction::Back);
                    }
                    MainMenuScreen::Join => {
                        label(panel, "SERVER ADDRESS", 16.0, TEXT);
                        text_field(panel, field_label(&menu.join_address));
                        label(
                            panel,
                            format!(
                                "Enter an IP or domain. If no port is supplied, {DEFAULT_MULTIPLAYER_PORT} is used. Ctrl+V pastes, Ctrl+Backspace clears."
                            ),
                            12.0,
                            MUTED,
                        );
                        button(panel, "CONNECT", MainMenuAction::Connect);
                        button(panel, "BACK", MainMenuAction::Back);
                    }
                }

                if let Some(error) = &menu.error {
                    label(panel, error.clone(), 13.0, ERROR);
                }
                if menu.screen == MainMenuScreen::Main && menu.pending.is_none() {
                    if menu.wc3_art_missing {
                        label(
                            panel,
                            "Warcraft III art was not found, so units and buildings will look like placeholders. Run cf-wc3-assets (see the README) and restart.",
                            12.0,
                            WARNING,
                        );
                    }
                    label(
                        panel,
                        format!(
                            "Castle Fight Native {}. Unofficial fan project. Warcraft III art and sound belong to Blizzard Entertainment and are not included.",
                            env!("CARGO_PKG_VERSION")
                        ),
                        11.0,
                        MUTED,
                    );
                }
            });
        });
}

fn field_label(value: &str) -> String {
    format!("> {value}_")
}

/// The single-line text box of the current screen.
fn text_field(parent: &mut ChildSpawnerCommands, value: String) {
    parent
        .spawn((
            Node {
                width: percent(100.0),
                min_height: px(38.0),
                padding: UiRect::axes(px(10.0), px(7.0)),
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(Color::srgb(0.045, 0.06, 0.085)),
            BorderColor::all(Color::srgb(0.32, 0.43, 0.52)),
        ))
        .with_child((
            MenuTextField,
            Text::new(value),
            TextFont::from_font_size(16.0),
            TextColor(TEXT),
        ));
}

fn label(parent: &mut ChildSpawnerCommands, value: impl Into<String>, size: f32, color: Color) {
    parent.spawn((
        Text::new(value),
        TextFont::from_font_size(size),
        TextColor(color),
    ));
}

fn button(parent: &mut ChildSpawnerCommands, value: impl Into<String>, action: MainMenuAction) {
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
            BorderColor::all(Color::srgb(0.32, 0.43, 0.52)),
        ))
        .with_child((
            Text::new(value),
            TextFont::from_font_size(16.0),
            TextColor(TEXT),
        ));
}

fn handle_menu_text_input(
    mut keyboard: MessageReader<KeyboardInput>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    focus: Option<Res<MenuFocus>>,
    mut clipboard: Option<ResMut<Clipboard>>,
    mut menu: ResMut<MainMenuState>,
    mut field_text: Query<&mut Text, With<MenuTextField>>,
) {
    if !menu.active || menu.screen == MainMenuScreen::Main {
        keyboard.clear();
        return;
    }
    let control = keys.is_some_and(|keys| {
        keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)
    });
    let screen = menu.screen;

    let mut changed = false;
    for event in keyboard.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        if menu.pending.is_some() {
            if event.logical_key == Key::Escape {
                menu.requested = Some(MainMenuAction::CancelConnect);
            }
            continue;
        }
        let confirm = match screen {
            MainMenuScreen::Join => MainMenuAction::Connect,
            _ => MainMenuAction::Host,
        };
        match &event.logical_key {
            Key::Backspace if control => {
                changed |= !menu.field_mut().is_empty();
                menu.field_mut().clear();
            }
            Key::Backspace => {
                changed |= menu.field_mut().pop().is_some();
            }
            // With a button focused, Enter presses that button instead of confirming the box.
            Key::Enter if focus.as_ref().is_some_and(|focus| focus.has_focus()) => {}
            Key::Enter => menu.requested = Some(confirm),
            Key::Escape => menu.requested = Some(MainMenuAction::Back),
            // Shortcuts never type their letter into the field.
            Key::Character(character) if control => {
                if character.eq_ignore_ascii_case("v")
                    && let Some(text) = clipboard
                        .as_mut()
                        .and_then(|clipboard| clipboard.fetch_text().poll_result())
                        .and_then(Result::ok)
                {
                    changed |= menu.insert_text(text.lines().next().unwrap_or_default());
                }
            }
            Key::Character(character) => changed |= menu.insert_text(character),
            _ => {}
        }
    }

    if changed {
        menu.error = None;
        let value = field_label(menu.field_mut());
        if let Ok(mut text) = field_text.single_mut() {
            **text = value;
        }
    }
}

type MenuButtonQuery<'w, 's> = Query<
    'w,
    's,
    (
        &'static Interaction,
        &'static MainMenuAction,
        &'static mut BackgroundColor,
    ),
    (Changed<Interaction>, With<Button>),
>;

#[derive(SystemParam)]
struct MenuGame<'w> {
    authoritative: ResMut<'w, AuthoritativeSimulation>,
    presentation: ResMut<'w, PresentationSamples>,
    selected_match: ResMut<'w, SelectedMatch>,
    playback: ResMut<'w, SimulationPlayback>,
    debug_menu: ResMut<'w, DebugMenuState>,
    notice: ResMut<'w, MenuNotice>,
}

/// Resources a main-menu button can reach beyond the menu itself.
#[derive(SystemParam)]
struct MainMenuEffects<'w> {
    settings_menu: Option<ResMut<'w, SettingsMenuState>>,
    settings: Res<'w, ClientSettings>,
    exit: MessageWriter<'w, AppExit>,
}

fn handle_main_menu_buttons(
    mut commands: Commands,
    mut menu: ResMut<MainMenuState>,
    mut buttons: MenuButtonQuery<'_, '_>,
    roots: Query<Entity, With<MainMenuRoot>>,
    mut game: MenuGame<'_>,
    mut effects: MainMenuEffects<'_>,
) {
    let MainMenuEffects {
        settings_menu,
        settings,
        exit,
    } = &mut effects;
    if !menu.active {
        return;
    }
    let Ok(root) = roots.single() else {
        return;
    };

    let mut action_to_run = menu.requested.take();

    for (interaction, action, mut background) in &mut buttons {
        match interaction {
            Interaction::Hovered => background.0 = BUTTON_HOVER,
            Interaction::None => background.0 = BUTTON,
            Interaction::Pressed => action_to_run = Some(*action),
        }
    }

    let Some(action) = action_to_run else {
        return;
    };
    match action {
        MainMenuAction::SinglePlayer => {
            game.debug_menu.set_single_player(true);
            commands.insert_resource(LobbyState::new(
                menu.options.clone(),
                game.selected_match.local_player,
                false,
            ));
            menu.active = false;
            commands.entity(root).despawn();
        }
        MainMenuAction::Settings | MainMenuAction::Controls => {
            if let Some(settings_menu) = settings_menu.as_deref_mut() {
                settings_menu.open(if matches!(action, MainMenuAction::Settings) {
                    Panel::Settings
                } else {
                    Panel::Controls
                });
            }
        }
        MainMenuAction::JoinScreen => {
            menu.screen = MainMenuScreen::Join;
            menu.error = None;
            redraw(&mut commands, root, &menu);
        }
        MainMenuAction::Back => {
            menu.screen = MainMenuScreen::Main;
            menu.error = None;
            redraw(&mut commands, root, &menu);
        }
        MainMenuAction::Exit => {
            exit.write(AppExit::Success);
        }
        MainMenuAction::HostScreen => {
            menu.screen = MainMenuScreen::Host;
            menu.error = None;
            redraw(&mut commands, root, &menu);
        }
        MainMenuAction::Host => {
            match menu.host_port_text.parse::<u16>() {
                Ok(port) if port >= MIN_HOST_PORT => {
                    let options = menu.options.clone();
                    let name = crate::network::player_name_for(&settings.player_name);
                    begin_session(
                        &mut menu,
                        format!("Starting game on port {port}..."),
                        Remember::HostPort(port),
                        move || start_hosted_session(&options, port, &name),
                    );
                }
                _ => {
                    menu.error = Some(format!("Choose a port between {MIN_HOST_PORT} and 65535."));
                }
            }
            redraw(&mut commands, root, &menu);
        }
        MainMenuAction::Connect => {
            let options = menu.options.clone();
            let address = menu.join_address.clone();
            let name = crate::network::player_name_for(&settings.player_name);
            begin_session(
                &mut menu,
                format!("Connecting to {}...", address.trim()),
                Remember::JoinAddress(address.trim().to_owned()),
                move || connect_to_session(&options, &address, &name),
            );
            redraw(&mut commands, root, &menu);
        }
        MainMenuAction::CancelConnect => {
            menu.pending = None;
            redraw(&mut commands, root, &menu);
        }
    }
}

fn begin_session(
    menu: &mut MainMenuState,
    description: String,
    remember: Remember,
    task: impl FnOnce() -> Result<NetworkSession, String> + Send + 'static,
) {
    if menu.pending.is_some() {
        return;
    }
    menu.error = None;
    match PendingSession::start(description, remember, task) {
        Ok(pending) => menu.pending = Some(pending),
        Err(error) => menu.error = Some(error),
    }
}

/// Completes a Host or Join attempt once its worker thread reports back.
fn finish_pending_session(
    mut commands: Commands,
    mut menu: ResMut<MainMenuState>,
    mut settings: ResMut<ClientSettings>,
    roots: Query<Entity, With<MainMenuRoot>>,
    mut game: MenuGame<'_>,
) {
    if !menu.active {
        return;
    }
    let Some(result) = menu.pending.as_ref().and_then(PendingSession::poll) else {
        return;
    };
    let remember = menu.pending.take().map(|pending| pending.remember);
    let Ok(root) = roots.single() else {
        return;
    };
    match result.and_then(|session| {
        install_network_session(&mut commands, &menu.options, session, &mut game)
    }) {
        Ok(()) => {
            match remember {
                Some(Remember::JoinAddress(address)) => settings.last_join_address = address,
                Some(Remember::HostPort(port)) => settings.host_port = port,
                None => {}
            }
            settings.save();
            menu.active = false;
            commands.entity(root).despawn();
        }
        Err(error) => {
            menu.error = Some(error);
            redraw(&mut commands, root, &menu);
        }
    }
}

/// Abandons the current match and shows the main menu again. The old simulation and its network
/// client are dropped (closing the connection), a hosted server is stopped, and a fresh offline
/// world replaces them exactly as at startup so the presentation has nothing stale to show.
fn return_to_main_menu(
    mut commands: Commands,
    mut requests: MessageReader<LeaveMatchRequest>,
    menu: Option<ResMut<MainMenuState>>,
    lobby: Option<Res<LobbyState>>,
    roots: Query<Entity, With<MainMenuRoot>>,
    mut game: MenuGame<'_>,
    mut reset: MessageWriter<MatchSessionReset>,
) {
    if requests.read().count() == 0 {
        return;
    }
    let Some(mut menu) = menu else {
        return;
    };
    let demo = match network_demo(&menu.options) {
        Ok(demo) => demo,
        Err(error) => {
            game.authoritative
                .pending_status
                .push(format!("Cannot return to the main menu: {error}"));
            return;
        }
    };
    let local_player = demo
        .match_config
        .participants
        .first()
        .expect("playable Castle Fight match must contain a local participant")
        .id;
    let observer_team = demo
        .simulation
        .player(local_player)
        .map(|player| player.team);
    let snapshot = PresentationSnapshot::capture(&demo.simulation);

    // Leaving a lobby on purpose frees the slot; a plain disconnect would keep it reserved for a
    // reconnect that is never coming. The message is flushed before the socket closes.
    if lobby.is_some_and(|lobby| lobby.is_open_network_lobby()) {
        let _ = game.authoritative.request_network_leave_lobby();
    }
    // Dropping the old resources closes the network client; removing the handle stops a hosted
    // server and releases its port.
    commands.remove_resource::<HostedServer>();
    commands.remove_resource::<LobbyState>();
    *game.authoritative = AuthoritativeSimulation::new(demo.simulation, demo.content);
    game.authoritative.commands_enabled = false;
    *game.presentation = PresentationSamples::new(snapshot).with_observer(observer_team);
    game.selected_match.content = demo.content;
    game.selected_match.direct_buildings = demo.direct_buildings;
    game.selected_match.local_player = local_player;
    game.playback.paused = true;
    *game.debug_menu = DebugMenuState::default();

    menu.active = true;
    menu.screen = MainMenuScreen::Main;
    menu.error = game.notice.0.take();
    menu.requested = None;
    menu.pending = None;
    for root in &roots {
        commands.entity(root).despawn();
    }
    spawn_main_menu(&mut commands, &menu);
    reset.write(MatchSessionReset);
}

fn redraw(commands: &mut Commands, root: Entity, menu: &MainMenuState) {
    commands.entity(root).despawn();
    spawn_main_menu(commands, menu);
}

struct NetworkSession {
    demo: crate::demo::DemoWorld,
    client: NetworkClient,
    local_player: PlayerId,
    next_sequence: u64,
    /// Set when this client also runs the server, so leaving can stop it.
    hosted: Option<HostedServer>,
}

fn start_hosted_session(
    options: &ClientOptions,
    port: u16,
    player_name: &str,
) -> Result<NetworkSession, String> {
    let demo = network_demo(options)?;
    let authoritative = AuthoritativeMatch::new(
        demo.match_config.clone(),
        default_worker_count(),
        ServerMatchOptions::default(),
    )
    .map_err(|error| format!("Cannot create hosted match: {error}"))?;
    let server = TcpAuthoritativeServer::bind_lobby(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), port),
        authoritative,
    )
    .map_err(|error| {
        format!("Cannot host on port {port}: {error}. Is another server already running?")
    })?;

    let stop = Arc::new(AtomicBool::new(false));
    let server_stop = Arc::clone(&stop);
    thread::Builder::new()
        .name("cf-hosted-server".to_owned())
        .spawn(move || {
            if let Err(error) = server.run_until_match_end_or_stopped(&server_stop) {
                eprintln!("hosted Castle Fight server stopped: {error}");
            }
        })
        .map_err(|error| format!("Cannot start hosted server thread: {error}"))?;

    let hosted = HostedServer {
        stop,
        port,
        lan_address: local_network_address(),
    };
    // If the local client cannot join its own server, dropping `hosted` stops the thread again.
    let joined = join_server(
        &demo,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        player_name,
    )?;
    let mut session = joined.into_session(demo);
    session.hosted = Some(hosted);
    Ok(session)
}

impl Drop for HostedServer {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
    }
}

fn connect_to_session(
    options: &ClientOptions,
    input: &str,
    player_name: &str,
) -> Result<NetworkSession, String> {
    let addresses = resolve_server_addresses(input)?;
    let demo = network_demo(options)?;
    let mut failures = Vec::new();
    for address in addresses {
        match join_server(&demo, address, player_name) {
            Ok(joined) => return Ok(joined.into_session(demo)),
            Err(error) => failures.push(format!("{address}: {error}")),
        }
    }
    let fallback = failures
        .last()
        .cloned()
        .unwrap_or_else(|| "no addresses resolved".to_owned());
    Err(format!("Could not join server: {fallback}"))
}

fn network_demo(options: &ClientOptions) -> Result<crate::demo::DemoWorld, String> {
    let config = client_match_config(options).map_err(|error| error.to_string())?;
    create_demo_world_for_match_config(default_worker_count(), None, config)
        .map_err(|error| error.to_string())
}

/// A completed lobby handshake, not yet bound to the local world.
struct JoinedServer {
    client: NetworkClient,
    local_player: PlayerId,
    next_sequence: u64,
}

impl JoinedServer {
    fn into_session(self, demo: crate::demo::DemoWorld) -> NetworkSession {
        NetworkSession {
            demo,
            client: self.client,
            local_player: self.local_player,
            next_sequence: self.next_sequence,
            hosted: None,
        }
    }
}

fn join_server(
    demo: &crate::demo::DemoWorld,
    address: SocketAddr,
    player_name: &str,
) -> Result<JoinedServer, String> {
    let compatibility = compatibility_identity_for_demo(demo);
    let (client, assignment) = NetworkClient::connect(
        address,
        compatibility,
        u64::from(std::process::id()),
        player_name,
    )
    .map_err(|error| error.to_string())?;
    if assignment.next_stream_position != 0
        || assignment.next_client_sequence != 0
        || assignment.completed_tick.is_some()
    {
        return Err(
            "server does not accept new lobby joins after simulation has advanced".to_owned(),
        );
    }

    let local_player = PlayerId(assignment.player_id);
    let expected_team = demo
        .match_config
        .participants
        .iter()
        .find(|participant| participant.id == local_player)
        .map(|participant| participant.team.0);
    if expected_team != Some(assignment.team) {
        return Err(format!(
            "server assigned player {} to unexpected team {}",
            assignment.player_id, assignment.team
        ));
    }

    Ok(JoinedServer {
        client,
        local_player,
        next_sequence: assignment.next_client_sequence,
    })
}

fn install_network_session(
    commands: &mut Commands,
    options: &ClientOptions,
    session: NetworkSession,
    game: &mut MenuGame<'_>,
) -> Result<(), String> {
    let NetworkSession {
        demo,
        client,
        local_player,
        next_sequence,
        hosted,
    } = session;
    let invite = hosted.as_ref().map(HostedServer::invite_address);
    if let Some(hosted) = hosted {
        commands.insert_resource(hosted);
    }
    *game.authoritative = AuthoritativeSimulation::new_networked(
        demo.simulation,
        demo.content,
        demo.match_config,
        client,
        local_player,
        next_sequence,
    );
    game.authoritative.commands_enabled = false;
    game.debug_menu.set_single_player(false);
    *game.presentation = PresentationSamples::new(PresentationSnapshot::capture(
        &game.authoritative.simulation,
    ))
    .with_observer(
        game.authoritative
            .simulation
            .player(local_player)
            .map(|player| player.team),
    );
    game.selected_match.content = demo.content;
    game.selected_match.direct_buildings = demo.direct_buildings;
    game.selected_match.local_player = local_player;
    game.playback.paused = true;
    let mut lobby = LobbyState::new(options.clone(), local_player, true);
    lobby.set_invite_address(invite);
    commands.insert_resource(lobby);
    Ok(())
}

fn resolve_server_addresses(input: &str) -> Result<Vec<SocketAddr>, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("Enter a server IP address or domain.".to_owned());
    }

    if let Ok(address) = input.parse::<SocketAddr>() {
        return Ok(vec![address]);
    }

    let target = if input.starts_with('[') && input.ends_with(']') {
        format!("{input}:{DEFAULT_MULTIPLAYER_PORT}")
    } else if input.matches(':').count() > 1 {
        format!("[{input}]:{DEFAULT_MULTIPLAYER_PORT}")
    } else if input.contains(':') {
        input.to_owned()
    } else {
        format!("{input}:{DEFAULT_MULTIPLAYER_PORT}")
    };

    let addresses = target
        .to_socket_addrs()
        .map_err(|error| format!("Cannot resolve {input}: {error}"))?
        .collect::<Vec<_>>();
    if addresses.is_empty() {
        return Err(format!("No network addresses found for {input}."));
    }
    Ok(addresses)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_options() -> ClientOptions {
        ClientOptions {
            stress_units: None,
            stress_visual: None,
            health_bars: true,
            perf_log: false,
            profile_quicksave: None,
            profile_screenshot: None,
            profile_theme: None,
            profile_select: None,
            profile_items: false,
            profile_shop: false,
            profile_warmup: std::time::Duration::ZERO,
            profile_duration: std::time::Duration::from_secs(1),
            profile_until_tick: None,
            profile_paused: false,
            profile: false,
            benchmark: false,
            render_experiment: crate::render_audit::RenderExperiment::Baseline,
            perf_disable: crate::perf_disable::PerfDisable::default(),
            capture_frames: None,
            map_version: castle_fight_sim::MapVersion::CASTLE_FIGHT_9_27,
            release_revision: "r1".to_owned(),
            match_seed: 1,
            rules: castle_fight_sim::MatchRules::for_version(
                castle_fight_sim::MapVersion::CASTLE_FIGHT_9_27,
            ),
            team_size: 1,
            builder_rawcodes: [None; 9],
            server: None,
            list_map_versions: false,
            sim_workers: 1,
        }
    }

    #[test]
    fn single_player_button_opens_an_offline_lobby_without_starting_the_match() {
        let options = test_options();
        let demo = crate::demo::create_demo_world(1, None);
        let snapshot = PresentationSnapshot::capture(&demo.simulation);
        let mut authoritative = AuthoritativeSimulation::new(demo.simulation, demo.content);
        authoritative.commands_enabled = false;
        let mut app = App::new();
        app.insert_resource(MainMenuState::with_settings(
            options,
            ClientSettings::default(),
        ))
        .insert_resource(authoritative)
        .insert_resource(PresentationSamples::new(snapshot))
        .insert_resource(SelectedMatch {
            content: demo.content,
            direct_buildings: demo.direct_buildings,
            local_player: PlayerId(0),
        })
        .insert_resource(SimulationPlayback { paused: true })
        .init_resource::<DebugMenuState>()
        .init_resource::<ClientSettings>()
        .add_message::<AppExit>()
        .add_message::<KeyboardInput>()
        .add_plugins((MainMenuPlugin, crate::lobby::LobbyPlugin));
        app.update();
        let button = app
            .world_mut()
            .query::<(Entity, &MainMenuAction)>()
            .iter(app.world())
            .find_map(|(entity, action)| {
                matches!(action, MainMenuAction::SinglePlayer).then_some(entity)
            })
            .unwrap();
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
        app.update();
        assert!(!app.world().resource::<MainMenuState>().active());
        assert!(app.world().resource::<LobbyState>().active());
        assert!(
            app.world()
                .resource::<DebugMenuState>()
                .controls_all_players()
        );
        assert!(app.world().resource::<SimulationPlayback>().paused);
        let authoritative = app.world().resource::<AuthoritativeSimulation>();
        assert!(!authoritative.is_networked());
        assert!(!authoritative.commands_enabled);
        assert_eq!(authoritative.simulation.tick(), 0);
    }

    fn menu_test_app() -> App {
        let demo = crate::demo::create_demo_world(1, None);
        let snapshot = PresentationSnapshot::capture(&demo.simulation);
        let mut authoritative = AuthoritativeSimulation::new(demo.simulation, demo.content);
        authoritative.commands_enabled = false;
        let mut app = App::new();
        app.insert_resource(MainMenuState::with_settings(
            test_options(),
            ClientSettings::default(),
        ))
        .insert_resource(authoritative)
        .insert_resource(PresentationSamples::new(snapshot))
        .insert_resource(SelectedMatch {
            content: demo.content,
            direct_buildings: demo.direct_buildings,
            local_player: PlayerId(0),
        })
        .insert_resource(SimulationPlayback { paused: true })
        .init_resource::<DebugMenuState>()
        .init_resource::<ClientSettings>()
        .add_message::<AppExit>()
        .add_message::<KeyboardInput>()
        .add_plugins((MainMenuPlugin, crate::lobby::LobbyPlugin));
        app
    }

    #[test]
    fn leaving_a_match_restores_a_fresh_main_menu() {
        let mut app = menu_test_app();
        app.update();
        let single_player = app
            .world_mut()
            .query::<(Entity, &MainMenuAction)>()
            .iter(app.world())
            .find_map(|(entity, action)| {
                matches!(action, MainMenuAction::SinglePlayer).then_some(entity)
            })
            .unwrap();
        *app.world_mut()
            .get_mut::<Interaction>(single_player)
            .unwrap() = Interaction::Pressed;
        app.update();
        // Start the match as the lobby would.
        {
            let mut authoritative = app.world_mut().resource_mut::<AuthoritativeSimulation>();
            authoritative.commands_enabled = true;
            authoritative.pending_status.push("stale".into());
        }
        app.world_mut().resource_mut::<SimulationPlayback>().paused = false;
        app.update();
        let lobby_screens = |app: &mut App| {
            app.world_mut()
                .query_filtered::<Entity, With<crate::lobby::LobbyRoot>>()
                .iter(app.world())
                .count()
        };
        assert_eq!(lobby_screens(&mut app), 1);
        assert!(!app.world().resource::<MainMenuState>().active());

        app.world_mut().write_message(LeaveMatchRequest);
        app.update();
        app.update();
        assert_eq!(
            lobby_screens(&mut app),
            0,
            "an abandoned lobby must not stay on screen behind the main menu"
        );

        assert!(app.world().resource::<MainMenuState>().active());
        assert!(app.world().get_resource::<LobbyState>().is_none());
        assert!(app.world().resource::<SimulationPlayback>().paused);
        assert!(
            !app.world()
                .resource::<DebugMenuState>()
                .controls_all_players()
        );
        let authoritative = app.world().resource::<AuthoritativeSimulation>();
        assert!(!authoritative.is_networked());
        assert!(!authoritative.commands_enabled);
        assert!(authoritative.pending_status.is_empty());
        assert_eq!(authoritative.simulation.tick(), 0);
        // Exactly one menu is on screen, and it is the top-level one.
        let roots = app
            .world_mut()
            .query_filtered::<Entity, With<MainMenuRoot>>()
            .iter(app.world())
            .count();
        assert_eq!(roots, 1);
        assert!(
            app.world_mut()
                .query::<&MainMenuAction>()
                .iter(app.world())
                .any(|action| matches!(action, MainMenuAction::HostScreen))
        );
    }

    #[test]
    fn join_address_defaults_to_game_port() {
        let addresses = resolve_server_addresses("127.0.0.1").unwrap();
        assert_eq!(
            addresses,
            vec![SocketAddr::new(
                IpAddr::V4(Ipv4Addr::LOCALHOST),
                DEFAULT_MULTIPLAYER_PORT
            )]
        );
    }

    #[test]
    fn explicit_join_port_is_preserved() {
        let addresses = resolve_server_addresses("127.0.0.1:7000").unwrap();
        assert_eq!(
            addresses,
            vec![SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 7000)]
        );
    }
}
