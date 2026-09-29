mod bridge;
mod build_ui;
mod builder_controls;
mod building_models;
mod cursor;
mod debug_menu;
mod demo;
mod doodads;
mod inspection;
mod lobby;
mod network;
mod performance_ui;
mod presentation;
mod render_audit;
mod render_tuning;
mod resource_ui;
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
    prelude::*,
    tasks::{IoTaskPool, TaskPoolBuilder, available_parallelism},
    time::Fixed,
    window::PresentMode,
};
use castle_fight_protocol::{
    CatchUpComplete, CheckpointReport, ClientMessage, CommandAcknowledgement, CommandRequest,
    CompatibilityIdentity, MAX_SNAPSHOT_BYTES, ProtocolErrorCode, SNAPSHOT_CHUNK_BYTES,
    ServerMessage, SnapshotChunk, SnapshotTransferBegin, WireCommandExecution,
};
use castle_fight_sim::{
    AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION, CANONICAL_CHECKSUM_SCHEMA_VERSION,
    CASTLE_FIGHT_SIMULATION_HZ, CanonicalStreamError, CanonicalStreamRecord,
    CastleFightBuilderRace, CastleFightContentAvailability, CastleFightContentBundle,
    CastleFightMatchConfig, CastleFightMatchSetupError, CastleFightParticipantConfig,
    CommandExecution, CommandExecutionResult, CommandOutcome, CommandSubmission, DriverTickResult,
    InputStreamPosition, MapVersion, MatchDriver, PlayerCommand, PlayerId, Simulation,
    SimulationSnapshot, Team, castle_fight_registered_releases,
};

#[cfg(test)]
use castle_fight_sim::SimPoint;

use bridge::{PresentationSamples, PresentationSnapshot};
use build_ui::{ActionPanelState, BuildUiPlugin};
use builder_controls::BuilderControlPlugin;
use cursor::CursorPresentationPlugin;
use debug_menu::DebugMenuPlugin;
use demo::{BuildKind, DEVELOPMENT_MATCH_SEED, create_demo_world_for_match_config};
use doodads::DoodadPresentationPlugin;
use inspection::InspectionPlugin;
use lobby::{LobbyPlugin, LobbyState};
use network::{NetworkClient, NetworkEvent};
use performance_ui::{
    PerformanceCounters, PerformanceUiPlugin, SystemTraceDisplay, performance_trace_layer,
};
use presentation::CastlePresentationPlugin;
use render_audit::{
    RenderAudit, RenderAuditPlugin, RenderExperiment, SceneCensus, format_render_passes,
};
use render_tuning::StandardMaterialBindlessSlabPlugin;
use resource_ui::{ResourceUiPlugin, TOP_BAR_HEIGHT};
use terrain::{TerrainSurface, TerrainTextureLayout, TerrainTextureSet, client_asset_root};
use view_state::{
    ViewState, ViewStatePersistence, persist_view_state, sync_cursor_grab, toggle_fullscreen,
};

const ASSET_IO_STACK_BYTES: usize = 8 * 1024 * 1024;

enum AuthorityMode {
    Local,
    Network {
        client: Box<NetworkClient>,
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
    simulation: Simulation,
    driver: MatchDriver,
    pending_feedback: Vec<CommandExecution>,
    pending_status: Vec<String>,
    expected_execution_batch: Option<(u64, Vec<WireCommandExecution>)>,
    catch_up: Option<SnapshotCatchUp>,
    authority: AuthorityMode,
    commands_enabled: bool,
}

impl AuthoritativeSimulation {
    fn new(simulation: Simulation, content: &'static CastleFightContentBundle) -> Self {
        let driver = MatchDriver::new(&simulation, content);
        Self {
            simulation,
            driver,
            pending_feedback: Vec::new(),
            pending_status: Vec::new(),
            expected_execution_batch: None,
            catch_up: None,
            authority: AuthorityMode::Local,
            commands_enabled: true,
        }
    }

    fn new_networked(
        simulation: Simulation,
        content: &'static CastleFightContentBundle,
        client: NetworkClient,
        assigned_player: PlayerId,
        next_sequence: u64,
    ) -> Self {
        let driver = MatchDriver::new(&simulation, content);
        Self {
            simulation,
            driver,
            pending_feedback: Vec::new(),
            pending_status: Vec::new(),
            expected_execution_batch: None,
            catch_up: None,
            authority: AuthorityMode::Network {
                client: Box::new(client),
                assigned_player,
                next_sequence,
                connected: true,
                pending_handoff_position: None,
            },
            commands_enabled: true,
        }
    }

    #[must_use]
    fn is_networked(&self) -> bool {
        matches!(self.authority, AuthorityMode::Network { .. })
    }

    pub(crate) fn submit_local_command(
        &mut self,
        player: PlayerId,
        command: PlayerCommand,
    ) -> ClientCommandSubmission {
        if !self.commands_enabled {
            return ClientCommandSubmission::Failed;
        }
        match &mut self.authority {
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
                        self.pending_status
                            .push(format!("Command not sent: {error}."));
                        ClientCommandSubmission::Failed
                    }
                }
            }
        }
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
    pub(crate) fn interpolation_alpha(self, fixed_time: &Time<Fixed>) -> f32 {
        if self.paused {
            1.0
        } else {
            fixed_time.overstep_fraction()
        }
    }
}

#[derive(Component)]
struct SimulationPausePanel;

#[derive(Component)]
struct SimulationPauseText;

fn main() {
    configure_asset_io_task_pool();
    let options = ClientOptions::parse();
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
    let match_config = client_match_config(&options).unwrap_or_else(|error| {
        eprintln!(
            "cannot configure Castle Fight {}/{}: {error}",
            options.map_version, options.release_revision
        );
        std::process::exit(2);
    });
    let mut demo = create_demo_world_for_match_config(
        default_worker_count(),
        options.stress_units,
        match_config,
    )
    .unwrap_or_else(|error| {
        eprintln!(
            "cannot start Castle Fight {}/{}: {error}",
            options.map_version, options.release_revision
        );
        std::process::exit(2);
    });
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
        let (client, assignment) =
            NetworkClient::connect(server_address, compatibility, u64::from(std::process::id()))
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
    let initial_snapshot = PresentationSnapshot::capture(&demo.simulation);
    let present_mode = if options.stress_units.is_some()
        || options.stress_visual.is_some()
        || options.profile_quicksave.is_some()
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
            client,
            local_player,
            next_sequence,
        )
    } else {
        AuthoritativeSimulation::new(demo.simulation, demo.content)
    };

    let view_state = ViewState::load();
    let show_lobby = options.profile_quicksave.is_none() && options.stress_units.is_none();
    authoritative.commands_enabled = !show_lobby;
    let lobby_state = LobbyState::new(options.clone(), local_player, networked);
    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.025, 0.03, 0.04)))
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
            paused: show_lobby || options.is_profiling(),
        })
        .insert_resource(PresentationSamples::new(initial_snapshot))
        .insert_resource(demo.metrics)
        .insert_resource(TerrainSurface::new(demo.terrain))
        .insert_resource(terrain_texture_layout)
        .insert_resource(terrain_textures)
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
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins((
            CastlePresentationPlugin::new(options.health_bars),
            DoodadPresentationPlugin,
            BuildUiPlugin,
            CursorPresentationPlugin,
            ResourceUiPlugin,
            PerformanceUiPlugin,
            InspectionPlugin,
            BuilderControlPlugin,
            DebugMenuPlugin,
        ))
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
        .add_systems(FixedUpdate, advance_authoritative_simulation);

    if show_lobby {
        app.insert_resource(lobby_state).add_plugins(LobbyPlugin);
    }

    if options.stress_units.is_some() || options.stress_visual.is_some() || options.is_profiling() {
        app.insert_resource(presentation::BenchmarkCamera {
            lock_input: options.is_profiling(),
        });
    }
    if let Some(stress_visual) = options.stress_visual {
        app.insert_resource(stress_visual);
    }

    if options.perf_log {
        app.insert_resource(PerfTelemetry(Timer::from_seconds(
            1.0,
            TimerMode::Repeating,
        )))
        .add_systems(Update, print_perf_telemetry);
    }
    if options.is_profiling() {
        app.insert_resource(options.render_experiment)
            .add_plugins(RenderAuditPlugin);
        let source = options.profile_quicksave.as_ref().map_or_else(
            || {
                if let Some(stress) = options.stress_visual {
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
        app.insert_resource(AutomatedProfileRun {
            source,
            warmup: options.profile_warmup,
            duration: options.profile_duration,
            warmup_started: None,
            capture_started: None,
            paused: options.profile_paused,
            experiment: options.render_experiment,
        })
        .add_systems(
            Update,
            finish_automated_profile.after(performance_ui::finish_presentation_profile),
        );
    }

    app.run();
}

#[derive(Resource)]
struct AutomatedProfileRun {
    source: String,
    warmup: Duration,
    duration: Duration,
    warmup_started: Option<Instant>,
    capture_started: Option<Instant>,
    paused: bool,
    experiment: RenderExperiment,
}

fn finish_automated_profile(
    mut run: ResMut<AutomatedProfileRun>,
    mut counters: ResMut<PerformanceCounters>,
    presentation: Res<PresentationSamples>,
    trace_display: Res<SystemTraceDisplay>,
    mut playback: ResMut<SimulationPlayback>,
    mut exit: MessageWriter<AppExit>,
    audit: (Res<RenderAudit>, Res<DiagnosticsStore>, SceneCensus),
) {
    let now = Instant::now();
    let warmup_started = *run.warmup_started.get_or_insert(now);
    if run.capture_started.is_none() {
        if now.duration_since(warmup_started) < run.warmup {
            return;
        }
        playback.paused = run.paused;
        counters.start_capture();
        audit.0.start_capture();
        run.capture_started = Some(now);
        return;
    }
    let capture_started = run
        .capture_started
        .expect("capture start is set after automated profile warmup");
    if now.duration_since(capture_started) < run.duration || !counters.capture_has_frame_sample() {
        return;
    }
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
        "client-profile paused={} experiment={:?}",
        run.paused, run.experiment
    );
    print!("{}", report.format());
    print!("{}", audit.0.format());
    print!("{}", audit.2.format());
    print!("{}", format_render_passes(&audit.1));
    print!("{}", trace_display.format());
    exit.write(AppExit::Success);
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
    profile_paused: bool,
    profile: bool,
    render_experiment: RenderExperiment,
    map_version: MapVersion,
    release_revision: String,
    match_seed: u64,
    team_size: usize,
    server: Option<std::net::SocketAddr>,
    list_map_versions: bool,
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
            profile_paused: false,
            profile: false,
            render_experiment: RenderExperiment::Baseline,
            map_version: MapVersion::CASTLE_FIGHT_9_27,
            release_revision: "r1".to_owned(),
            match_seed: DEVELOPMENT_MATCH_SEED,
            team_size: 1,
            server: None,
            list_map_versions: false,
        };
        let mut args = std::env::args().skip(1);
        while let Some(arg) = args.next() {
            match arg.as_str() {
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
                "--render-experiment" => {
                    let value = args.next().expect("--render-experiment requires a name");
                    options.render_experiment =
                        RenderExperiment::parse(&value).unwrap_or_else(|error| panic!("{error}"));
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
                }
                "--profile-duration" => {
                    let value = args
                        .next()
                        .expect("--profile-duration requires a positive number of seconds");
                    options.profile_duration =
                        parse_profile_duration(&value).unwrap_or_else(|error| panic!("{error}"));
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
                        "Usage: cargo run -p castle-fight-client -- [--server 127.0.0.1:6112] [--map-version 9.27] [--map-revision r1] [--seed N] [--team-size 1|2|3] [--list-map-versions] [--stress-units N] [--stress-visual RAWCODE N] [--no-health-bars] [--perf-log] [--profile-quicksave] [--profile-quicksave-path PATH] [--profile-warmup SECONDS] [--profile-duration SECONDS] [--profile-paused] [--profile] [--render-experiment baseline|freeze-bounds|hide-skinned|hide-particles|hide-transparent|legacy-team-color|legacy-geoset-visibility|legacy-attachment-search|legacy-attachment-index|legacy-effect-pooling|legacy-splat-updates|legacy-splat-material-state|legacy-animated-alpha-state|legacy-animated-texture-state|freeze-materials|freeze-poses|bindless-auto|bindless-64|bindless-128|bindless-256|no-bindless]"
                    );
                    std::process::exit(0);
                }
                unknown => panic!("unknown client option: {unknown}"),
            }
        }
        assert!(
            !options.profile
                || options.stress_units.is_some()
                || options.stress_visual.is_some()
                || options.profile_quicksave.is_some(),
            "--profile requires --stress-units, --stress-visual, or --profile-quicksave",
        );
        assert!(
            options.is_profiling()
                || (!options.profile_paused
                    && options.render_experiment == RenderExperiment::Baseline
                    && options.stress_visual.is_none()),
            "render experiments, --profile-paused, and --stress-visual require --profile or --profile-quicksave",
        );
        options
    }
}

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
    let western = [0u8, 1, 2];
    let eastern = [6u8, 7, 8];
    let participants =
        western
            .iter()
            .take(options.team_size)
            .map(|slot| CastleFightParticipantConfig {
                id: PlayerId(*slot),
                team: Team(0),
                builder_race: CastleFightBuilderRace::Human,
            })
            .chain(eastern.iter().take(options.team_size).map(|slot| {
                CastleFightParticipantConfig {
                    id: PlayerId(*slot),
                    team: Team(1),
                    builder_race: CastleFightBuilderRace::Human,
                }
            }))
            .collect();
    CastleFightMatchConfig::development_subset_with_participants(
        options.map_version,
        &options.release_revision,
        options.match_seed,
        participants,
    )
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
                left: px(580.0),
                top: px(TOP_BAR_HEIGHT + 10.0),
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
    selected_match: Res<SelectedMatch>,
    debug_menu: Res<debug_menu::DebugMenuState>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut presentation: ResMut<PresentationSamples>,
) {
    if !control_modifier_pressed(&keys) {
        return;
    }

    if keys.just_pressed(KeyCode::KeyS) {
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
        return;
    }

    if !keys.just_pressed(KeyCode::KeyO) {
        return;
    }
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
        authoritative
            .simulation
            .debug_set_buildings_invulnerable(debug_menu.buildings_invulnerable());
        authoritative.driver = MatchDriver::new(&authoritative.simulation, selected_match.content);
        authoritative.pending_feedback.clear();
        authoritative.expected_execution_batch = None;
        authoritative.catch_up = None;
        *presentation =
            PresentationSamples::new(PresentationSnapshot::capture(&authoritative.simulation));
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
    action_panel: Option<Res<build_ui::ActionPanelState>>,
    authoritative: Res<AuthoritativeSimulation>,
    mut playback: ResMut<SimulationPlayback>,
) {
    if lobby.as_ref().is_some_and(|lobby| lobby.active()) {
        return;
    }
    if authoritative.is_networked() {
        playback.paused = false;
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

fn advance_authoritative_simulation(
    playback: Res<SimulationPlayback>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut presentation: ResMut<PresentationSamples>,
    mut performance: Option<ResMut<PerformanceCounters>>,
) {
    let fixed_started = std::time::Instant::now();

    if authoritative.is_networked() {
        let result = process_network_events(
            &mut authoritative,
            &mut presentation,
            performance.as_deref_mut(),
        );
        if let Some(performance) = performance.as_deref_mut() {
            performance.record_fixed_update_wall(fixed_started.elapsed());
        }
        if let Err(error) = result {
            panic!("network canonical stream invariant failed: {error}");
        }
        return;
    }
    if playback.paused {
        return;
    }
    match advance_authoritative_simulation_once(&mut authoritative, &mut presentation) {
        Ok(result) => {
            if let Some(performance) = performance.as_deref_mut() {
                performance.record_sim_tick(
                    result.tick_result.completed_tick,
                    result.tick_result.timings,
                    result.tick_result.collision_fallback_searches,
                    result.tick_result.collision_fallback_candidate_checks,
                    result.tick_result.collision_fallback_max_ring,
                );
            }
        }
        Err(CanonicalStreamError::MatchNotRunning(_)) => {}
        Err(error) => panic!("local canonical stream invariant failed: {error:?}"),
    }
    if let Some(performance) = performance.as_deref_mut() {
        performance.record_fixed_update_wall(fixed_started.elapsed());
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
        ..
    } = authoritative;
    let result = driver.advance_local_tick(simulation)?;
    pending_feedback.extend(result.executions.iter().copied());
    presentation.publish(PresentationSnapshot::capture(simulation));
    Ok(result)
}

fn process_network_events(
    authoritative: &mut AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
    mut performance: Option<&mut PerformanceCounters>,
) -> Result<(), String> {
    let events = match &mut authoritative.authority {
        AuthorityMode::Local => return Ok(()),
        AuthorityMode::Network { client, .. } => {
            client.poll_reconnect();
            client.drain_events()
        }
    };
    for event in events {
        match event {
            NetworkEvent::Disconnected(reason) => {
                authoritative.catch_up = None;
                authoritative.expected_execution_batch = None;
                if let AuthorityMode::Network {
                    connected,
                    pending_handoff_position,
                    ..
                } = &mut authoritative.authority
                {
                    *connected = false;
                    *pending_handoff_position = None;
                }
                authoritative
                    .pending_status
                    .push(format!("Disconnected from server: {reason}"));
            }
            NetworkEvent::Reconnected(assignment) => {
                let (assigned_player, expected_team) = match &authoritative.authority {
                    AuthorityMode::Network {
                        assigned_player, ..
                    } => {
                        let expected_team = authoritative
                            .simulation
                            .player(*assigned_player)
                            .ok_or_else(|| {
                                "assigned network player disappeared locally".to_owned()
                            })?
                            .team
                            .0;
                        (*assigned_player, expected_team)
                    }
                    AuthorityMode::Local => unreachable!("network event in local mode"),
                };
                if assignment.player_id != assigned_player.0 || assignment.team != expected_team {
                    return Err("server reconnect assignment changed player identity".to_owned());
                }
                authoritative.catch_up = None;
                authoritative.expected_execution_batch = None;
                authoritative.pending_feedback.clear();
                if let AuthorityMode::Network {
                    next_sequence,
                    connected,
                    pending_handoff_position,
                    ..
                } = &mut authoritative.authority
                {
                    *next_sequence = assignment.next_client_sequence;
                    *connected = false;
                    *pending_handoff_position = Some(assignment.next_stream_position);
                }
                authoritative
                    .pending_status
                    .push("Reconnected; synchronizing authoritative state.".to_owned());
            }
            NetworkEvent::ReconnectFailed(reason) => {
                authoritative
                    .pending_status
                    .push(format!("Reconnect attempt failed: {reason}"));
            }
            NetworkEvent::Message(message) => match message {
                ServerMessage::SnapshotBegin { begin } => {
                    begin_snapshot_catch_up(authoritative, begin)?;
                }
                ServerMessage::SnapshotChunk { chunk } => {
                    append_snapshot_chunk(authoritative, chunk)?;
                }
                ServerMessage::CatchUpComplete { complete } => {
                    finish_snapshot_catch_up(authoritative, presentation, complete)?;
                }
                ServerMessage::CommandAcknowledged { acknowledgement } => {
                    let status = match acknowledgement {
                        CommandAcknowledgement::Scheduled {
                            client_sequence,
                            tick,
                            duplicate,
                            ..
                        } => {
                            if duplicate {
                                format!(
                                    "Command #{client_sequence} already scheduled for tick {tick}."
                                )
                            } else {
                                format!("Command #{client_sequence} accepted for tick {tick}.")
                            }
                        }
                        CommandAcknowledgement::Rejected {
                            client_sequence,
                            reason,
                            duplicate,
                        } => {
                            let duplicate = if duplicate { " duplicate" } else { "" };
                            format!(
                                "Command #{client_sequence}{duplicate} rejected by server: {reason:?}."
                            )
                        }
                    };
                    authoritative.pending_status.push(status);
                }
                ServerMessage::StreamRecord { record } => {
                    let canonical: CanonicalStreamRecord = record.into();
                    if let Some((snapshot_loaded, handoff_stream_position)) =
                        authoritative.catch_up.as_ref().map(|catch_up| {
                            (
                                catch_up.snapshot_loaded,
                                catch_up.begin.handoff_stream_position,
                            )
                        })
                    {
                        if !snapshot_loaded {
                            return Err(
                                "received reconnect history before snapshot completion".to_owned()
                            );
                        }
                        if authoritative.driver.next_stream_position().0 >= handoff_stream_position
                        {
                            return Err(
                                "received reconnect history beyond pinned handoff boundary"
                                    .to_owned(),
                            );
                        }
                        let applied = authoritative
                            .driver
                            .apply_stream_record(&mut authoritative.simulation, canonical)
                            .map_err(|error| {
                                format!("canonical catch-up stream error: {error:?}")
                            })?;
                        if let (Some(result), Some(performance)) =
                            (applied, performance.as_deref_mut())
                        {
                            performance.record_sim_tick(
                                result.tick_result.completed_tick,
                                result.tick_result.timings,
                                result.tick_result.collision_fallback_searches,
                                result.tick_result.collision_fallback_candidate_checks,
                                result.tick_result.collision_fallback_max_ring,
                            );
                        }
                        authoritative.simulation.clear_presentation_events();
                        if authoritative.driver.next_stream_position().0 > handoff_stream_position {
                            return Err(
                                "reconnect history crossed pinned handoff boundary".to_owned()
                            );
                        }
                    } else {
                        let applied = authoritative
                            .driver
                            .apply_stream_record(&mut authoritative.simulation, canonical)
                            .map_err(|error| format!("canonical stream error: {error:?}"))?;
                        if let Some(result) = applied {
                            if let Some(performance) = performance.as_deref_mut() {
                                performance.record_sim_tick(
                                    result.tick_result.completed_tick,
                                    result.tick_result.timings,
                                    result.tick_result.collision_fallback_searches,
                                    result.tick_result.collision_fallback_candidate_checks,
                                    result.tick_result.collision_fallback_max_ring,
                                );
                            }
                            let expected = result
                                .executions
                                .iter()
                                .copied()
                                .map(WireCommandExecution::from)
                                .collect();
                            authoritative.expected_execution_batch =
                                Some((result.finalized.tick, expected));
                            authoritative
                                .pending_feedback
                                .extend(result.executions.iter().copied());
                        }
                        presentation
                            .publish(PresentationSnapshot::capture(&authoritative.simulation));
                    }
                }
                ServerMessage::TickExecutions { batch } => {
                    let Some((expected_tick, expected_executions)) =
                        authoritative.expected_execution_batch.take()
                    else {
                        return Err(format!(
                            "received execution batch for tick {} before its finalized input record",
                            batch.tick
                        ));
                    };
                    if expected_tick != batch.tick || expected_executions != batch.executions {
                        return Err(format!(
                            "server execution batch disagrees with deterministic local execution for tick {}",
                            batch.tick
                        ));
                    }
                }
                ServerMessage::Checkpoint { checkpoint } => {
                    let completed_tick = authoritative.simulation.tick().checked_sub(1);
                    if completed_tick != Some(checkpoint.completed_tick) {
                        return Err(format!(
                            "checkpoint for tick {} arrived while local completed tick is {completed_tick:?}",
                            checkpoint.completed_tick
                        ));
                    }
                    let local_checksum = authoritative.simulation.checksum();
                    let report = ClientMessage::CheckpointReport {
                        report: CheckpointReport {
                            completed_tick: checkpoint.completed_tick,
                            checksum: local_checksum,
                        },
                    };
                    let send_result = match &authoritative.authority {
                        AuthorityMode::Network { client, .. } => client.send(report),
                        AuthorityMode::Local => unreachable!("network event in local mode"),
                    };
                    if let Err(error) = send_result {
                        return Err(format!("could not report checkpoint: {error}"));
                    }
                    if local_checksum != checkpoint.checksum {
                        if let AuthorityMode::Network { connected, .. } =
                            &mut authoritative.authority
                        {
                            *connected = false;
                        }
                        authoritative.pending_status.push(format!(
                            "Checksum mismatch at tick {}; requesting authoritative state replacement.",
                            checkpoint.completed_tick
                        ));
                    }
                }
                ServerMessage::ProtocolError { code } => {
                    authoritative
                        .pending_status
                        .push(format!("Server protocol error: {code:?}."));
                    if matches!(
                        code,
                        ProtocolErrorCode::MalformedMessage | ProtocolErrorCode::Unauthorized
                    ) {
                        return Err(format!("fatal server protocol error: {code:?}"));
                    }
                }
                ServerMessage::HelloAccepted { .. } | ServerMessage::HelloRejected { .. } => {
                    return Err("received handshake response after session startup".to_owned());
                }
            },
        }
    }
    Ok(())
}

fn begin_snapshot_catch_up(
    authoritative: &mut AuthoritativeSimulation,
    begin: SnapshotTransferBegin,
) -> Result<(), String> {
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
        authoritative.driver = MatchDriver::new_replica_from_snapshot(
            &authoritative.simulation,
            content,
            InputStreamPosition(catch_up.begin.snapshot_stream_position),
        );
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
    *presentation =
        PresentationSamples::new(PresentationSnapshot::capture(&authoritative.simulation));
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
) {
    for status in std::mem::take(&mut authoritative.pending_status) {
        action_panel.status = status;
    }
    let feedback = std::mem::take(&mut authoritative.pending_feedback);
    for execution in feedback {
        if execution.scheduled.player != selected_match.local_player
            && !debug_menu.controls_all_players()
        {
            continue;
        }
        action_panel.status = local_command_feedback_text(execution);
    }
}

fn local_command_feedback_text(execution: CommandExecution) -> String {
    let action = match execution.scheduled.command {
        PlayerCommand::MoveBuilder { .. } => "Move",
        PlayerCommand::FollowWithBuilder { .. } => "Follow",
        PlayerCommand::StopBuilder { .. } => "Stop",
        PlayerCommand::BlinkBuilder { .. } => "Blink",
        PlayerCommand::RepairWithBuilder { .. } => "Repair",
        PlayerCommand::SetBuilderRepairAutocast { .. } => "Repair autocast",
        PlayerCommand::PlaceBuilding { .. } => "Build",
        PlayerCommand::CancelBuildingConstruction { .. } => "Cancel construction",
        PlayerCommand::QueueProductionUnit { .. } => "Train unit",
        PlayerCommand::CancelProductionUnit { .. } => "Cancel training",
        PlayerCommand::UpgradeBuilding { .. } => "Upgrade",
        PlayerCommand::AttackWithBuilding { .. } => "Attack",
        PlayerCommand::CastBuildingSpell { .. } => "Cast spell",
        PlayerCommand::SetBuildingSpellAutocast { .. } => "Spell autocast",
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
        CommandOutcome::Rejected(reason) => format!(
            "{action} rejected on tick {}: {reason:?}.",
            execution.scheduled.tick
        ),
    }
}

fn configure_asset_io_task_pool() {
    // Warcraft's generated glTF set exercises Bevy's recursive glTF loader deeply enough that the
    // platform-default worker stack can occasionally overflow during the startup asset burst. Keep
    // Bevy's normal IO-thread count, but give only that pool a larger stack instead of inflating the
    // simulation/compute pools (or requiring RUST_MIN_STACK globally).
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
            client,
            PlayerId(0),
            0,
        );
        let mut presentation = PresentationSamples::new(initial_presentation);
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
            .add_systems(Update, advance_authoritative_simulation);

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
