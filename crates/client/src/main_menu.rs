use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs},
    thread,
};

use bevy::{
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
    client_match_config, compatibility_identity_for_demo, create_demo_world_for_match_config,
    debug_menu::DebugMenuState,
    default_worker_count,
    lobby::LobbyState,
    network::NetworkClient,
};

const DEFAULT_MULTIPLAYER_PORT: u16 = DEFAULT_GAME_PORT;

const BACKGROUND: Color = Color::srgba(0.025, 0.035, 0.055, 0.995);
const PANEL: Color = Color::srgb(0.075, 0.095, 0.13);
const BUTTON: Color = Color::srgb(0.13, 0.17, 0.22);
const BUTTON_HOVER: Color = Color::srgb(0.22, 0.29, 0.37);
const TEXT: Color = Color::srgb(0.92, 0.94, 0.97);
const MUTED: Color = Color::srgb(0.60, 0.68, 0.74);
const ERROR: Color = Color::srgb(1.0, 0.45, 0.4);

pub(crate) struct MainMenuPlugin;

impl Plugin for MainMenuPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_main_menu).add_systems(
            Update,
            (handle_join_address_input, handle_main_menu_buttons).chain(),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MainMenuScreen {
    Main,
    Join,
}

#[derive(Resource)]
pub(crate) struct MainMenuState {
    options: ClientOptions,
    screen: MainMenuScreen,
    join_address: String,
    error: Option<String>,
    active: bool,
    connect_requested: bool,
}

impl MainMenuState {
    pub(crate) fn new(options: ClientOptions) -> Self {
        Self {
            options,
            screen: MainMenuScreen::Main,
            join_address: "127.0.0.1".to_owned(),
            error: None,
            active: true,
            connect_requested: false,
        }
    }

    pub(crate) fn active(&self) -> bool {
        self.active
    }
}

#[derive(Component)]
struct MainMenuRoot;

#[derive(Component)]
struct JoinAddressText;

#[derive(Component, Clone, Copy)]
enum MainMenuAction {
    SinglePlayer,
    Host,
    JoinScreen,
    Connect,
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

                match menu.screen {
                    MainMenuScreen::Main => {
                        panel.spawn(Node {
                            height: px(12.0),
                            ..default()
                        });
                        button(panel, "SINGLE PLAYER", MainMenuAction::SinglePlayer);
                        button(panel, "HOST GAME", MainMenuAction::Host);
                        button(panel, "JOIN GAME", MainMenuAction::JoinScreen);
                        button(panel, "EXIT", MainMenuAction::Exit);
                        label(
                            panel,
                            format!("Host port: {DEFAULT_MULTIPLAYER_PORT}"),
                            13.0,
                            MUTED,
                        );
                    }
                    MainMenuScreen::Join => {
                        label(panel, "SERVER ADDRESS", 16.0, TEXT);
                        panel
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
                                JoinAddressText,
                                Text::new(join_address_label(&menu.join_address)),
                                TextFont::from_font_size(16.0),
                                TextColor(TEXT),
                            ));
                        label(
                            panel,
                            format!(
                                "Enter an IP or domain. If no port is supplied, {DEFAULT_MULTIPLAYER_PORT} is used."
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
            });
        });
}

fn join_address_label(address: &str) -> String {
    format!("> {address}_")
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

fn handle_join_address_input(
    mut keyboard: MessageReader<KeyboardInput>,
    mut menu: ResMut<MainMenuState>,
    mut address_text: Query<&mut Text, With<JoinAddressText>>,
) {
    if !menu.active || menu.screen != MainMenuScreen::Join {
        return;
    }

    let mut changed = false;
    for event in keyboard.read() {
        if event.state != ButtonState::Pressed {
            continue;
        }
        match &event.logical_key {
            Key::Backspace => {
                changed |= menu.join_address.pop().is_some();
            }
            Key::Enter => {
                menu.connect_requested = true;
            }
            Key::Character(character) => {
                if menu.join_address.len() >= 255 {
                    continue;
                }
                for ch in character.chars() {
                    if !ch.is_control() && !ch.is_whitespace() {
                        menu.join_address.push(ch);
                        changed = true;
                    }
                }
            }
            _ => {}
        }
    }

    if changed {
        menu.error = None;
        if let Ok(mut text) = address_text.single_mut() {
            **text = join_address_label(&menu.join_address);
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
}

fn handle_main_menu_buttons(
    mut commands: Commands,
    mut menu: ResMut<MainMenuState>,
    mut buttons: MenuButtonQuery<'_, '_>,
    roots: Query<Entity, With<MainMenuRoot>>,
    mut game: MenuGame<'_>,
    mut exit: MessageWriter<AppExit>,
) {
    if !menu.active {
        return;
    }
    let Ok(root) = roots.single() else {
        return;
    };

    let mut action_to_run = menu.connect_requested.then_some(MainMenuAction::Connect);
    menu.connect_requested = false;

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
        MainMenuAction::Host => {
            let result = start_hosted_session(&menu.options).and_then(|session| {
                install_network_session(&mut commands, &menu.options, session, &mut game)
            });
            match result {
                Ok(()) => {
                    menu.active = false;
                    commands.entity(root).despawn();
                }
                Err(error) => {
                    menu.error = Some(error);
                    redraw(&mut commands, root, &menu);
                }
            }
        }
        MainMenuAction::Connect => {
            let result =
                connect_to_session(&menu.options, &menu.join_address).and_then(|session| {
                    install_network_session(&mut commands, &menu.options, session, &mut game)
                });
            match result {
                Ok(()) => {
                    menu.active = false;
                    commands.entity(root).despawn();
                }
                Err(error) => {
                    menu.error = Some(error);
                    redraw(&mut commands, root, &menu);
                }
            }
        }
    }
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
}

fn start_hosted_session(options: &ClientOptions) -> Result<NetworkSession, String> {
    let demo = network_demo(options)?;
    let authoritative = AuthoritativeMatch::new(
        demo.match_config.clone(),
        default_worker_count(),
        ServerMatchOptions::default(),
    )
    .map_err(|error| format!("Cannot create hosted match: {error}"))?;
    let server = TcpAuthoritativeServer::bind_lobby(
        SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), DEFAULT_MULTIPLAYER_PORT),
        authoritative,
    )
    .map_err(|error| {
        format!(
            "Cannot host on port {DEFAULT_MULTIPLAYER_PORT}: {error}. Is another server already running?"
        )
    })?;

    thread::Builder::new()
        .name("cf-hosted-server".to_owned())
        .spawn(move || {
            if let Err(error) = server.run_until_match_end() {
                eprintln!("hosted Castle Fight server stopped: {error}");
            }
        })
        .map_err(|error| format!("Cannot start hosted server thread: {error}"))?;

    connect_demo_to_address(
        demo,
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), DEFAULT_MULTIPLAYER_PORT),
    )
}

fn connect_to_session(options: &ClientOptions, input: &str) -> Result<NetworkSession, String> {
    let addresses = resolve_server_addresses(input)?;
    let mut failures = Vec::new();
    for address in addresses {
        match network_demo(options).and_then(|demo| connect_demo_to_address(demo, address)) {
            Ok(session) => return Ok(session),
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

fn connect_demo_to_address(
    demo: crate::demo::DemoWorld,
    address: SocketAddr,
) -> Result<NetworkSession, String> {
    let compatibility = compatibility_identity_for_demo(&demo);
    let (client, assignment) =
        NetworkClient::connect(address, compatibility, u64::from(std::process::id()))
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

    Ok(NetworkSession {
        demo,
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
    } = session;
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
    commands.insert_resource(LobbyState::new(options.clone(), local_player, true));
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

    #[test]
    fn single_player_button_opens_an_offline_lobby_without_starting_the_match() {
        let options = ClientOptions {
            stress_units: None,
            stress_visual: None,
            health_bars: true,
            perf_log: false,
            profile_quicksave: None,
            profile_screenshot: None,
            profile_warmup: std::time::Duration::ZERO,
            profile_duration: std::time::Duration::from_secs(1),
            profile_paused: false,
            profile: false,
            render_experiment: crate::render_audit::RenderExperiment::Baseline,
            map_version: castle_fight_sim::MapVersion::CASTLE_FIGHT_9_27,
            release_revision: "r1".to_owned(),
            match_seed: 1,
            team_size: 1,
            builder_rawcodes: [None; 9],
            server: None,
            list_map_versions: false,
        };
        let demo = crate::demo::create_demo_world(1, None);
        let snapshot = PresentationSnapshot::capture(&demo.simulation);
        let mut authoritative = AuthoritativeSimulation::new(demo.simulation, demo.content);
        authoritative.commands_enabled = false;
        let mut app = App::new();
        app.insert_resource(MainMenuState::new(options))
            .insert_resource(authoritative)
            .insert_resource(PresentationSamples::new(snapshot))
            .insert_resource(SelectedMatch {
                content: demo.content,
                direct_buildings: demo.direct_buildings,
                local_player: PlayerId(0),
            })
            .insert_resource(SimulationPlayback { paused: true })
            .init_resource::<DebugMenuState>()
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
