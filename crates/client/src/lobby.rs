use bevy::{ecs::system::SystemParam, prelude::*};
use castle_fight_sim::{CastleFightBuilderRace, CastleFightContentBundle, PlayerId};

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
            LobbyAction::RaceDropdown(_) => false,
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
            LobbyAction::Start => {
                self.host_player != Some(self.local_player)
                    || self.connected_players.len() < self.required_players
            }
            LobbyAction::RaceDropdown(player) | LobbyAction::Race(player, _) => {
                player != self.local_player.0
            }
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
    Race(u8, CastleFightBuilderRace),
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
                    width: px(800.0),
                    max_width: percent(94.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(20.0)),
                    row_gap: px(18.0),
                    border: UiRect::all(px(1.0)),
                    border_radius: BorderRadius::all(px(10.0)),
                    ..default()
                },
                BackgroundColor(PANEL),
                BorderColor::all(Color::srgb(0.28, 0.36, 0.45)),
            ))
            .with_children(|panel| {
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6.0),
                        ..default()
                    })
                    .with_children(|header| {
                        label(header, "Castle Fight", 28.0, TEXT);
                        label(
                            header,
                            format!(
                                "All Pick  |  Map {} / {}",
                                lobby.options.map_version, lobby.options.release_revision
                            ),
                            13.0,
                            MUTED,
                        );
                    });
                panel
                    .spawn(Node {
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        column_gap: px(12.0),
                        ..default()
                    })
                    .with_children(|row| {
                        label(row, "Players per team", 14.0, MUTED);
                        row.spawn(Node {
                            column_gap: px(6.0),
                            ..default()
                        })
                        .with_children(|sizes| {
                            for size in 1..=3 {
                                button(
                                    sizes,
                                    format!("{size}v{size}"),
                                    LobbyAction::TeamSize(size),
                                    size == lobby.team_size,
                                    lobby.networked,
                                );
                            }
                        });
                    });
                panel
                    .spawn(Node {
                        width: percent(100.0),
                        column_gap: px(16.0),
                        ..default()
                    })
                    .with_children(|sides| {
                        for team in 0..=1 {
                            sides
                                .spawn((
                                    Node {
                                        flex_basis: px(0.0),
                                        flex_grow: 1.0,
                                        min_width: px(0.0),
                                        flex_direction: FlexDirection::Column,
                                        padding: UiRect::all(px(14.0)),
                                        row_gap: px(12.0),
                                        border: UiRect::all(px(1.0)),
                                        border_radius: BorderRadius::all(px(6.0)),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgb(0.095, 0.12, 0.16)),
                                    BorderColor::all(Color::srgb(0.20, 0.26, 0.33)),
                                ))
                                .with_children(|column| {
                                    label(
                                        column,
                                        if team == 0 { "WEST TEAM" } else { "EAST TEAM" },
                                        14.0,
                                        Color::srgb(0.45, 0.79, 0.89),
                                    );
                                    for slot in 0..lobby.team_size {
                                        let player =
                                            PlayerId(slot as u8 + if team == 0 { 0 } else { 6 });
                                        spawn_position_row(column, lobby, player, slot);
                                    }
                                });
                        }
                    });
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(10.0),
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
                                13.0,
                                MUTED,
                            );
                        } else {
                            label(
                                footer,
                                "Choose your position and each player's race. You control all players.",
                                13.0,
                                MUTED,
                            );
                        }
                        if let Some(error) = &lobby.error {
                            label(footer, error.clone(), 13.0, Color::srgb(1.0, 0.45, 0.4));
                        }
                        let start_disabled = lobby.action_disabled(LobbyAction::Start);
                        let start_label =
                            if lobby.networked && lobby.host_player != Some(lobby.local_player()) {
                                "Waiting for host"
                            } else if start_disabled {
                                "Waiting for players"
                            } else {
                                "Start match"
                            };
                        button(
                            footer,
                            start_label,
                            LobbyAction::Start,
                            true,
                            start_disabled,
                        );
                    });
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
                        15.0,
                        if player == lobby.local_player() {
                            TEXT
                        } else {
                            MUTED
                        },
                    );
                    if lobby.host_player == Some(player) {
                        label(identity, "Host", 12.0, Color::srgb(0.45, 0.79, 0.89));
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
            row.spawn(Node {
                width: percent(100.0),
                min_width: px(0.0),
                height: px(40.0),
                ..default()
            })
            .with_children(|selector| {
                let builder = lobby
                    .content()
                    .builder(lobby.race(player.0))
                    .expect("selected builder");
                let action = LobbyAction::RaceDropdown(player.0);
                let disabled = lobby.action_disabled(action);
                let name = builder.name.trim_end_matches(" Builder");
                button(
                    selector,
                    if disabled {
                        name.to_owned()
                    } else {
                        format!("{name}   v")
                    },
                    action,
                    lobby.open_race == Some(player.0),
                    disabled,
                );
                if lobby.open_race == Some(player.0) {
                    selector
                        .spawn((
                            Node {
                                position_type: PositionType::Absolute,
                                top: px(44.0),
                                left: px(0.0),
                                width: percent(100.0),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(4.0)),
                                row_gap: px(4.0),
                                border: UiRect::all(px(1.0)),
                                border_radius: BorderRadius::all(px(4.0)),
                                ..default()
                            },
                            BackgroundColor(PANEL),
                            BorderColor::all(Color::srgb(0.32, 0.43, 0.52)),
                            GlobalZIndex(10_001),
                        ))
                        .with_children(|menu| {
                            for &race in lobby.content().supported_builder_races() {
                                let builder =
                                    lobby.content().builder(race).expect("supported builder");
                                let action = LobbyAction::Race(player.0, race);
                                button(
                                    menu,
                                    builder.name.trim_end_matches(" Builder"),
                                    action,
                                    lobby.race(player.0) == race,
                                    lobby.action_disabled(action),
                                );
                            }
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

fn button_color(action: LobbyAction, selected: bool, disabled: bool) -> Color {
    if disabled && !matches!(action, LobbyAction::TeamSize(_)) {
        Color::srgb(0.10, 0.13, 0.17)
    } else if selected {
        BUTTON_SELECTED
    } else {
        BUTTON
    }
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
                width: if matches!(action, LobbyAction::TeamSize(_)) {
                    px(64.0)
                } else {
                    percent(100.0)
                },
                min_width: px(0.0),
                min_height: px(40.0),
                padding: UiRect::axes(px(10.0), px(8.0)),
                border_radius: BorderRadius::all(px(4.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(px(1.0)),
                ..default()
            },
            BackgroundColor(button_color(action, selected, disabled)),
            BorderColor::all(if disabled {
                Color::srgb(0.19, 0.23, 0.27)
            } else {
                Color::srgb(0.32, 0.43, 0.52)
            }),
        ))
        .with_child((
            Text::new(value),
            TextFont::from_font_size(15.0),
            TextLayout::no_wrap(),
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
    let mut redraw = false;
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
                LobbyAction::Race(player, race) => {
                    if lobby.content().supports_builder_race(race) {
                        let rawcode = lobby
                            .content()
                            .builder(race)
                            .expect("supported builder")
                            .rawcode;
                        if lobby.networked {
                            if let Err(error) = game.authoritative.request_network_race(rawcode) {
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
}
