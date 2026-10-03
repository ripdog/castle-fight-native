use bevy::{ecs::system::SystemParam, prelude::*};
use castle_fight_sim::PlayerId;

use crate::{
    AuthoritativeSimulation, ClientOptions, SelectedMatch, SimulationPlayback,
    bridge::{PresentationSamples, PresentationSnapshot},
    client_match_config, create_demo_world_for_match_config, default_worker_count,
    presentation::CameraFocusRequest,
};

const BACKGROUND: Color = Color::srgba(0.025, 0.035, 0.055, 0.99);
const PANEL: Color = Color::srgb(0.075, 0.095, 0.13);
const BUTTON: Color = Color::srgb(0.13, 0.17, 0.22);
const BUTTON_HOVER: Color = Color::srgb(0.22, 0.29, 0.37);
const BUTTON_SELECTED: Color = Color::srgb(0.22, 0.36, 0.46);
const TEXT: Color = Color::srgb(0.92, 0.94, 0.97);
const MUTED: Color = Color::srgb(0.60, 0.68, 0.74);

pub(crate) struct LobbyPlugin;

impl Plugin for LobbyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (ensure_lobby_visible, handle_lobby_buttons).chain());
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

    fn selected(&self, action: LobbyAction) -> bool {
        match action {
            LobbyAction::TeamSize(size) => size == self.team_size,
            LobbyAction::Position(player) => player == self.local_player,
            LobbyAction::RaceHuman(_) | LobbyAction::Start => true,
            LobbyAction::RaceDropdown(_) => false,
        }
    }

    fn player_label(&self, player: PlayerId) -> String {
        let Some(index) = self
            .connected_players
            .iter()
            .position(|connected| *connected == player)
        else {
            return "Empty".to_owned();
        };
        if player == self.local_player {
            format!("Player {} (You)", index + 1)
        } else {
            format!("Player {}", index + 1)
        }
    }

    fn action_disabled(&self, action: LobbyAction) -> bool {
        if !self.networked {
            return false;
        }
        match action {
            LobbyAction::TeamSize(_) | LobbyAction::Position(_) => true,
            LobbyAction::Start => {
                self.host_player != Some(self.local_player)
                    || self.connected_players.len() < self.required_players
            }
            LobbyAction::RaceDropdown(_) | LobbyAction::RaceHuman(_) => false,
        }
    }

    pub(crate) fn active(&self) -> bool {
        self.active
    }
}

#[derive(Component)]
struct LobbyRoot;

#[derive(Component, Clone, Copy)]
enum LobbyAction {
    TeamSize(usize),
    Position(PlayerId),
    RaceDropdown(u8),
    RaceHuman(u8),
    Start,
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

fn spawn_lobby(commands: &mut Commands, lobby: &LobbyState) {
    commands
        .spawn((
            LobbyRoot,
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
            ZIndex(10_000),
        ))
        .with_children(|root| {
            root.spawn((
                Node {
                    width: px(620.0),
                    max_width: percent(92.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(16.0)),
                    row_gap: px(6.0),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                BackgroundColor(PANEL),
                BorderColor::all(Color::srgb(0.28, 0.36, 0.45)),
            ))
            .with_children(|panel| {
                label(panel, "CASTLE FIGHT", 28.0, TEXT);
                label(
                    panel,
                    "GAME SETUP  |  ALL PICK",
                    15.0,
                    Color::srgb(0.45, 0.79, 0.89),
                );
                label(
                    panel,
                    format!(
                        "Map {} / {}",
                        lobby.options.map_version, lobby.options.release_revision
                    ),
                    14.0,
                    MUTED,
                );

                label(panel, "PLAYERS PER TEAM", 17.0, TEXT);
                panel
                    .spawn((Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: px(8.0),
                        ..default()
                    },))
                    .with_children(|row| {
                        for size in 1..=3 {
                            button(
                                row,
                                format!("{}v{}", size, size),
                                LobbyAction::TeamSize(size),
                                size == lobby.team_size,
                                lobby.networked,
                            );
                        }
                    });

                label(panel, "POSITIONS AND RACES", 17.0, TEXT);
                panel
                    .spawn((Node {
                        width: percent(100.0),
                        flex_direction: FlexDirection::Row,
                        column_gap: px(12.0),
                        ..default()
                    },))
                    .with_children(|sides| {
                        for team in 0..=1 {
                            sides
                                .spawn((Node {
                                    width: percent(50.0),
                                    flex_direction: FlexDirection::Column,
                                    row_gap: px(6.0),
                                    ..default()
                                },))
                                .with_children(|column| {
                                    label(
                                        column,
                                        if team == 0 { "WEST" } else { "EAST" },
                                        13.0,
                                        MUTED,
                                    );
                                    for slot in 0..lobby.team_size {
                                        let player =
                                            PlayerId(slot as u8 + if team == 0 { 0 } else { 6 });
                                        spawn_position_row(column, lobby, player, slot);
                                    }
                                });
                        }
                    });

                if lobby.networked {
                    label(
                        panel,
                        format!(
                            "Connected players: {} / {}",
                            lobby.connected_players.len(),
                            lobby.required_players
                        ),
                        13.0,
                        MUTED,
                    );
                    label(
                        panel,
                        if lobby.host_player == Some(lobby.local_player()) {
                            "You are the host. Start when every slot is connected."
                        } else {
                            "Waiting for the host to start the match."
                        },
                        13.0,
                        MUTED,
                    );
                }
                if let Some(error) = &lobby.error {
                    label(panel, error.clone(), 14.0, Color::srgb(1.0, 0.45, 0.4));
                }
                let start_disabled = lobby.networked
                    && (lobby.host_player != Some(lobby.local_player())
                        || lobby.connected_players.len() < lobby.required_players);
                button(
                    panel,
                    "START GAME",
                    LobbyAction::Start,
                    true,
                    start_disabled,
                );
            });
        });
}

fn spawn_position_row(
    parent: &mut ChildSpawnerCommands,
    lobby: &LobbyState,
    player: PlayerId,
    slot: usize,
) {
    parent
        .spawn((Node {
            width: percent(100.0),
            min_height: px(34.0),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            column_gap: px(7.0),
            ..default()
        },))
        .with_children(|row| {
            label(row, format!("{}", slot + 1), 13.0, MUTED);
            button(
                row,
                lobby.player_label(player),
                LobbyAction::Position(player),
                player == lobby.local_player(),
                lobby.networked,
            );
            row.spawn((Node {
                width: px(112.0),
                height: px(32.0),
                ..default()
            },))
                .with_children(|race| {
                    button(
                        race,
                        "Human  v",
                        LobbyAction::RaceDropdown(player.0),
                        false,
                        false,
                    );
                    if lobby.open_race == Some(player.0) {
                        race.spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                top: px(34.0),
                                left: px(0.0),
                                width: px(112.0),
                                padding: UiRect::all(px(2.0)),
                                border: UiRect::all(px(1.0)),
                                ..default()
                            },
                            BackgroundColor(PANEL),
                            BorderColor::all(Color::srgb(0.32, 0.43, 0.52)),
                            GlobalZIndex(10_001),
                        ))
                        .with_children(|menu| {
                            button(menu, "Human", LobbyAction::RaceHuman(player.0), true, false);
                        });
                    }
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

fn button(
    parent: &mut ChildSpawnerCommands,
    value: impl Into<String>,
    action: LobbyAction,
    selected: bool,
    disabled: bool,
) {
    parent
        .spawn((
            Button,
            action,
            Node {
                min_width: px(112.0),
                min_height: px(32.0),
                padding: UiRect::axes(px(14.0), px(6.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(if selected { BUTTON_SELECTED } else { BUTTON }),
            BorderColor::all(if disabled {
                Color::srgb(0.19, 0.23, 0.27)
            } else {
                Color::srgb(0.32, 0.43, 0.52)
            }),
        ))
        .with_child((
            Text::new(value),
            TextFont::from_font_size(15.0),
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
    let mut redraw = false;
    if lobby.networked {
        if let Some(status) = game.authoritative.network_lobby_status.clone() {
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
        let disabled = lobby.action_disabled(*action);
        match interaction {
            Interaction::Hovered => {
                if !disabled {
                    background.0 = BUTTON_HOVER;
                }
            }
            Interaction::None => {
                background.0 = if lobby.selected(*action) {
                    BUTTON_SELECTED
                } else {
                    BUTTON
                };
            }
            Interaction::Pressed if disabled => {}
            Interaction::Pressed => match *action {
                LobbyAction::TeamSize(size) if !lobby.networked => {
                    lobby.set_team_size(size);
                    redraw = true;
                }
                LobbyAction::Position(player) if !lobby.networked => {
                    lobby.local_player = player;
                    redraw = true;
                }
                LobbyAction::RaceDropdown(player) => {
                    lobby.open_race = (lobby.open_race != Some(player)).then_some(player);
                    redraw = true;
                }
                LobbyAction::RaceHuman(player) => {
                    if lobby.open_race == Some(player) {
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
                            *game.authoritative =
                                AuthoritativeSimulation::new(demo.simulation, demo.content);
                            *game.presentation = PresentationSamples::new(
                                PresentationSnapshot::capture(&game.authoritative.simulation),
                            );
                            game.selected_match.content = demo.content;
                            game.selected_match.direct_buildings = demo.direct_buildings;
                            game.selected_match.local_player = lobby.local_player();
                            if let Some(camera_focus) = game.camera_focus.as_deref_mut() {
                                camera_focus.0 = game
                                    .authoritative
                                    .simulation
                                    .builder_for_player(lobby.local_player())
                                    .map(|builder| builder.id);
                            }
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
            },
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
            render_experiment: crate::render_audit::RenderExperiment::Baseline,
            map_version: MapVersion::CASTLE_FIGHT_9_27,
            release_revision: "r1".to_owned(),
            match_seed: 1,
            team_size: 1,
            server: None,
            list_map_versions: false,
        }
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
        assert_eq!(
            app.world().resource::<CameraFocusRequest>().0,
            simulation
                .builder_for_player(PlayerId(8))
                .map(|builder| builder.id)
        );
    }
}
