mod alerts;
mod bridge;
mod build_orders;
mod build_ui;
mod builder_controls;
mod building_models;
mod chat;
mod client_settings;
mod compact_material;
mod console;
mod cursor;
mod debug_menu;
mod demo;
mod doodads;
mod fixed_catch_up;
mod fog;
mod frame_capture;
mod game_menu;
mod inspection;
mod item_ui;
mod lobby;
mod main_menu;
mod match_ui;
mod menu_focus;
mod minimap;
mod network;
mod network_sim;
mod notifications;
mod offscreen_animation;
mod particle_renderer;
mod perf_disable;
mod performance_ui;
mod presentation;
mod render_audit;
mod render_tuning;
mod resource_ui;
mod scene_budget;
mod scoreboard_ui;
mod settings_menu;
mod sim_thread;
mod skin_influences;
mod skinned_bounds;
mod terrain;
mod ui_icons;
mod unit_models;
mod view_state;
mod wc3_effects;
mod wc3_text;

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use bevy::{
    asset::AssetPlugin,
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    ecs::system::SystemParam,
    prelude::*,
    tasks::{IoTaskPool, TaskPoolBuilder, available_parallelism},
    time::{Fixed, Virtual},
    window::PresentMode,
};
use castle_fight_protocol::{
    CatchUpComplete, ClientMessage, CommandRequest, CompatibilityIdentity, MAX_SNAPSHOT_BYTES,
    SNAPSHOT_CHUNK_BYTES, SnapshotChunk, SnapshotTransferBegin, WireCommandExecution,
};
use castle_fight_sim::{
    AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION, CANONICAL_CHECKSUM_SCHEMA_VERSION,
    CASTLE_FIGHT_SIMULATION_HZ, CanonicalStreamError, CastleFightBuilderRace,
    CastleFightContentAvailability, CastleFightContentBundle, CastleFightMatchConfig,
    CastleFightMatchSetupError, CastleFightParticipantConfig, CommandExecution,
    CommandExecutionResult, CommandOutcome, CommandSubmission, DriverTickResult,
    InputStreamPosition, MapVersion, MatchDriver, PlayerCommand, PlayerId, Simulation,
    SimulationSnapshot, castle_fight_registered_releases,
    debug::{
        BENCHMARK_MATCH_SEED, BenchmarkSetupOptions, apply_benchmark_setup,
        castle_fight_benchmark_match_config,
    },
};

#[cfg(test)]
use castle_fight_protocol::{CommandAcknowledgement, ServerMessage};
#[cfg(test)]
use castle_fight_sim::{CanonicalStreamRecord, SimPoint};

use bridge::{PresentationSamples, PresentationSnapshot};
use build_ui::{ActionPanelState, BuildUiPlugin};
use builder_controls::BuilderControlPlugin;
use cursor::CursorPresentationPlugin;
use debug_menu::DebugMenuPlugin;
use demo::{BuildKind, DEVELOPMENT_MATCH_SEED, create_demo_world_for_match_config};
use doodads::DoodadPresentationPlugin;
use inspection::InspectionPlugin;
use item_ui::ItemUiPlugin;
use lobby::{LobbyPlugin, LobbyState};
use main_menu::{MainMenuPlugin, MainMenuState};
use network::NetworkClient;
use performance_ui::{
    PerformanceCounters, PerformanceUiPlugin, SystemTraceDisplay, SystemTraceSink,
    performance_trace_layer,
};
use presentation::CastlePresentationPlugin;
use render_audit::{
    RenderAudit, RenderAuditPlugin, RenderExperiment, SceneCensus, format_render_passes,
};
use render_tuning::StandardMaterialBindlessSlabPlugin;
use resource_ui::ResourceUiPlugin;
use sim_thread::{Movable, SimThreadPlugin};
use terrain::{TerrainSurface, TerrainTextureLayout, TerrainTextureSet, client_asset_root};
use view_state::{
    ViewState, ViewStatePersistence, persist_view_state, sync_cursor_grab, toggle_fullscreen,
};

// Bevy's glTF loader loads textures through `TaskPool::scope`, which runs other queued IO tasks on
// the waiting thread. Each queued glTF load can therefore nest inside the previous one, so stack
// depth scales with the number of glTFs queued per IO thread (about 22 KiB per level, and a
// single IO thread on 4-core machines). The generated set has ~1150 glTFs; 64 MiB covers that
// with margin, and untouched stack pages are never committed.
const ASSET_IO_STACK_BYTES: usize = 64 * 1024 * 1024;

enum AuthorityMode {
    Local,
    Network {
        client: Box<NetworkClient>,
        match_config: CastleFightMatchConfig,
        assigned_player: PlayerId,
        next_sequence: u64,
        connected: bool,
        pending_handoff_position: Option<u64>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientCommandSubmission {
    Local(CommandSubmission),
    Submitted { client_sequence: u64 },
    Failed,
}

struct SnapshotCatchUp {
    begin: SnapshotTransferBegin,
    bytes: Vec<u8>,
    next_chunk_index: u32,
    snapshot_loaded: bool,
}

#[derive(Resource)]
pub(crate) struct AuthoritativeSimulation {
    /// Absent while the offline simulation thread owns it; see `sim_thread`.
    simulation: Movable<Simulation>,
    driver: Movable<MatchDriver>,
    /// Fixed steps counted since the last launch of the simulation thread (offline only).
    due_ticks: u32,
    pending_feedback: Vec<CommandExecution>,
    pending_build_commands: build_orders::PendingBuildCommands,
    pending_status: Vec<String>,
    expected_execution_batch: Option<(u64, Vec<WireCommandExecution>)>,
    /// Server records and checks received this frame, applied by the next simulation job.
    network_queue: Vec<network_sim::NetItem>,
    catch_up: Option<SnapshotCatchUp>,
    authority: AuthorityMode,
    network_lobby_status: Option<castle_fight_protocol::LobbyStatus>,
    network_lobby_error: Option<String>,
    /// Why the connection to the server is down, shown by the connection banner.
    connection_issue: Option<String>,
    /// Chat lines received from the server, waiting to be shown.
    pending_chat: Vec<chat::IncomingChat>,
    /// The host removed this player from the lobby.
    kicked: bool,
    commands_enabled: bool,
}

impl AuthoritativeSimulation {
    fn new(simulation: Simulation, content: &'static CastleFightContentBundle) -> Self {
        let driver = MatchDriver::new(&simulation, content);
        Self {
            simulation: Movable::new(simulation, "simulation"),
            driver: Movable::new(driver, "driver"),
            due_ticks: 0,
            pending_feedback: Vec::new(),
            pending_build_commands: default(),
            pending_status: Vec::new(),
            expected_execution_batch: None,
            network_queue: Vec::new(),
            catch_up: None,
            authority: AuthorityMode::Local,
            network_lobby_status: None,
            network_lobby_error: None,
            connection_issue: None,
            pending_chat: Vec::new(),
            kicked: false,
            commands_enabled: true,
        }
    }

    fn new_networked(
        simulation: Simulation,
        content: &'static CastleFightContentBundle,
        match_config: CastleFightMatchConfig,
        client: NetworkClient,
        assigned_player: PlayerId,
        next_sequence: u64,
    ) -> Self {
        let driver = MatchDriver::new(&simulation, content);
        Self {
            simulation: Movable::new(simulation, "simulation"),
            driver: Movable::new(driver, "driver"),
            due_ticks: 0,
            pending_feedback: Vec::new(),
            pending_build_commands: default(),
            pending_status: Vec::new(),
            expected_execution_batch: None,
            network_queue: Vec::new(),
            catch_up: None,
            authority: AuthorityMode::Network {
                client: Box::new(client),
                match_config,
                assigned_player,
                next_sequence,
                connected: true,
                pending_handoff_position: None,
            },
            network_lobby_status: None,
            network_lobby_error: None,
            connection_issue: None,
            pending_chat: Vec::new(),
            kicked: false,
            commands_enabled: true,
        }
    }

    #[must_use]
    fn is_networked(&self) -> bool {
        matches!(self.authority, AuthorityMode::Network { .. })
    }

    pub(crate) fn debug_available(&self) -> bool {
        if self.simulation.lifecycle() != castle_fight_sim::MatchLifecycle::Running {
            return false;
        }
        match &self.authority {
            AuthorityMode::Local => self.commands_enabled,
            AuthorityMode::Network {
                assigned_player,
                connected,
                ..
            } => {
                *connected
                    && self.catch_up.is_none()
                    && self.network_lobby_status.as_ref().is_some_and(|status| {
                        status.started && status.host_player_id == assigned_player.0
                    })
            }
        }
    }

    pub(crate) fn submit_debug_request(
        &mut self,
        request: castle_fight_protocol::DebugRequest,
    ) -> Result<(), String> {
        if !self.debug_available() {
            return Err(
                "Debug commands are available only to the connected host during a match.".into(),
            );
        }
        if matches!(request, castle_fight_protocol::DebugRequest::StepOneTick)
            && !self
                .network_lobby_status
                .as_ref()
                .is_some_and(|status| status.debug_paused)
        {
            return Err("Pause the simulation before single-stepping.".into());
        }
        self.send_network_lobby_request(ClientMessage::Debug { request })
    }

    pub(crate) fn apply_local_debug_command(
        &mut self,
        command: castle_fight_sim::debug::DebugCommand,
    ) -> Result<(), CanonicalStreamError> {
        self.driver
            .emit_local_control(
                &mut self.simulation,
                castle_fight_sim::MatchControlEvent::Debug(command),
            )
            .map(|_| ())
    }

    pub(crate) fn request_network_start(&mut self) -> Result<(), String> {
        self.send_network_lobby_request(ClientMessage::StartMatch)
    }

    pub(crate) fn request_network_match_rules(
        &mut self,
        rules: castle_fight_sim::MatchRules,
    ) -> Result<(), String> {
        self.send_network_lobby_request(ClientMessage::SelectMatchRules { rules })
    }

    pub(crate) fn request_network_race(&mut self, builder_rawcode: u32) -> Result<(), String> {
        self.send_network_lobby_request(ClientMessage::SelectRace { builder_rawcode })
    }

    pub(crate) fn request_network_team_size(&mut self, team_size: u8) -> Result<(), String> {
        self.send_network_lobby_request(ClientMessage::SelectTeamSize { team_size })
    }

    pub(crate) fn request_network_slot(&mut self, player_id: u8) -> Result<(), String> {
        self.send_network_lobby_request(ClientMessage::SelectSlot { player_id })
    }

    /// Frees this player's lobby slot for good, as opposed to a dropped connection that keeps it
    /// reserved for a reconnect.
    pub(crate) fn request_network_leave_lobby(&mut self) -> Result<(), String> {
        self.send_network_lobby_request(ClientMessage::LeaveLobby)
    }

    pub(crate) fn request_network_kick(&mut self, player_id: u8) -> Result<(), String> {
        self.send_network_lobby_request(ClientMessage::KickPlayer { player_id })
    }

    /// Sends a chat line; chat exists only in networked games.
    pub(crate) fn request_chat(&mut self, text: String, team_only: bool) -> Result<(), String> {
        self.send_network_lobby_request(ClientMessage::Chat { text, team_only })
    }

    pub(crate) fn assigned_player(&self) -> Option<PlayerId> {
        match &self.authority {
            AuthorityMode::Network {
                assigned_player, ..
            } => Some(*assigned_player),
            AuthorityMode::Local => None,
        }
    }

    /// The display name a player gave when joining. Only networked matches know names.
    pub(crate) fn player_name(&self, player: PlayerId) -> Option<&str> {
        self.network_lobby_status
            .as_ref()?
            .participants
            .iter()
            .find(|participant| participant.player_id == player.0)
            .map(|participant| participant.name.as_str())
            .filter(|name| !name.is_empty())
    }

    fn send_network_lobby_request(&mut self, message: ClientMessage) -> Result<(), String> {
        let result = match &mut self.authority {
            AuthorityMode::Network {
                client,
                connected: true,
                ..
            } => client.send(message).map_err(|error| error.to_string()),
            AuthorityMode::Network { .. } => Err("network session is unavailable".to_owned()),
            AuthorityMode::Local => Err("match is not connected to a server".to_owned()),
        };
        self.network_lobby_error = result.as_ref().err().cloned();
        result
    }

    pub(crate) fn submit_local_command(
        &mut self,
        player: PlayerId,
        command: PlayerCommand,
    ) -> ClientCommandSubmission {
        if !self.commands_enabled {
            return ClientCommandSubmission::Failed;
        }
        if self.debug_available()
            && let AuthorityMode::Network {
                assigned_player, ..
            } = &self.authority
            && player != *assigned_player
        {
            return match self.submit_debug_request(castle_fight_protocol::DebugRequest::Command {
                command: castle_fight_protocol::WireDebugCommand::PlayerCommand {
                    player: player.0,
                    command: command.into(),
                },
            }) {
                Ok(()) => ClientCommandSubmission::Submitted { client_sequence: 0 },
                Err(error) => {
                    self.pending_status.push(error);
                    ClientCommandSubmission::Failed
                }
            };
        }
        let submission = match &mut self.authority {
            AuthorityMode::Local => ClientCommandSubmission::Local(
                self.driver
                    .submit_local_command(&self.simulation, player, command),
            ),
            AuthorityMode::Network {
                client,
                assigned_player,
                next_sequence,
                connected,
                ..
            } => {
                if !*connected || player != *assigned_player {
                    self.pending_status
                        .push("Command not sent: network session is unavailable.".to_owned());
                    return ClientCommandSubmission::Failed;
                }
                let client_sequence = *next_sequence;
                let request = ClientMessage::SubmitCommand {
                    request: CommandRequest {
                        client_sequence,
                        observed_completed_tick: self.simulation.tick().checked_sub(1),
                        command: command.into(),
                    },
                };
                match client.send(request) {
                    Ok(()) => {
                        *next_sequence = next_sequence
                            .checked_add(1)
                            .expect("client command sequence exhausted");
                        ClientCommandSubmission::Submitted { client_sequence }
                    }
                    Err(error) => {
                        *connected = false;
                        self.pending_build_commands.clear();
                        self.pending_status
                            .push(format!("Command not sent: {error}."));
                        ClientCommandSubmission::Failed
                    }
                }
            }
        };
        self.pending_build_commands
            .record(player, command, submission);
        submission
    }
}

#[derive(Resource)]
pub(crate) struct SelectedMatch {
    pub(crate) content: &'static CastleFightContentBundle,
    pub(crate) direct_buildings: Vec<BuildKind>,
    pub(crate) local_player: PlayerId,
}

#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SimulationPlayback {
    pub(crate) paused: bool,
}

impl SimulationPlayback {
    pub(crate) fn interpolation_alpha(
        self,
        fixed_time: &Time<Fixed>,
        samples: &PresentationSamples,
    ) -> f32 {
        if let Some(alpha) = samples.network_alpha() {
            alpha
        } else if self.paused {
            1.0
        } else {
            samples
                .render_overstep()
                .unwrap_or_else(|| fixed_time.overstep_fraction())
        }
    }
}

#[derive(Component)]
struct SimulationPausePanel;

#[derive(Component)]
struct SimulationPauseText;

fn main() {
    raise_open_file_limit();
    configure_asset_io_task_pool();
    let options = ClientOptions::parse();
    let mut client_settings = client_settings::ClientSettings::load();
    if let Some(theme) = options.profile_theme {
        client_settings.ui_theme = theme;
    }
    if options.list_map_versions {
        print_registered_map_versions();
        return;
    }
    if options.server.is_some() && options.stress_units.is_some() {
        eprintln!(
            "--stress-units is an offline presentation fixture and cannot be used with --server"
        );
        std::process::exit(2);
    }
    if options.server.is_some() && options.stress_visual.is_some() {
        eprintln!(
            "--stress-visual is an offline presentation fixture and cannot be used with --server"
        );
        std::process::exit(2);
    }
    if options.server.is_some() && options.profile_quicksave.is_some() {
        eprintln!("--profile-quicksave is an offline mode and cannot be used with --server");
        std::process::exit(2);
    }
    if options.stress_units.is_some() && options.profile_quicksave.is_some() {
        eprintln!("--profile-quicksave cannot be combined with --stress-units");
        std::process::exit(2);
    }
    if options.stress_visual.is_some() && options.profile_quicksave.is_some() {
        eprintln!("--profile-quicksave cannot be combined with --stress-visual");
        std::process::exit(2);
    }
    if options.benchmark
        && (options.server.is_some()
            || options.stress_units.is_some()
            || options.stress_visual.is_some()
            || options.profile_quicksave.is_some())
    {
        eprintln!(
            "--benchmark is an offline profile and cannot be combined with --server, --stress-units, --stress-visual, or --profile-quicksave"
        );
        std::process::exit(2);
    }
    let match_config = client_match_config(&options).unwrap_or_else(|error| {
        eprintln!(
            "cannot configure Castle Fight {}/{}: {error}",
            options.map_version, options.release_revision
        );
        std::process::exit(2);
    });
    let mut demo =
        create_demo_world_for_match_config(options.sim_workers, options.stress_units, match_config)
            .unwrap_or_else(|error| {
                eprintln!(
                    "cannot start Castle Fight {}/{}: {error}",
                    options.map_version, options.release_revision
                );
                std::process::exit(2);
            });
    if options.benchmark {
        let mana_randomized =
            apply_benchmark_setup(&mut demo.simulation, BenchmarkSetupOptions::default());
        println!(
            "client-benchmark setup: players-per-team={} mana-randomized={mana_randomized} initial-units={} initial-buildings={}",
            options.team_size,
            demo.simulation.unit_count(),
            demo.simulation.building_count(),
        );
    }
    if let Some(path) = &options.profile_quicksave {
        let completed_tick = restore_simulation_quicksave(&mut demo.simulation, demo.content, path)
            .unwrap_or_else(|error| {
                eprintln!("cannot load profiling quicksave: {error}");
                std::process::exit(2);
            });
        println!(
            "client-profile loaded_tick={} source={} warmup={:.3}s duration={:.3}s",
            completed_tick
                .map(|tick| tick.to_string())
                .unwrap_or_else(|| "initial".into()),
            path.display(),
            options.profile_warmup.as_secs_f64(),
            options.profile_duration.as_secs_f64(),
        );
    }
    let (local_player, network_client) = if let Some(server_address) = options.server {
        let compatibility = compatibility_identity_for_demo(&demo);
        let (client, assignment) = NetworkClient::connect(
            server_address,
            compatibility,
            u64::from(std::process::id()),
            &network::player_name_for(&client_settings.player_name),
        )
        .unwrap_or_else(|error| {
            eprintln!("cannot join Castle Fight server {server_address}: {error}");
            std::process::exit(2);
        });
        if assignment.next_stream_position != 0
            || assignment.next_client_sequence != 0
            || assignment.completed_tick.is_some()
        {
            eprintln!("server attempted an unsupported late initial join handoff");
            std::process::exit(2);
        }
        let local_player = PlayerId(assignment.player_id);
        let expected_team = demo
            .match_config
            .participants
            .iter()
            .find(|participant| participant.id == local_player)
            .map(|participant| participant.team.0);
        if expected_team != Some(assignment.team) {
            eprintln!(
                "server assigned player {} to unexpected team {}",
                assignment.player_id, assignment.team
            );
            std::process::exit(2);
        }
        (
            local_player,
            Some((client, assignment.next_client_sequence)),
        )
    } else {
        let local_player = demo
            .match_config
            .participants
            .first()
            .expect("playable Castle Fight match must contain a local participant")
            .id;
        (local_player, None)
    };
    let observer_team = (!options.is_profiling()
        && options.stress_units.is_none()
        && options.stress_visual.is_none())
    .then(|| {
        demo.simulation
            .player(local_player)
            .expect("local player exists")
            .team
    });
    let initial_snapshot = PresentationSnapshot::capture(&demo.simulation);
    let present_mode = if options.stress_units.is_some()
        || options.stress_visual.is_some()
        || options.profile_quicksave.is_some()
        || options.benchmark
    {
        PresentMode::AutoNoVsync
    } else {
        PresentMode::AutoVsync
    };
    let network_suffix = options
        .server
        .map_or(String::new(), |server| format!(" — server {server}"));
    let window_title = format!(
        "Castle Fight Native 3D — CF {}/{} ({}){}",
        demo.match_config.release.map_version,
        demo.match_config.release.release_revision,
        demo.match_config
            .release
            .content_revision
            .unwrap_or("archived-only"),
        network_suffix,
    );

    let terrain_texture_layout =
        TerrainTextureLayout::from_wc3_terrain_json(demo.terrain_source_json)
            .expect("selected Warcraft terrain texture layout must be valid");
    let terrain_textures = TerrainTextureSet::load_default();

    let networked = network_client.is_some();
    let mut authoritative = if let Some((client, next_sequence)) = network_client {
        AuthoritativeSimulation::new_networked(
            demo.simulation,
            demo.content,
            demo.match_config,
            client,
            local_player,
            next_sequence,
        )
    } else {
        AuthoritativeSimulation::new(demo.simulation, demo.content)
    };

    let view_state = ViewState::load();
    let interactive_frontend = options.profile_quicksave.is_none()
        && options.stress_units.is_none()
        && options.stress_visual.is_none()
        && !options.profile;
    let show_main_menu = interactive_frontend && !networked;
    authoritative.commands_enabled = !show_main_menu;
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.025, 0.03, 0.04)))
        .insert_resource(client_settings)
        .insert_resource(ViewStatePersistence::new(view_state))
        .insert_resource(Time::<Fixed>::from_hz(f64::from(
            CASTLE_FIGHT_SIMULATION_HZ,
        )))
        .insert_resource(SelectedMatch {
            content: demo.content,
            direct_buildings: demo.direct_buildings,
            local_player,
        })
        .insert_resource(authoritative)
        .insert_resource(SimulationPlayback {
            // The benchmark runs the battle through warmup as well, so it is never paused.
            paused: show_main_menu || (options.is_profiling() && !options.benchmark),
        })
        .insert_resource(PresentationSamples::new(initial_snapshot).with_observer(observer_team))
        .insert_resource(demo.metrics)
        .insert_resource(TerrainSurface::new(demo.terrain))
        .insert_resource(terrain_texture_layout)
        .insert_resource(terrain_textures)
        .insert_resource(options.render_experiment)
        .insert_resource(options.perf_disable)
        .add_plugins(
            DefaultPlugins
                .set(bevy::render::RenderPlugin {
                    render_creation: bevy::render::settings::RenderCreation::Automatic(Box::new(
                        bevy::render::settings::WgpuSettings {
                            // Profiling-only comparison of texture-array validation/binding
                            // overhead against Bevy's ordinary material fallback path.
                            disabled_features: (options.render_experiment
                                == RenderExperiment::NoBindless)
                                .then_some(
                                    bevy::render::settings::WgpuFeatures::TEXTURE_BINDING_ARRAY,
                                ),
                            ..default()
                        },
                    )),
                    ..default()
                })
                .set(bevy::log::LogPlugin {
                    custom_layer: performance_trace_layer,
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: client_asset_root().to_string_lossy().into_owned(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: window_title,
                        resolution: (view_state.width, view_state.height).into(),
                        mode: view_state.window_mode(),
                        position: view_state.window_position(),
                        present_mode,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(StandardMaterialBindlessSlabPlugin::new(
            options.render_experiment.standard_material_bindless_slots(),
        ))
        .add_plugins(compact_material::CompactMaterialPlugin)
        .add_plugins(skin_influences::SkinInfluencePlugin)
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins((
            skinned_bounds::SkinnedMeshBoundsPlugin,
            offscreen_animation::OffscreenAnimationPlugin,
            scene_budget::SceneBudgetPlugin,
        ))
        .add_plugins((
            console::ConsolePlugin,
            CastlePresentationPlugin::new(options.health_bars),
            DoodadPresentationPlugin,
            BuildUiPlugin,
            CursorPresentationPlugin,
            ResourceUiPlugin,
            match_ui::MatchUiPlugin,
            scoreboard_ui::ScoreboardPlugin,
            PerformanceUiPlugin,
            InspectionPlugin,
            BuilderControlPlugin,
            DebugMenuPlugin,
            notifications::NotificationsPlugin,
            alerts::AlertsPlugin,
            minimap::MinimapPlugin,
        ))
        .add_plugins(ItemUiPlugin)
        .add_message::<QuickSaveRequest>()
        .add_systems(Startup, setup_simulation_pause_ui)
        .add_systems(
            Update,
            (toggle_fullscreen, sync_cursor_grab, persist_view_state).chain(),
        )
        .add_systems(
            Update,
            (
                handle_quicksave_hotkeys,
                toggle_simulation_pause,
                update_simulation_pause_ui,
                sync_local_command_feedback,
            )
                .chain(),
        )
        .add_systems(
            PostUpdate,
            (
                wc3_effects::throttle_gameplay_animation_poses,
                wc3_effects::skip_unchanged_paused_animation_poses,
            )
                .chain()
                .after(bevy::animation::advance_animations)
                .before(bevy::animation::animate_targets),
        )
        .add_systems(
            PostUpdate,
            wc3_effects::apply_wc3_geoset_visibility
                .after(bevy::app::AnimationSystems)
                .before(bevy::transform::TransformSystems::Propagate),
        )
        .add_plugins(SimThreadPlugin);

    if interactive_frontend {
        app.add_plugins((
            LobbyPlugin,
            game_menu::GameMenuPlugin,
            settings_menu::SettingsMenuPlugin,
            menu_focus::MenuFocusPlugin,
            chat::ChatPlugin,
        ));
    }
    if show_main_menu {
        app.insert_resource(MainMenuState::new(options.clone()))
            .add_plugins(MainMenuPlugin);
    }

    if options.stress_units.is_some() || options.stress_visual.is_some() || options.is_profiling() {
        app.insert_resource(presentation::BenchmarkCamera {
            lock_input: options.is_profiling(),
        });
    }
    if let Some(stress_visual) = options.stress_visual {
        app.insert_resource(stress_visual);
    }
    if let Some(capture) = options.capture_frames.clone() {
        app.add_plugins(frame_capture::FrameCapturePlugin(capture));
    }

    if options.perf_log {
        app.insert_resource(PerfTelemetry(Timer::from_seconds(
            1.0,
            TimerMode::Repeating,
        )))
        .add_systems(Update, print_perf_telemetry);
    }
    if options.is_profiling() {
        app.add_plugins(RenderAuditPlugin);
        let source = options.profile_quicksave.as_ref().map_or_else(
            || {
                if options.benchmark {
                    "benchmark".to_owned()
                } else if let Some(stress) = options.stress_visual {
                    let rawcode = stress.rawcode.to_be_bytes();
                    format!(
                        "stress-visual={}:{}",
                        String::from_utf8_lossy(&rawcode),
                        stress.count
                    )
                } else {
                    format!(
                        "stress-units={}",
                        options.stress_units.expect("profiling needs a scene")
                    )
                }
            },
            |path| path.display().to_string(),
        );
        if let Some(selection) = options.profile_select {
            app.insert_resource(selection)
                .add_systems(Update, apply_profile_selection);
        }
        if options.profile_items || options.profile_shop {
            app.insert_resource(ProfileItemFixture {
                items: options.profile_items,
                shop: options.profile_shop,
            })
            .add_systems(Update, apply_profile_item_fixture);
        }
        app.insert_resource(AutomatedProfileRun {
            source,
            warmup: options.profile_warmup,
            duration: options.profile_duration,
            until_tick: options.profile_until_tick,
            warmup_started: None,
            capture_started: None,
            paused: options.profile_paused,
            experiment: options.render_experiment,
            screenshot: options.profile_screenshot.clone(),
            finished: None,
        })
        .add_systems(
            Update,
            finish_automated_profile.after(performance_ui::finish_presentation_profile),
        );
    }

    if !networked {
        let timestep = app.world().resource::<Time<Fixed>>().timestep();
        fixed_catch_up::limit_offline_catch_up(
            app.world_mut().resource_mut::<Time<Virtual>>().as_mut(),
            timestep,
        );
    }
    app.run();
}

/// Selects something once the match has started, so screenshots show the selection UI.
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileSelection {
    Builder,
    Units,
    Building,
}

fn apply_profile_selection(
    request: Res<ProfileSelection>,
    presentation: Res<PresentationSamples>,
    selected_match: Res<SelectedMatch>,
    mut selection: ResMut<inspection::InspectionSelection>,
    mut done: Local<bool>,
) {
    if *done || presentation.current.tick < 1 {
        return;
    }
    let local = selected_match.local_player;
    let ids: Vec<_> = match *request {
        ProfileSelection::Builder => presentation
            .current
            .builders
            .values()
            .filter(|builder| builder.owner == local)
            .map(|builder| builder.id)
            .take(1)
            .collect(),
        ProfileSelection::Units => presentation
            .current
            .units
            .values()
            .filter(|unit| unit.owner == local)
            .map(|unit| unit.id)
            .take(6)
            .collect(),
        ProfileSelection::Building => presentation
            .current
            .buildings
            .values()
            .filter(|building| building.owner == Some(local))
            .map(|building| building.id)
            .take(1)
            .collect(),
    };
    if !ids.is_empty() {
        selection.replace(ids);
        *done = true;
    }
}

#[derive(Resource)]
struct ProfileItemFixture {
    items: bool,
    shop: bool,
}

/// Item UI screenshots: carry a spread of items, grant funds, leave an item on the ground and/or
/// open the shop spellbook for the builder the selection fixture picked.
fn apply_profile_item_fixture(
    fixture: Res<ProfileItemFixture>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut panel: ResMut<ActionPanelState>,
    selected_match: Res<SelectedMatch>,
    mut done: Local<bool>,
) {
    use castle_fight_sim::items::ItemId;
    let Some(actor) = panel.actor.filter(|_| !*done) else {
        return;
    };
    let Some(builder) = authoritative.simulation.builder(actor) else {
        return;
    };
    *done = true;
    if fixture.items {
        // The profile camera is fixed over the stress crowd, so bring the builder into it.
        let stage = castle_fight_sim::SimPoint::new(
            -1_000 * castle_fight_sim::SUBUNITS_PER_WORLD_UNIT,
            -200 * castle_fight_sim::SUBUNITS_PER_WORLD_UNIT,
        );
        authoritative.simulation.debug_place_builder(actor, stage);
        let builder = authoritative.simulation.builder(actor).unwrap_or(builder);
        let item = |code: &[u8; 4]| ItemId(u32::from_be_bytes(*code));
        for (code, charges) in [
            (b"I009", 0),
            (b"I006", 0),
            (b"I001", 0),
            (b"I001", 0),
            (b"I001", 0),
            (b"I001", 0),
            (b"I005", 2),
            (b"I00F", 1),
        ] {
            authoritative
                .simulation
                .debug_grant_item(actor, item(code), charges);
        }
        // Enemies inside Blast Staff range, so the missiles are visible in flight.
        for (index, kind) in castle_fight_sim::CastleFightUnitKind::ALL
            .iter()
            .take(6)
            .enumerate()
        {
            let definition = selected_match
                .content
                .unit(*kind)
                .expect("fixture roster must belong to selected content bundle");
            authoritative.simulation.spawn_resolved_unit(
                castle_fight_sim::Team(1 - builder.team.0),
                castle_fight_sim::SimPoint::new(
                    builder.position.x
                        + (300 + 60 * index as i32) * castle_fight_sim::SUBUNITS_PER_WORLD_UNIT,
                    builder.position.y
                        + (index as i32 - 3) * 80 * castle_fight_sim::SUBUNITS_PER_WORLD_UNIT,
                ),
                definition.resolved(),
            );
        }
        // Let the staff cooldowns elapse so missiles are in flight when the capture lands.
        for _ in 0..40 {
            authoritative.simulation.step();
        }
        authoritative.simulation.debug_spawn_ground_item(
            item(b"I003"),
            0,
            castle_fight_sim::SimPoint::new(
                builder.position.x + 350 * castle_fight_sim::SUBUNITS_PER_WORLD_UNIT,
                builder.position.y,
            ),
        );
    }
    if fixture.shop {
        authoritative.simulation.debug_grant_player_resources_for(
            selected_match.local_player,
            5_000,
            500,
        );
        panel.mode = build_ui::ActionPanelMode::Shop;
    }
}

#[derive(Resource)]
struct AutomatedProfileRun {
    source: String,
    warmup: Duration,
    duration: Duration,
    /// When set, the capture ends at this simulation tick and `duration` is not used.
    until_tick: Option<u64>,
    warmup_started: Option<Instant>,
    capture_started: Option<Instant>,
    paused: bool,
    experiment: RenderExperiment,
    screenshot: Option<PathBuf>,
    finished: Option<Instant>,
}

#[derive(SystemParam)]
struct ProfileSystemTrace<'w> {
    /// Rolling one-second per-system averages shown by the in-game panel.
    window: Res<'w, SystemTraceDisplay>,
    /// Absent when no trace layer is installed.
    capture: Option<Res<'w, SystemTraceSink>>,
}

fn finish_automated_profile(
    mut run: ResMut<AutomatedProfileRun>,
    mut counters: ResMut<PerformanceCounters>,
    presentation: Res<PresentationSamples>,
    trace: ProfileSystemTrace,
    mut playback: ResMut<SimulationPlayback>,
    mut exit: MessageWriter<AppExit>,
    mut audit: (
        Res<RenderAudit>,
        Res<DiagnosticsStore>,
        SceneCensus,
        Commands,
    ),
) {
    let now = Instant::now();
    if let Some(finished) = run.finished {
        if now.duration_since(finished) > Duration::from_secs(10) {
            eprintln!("profile screenshot capture timed out");
            exit.write(AppExit::error());
        }
        return;
    }
    let warmup_started = *run.warmup_started.get_or_insert(now);
    if run.capture_started.is_none() {
        if now.duration_since(warmup_started) < run.warmup {
            return;
        }
        playback.paused = run.paused;
        counters.start_capture();
        if let Some(trace_sink) = trace.capture.as_deref() {
            trace_sink.begin_capture();
        }
        audit.0.start_capture();
        run.capture_started = Some(now);
        return;
    }
    let capture_started = run
        .capture_started
        .expect("capture start is set after automated profile warmup");
    let capture_finished = match run.until_tick {
        Some(until_tick) => presentation.current.tick >= until_tick,
        None => now.duration_since(capture_started) >= run.duration,
    };
    if !capture_finished || !counters.capture_has_frame_sample() {
        return;
    }
    let capture_wall = now.duration_since(capture_started);
    let report = counters
        .finish_capture()
        .expect("automated profiling run must own an active performance capture");
    println!(
        "client-profile source={} final_tick={} builders={} units={} buildings={} corpses={} projectiles={}",
        run.source,
        presentation.current.tick,
        presentation.current.builders.len(),
        presentation.current.units.len(),
        presentation.current.buildings.len(),
        presentation.current.corpses.len(),
        presentation.current.projectiles.len(),
    );
    println!(
        "client-profile paused={} experiment={:?} capture_wall={:.3}s until_tick={}",
        run.paused,
        run.experiment,
        capture_wall.as_secs_f64(),
        run.until_tick
            .map_or_else(|| "none".to_owned(), |tick| tick.to_string()),
    );
    print!("{}", report.format());
    print!("{}", audit.0.format());
    print!("{}", audit.2.format());
    print!("{}", format_render_passes(&audit.1));
    print!("{}", trace.window.format());
    match trace.capture.as_deref() {
        Some(trace_sink) => print!("{}", trace_sink.format_capture(report.frame_count())),
        None => println!("SYSTEM CPU  whole capture: no trace sink"),
    }
    run.finished = Some(now);
    if let Some(path) = run.screenshot.take() {
        use bevy::render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk};
        audit
            .3
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path))
            .observe(
                |_: On<ScreenshotCaptured>, mut exit: MessageWriter<AppExit>| {
                    exit.write(AppExit::Success);
                },
            );
    } else {
        exit.write(AppExit::Success);
    }
}

#[derive(Resource)]
struct PerfTelemetry(Timer);

fn print_perf_telemetry(
    time: Res<Time>,
    diagnostics: Res<DiagnosticsStore>,
    mut telemetry: ResMut<PerfTelemetry>,
    presentation: Res<PresentationSamples>,
) {
    if !telemetry.0.tick(time.delta()).just_finished() {
        return;
    }
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|diagnostic| diagnostic.smoothed());
    let frame_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|diagnostic| diagnostic.smoothed());
    println!(
        "client-perf fps={:.1} frame_ms={:.2} builders={} units={} buildings={} corpses={} projectiles={}",
        fps.unwrap_or_default(),
        frame_ms.unwrap_or_default(),
        presentation.current.builders.len(),
        presentation.current.units.len(),
        presentation.current.buildings.len(),
        presentation.current.corpses.len(),
        presentation.current.projectiles.len(),
    );
}

#[derive(Debug, Clone)]
struct ClientOptions {
    stress_units: Option<usize>,
    stress_visual: Option<presentation::ProfileVisualStress>,
    health_bars: bool,
    perf_log: bool,
    profile_quicksave: Option<PathBuf>,
    profile_warmup: Duration,
    profile_duration: Duration,
    /// Ends the automated capture at this simulation tick instead of after `profile_duration`.
    profile_until_tick: Option<u64>,
    profile_paused: bool,
    profile: bool,
    /// Offline all-buildings benchmark: runs the `sim-bench` match under the automated profile.
    benchmark: bool,
    profile_screenshot: Option<PathBuf>,
    /// Interface theme for this run only; the saved setting is left alone.
    profile_theme: Option<ui_icons::ConsoleTheme>,
    /// What to select for a screenshot of the info panel and command card.
    profile_select: Option<ProfileSelection>,
    /// Stocks the local builder's inventory and lays an item on the ground for a screenshot.
    profile_items: bool,
    /// Opens the castle shop spellbook for a screenshot.
    profile_shop: bool,
    render_experiment: RenderExperiment,
    /// Default-on performance optimisations switched off for visual bisection.
    perf_disable: perf_disable::PerfDisable,
    /// Saves consecutive rendered frames as PNGs and exits.
    capture_frames: Option<frame_capture::FrameCaptureOptions>,
    map_version: MapVersion,
    release_revision: String,
    match_seed: u64,
    rules: castle_fight_sim::MatchRules,
    team_size: usize,
    builder_rawcodes: [Option<u32>; 9],
    server: Option<std::net::SocketAddr>,
    list_map_versions: bool,
    /// Simulation worker threads for the offline match created at startup.
    sim_workers: usize,
}

impl ClientOptions {
    fn is_profiling(&self) -> bool {
        self.profile || self.profile_quicksave.is_some()
    }

    fn parse() -> Self {
        let mut options = Self {
            stress_units: None,
            stress_visual: None,
            health_bars: true,
            perf_log: false,
            profile_quicksave: None,
            profile_warmup: Duration::from_secs(5),
            profile_duration: Duration::from_secs(10),
            profile_until_tick: None,
            profile_paused: false,
            profile: false,
            benchmark: false,
            profile_screenshot: None,
            profile_theme: None,
            profile_select: None,
            profile_items: false,
            profile_shop: false,
            render_experiment: RenderExperiment::Baseline,
            perf_disable: perf_disable::PerfDisable::default(),
            capture_frames: None,
            map_version: MapVersion::CASTLE_FIGHT_9_27,
            release_revision: "r1".to_owned(),
            match_seed: DEVELOPMENT_MATCH_SEED,
            rules: castle_fight_sim::MatchRules::for_version(MapVersion::CASTLE_FIGHT_9_27),
            team_size: 1,
            builder_rawcodes: [None; 9],
            server: None,
            list_map_versions: false,
            sim_workers: default_worker_count(),
        };
        let mut explicit_warmup = false;
        let mut explicit_duration = false;
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--benchmark" => options.benchmark = true,
                "--stress-units" => {
                    let value = args
                        .next()
                        .expect("--stress-units requires a non-negative integer");
                    options.stress_units = Some(
                        value
                            .parse()
                            .expect("--stress-units requires a non-negative integer"),
                    );
                }
                "--stress-visual" => {
                    let rawcode = args
                        .next()
                        .expect("--stress-visual requires a four-character unit rawcode and count");
                    let count = args
                        .next()
                        .expect("--stress-visual requires a four-character unit rawcode and count")
                        .parse()
                        .expect("--stress-visual count must be a non-negative integer");
                    options.stress_visual = Some(presentation::ProfileVisualStress {
                        rawcode: parse_profile_rawcode(&rawcode)
                            .unwrap_or_else(|error| panic!("{error}")),
                        count,
                    });
                }
                "--no-health-bars" => options.health_bars = false,
                "--perf-log" => options.perf_log = true,
                "--profile-paused" => options.profile_paused = true,
                "--profile" => options.profile = true,
                "--profile-screenshot" => {
                    options.profile_screenshot = Some(PathBuf::from(
                        args.next()
                            .expect("--profile-screenshot requires an output path"),
                    ));
                }
                "--profile-theme" => {
                    let value = args.next().expect("--profile-theme requires a theme name");
                    options.profile_theme = Some(
                        serde_json::from_value(serde_json::Value::String(value.clone()))
                            .unwrap_or_else(|_| panic!("unknown --profile-theme {value:?}")),
                    );
                }
                "--profile-items" => options.profile_items = true,
                "--profile-shop" => options.profile_shop = true,
                "--profile-select" => {
                    let value = args.next().expect("--profile-select requires a target");
                    options.profile_select = Some(match value.as_str() {
                        "builder" => ProfileSelection::Builder,
                        "units" => ProfileSelection::Units,
                        "building" => ProfileSelection::Building,
                        other => panic!("unknown --profile-select {other:?}"),
                    });
                }
                "--render-experiment" => {
                    let value = args.next().expect("--render-experiment requires a name");
                    options.render_experiment =
                        RenderExperiment::parse(&value).unwrap_or_else(|error| panic!("{error}"));
                }
                "--perf-disable" => {
                    let value = args.next().expect("--perf-disable requires a comma list");
                    options.perf_disable = perf_disable::PerfDisable::parse(&value)
                        .unwrap_or_else(|error| panic!("{error}"));
                }
                "--capture-frames" => {
                    const USAGE: &str = "--capture-frames requires FIRST_TICK COUNT DIR";
                    let first_tick = args
                        .next()
                        .and_then(|value| value.parse().ok())
                        .expect(USAGE);
                    let count = args
                        .next()
                        .and_then(|value| value.parse().ok())
                        .filter(|count| *count >= 1)
                        .expect(USAGE);
                    let directory = PathBuf::from(args.next().expect(USAGE));
                    options.capture_frames = Some(frame_capture::FrameCaptureOptions {
                        first_tick,
                        count,
                        directory,
                    });
                }
                "--profile-quicksave" => options.profile_quicksave = Some(quicksave_path()),
                "--profile-quicksave-path" => {
                    options.profile_quicksave = Some(PathBuf::from(
                        args.next()
                            .expect("--profile-quicksave-path requires a snapshot path"),
                    ));
                }
                "--profile-warmup" => {
                    let value = args
                        .next()
                        .expect("--profile-warmup requires a non-negative number of seconds");
                    options.profile_warmup =
                        parse_profile_warmup(&value).unwrap_or_else(|error| panic!("{error}"));
                    explicit_warmup = true;
                }
                "--profile-duration" => {
                    let value = args
                        .next()
                        .expect("--profile-duration requires a positive number of seconds");
                    options.profile_duration =
                        parse_profile_duration(&value).unwrap_or_else(|error| panic!("{error}"));
                    explicit_duration = true;
                }
                "--profile-until-tick" => {
                    let value = args
                        .next()
                        .expect("--profile-until-tick requires a simulation tick number");
                    options.profile_until_tick = Some(
                        value
                            .parse()
                            .expect("--profile-until-tick requires a simulation tick number"),
                    );
                }
                "--sim-workers" => {
                    let value = args
                        .next()
                        .expect("--sim-workers requires a positive worker count");
                    options.sim_workers = value
                        .parse()
                        .ok()
                        .filter(|workers| *workers >= 1)
                        .expect("--sim-workers requires a positive worker count");
                }
                "--map-version" => {
                    let value = args
                        .next()
                        .expect("--map-version requires a version such as 9.27");
                    options.map_version = value
                        .parse()
                        .expect("--map-version requires a version such as 9.27");
                }
                "--map-revision" => {
                    options.release_revision = args
                        .next()
                        .expect("--map-revision requires an exact revision such as r1");
                }
                "--seed" => {
                    options.match_seed = args
                        .next()
                        .expect("--seed requires an unsigned integer")
                        .parse()
                        .expect("--seed requires an unsigned integer");
                }
                "--team-size" => {
                    options.team_size = args
                        .next()
                        .expect("--team-size requires 1, 2, or 3")
                        .parse()
                        .expect("--team-size requires 1, 2, or 3");
                    assert!(
                        (1..=3).contains(&options.team_size),
                        "--team-size requires 1, 2, or 3"
                    );
                }
                "--builder" => {
                    let value = args
                        .next()
                        .expect("--builder requires SLOT:RAWCODE (e.g. 6:X00P)");
                    let (slot, rawcode) = value
                        .split_once(':')
                        .expect("--builder requires SLOT:RAWCODE");
                    let slot = slot.parse::<usize>().expect("builder slot must be numeric");
                    assert!(
                        matches!(slot, 0..=2 | 6..=8),
                        "builder slot must be 0/1/2/6/7/8"
                    );
                    options.builder_rawcodes[slot] = Some(
                        parse_profile_rawcode(rawcode)
                            .expect("builder rawcode must be four ASCII characters"),
                    );
                }
                "--server" => {
                    options.server = Some(
                        args.next()
                            .expect("--server requires an address such as 127.0.0.1:6112")
                            .parse()
                            .expect("--server requires a valid socket address"),
                    );
                }
                "--list-map-versions" => options.list_map_versions = true,
                "-h" | "--help" => {
                    println!(
                        "Usage: cargo run -p castle-fight-client -- [--server 127.0.0.1:6112] [--map-version 9.27] [--map-revision r1] [--seed N] [--team-size 1|2|3] [--builder SLOT:RAWCODE] [--list-map-versions] [--stress-units N] [--stress-visual RAWCODE N] [--no-health-bars] [--perf-log] [--profile-quicksave] [--profile-quicksave-path PATH] [--profile-warmup SECONDS] [--profile-duration SECONDS] [--profile-until-tick TICK] [--sim-workers N] [--profile-paused] [--profile] [--profile-screenshot PATH] [--benchmark] [--perf-disable LIST] [--capture-frames FIRST_TICK COUNT DIR] [--render-experiment baseline|freeze-bounds|hide-skinned|hide-particles|hide-transparent|legacy-team-color|legacy-geoset-visibility|legacy-attachment-search|legacy-attachment-index|legacy-effect-pooling|legacy-splat-updates|legacy-splat-material-state|legacy-animated-alpha-state|legacy-animated-texture-state|freeze-materials|freeze-poses|bindless-auto|bindless-64|bindless-128|bindless-256|no-bindless]"
                    );
                    std::process::exit(0);
                }
                unknown => panic!("unknown client option: {unknown}"),
            }
        }
        if options.benchmark {
            // Matches `sim-bench --scenario all-buildings`: 3v3, fixed seed, the shared
            // benchmark setup, and an automated profile run that exits after its report.
            // --seed, --team-size, and --map-revision are fixed by the benchmark.
            options.team_size = BENCHMARK_PLAYERS_PER_TEAM;
            options.profile = true;
            if !explicit_warmup {
                options.profile_warmup = BENCHMARK_WARMUP;
            }
            if !explicit_duration && options.profile_until_tick.is_none() {
                options.profile_until_tick = Some(BENCHMARK_UNTIL_TICK);
            }
        }
        assert!(
            !options.profile
                || options.benchmark
                || options.stress_units.is_some()
                || options.stress_visual.is_some()
                || options.profile_quicksave.is_some(),
            "--profile requires --stress-units, --stress-visual, --profile-quicksave, or --benchmark",
        );
        assert!(
            options.capture_frames.is_none() || options.is_profiling(),
            "--capture-frames requires --profile, --profile-quicksave, or --benchmark",
        );
        assert!(
            options.is_profiling()
                || (!options.profile_paused
                    && options.render_experiment == RenderExperiment::Baseline
                    && options.profile_screenshot.is_none()
                    && options.profile_select.is_none()
                    && !options.profile_items
                    && !options.profile_shop
                    && options.stress_visual.is_none()
                    && options.profile_until_tick.is_none()),
            "render experiments, --profile-paused, --profile-until-tick, and --stress-visual require --profile, --profile-quicksave, or --benchmark",
        );
        options
    }
}

/// Players per team in the `--benchmark` match, matching `sim-bench` all-buildings.
const BENCHMARK_PLAYERS_PER_TEAM: usize = 3;
const BENCHMARK_WARMUP: Duration = Duration::from_secs(2);
const BENCHMARK_UNTIL_TICK: u64 = 1350;

fn parse_profile_rawcode(value: &str) -> Result<u32, String> {
    let bytes = value.as_bytes();
    if bytes.len() != 4 || !bytes.is_ascii() {
        return Err("--stress-visual rawcode must be exactly four ASCII characters".to_owned());
    }
    Ok(u32::from_be_bytes(
        bytes.try_into().expect("validated four-byte rawcode"),
    ))
}

fn parse_profile_warmup(value: &str) -> Result<Duration, String> {
    let seconds = value
        .parse::<f64>()
        .map_err(|_| "--profile-warmup requires a non-negative number of seconds".to_owned())?;
    if !seconds.is_finite() || seconds < 0.0 {
        return Err("--profile-warmup requires a non-negative finite number of seconds".to_owned());
    }
    Ok(Duration::from_secs_f64(seconds))
}

fn parse_profile_duration(value: &str) -> Result<Duration, String> {
    let seconds = value
        .parse::<f64>()
        .map_err(|_| "--profile-duration requires a positive number of seconds".to_owned())?;
    if !seconds.is_finite() || seconds <= 0.0 {
        return Err("--profile-duration requires a positive finite number of seconds".to_owned());
    }
    Ok(Duration::from_secs_f64(seconds))
}

fn client_match_config(
    options: &ClientOptions,
) -> Result<CastleFightMatchConfig, CastleFightMatchSetupError> {
    if options.benchmark {
        return castle_fight_benchmark_match_config(
            options.map_version,
            options.team_size,
            BENCHMARK_MATCH_SEED,
        );
    }
    let participants = castle_fight_sim::standard_match_slots(options.team_size)
        .ok_or(CastleFightMatchSetupError::UnsupportedParticipants)?
        .into_iter()
        .map(|(id, team)| CastleFightParticipantConfig {
            id,
            team,
            builder_race: CastleFightBuilderRace::Human,
        })
        .collect::<Vec<_>>();
    let content = castle_fight_sim::castle_fight_content_bundle(options.map_version)
        .map_err(CastleFightMatchSetupError::UnsupportedContent)?;
    let participants = participants
        .into_iter()
        .map(|mut participant: CastleFightParticipantConfig| {
            if let Some(rawcode) = options.builder_rawcodes[usize::from(participant.id.0)] {
                participant.builder_race = content
                    .builder_race_for_rawcode(rawcode)
                    .ok_or(CastleFightMatchSetupError::UnsupportedParticipants)?;
            }
            Ok(participant)
        })
        .collect::<Result<Vec<_>, CastleFightMatchSetupError>>()?;
    let mut config = CastleFightMatchConfig::development_subset_with_participants(
        options.map_version,
        &options.release_revision,
        options.match_seed,
        participants,
    )?;
    config.rules = options.rules;
    Ok(config)
}

fn compatibility_identity_for_demo(demo: &demo::DemoWorld) -> CompatibilityIdentity {
    let snapshot = demo.simulation.capture_snapshot();
    CompatibilityIdentity {
        snapshot_schema_version: AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION,
        checksum_schema_version: CANONICAL_CHECKSUM_SCHEMA_VERSION,
        map_version: demo.match_config.release.map_version,
        release_revision: demo.match_config.release.release_revision.to_owned(),
        content_schema_version: demo.content.identity.schema_version,
        content_gameplay_hash: demo.content.identity.gameplay_hash,
        configuration_identity: snapshot.configuration_identity(),
    }
}

fn print_registered_map_versions() {
    println!("Registered Castle Fight releases:");
    for release in castle_fight_registered_releases() {
        let availability = match release.availability {
            CastleFightContentAvailability::SupportedDevelopmentSubset => {
                "playable development subset"
            }
            CastleFightContentAvailability::SupportedFull => "playable full ruleset",
            CastleFightContentAvailability::Archived => "archived, not playable",
            CastleFightContentAvailability::Unavailable => "unavailable",
        };
        let content = release.content_revision.map_or_else(
            || "no runtime content".to_owned(),
            |revision| revision.to_owned(),
        );
        println!(
            "  {}/{}: {availability} ({content})",
            release.map_version, release.release_revision
        );
    }
}

fn setup_simulation_pause_ui(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: percent(50.0),
                margin: UiRect::new(px(-140.0), px(0.0), px(10.0), px(0.0)),
                top: console::wc3(console::TOP_BAR_UNITS),
                width: px(280.0),
                padding: UiRect::axes(px(14.0), px(8.0)),
                justify_content: JustifyContent::Center,
                border_radius: BorderRadius::all(px(7.0)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.035, 0.045, 0.060, 0.90)),
            Visibility::Hidden,
            SimulationPausePanel,
        ))
        .with_child((
            Text::new("SIMULATION PAUSED"),
            TextFont::from_font_size(18.0),
            TextColor(Color::srgb(1.0, 0.78, 0.20)),
            SimulationPauseText,
        ));
}

pub(crate) fn control_modifier_pressed(keys: &ButtonInput<KeyCode>) -> bool {
    keys.pressed(KeyCode::ControlLeft) || keys.pressed(KeyCode::ControlRight)
}

fn quicksave_path() -> PathBuf {
    if let Some(state_home) = std::env::var_os("XDG_STATE_HOME") {
        return PathBuf::from(state_home)
            .join("castle-fight-native")
            .join("quicksave.json");
    }
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("castle-fight-native")
            .join("quicksave.json");
    }
    PathBuf::from("castle-fight-native-quicksave.json")
}

fn restore_simulation_quicksave(
    simulation: &mut Simulation,
    content: &CastleFightContentBundle,
    path: &std::path::Path,
) -> Result<Option<u64>, String> {
    let bytes = std::fs::read(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let snapshot = SimulationSnapshot::decode_wire(&bytes, content)
        .map_err(|error| format!("could not decode quicksave: {error}"))?;
    let completed_tick = snapshot.completed_tick();
    simulation
        .restore_snapshot(&snapshot)
        .map_err(|error| format!("could not restore quicksave: {error:?}"))?;
    Ok(completed_tick)
}

fn handle_quicksave_hotkeys(
    keys: Res<ButtonInput<KeyCode>>,
    mut requests: MessageReader<QuickSaveRequest>,
    selected_match: Res<SelectedMatch>,
    debug_menu: Res<debug_menu::DebugMenuState>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut presentation: ResMut<PresentationSamples>,
) {
    let mut action = requests.read().last().copied();
    if control_modifier_pressed(&keys) {
        if keys.just_pressed(KeyCode::KeyS) {
            action = Some(QuickSaveRequest::Save);
        } else if keys.just_pressed(KeyCode::KeyO) {
            action = Some(QuickSaveRequest::Load);
        }
    }
    match action {
        Some(QuickSaveRequest::Save) => perform_quicksave(&mut authoritative),
        Some(QuickSaveRequest::Load) => perform_quickload(
            &mut authoritative,
            &selected_match,
            &debug_menu,
            &mut presentation,
        ),
        None => {}
    }
}

/// Quicksave / quickload, from the hotkeys or the F10 menu.
#[derive(Message, Clone, Copy)]
pub(crate) enum QuickSaveRequest {
    Save,
    Load,
}

/// Writes the offline match to the quicksave file. Results are reported as messages.
fn perform_quicksave(authoritative: &mut AuthoritativeSimulation) {
    if authoritative.is_networked() {
        authoritative
            .pending_status
            .push("Quicksave is available only for offline matches.".into());
        return;
    }

    let path = quicksave_path();
    let save_result = (|| -> Result<(usize, Option<u64>), String> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|error| {
                format!(
                    "could not create quicksave directory {}: {error}",
                    parent.display()
                )
            })?;
        }
        let snapshot = authoritative.simulation.capture_snapshot();
        let completed_tick = snapshot.completed_tick();
        let bytes = snapshot
            .encode_wire()
            .map_err(|error| format!("could not encode quicksave: {error}"))?;
        std::fs::write(&path, &bytes)
            .map_err(|error| format!("could not write {}: {error}", path.display()))?;
        Ok((bytes.len(), completed_tick))
    })();

    authoritative.pending_status.push(match save_result {
        Ok((bytes, completed_tick)) => format!(
            "Saved tick {} to {} ({bytes} bytes).",
            completed_tick
                .map(|tick| tick.to_string())
                .unwrap_or_else(|| "initial".into()),
            path.display()
        ),
        Err(error) => format!("Quicksave failed: {error}"),
    });
}

/// Restores the offline match from the quicksave file. Results are reported as messages.
fn perform_quickload(
    authoritative: &mut AuthoritativeSimulation,
    selected_match: &SelectedMatch,
    debug_menu: &debug_menu::DebugMenuState,
    presentation: &mut PresentationSamples,
) {
    if authoritative.is_networked() {
        authoritative
            .pending_status
            .push("Quickload is available only for offline matches.".into());
        return;
    }

    let path = quicksave_path();
    let load_result = (|| -> Result<Option<u64>, String> {
        let completed_tick = restore_simulation_quicksave(
            &mut authoritative.simulation,
            selected_match.content,
            &path,
        )?;
        let driver = MatchDriver::new(&authoritative.simulation, selected_match.content);
        authoritative.driver.set(driver);
        authoritative.pending_feedback.clear();
        authoritative.expected_execution_batch = None;
        authoritative.catch_up = None;
        authoritative.pending_build_commands.clear();
        *presentation =
            PresentationSamples::new(PresentationSnapshot::capture(&authoritative.simulation))
                .with_player_control(
                    authoritative
                        .simulation
                        .player(selected_match.local_player)
                        .map(|player| player.team),
                    debug_menu.controls_all_players(),
                );
        Ok(completed_tick)
    })();

    authoritative.pending_status.clear();
    authoritative.pending_status.push(match load_result {
        Ok(completed_tick) => format!(
            "Loaded tick {} from {}.",
            completed_tick
                .map(|tick| tick.to_string())
                .unwrap_or_else(|| "initial".into()),
            path.display()
        ),
        Err(error) => format!("Quickload failed: {error}"),
    });
}

fn toggle_simulation_pause(
    keys: Res<ButtonInput<KeyCode>>,
    lobby: Option<Res<LobbyState>>,
    main_menu: Option<Res<MainMenuState>>,
    action_panel: Option<Res<build_ui::ActionPanelState>>,
    authoritative: Res<AuthoritativeSimulation>,
    mut playback: ResMut<SimulationPlayback>,
) {
    if lobby.as_ref().is_some_and(|lobby| lobby.active())
        || main_menu.as_ref().is_some_and(|menu| menu.active())
    {
        return;
    }
    if authoritative.is_networked() {
        playback.paused = authoritative
            .network_lobby_status
            .as_ref()
            .is_some_and(|status| status.debug_paused);
        return;
    }
    let build_menu_open = action_panel
        .as_ref()
        .is_some_and(|panel| panel.mode == build_ui::ActionPanelMode::BuildMenu);
    if keys.just_pressed(KeyCode::Space) || (!build_menu_open && keys.just_pressed(KeyCode::KeyP)) {
        playback.paused = !playback.paused;
    }
}

fn update_simulation_pause_ui(
    playback: Res<SimulationPlayback>,
    mut visibility: Single<&mut Visibility, With<SimulationPausePanel>>,
) {
    **visibility = if playback.paused {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
}

fn advance_network_presentation(
    time: Res<Time>,
    authoritative: Res<AuthoritativeSimulation>,
    mut presentation: ResMut<PresentationSamples>,
) {
    let AuthorityMode::Network { connected, .. } = &authoritative.authority else {
        return;
    };
    let (paused, speed) = authoritative
        .network_lobby_status
        .as_ref()
        .map_or((false, 1.0), |status| {
            (status.debug_paused, status.debug_speed.multiplier())
        });
    if presentation
        .bypass_change_detection()
        .advance_network_timeline(
            time.delta_secs_f64(),
            speed,
            paused,
            *connected && authoritative.catch_up.is_none(),
        )
    {
        presentation.set_changed();
    }
}

pub(crate) fn advance_authoritative_simulation_once(
    authoritative: &mut AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
) -> Result<DriverTickResult, CanonicalStreamError> {
    let AuthoritativeSimulation {
        simulation,
        driver,
        pending_feedback,
        pending_build_commands,
        ..
    } = authoritative;
    let result = driver.advance_local_tick(simulation)?;
    for execution in &result.executions {
        pending_build_commands.resolve(
            execution.scheduled.player,
            execution.scheduled.client_sequence.0,
        );
    }
    pending_feedback.extend(result.executions.iter().copied());
    presentation.publish(PresentationSnapshot::capture(simulation));
    Ok(result)
}

/// Drains network events and applies them synchronously. The running client instead splits this
/// across the frame (see `network_sim`); tests use this single-call form.
#[cfg(test)]
fn process_network_events(
    authoritative: &mut AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
    selected_match: &mut SelectedMatch,
    mut performance: Option<&mut PerformanceCounters>,
) -> Result<(), String> {
    network_sim::ingest_events(
        authoritative,
        presentation,
        selected_match,
        performance.as_deref_mut(),
    )?;
    network_sim::run_queue_inline(authoritative, presentation, performance)
}

/// Validates a lobby status against the authenticated slots. Returns the replacement match
/// configuration when the roster or rules changed, or `None` when nothing about the simulation
/// has to change. Reads no simulation state, so it may run while the simulation is on the worker.
fn plan_network_lobby_roster(
    authority: &AuthorityMode,
    content: &'static CastleFightContentBundle,
    status: &castle_fight_protocol::LobbyStatus,
) -> Result<Option<CastleFightMatchConfig>, String> {
    let AuthorityMode::Network {
        match_config,
        assigned_player,
        ..
    } = authority
    else {
        return Err("network lobby status arrived in local mode".to_owned());
    };
    let mut roster = status.participants.clone();
    roster.sort_unstable_by_key(|participant| participant.player_id);
    // The host may change the team size, so the slots are whatever the server says they are, as
    // long as they are a standard roster that still contains the local player.
    let slots = castle_fight_sim::standard_match_slots(roster.len() / 2)
        .filter(|slots| {
            roster.len().is_multiple_of(2)
                && usize::from(status.required_players) == roster.len()
                && slots.len() == roster.len()
        })
        .ok_or_else(|| "server lobby sent an unsupported roster".to_owned())?;
    let mut ordered = roster.iter().collect::<Vec<_>>();
    ordered.sort_unstable_by_key(|participant| {
        slots
            .iter()
            .position(|(id, _)| id.0 == participant.player_id)
            .unwrap_or(usize::MAX)
    });
    if ordered
        .iter()
        .zip(&slots)
        .any(|(wire, (id, _))| wire.player_id != id.0)
    {
        return Err("server lobby sent an unsupported roster".to_owned());
    }
    let mut config = match_config.clone();
    config.rules = status.rules;
    config.participants = ordered
        .iter()
        .zip(&slots)
        .map(|(wire, &(id, team))| {
            let builder_race = content
                .builder_race_for_rawcode(wire.builder_rawcode)
                .filter(|race| content.supports_builder_race(*race))
                .ok_or_else(|| "server lobby selected an unavailable builder".to_owned())?;
            Ok(CastleFightParticipantConfig {
                id,
                team,
                builder_race,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    if !config
        .participants
        .iter()
        .any(|participant| participant.id == *assigned_player)
    {
        return Err("server lobby removed the local player's slot".to_owned());
    }
    Ok((config != *match_config).then_some(config))
}

fn apply_network_lobby_roster(
    authoritative: &mut AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
    selected_match: &mut SelectedMatch,
    status: &castle_fight_protocol::LobbyStatus,
) -> Result<(), String> {
    let Some(config) =
        plan_network_lobby_roster(&authoritative.authority, selected_match.content, status)?
    else {
        return Ok(());
    };
    if authoritative.simulation.tick() != 0
        || authoritative.driver.next_stream_position().0 != 0
        || authoritative.catch_up.is_some()
    {
        return Err("server lobby changed the roster after canonical play began".to_owned());
    }
    let demo = create_demo_world_for_match_config(
        authoritative.simulation.worker_count(),
        None,
        config.clone(),
    )
    .map_err(|error| format!("invalid server lobby roster: {error}"))?;
    authoritative
        .driver
        .set(MatchDriver::new(&demo.simulation, demo.content));
    authoritative.simulation.set(demo.simulation);
    authoritative.pending_feedback.clear();
    authoritative.expected_execution_batch = None;
    presentation.reset(PresentationSnapshot::capture(&authoritative.simulation));
    selected_match.direct_buildings = demo.direct_buildings;
    if let AuthorityMode::Network { match_config, .. } = &mut authoritative.authority {
        *match_config = config;
    }
    Ok(())
}

fn begin_snapshot_catch_up(
    authoritative: &mut AuthoritativeSimulation,
    begin: SnapshotTransferBegin,
) -> Result<(), String> {
    authoritative.pending_build_commands.clear();
    if authoritative.catch_up.is_some() {
        return Err(
            "server started a second snapshot transfer before the first completed".to_owned(),
        );
    }
    let snapshot_bytes = usize::try_from(begin.snapshot_bytes)
        .map_err(|_| "snapshot byte length does not fit this client".to_owned())?;
    if snapshot_bytes == 0 || snapshot_bytes > MAX_SNAPSHOT_BYTES {
        return Err(format!(
            "server snapshot length {snapshot_bytes} exceeds reconnect bound {MAX_SNAPSHOT_BYTES}"
        ));
    }
    let expected_chunks = snapshot_bytes.div_ceil(SNAPSHOT_CHUNK_BYTES);
    if usize::try_from(begin.chunk_count).ok() != Some(expected_chunks) {
        return Err(format!(
            "server snapshot chunk count {} does not match byte length {snapshot_bytes}",
            begin.chunk_count
        ));
    }
    if begin.snapshot_stream_position > begin.handoff_stream_position {
        return Err("snapshot stream boundary lies beyond handoff boundary".to_owned());
    }

    match &mut authoritative.authority {
        AuthorityMode::Network {
            connected,
            pending_handoff_position,
            ..
        } => match *pending_handoff_position {
            Some(expected) if expected != begin.handoff_stream_position => {
                return Err(format!(
                    "snapshot handoff position {} disagrees with reconnect assignment {expected}",
                    begin.handoff_stream_position
                ));
            }
            Some(_) => {}
            None => {
                // An already-connected client enters this state after reporting a checksum mismatch;
                // command submission is disabled before the server's replacement header arrives.
                *connected = false;
                *pending_handoff_position = Some(begin.handoff_stream_position);
            }
        },
        AuthorityMode::Local => return Err("snapshot transfer arrived in local mode".to_owned()),
    }

    authoritative.expected_execution_batch = None;
    authoritative.pending_feedback.clear();
    authoritative.catch_up = Some(SnapshotCatchUp {
        begin,
        bytes: Vec::with_capacity(snapshot_bytes),
        next_chunk_index: 0,
        snapshot_loaded: false,
    });
    Ok(())
}

fn append_snapshot_chunk(
    authoritative: &mut AuthoritativeSimulation,
    chunk: SnapshotChunk,
) -> Result<(), String> {
    let mut catch_up = authoritative
        .catch_up
        .take()
        .ok_or_else(|| "received snapshot chunk without snapshot header".to_owned())?;
    if catch_up.snapshot_loaded {
        return Err("received snapshot chunk after snapshot was already loaded".to_owned());
    }
    if chunk.transfer_id != catch_up.begin.transfer_id {
        return Err("snapshot chunk belongs to a different transfer".to_owned());
    }
    if chunk.chunk_index != catch_up.next_chunk_index {
        return Err(format!(
            "snapshot chunk gap: expected {}, got {}",
            catch_up.next_chunk_index, chunk.chunk_index
        ));
    }
    if chunk.bytes.is_empty() || chunk.bytes.len() > SNAPSHOT_CHUNK_BYTES {
        return Err(format!(
            "invalid snapshot chunk length {}",
            chunk.bytes.len()
        ));
    }
    let declared_bytes = usize::try_from(catch_up.begin.snapshot_bytes)
        .map_err(|_| "snapshot byte length does not fit this client".to_owned())?;
    let new_length = catch_up
        .bytes
        .len()
        .checked_add(chunk.bytes.len())
        .ok_or_else(|| "snapshot transfer length overflow".to_owned())?;
    if new_length > declared_bytes || new_length > MAX_SNAPSHOT_BYTES {
        return Err("snapshot chunks exceed declared bounded transfer length".to_owned());
    }
    catch_up.bytes.extend_from_slice(&chunk.bytes);
    catch_up.next_chunk_index = catch_up
        .next_chunk_index
        .checked_add(1)
        .ok_or_else(|| "snapshot chunk index exhausted".to_owned())?;

    if catch_up.next_chunk_index == catch_up.begin.chunk_count {
        if catch_up.bytes.len() != declared_bytes {
            return Err(format!(
                "snapshot completed with {} bytes, expected {declared_bytes}",
                catch_up.bytes.len()
            ));
        }
        let content = authoritative.driver.content();
        let snapshot = SimulationSnapshot::decode_wire(&catch_up.bytes, content)
            .map_err(|error| format!("invalid authoritative snapshot: {error}"))?;
        if snapshot.completed_tick() != catch_up.begin.snapshot_completed_tick
            || snapshot.checksum() != catch_up.begin.snapshot_checksum
        {
            return Err("snapshot metadata disagrees with transferred snapshot body".to_owned());
        }
        authoritative
            .simulation
            .restore_snapshot(&snapshot)
            .map_err(|error| format!("could not restore authoritative snapshot: {error:?}"))?;
        let replica_driver = MatchDriver::new_replica_from_snapshot(
            &authoritative.simulation,
            content,
            InputStreamPosition(catch_up.begin.snapshot_stream_position),
        );
        authoritative.driver.set(replica_driver);
        authoritative.simulation.clear_presentation_events();
        authoritative.expected_execution_batch = None;
        authoritative.pending_feedback.clear();
        catch_up.bytes.clear();
        catch_up.snapshot_loaded = true;
    }

    authoritative.catch_up = Some(catch_up);
    Ok(())
}

fn finish_snapshot_catch_up(
    authoritative: &mut AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
    complete: CatchUpComplete,
) -> Result<(), String> {
    let catch_up = authoritative
        .catch_up
        .take()
        .ok_or_else(|| "received catch-up completion without snapshot transfer".to_owned())?;
    if !catch_up.snapshot_loaded {
        return Err("server completed catch-up before the snapshot was loaded".to_owned());
    }
    if complete.transfer_id != catch_up.begin.transfer_id
        || complete.handoff_stream_position != catch_up.begin.handoff_stream_position
    {
        return Err("catch-up completion disagrees with pinned snapshot handoff".to_owned());
    }
    if authoritative.driver.next_stream_position().0 != complete.handoff_stream_position {
        return Err(format!(
            "catch-up stream ended at {}, expected {}",
            authoritative.driver.next_stream_position().0,
            complete.handoff_stream_position
        ));
    }
    let completed_tick = authoritative.simulation.tick().checked_sub(1);
    let checksum = authoritative.simulation.checksum();
    if completed_tick != complete.completed_tick || checksum != complete.checksum {
        return Err(format!(
            "catch-up state mismatch at {completed_tick:?}: local {checksum:#018x}, server {:#018x}",
            complete.checksum
        ));
    }

    match &mut authoritative.authority {
        AuthorityMode::Network {
            connected,
            pending_handoff_position,
            ..
        } => {
            if *pending_handoff_position != Some(complete.handoff_stream_position) {
                return Err("catch-up completion does not match pending handoff".to_owned());
            }
            *pending_handoff_position = None;
            *connected = true;
        }
        AuthorityMode::Local => return Err("catch-up completed in local mode".to_owned()),
    }

    authoritative.simulation.clear_presentation_events();
    authoritative.expected_execution_batch = None;
    authoritative.pending_feedback.clear();
    presentation.reset(PresentationSnapshot::capture(&authoritative.simulation));
    authoritative
        .pending_status
        .push("Authoritative state synchronized.".to_owned());
    Ok(())
}

fn sync_local_command_feedback(
    selected_match: Res<SelectedMatch>,
    debug_menu: Res<debug_menu::DebugMenuState>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut action_panel: ResMut<ActionPanelState>,
    mut notifications: ResMut<notifications::Notifications>,
) {
    for status in std::mem::take(&mut authoritative.pending_status) {
        let severity =
            if status.starts_with("Command not sent") || status.starts_with("Disconnected") {
                notifications::Severity::Warning
            } else {
                notifications::Severity::Info
            };
        notifications.push(severity, status.clone());
        action_panel.status = status;
    }
    let feedback = std::mem::take(&mut authoritative.pending_feedback);
    for execution in feedback {
        if execution.scheduled.player != selected_match.local_player
            && !debug_menu.controls_all_players()
        {
            continue;
        }
        if let CommandOutcome::Rejected(_) = execution.outcome {
            let text = local_command_feedback_text(execution);
            notifications.push(notifications::Severity::Warning, text.clone());
            action_panel.status = text;
        } else {
            action_panel.status = local_command_feedback_text(execution);
        }
    }
}

fn local_command_feedback_text(execution: CommandExecution) -> String {
    let action = match execution.scheduled.command {
        PlayerCommand::MoveBuilder { .. } => "Move",
        PlayerCommand::FollowWithBuilder { .. } => "Follow",
        PlayerCommand::StopBuilder { .. } => "Stop",
        PlayerCommand::BlinkBuilder { .. } => "Blink",
        PlayerCommand::RescueStrike { .. } => "Rescue Strike",
        PlayerCommand::RepairWithBuilder { .. } => "Repair",
        PlayerCommand::SetBuilderRepairAutocast { .. } => "Repair autocast",
        PlayerCommand::PlaceBuilding { .. } => "Build",
        PlayerCommand::QueueBuilderCommand { .. } => "Queued builder order",
        PlayerCommand::CancelBuildingConstruction { .. } => "Cancel construction",
        PlayerCommand::QueueProductionUnit { .. } => "Train unit",
        PlayerCommand::CancelProductionUnit { .. } => "Cancel training",
        PlayerCommand::UpgradeBuilding { .. } => "Upgrade",
        PlayerCommand::AttackWithBuilding { .. } => "Attack",
        PlayerCommand::CastBuildingSpellOn { .. } => "Mana Transfusion",
        PlayerCommand::CastBuildingSpellAt { .. } => "Winter’s Wrath",
        PlayerCommand::CastBuildingSpell { .. } => "Cast spell",
        PlayerCommand::SetBuildingSpellAutocast { .. } => "Spell autocast",
        PlayerCommand::BuyItem { .. } => "Buy item",
        PlayerCommand::MoveItem { .. } => "Move item",
        PlayerCommand::UseItem { .. } => "Use item",
        PlayerCommand::DropItem { .. } => "Drop item",
        PlayerCommand::GiveItem { .. } => "Give item",
        PlayerCommand::PickUpItem { .. } => "Pick up item",
    };
    match execution.outcome {
        CommandOutcome::Executed(CommandExecutionResult::BuilderBlinkedTo(position)) => format!(
            "{action} executed on tick {} at ({}, {}).",
            execution.scheduled.tick, position.x, position.y
        ),
        CommandOutcome::Executed(CommandExecutionResult::BuildingConstructionCancelled(_))
        | CommandOutcome::Executed(CommandExecutionResult::Applied) => {
            format!("{action} executed on tick {}.", execution.scheduled.tick)
        }
        CommandOutcome::Rejected(reason) => {
            format!("{action} failed: {}", notifications::rejection_text(reason))
        }
    }
}

/// Distributions commonly default the soft `RLIMIT_NOFILE` to 1024. Bevy's glTF loader keeps every
/// in-progress glTF (and its buffers/textures) open while nested loads run, so the startup asset
/// burst can exceed that and fail with `EMFILE` ("Too many open files"). Raise the soft limit to
/// the hard limit, capped to something every kernel accepts.
#[cfg(unix)]
fn raise_open_file_limit() {
    use rustix::process::{Resource, getrlimit, setrlimit};

    const WANTED_OPEN_FILES: u64 = 65_536;

    let mut limit = getrlimit(Resource::Nofile);
    let wanted = limit
        .maximum
        .map_or(WANTED_OPEN_FILES, |hard| hard.min(WANTED_OPEN_FILES));
    if limit.current.is_some_and(|soft| soft >= wanted) {
        return;
    }
    limit.current = Some(wanted);
    if let Err(error) = setrlimit(Resource::Nofile, limit) {
        eprintln!("warning: could not raise the open file limit to {wanted}: {error}");
    }
}

#[cfg(not(unix))]
fn raise_open_file_limit() {}

fn configure_asset_io_task_pool() {
    // Warcraft's generated glTF set exercises Bevy's recursive glTF loader deeply enough that the
    // platform-default worker stack overflows during the startup asset burst (see
    // `ASSET_IO_STACK_BYTES`). Keep Bevy's normal IO-thread count, but give only that pool a larger
    // stack instead of inflating the simulation/compute pools (or requiring RUST_MIN_STACK
    // globally).
    let thread_count = default_io_thread_count(available_parallelism());
    IoTaskPool::get_or_init(|| {
        TaskPoolBuilder::new()
            .num_threads(thread_count)
            .stack_size(ASSET_IO_STACK_BYTES)
            .thread_name("IO Task Pool".to_owned())
            .build()
    });
}

fn default_io_thread_count(total_threads: usize) -> usize {
    // Match Bevy's default IO assignment policy: 25% of available threads, rounded to nearest
    // integer (halves upward), with a floor of one thread and a ceiling of four.
    total_threads.saturating_add(2).div_euclid(4).clamp(1, 4)
}

fn default_worker_count() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .min(8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiling_visual_rawcode_requires_exact_ascii_fourcc() {
        assert_eq!(
            parse_profile_rawcode("h02W").unwrap(),
            u32::from_be_bytes(*b"h02W")
        );
        for invalid in ["h02", "h02WW", "水水"] {
            assert!(parse_profile_rawcode(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn profiling_duration_accepts_fractional_seconds_and_rejects_invalid_values() {
        assert_eq!(
            parse_profile_duration("2.5").unwrap(),
            Duration::from_millis(2_500)
        );
        for invalid in ["0", "-1", "NaN", "inf", "later"] {
            assert!(parse_profile_duration(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn profiling_warmup_accepts_zero_and_fractional_seconds() {
        assert_eq!(parse_profile_warmup("0").unwrap(), Duration::ZERO);
        assert_eq!(
            parse_profile_warmup("2.5").unwrap(),
            Duration::from_millis(2_500)
        );
        for invalid in ["-1", "NaN", "inf", "later"] {
            assert!(parse_profile_warmup(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn asset_io_pool_keeps_bevys_default_thread_assignment() {
        assert_eq!(default_io_thread_count(1), 1);
        assert_eq!(default_io_thread_count(4), 1);
        assert_eq!(default_io_thread_count(6), 2);
        assert_eq!(default_io_thread_count(10), 3);
        assert_eq!(default_io_thread_count(16), 4);
        assert_eq!(default_io_thread_count(64), 4);
    }

    #[test]
    fn local_command_submission_waits_for_its_canonical_tick() {
        let demo = crate::demo::create_demo_world(1, Some(0));
        let builder = demo
            .simulation
            .builder_for_player(PlayerId(0))
            .expect("western builder");
        let builder_id = builder.id;
        let start_position = builder.position;
        let destination = SimPoint::new(
            builder.position.x + 20 * castle_fight_sim::SUBUNITS_PER_WORLD_UNIT,
            builder.position.y,
        );
        let initial_snapshot = PresentationSnapshot::capture(&demo.simulation);
        let mut authoritative = AuthoritativeSimulation::new(demo.simulation, demo.content);
        let submission = authoritative.submit_local_command(
            PlayerId(0),
            PlayerCommand::MoveBuilder {
                builder: builder_id,
                destination,
            },
        );
        let ClientCommandSubmission::Local(submission) = submission else {
            panic!("offline command submission must remain local")
        };
        let scheduled = submission.scheduled().expect("move must be admitted");
        assert_eq!(scheduled.tick, 0);
        assert_eq!(
            authoritative
                .simulation
                .builder(builder_id)
                .unwrap()
                .destination,
            None,
            "submission alone must not mutate authoritative gameplay state"
        );

        let mut presentation = PresentationSamples::new(initial_snapshot);
        let result = advance_authoritative_simulation_once(&mut authoritative, &mut presentation)
            .expect("canonical tick must execute");
        assert_eq!(result.finalized.tick, 0);
        assert_eq!(result.executions.len(), 1);
        assert_eq!(
            result.executions[0].outcome,
            CommandOutcome::Executed(CommandExecutionResult::Applied)
        );
        assert_ne!(
            authoritative
                .simulation
                .builder(builder_id)
                .unwrap()
                .position,
            start_position,
            "the queued command must begin affecting gameplay only when its canonical tick executes"
        );
        assert_eq!(authoritative.simulation.tick(), 1);
    }

    #[test]
    fn network_admission_rejection_rolls_back_construction_feedback() {
        let demo = crate::demo::create_demo_world(1, None);
        let builder = demo.simulation.builder_for_player(PlayerId(0)).unwrap();
        let kind = castle_fight_sim::CastleFightBuildingKind::Production(
            castle_fight_sim::CastleFightProductionKind::Barracks,
        );
        let position = crate::build_orders::tests::legal_position(
            &demo.simulation,
            demo.content,
            builder.team,
            kind,
        );
        let snapshot = PresentationSnapshot::capture(&demo.simulation);
        let client = NetworkClient::connected_test_fixture(compatibility_identity_for_demo(&demo));
        let mut authoritative = AuthoritativeSimulation::new_networked(
            demo.simulation,
            demo.content,
            demo.match_config,
            client,
            builder.owner,
            0,
        );
        let before = authoritative
            .simulation
            .player_resources_for(builder.owner)
            .unwrap();
        authoritative.pending_build_commands.record(
            builder.owner,
            PlayerCommand::PlaceBuilding {
                builder: builder.id,
                building: kind.stable_id(),
                position,
            },
            ClientCommandSubmission::Submitted { client_sequence: 7 },
        );
        assert!(
            authoritative
                .pending_build_commands
                .project(&authoritative.simulation, demo.content)
                .orders
                .contains_key(&builder.id)
        );
        let AuthorityMode::Network { client, .. } = &authoritative.authority else {
            unreachable!();
        };
        client.inject_server_message_for_test(ServerMessage::CommandAcknowledged {
            acknowledgement: CommandAcknowledgement::Rejected {
                client_sequence: 7,
                reason: castle_fight_protocol::WireAdmissionError::MatchNotRunning,
                duplicate: false,
            },
        });
        let mut presentation = PresentationSamples::new(snapshot);
        let mut selected = SelectedMatch {
            content: demo.content,
            direct_buildings: demo.direct_buildings,
            local_player: builder.owner,
        };
        process_network_events(&mut authoritative, &mut presentation, &mut selected, None).unwrap();
        let rejected = authoritative
            .pending_build_commands
            .project(&authoritative.simulation, demo.content);
        assert!(rejected.orders.is_empty());
        assert_eq!(rejected.resources[&builder.owner], before);
    }

    #[test]
    fn checkpoint_batched_before_disconnect_does_not_fail_the_session() {
        let demo = crate::demo::create_demo_world(1, None);
        let snapshot = PresentationSnapshot::capture(&demo.simulation);
        let client = NetworkClient::connected_test_fixture(compatibility_identity_for_demo(&demo));
        let mut authoritative = AuthoritativeSimulation::new_networked(
            demo.simulation,
            demo.content,
            demo.match_config,
            client,
            PlayerId(0),
            0,
        );
        let mut presentation = PresentationSamples::new(snapshot);
        advance_authoritative_simulation_once(&mut authoritative, &mut presentation)
            .expect("canonical tick must execute");
        let checkpoint = castle_fight_protocol::Checkpoint {
            completed_tick: 0,
            checksum: authoritative.simulation.checksum(),
        };
        let AuthorityMode::Network { client, .. } = &authoritative.authority else {
            unreachable!();
        };
        client.inject_server_message_for_test(ServerMessage::Checkpoint { checkpoint });
        client.inject_disconnect_for_test("connection reset");
        let mut selected = SelectedMatch {
            content: demo.content,
            direct_buildings: demo.direct_buildings,
            local_player: PlayerId(0),
        };
        process_network_events(&mut authoritative, &mut presentation, &mut selected, None)
            .expect("a closed transport is a recoverable disconnect, not an invariant failure");
        let AuthorityMode::Network { connected, .. } = &authoritative.authority else {
            unreachable!();
        };
        assert!(!connected);
    }

    #[test]
    fn network_debug_access_tracks_host_start_connection_and_handoff() {
        let demo = crate::demo::create_demo_world(1, None);
        let client = NetworkClient::connected_test_fixture(compatibility_identity_for_demo(&demo));
        let mut authoritative = AuthoritativeSimulation::new_networked(
            demo.simulation,
            demo.content,
            demo.match_config,
            client,
            PlayerId(0),
            0,
        );
        assert!(!authoritative.debug_available());
        authoritative.network_lobby_status = Some(castle_fight_protocol::LobbyStatus {
            rules: authoritative.simulation.match_rules(),
            host_player_id: 6,
            debug_paused: false,
            debug_speed: castle_fight_protocol::DebugSpeed::Normal,
            connected_player_ids: vec![0, 6],
            required_players: 2,
            participants: Vec::new(),
            started: true,
        });
        assert!(!authoritative.debug_available());
        let before = authoritative.simulation.checksum();
        assert!(
            authoritative
                .submit_debug_request(castle_fight_protocol::DebugRequest::Command {
                    command: castle_fight_protocol::WireDebugCommand::GrantResources
                })
                .is_err()
        );
        assert_eq!(before, authoritative.simulation.checksum());
        authoritative
            .network_lobby_status
            .as_mut()
            .unwrap()
            .host_player_id = 0;
        assert!(authoritative.debug_available());
        assert!(
            authoritative
                .submit_debug_request(castle_fight_protocol::DebugRequest::StepOneTick)
                .is_err()
        );
        authoritative.network_lobby_status.as_mut().unwrap().started = false;
        assert!(!authoritative.debug_available());
        authoritative.network_lobby_status.as_mut().unwrap().started = true;
        if let AuthorityMode::Network { connected, .. } = &mut authoritative.authority {
            *connected = false;
        }
        assert!(!authoritative.debug_available());
    }

    #[test]
    fn network_lobby_team_size_change_rebuilds_the_world_the_server_holds() {
        use castle_fight_protocol::{LobbyParticipant, LobbyStatus};
        let small = crate::demo::create_demo_world(1, None);
        let mut large_config = small.match_config.clone();
        large_config.participants = castle_fight_sim::standard_match_slots(2)
            .unwrap()
            .into_iter()
            .map(|(id, team)| CastleFightParticipantConfig {
                id,
                team,
                builder_race: CastleFightBuilderRace::Human,
            })
            .collect();
        let server = castle_fight_server::AuthoritativeMatch::new(
            large_config.clone(),
            1,
            Default::default(),
        )
        .unwrap();
        let status = LobbyStatus {
            rules: large_config.rules,
            host_player_id: 0,
            debug_paused: false,
            debug_speed: castle_fight_protocol::DebugSpeed::Normal,
            connected_player_ids: vec![0, 6],
            required_players: 4,
            started: false,
            participants: large_config
                .participants
                .iter()
                .map(|participant| LobbyParticipant {
                    player_id: participant.id.0,
                    builder_rawcode: small
                        .content
                        .builder(participant.builder_race)
                        .unwrap()
                        .rawcode,
                    name: String::new(),
                })
                .collect(),
        };

        let local = PlayerId(6);
        let client = NetworkClient::connected_test_fixture(compatibility_identity_for_demo(&small));
        let mut presentation =
            PresentationSamples::new(PresentationSnapshot::capture(&small.simulation));
        let mut selected = SelectedMatch {
            content: small.content,
            direct_buildings: small.direct_buildings,
            local_player: local,
        };
        let mut authoritative = AuthoritativeSimulation::new_networked(
            small.simulation,
            small.content,
            small.match_config,
            client,
            local,
            0,
        );
        apply_network_lobby_roster(
            &mut authoritative,
            &mut presentation,
            &mut selected,
            &status,
        )
        .unwrap();
        assert_eq!(
            authoritative.simulation.checksum(),
            server.simulation().checksum()
        );
    }

    #[test]
    fn network_lobby_roster_is_applied_before_first_tick_in_same_event_batch() {
        use castle_fight_protocol::{LobbyParticipant, LobbyStatus};
        let config = crate::demo::create_demo_world(1, None).match_config;
        let mut selected_config = config.clone();
        selected_config.participants[1].builder_race = CastleFightBuilderRace::Elf;
        let mut server = castle_fight_server::AuthoritativeMatch::new(
            selected_config.clone(),
            1,
            Default::default(),
        )
        .unwrap();
        let expected_initial_checksum = server.simulation().checksum();
        let content =
            castle_fight_sim::castle_fight_content_bundle(config.release.map_version).unwrap();
        let status = LobbyStatus {
            rules: selected_config.rules,
            host_player_id: 0,
            debug_paused: false,
            debug_speed: castle_fight_protocol::DebugSpeed::Normal,
            connected_player_ids: vec![0, 6],
            required_players: 2,
            started: false,
            participants: selected_config
                .participants
                .iter()
                .map(|participant| LobbyParticipant {
                    player_id: participant.id.0,
                    builder_rawcode: content.builder(participant.builder_race).unwrap().rawcode,
                    name: String::new(),
                })
                .collect(),
        };
        let tick_messages = server.finalize_next_tick().unwrap();
        for player in [PlayerId(0), PlayerId(6)] {
            let demo = create_demo_world_for_match_config(4, None, config.clone()).unwrap();
            let compatibility = compatibility_identity_for_demo(&demo);
            assert_eq!(&compatibility, server.compatibility());
            let client = NetworkClient::connected_test_fixture(compatibility);
            let mut presentation =
                PresentationSamples::new(PresentationSnapshot::capture(&demo.simulation));
            let mut selected = SelectedMatch {
                content: demo.content,
                direct_buildings: demo.direct_buildings,
                local_player: player,
            };
            let mut authoritative = AuthoritativeSimulation::new_networked(
                demo.simulation,
                demo.content,
                demo.match_config,
                client,
                player,
                0,
            );
            let checksum = authoritative.simulation.checksum();
            let mut invalid = status.clone();
            invalid.participants[1].builder_rawcode = 0;
            assert!(
                apply_network_lobby_roster(
                    &mut authoritative,
                    &mut presentation,
                    &mut selected,
                    &invalid
                )
                .is_err()
            );
            invalid = status.clone();
            invalid.participants[1].player_id = 0;
            assert!(
                apply_network_lobby_roster(
                    &mut authoritative,
                    &mut presentation,
                    &mut selected,
                    &invalid
                )
                .is_err()
            );
            assert_eq!(authoritative.simulation.checksum(), checksum);
            apply_network_lobby_roster(
                &mut authoritative,
                &mut presentation,
                &mut selected,
                &status,
            )
            .unwrap();
            assert_eq!(
                authoritative.simulation.checksum(),
                expected_initial_checksum
            );
            let selected_catalog = &authoritative
                .simulation
                .builder_for_player(PlayerId(6))
                .unwrap()
                .configuration
                .build_catalog;
            assert!(
                selected
                    .direct_buildings
                    .iter()
                    .any(|kind| selected_catalog.contains(&kind.rawcode(content)))
            );
            let AuthorityMode::Network { client, .. } = &authoritative.authority else {
                unreachable!()
            };
            client.inject_server_message_for_test(ServerMessage::LobbyStatus {
                status: status.clone(),
            });
            client.inject_server_message_for_test(ServerMessage::LobbyStatus {
                status: LobbyStatus {
                    started: true,
                    ..status.clone()
                },
            });
            for outbound in &tick_messages {
                client.inject_server_message_for_test(outbound.message.clone());
            }
            process_network_events(&mut authoritative, &mut presentation, &mut selected, None)
                .unwrap();
            assert_eq!(
                authoritative.simulation.checksum(),
                server.simulation().checksum()
            );
            assert_eq!(authoritative.simulation.tick(), 1);
            invalid = status.clone();
            invalid.participants[1].builder_rawcode = content
                .builder(CastleFightBuilderRace::Human)
                .unwrap()
                .rawcode;
            assert!(
                apply_network_lobby_roster(
                    &mut authoritative,
                    &mut presentation,
                    &mut selected,
                    &invalid
                )
                .is_err()
            );
        }
    }

    #[test]
    fn network_snapshot_handoff_replaces_divergent_state_and_resets_presentation() {
        let mut source = crate::demo::create_demo_world(1, Some(0));
        let mut source_driver = MatchDriver::new(&source.simulation, source.content);
        for _ in 0..3 {
            source_driver
                .advance_local_tick(&mut source.simulation)
                .unwrap();
        }
        let snapshot_stream_position = source_driver.next_stream_position().0;
        let snapshot = source.simulation.capture_snapshot();
        let bytes = snapshot.encode_wire().unwrap();
        let chunk_count = bytes.len().div_ceil(SNAPSHOT_CHUNK_BYTES);
        let begin = SnapshotTransferBegin {
            transfer_id: 77,
            snapshot_stream_position,
            snapshot_completed_tick: snapshot.completed_tick(),
            snapshot_checksum: snapshot.checksum(),
            snapshot_bytes: u32::try_from(bytes.len()).unwrap(),
            chunk_count: u32::try_from(chunk_count).unwrap(),
            handoff_stream_position: snapshot_stream_position,
        };
        let complete = CatchUpComplete {
            transfer_id: begin.transfer_id,
            handoff_stream_position: begin.handoff_stream_position,
            completed_tick: snapshot.completed_tick(),
            checksum: snapshot.checksum(),
        };

        let mut replica = crate::demo::create_demo_world(1, Some(0));
        replica.simulation.step();
        assert_ne!(replica.simulation.checksum(), source.simulation.checksum());
        let initial_presentation = PresentationSnapshot::capture(&replica.simulation);
        let compatibility = compatibility_identity_for_demo(&replica);
        let client = NetworkClient::connected_test_fixture(compatibility);
        let mut authoritative = AuthoritativeSimulation::new_networked(
            replica.simulation,
            replica.content,
            replica.match_config,
            client,
            PlayerId(0),
            0,
        );
        let mut presentation = PresentationSamples::new(initial_presentation);
        let mut selected_match = SelectedMatch {
            content: replica.content,
            direct_buildings: replica.direct_buildings,
            local_player: PlayerId(0),
        };
        let mut performance = PerformanceCounters::default();

        let network = match &authoritative.authority {
            AuthorityMode::Network { client, .. } => client,
            AuthorityMode::Local => unreachable!(),
        };
        network.inject_server_message_for_test(ServerMessage::SnapshotBegin { begin });
        for (chunk_index, chunk) in bytes.chunks(SNAPSHOT_CHUNK_BYTES).enumerate() {
            network.inject_server_message_for_test(ServerMessage::SnapshotChunk {
                chunk: SnapshotChunk {
                    transfer_id: begin.transfer_id,
                    chunk_index: u32::try_from(chunk_index).unwrap(),
                    bytes: chunk.to_vec(),
                },
            });
        }
        network.inject_server_message_for_test(ServerMessage::CatchUpComplete { complete });

        process_network_events(
            &mut authoritative,
            &mut presentation,
            &mut selected_match,
            Some(&mut performance),
        )
        .unwrap();
        assert_eq!(
            authoritative.simulation.checksum(),
            source.simulation.checksum()
        );
        assert_eq!(
            authoritative.driver.next_stream_position().0,
            snapshot_stream_position
        );
        assert!(authoritative.catch_up.is_none());
        assert_eq!(
            presentation.current.tick,
            PresentationSnapshot::capture(&source.simulation).tick
        );
        assert!(matches!(
            authoritative.authority,
            AuthorityMode::Network {
                connected: true,
                pending_handoff_position: None,
                ..
            }
        ));
    }

    #[test]
    fn network_movement_survives_empty_fixed_polls_and_visible_boundary_controls() {
        #[derive(Resource, Default)]
        struct Publications(usize);
        fn count_publications(samples: Res<PresentationSamples>, mut count: ResMut<Publications>) {
            if samples.is_changed() {
                count.0 += 1;
            }
        }

        let mut source = crate::demo::create_demo_world(1, None);
        let builder = source.simulation.builder_for_player(PlayerId(0)).unwrap();
        let mut driver = MatchDriver::new(&source.simulation, source.content);
        driver.submit_local_command(
            &source.simulation,
            PlayerId(0),
            PlayerCommand::MoveBuilder {
                builder: builder.id,
                destination: SimPoint::new(
                    builder.position.x + 20 * castle_fight_sim::SUBUNITS_PER_WORLD_UNIT,
                    builder.position.y,
                ),
            },
        );
        let demo = crate::demo::create_demo_world(4, None);
        let client = NetworkClient::connected_test_fixture(compatibility_identity_for_demo(&demo));
        for _ in 0..2 {
            let result = driver.advance_local_tick(&mut source.simulation).unwrap();
            client.inject_server_message_for_test(ServerMessage::StreamRecord {
                record: (&CanonicalStreamRecord::Tick(result.finalized.clone())).into(),
            });
            client.inject_server_message_for_test(ServerMessage::TickExecutions {
                batch: castle_fight_protocol::WireExecutionBatch {
                    tick: result.finalized.tick,
                    executions: result.executions.iter().copied().map(Into::into).collect(),
                },
            });
        }
        let snapshot = PresentationSnapshot::capture(&demo.simulation);
        let mut app = App::new();
        app.insert_resource(AuthoritativeSimulation::new_networked(
            demo.simulation,
            demo.content,
            demo.match_config,
            client,
            PlayerId(0),
            0,
        ))
        .insert_resource(SelectedMatch {
            content: demo.content,
            direct_buildings: demo.direct_buildings,
            local_player: PlayerId(0),
        })
        .insert_resource(PresentationSamples::new(snapshot))
        .insert_resource(SimulationPlayback::default())
        .insert_resource(Time::<()>::default())
        .insert_resource(Time::<Fixed>::from_hz(f64::from(
            CASTLE_FIGHT_SIMULATION_HZ,
        )))
        .init_resource::<Publications>()
        .init_resource::<sim_thread::SimWorker>()
        .add_systems(
            Update,
            (
                sim_thread::ingest_network_events,
                sim_thread::launch_sim_ticks,
                sim_thread::join_sim_ticks,
                advance_network_presentation,
                count_publications,
            )
                .chain(),
        );
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(60));
        app.update();
        let displayed_x = |world: &World| {
            let samples = world.resource::<PresentationSamples>();
            let alpha = world
                .resource::<SimulationPlayback>()
                .interpolation_alpha(world.resource::<Time<Fixed>>(), samples);
            presentation::sim_point_to_world_lerp(
                samples.previous.builders[&builder.id].position,
                samples.current.builders[&builder.id].position,
                alpha,
            )
            .x
        };
        let before = displayed_x(app.world());
        assert_eq!(
            app.world().resource::<PresentationSamples>().previous.tick,
            1
        );
        let publications = app.world().resource::<Publications>().0;
        {
            let mut fixed = app.world_mut().resource_mut::<Time<Fixed>>();
            fixed.accumulate_overstep(Duration::from_millis(36));
            let timestep = fixed.timestep();
            fixed.discard_overstep(timestep);
        }
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(Duration::from_millis(2));
        app.update();
        assert!(displayed_x(app.world()) > before);
        assert_eq!(app.world().resource::<Publications>().0, publications);

        let control = driver
            .emit_local_control(
                &mut source.simulation,
                castle_fight_sim::MatchControlEvent::Debug(
                    castle_fight_sim::debug::DebugCommand::GrantResources,
                ),
            )
            .unwrap();
        let authoritative = app.world().resource::<AuthoritativeSimulation>();
        let AuthorityMode::Network { client, .. } = &authoritative.authority else {
            unreachable!()
        };
        client.inject_server_message_for_test(ServerMessage::StreamRecord {
            record: (&CanonicalStreamRecord::Control(control)).into(),
        });
        let before = displayed_x(app.world());
        app.update();
        let samples = app.world().resource::<PresentationSamples>();
        assert_eq!(samples.previous.tick, 1);
        assert_eq!(samples.current.tick, 2);
        assert_eq!(
            samples.current.players[&PlayerId(0)].resources,
            source.simulation.player(PlayerId(0)).unwrap().resources
        );
        assert!(samples.current.attacks.is_empty());
        assert!(!samples.tick_advanced());
        assert!(displayed_x(app.world()) > before);
        assert_eq!(app.world().resource::<Publications>().0, publications + 1);
        assert_eq!(
            app.world()
                .resource::<AuthoritativeSimulation>()
                .simulation
                .checksum(),
            source.simulation.checksum()
        );
    }

    #[test]
    fn paused_fixed_update_does_not_advance_authoritative_tick() {
        let simulation = Simulation::new(Default::default(), 1);
        let initial_snapshot = PresentationSnapshot::capture(&simulation);
        let content = castle_fight_sim::castle_fight_content_bundle(MapVersion::CASTLE_FIGHT_9_27)
            .expect("default development content");
        let mut app = App::new();
        app.insert_resource(SimulationPlayback { paused: true })
            .insert_resource(AuthoritativeSimulation::new(simulation, content))
            .insert_resource(PresentationSamples::new(initial_snapshot))
            .init_resource::<ActionPanelState>()
            .insert_resource(SelectedMatch {
                content,
                direct_buildings: Vec::new(),
                local_player: PlayerId(0),
            })
            .init_resource::<sim_thread::SimWorker>()
            .insert_resource(Time::<Fixed>::from_hz(f64::from(
                CASTLE_FIGHT_SIMULATION_HZ,
            )))
            .add_systems(
                Update,
                (
                    sim_thread::count_due_sim_ticks,
                    sim_thread::launch_sim_ticks,
                    sim_thread::join_sim_ticks,
                )
                    .chain(),
            );

        app.update();
        assert_eq!(
            app.world()
                .resource::<AuthoritativeSimulation>()
                .simulation
                .tick(),
            0
        );
        assert_eq!(
            app.world().resource::<PresentationSamples>().current.tick,
            0
        );

        app.world_mut().resource_mut::<SimulationPlayback>().paused = false;
        app.update();
        assert_eq!(
            app.world()
                .resource::<AuthoritativeSimulation>()
                .simulation
                .tick(),
            1
        );
        assert_eq!(
            app.world().resource::<PresentationSamples>().current.tick,
            1
        );
    }
}
