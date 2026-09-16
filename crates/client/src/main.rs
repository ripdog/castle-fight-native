mod bridge;
mod build_ui;
mod builder_controls;
mod building_models;
mod cursor;
mod debug_menu;
mod demo;
mod doodads;
mod inspection;
mod network;
mod presentation;
mod resource_ui;
mod terrain;
mod ui_icons;
mod unit_models;
mod wc3_effects;
mod wc3_text;

use bevy::{
    asset::AssetPlugin,
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
    tasks::{IoTaskPool, TaskPoolBuilder, available_parallelism},
    time::Fixed,
    window::PresentMode,
};
use castle_fight_protocol::{
    CheckpointReport, ClientMessage, CommandAcknowledgement, CommandRequest, CompatibilityIdentity,
    ProtocolErrorCode, ServerMessage, WireCommandExecution,
};
use castle_fight_sim::{
    AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION, CANONICAL_CHECKSUM_SCHEMA_VERSION,
    CASTLE_FIGHT_SIMULATION_HZ, CanonicalStreamError, CanonicalStreamRecord,
    CastleFightBuilderRace, CastleFightContentAvailability, CastleFightContentBundle,
    CastleFightMatchConfig, CastleFightMatchSetupError, CastleFightParticipantConfig,
    CommandExecution, CommandExecutionResult, CommandOutcome, CommandSubmission, DriverTickResult,
    MapVersion, MatchDriver, PlayerCommand, PlayerId, Simulation, Team,
    castle_fight_registered_releases,
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
use network::{NetworkClient, NetworkEvent};
use presentation::CastlePresentationPlugin;
use resource_ui::{ResourceUiPlugin, TOP_BAR_HEIGHT};
use terrain::{TerrainSurface, TerrainTextureLayout, TerrainTextureSet, client_asset_root};

const ASSET_IO_STACK_BYTES: usize = 8 * 1024 * 1024;

enum AuthorityMode {
    Local,
    Network {
        client: NetworkClient,
        assigned_player: PlayerId,
        next_sequence: u64,
        connected: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ClientCommandSubmission {
    Local(CommandSubmission),
    Submitted { client_sequence: u64 },
    Failed,
}

#[derive(Resource)]
pub(crate) struct AuthoritativeSimulation {
    simulation: Simulation,
    driver: MatchDriver,
    pending_feedback: Vec<CommandExecution>,
    pending_status: Vec<String>,
    expected_execution_batch: Option<(u64, Vec<WireCommandExecution>)>,
    authority: AuthorityMode,
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
            authority: AuthorityMode::Local,
        }
    }

    fn new_networked(
        simulation: Simulation,
        content: &'static CastleFightContentBundle,
        client: NetworkClient,
        assigned_player: PlayerId,
    ) -> Self {
        let driver = MatchDriver::new(&simulation, content);
        Self {
            simulation,
            driver,
            pending_feedback: Vec::new(),
            pending_status: Vec::new(),
            expected_execution_batch: None,
            authority: AuthorityMode::Network {
                client,
                assigned_player,
                next_sequence: 0,
                connected: true,
            },
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
    let match_config = client_match_config(&options).unwrap_or_else(|error| {
        eprintln!(
            "cannot configure Castle Fight {}/{}: {error}",
            options.map_version, options.release_revision
        );
        std::process::exit(2);
    });
    let demo = create_demo_world_for_match_config(
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
    let (local_player, network_client) = if let Some(server_address) = options.server {
        let compatibility = compatibility_identity_for_demo(&demo);
        let (client, assignment) =
            NetworkClient::connect(server_address, compatibility, u64::from(std::process::id()))
                .unwrap_or_else(|error| {
                    eprintln!("cannot join Castle Fight server {server_address}: {error}");
                    std::process::exit(2);
                });
        if assignment.next_stream_position != 0 || assignment.completed_tick.is_some() {
            eprintln!(
                "server attempted a late-join handoff that Step 9 clients do not support yet"
            );
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
        (local_player, Some(client))
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
    let present_mode = if options.stress_units.is_some() {
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

    let authoritative = if let Some(client) = network_client {
        AuthoritativeSimulation::new_networked(demo.simulation, demo.content, client, local_player)
    } else {
        AuthoritativeSimulation::new(demo.simulation, demo.content)
    };

    let mut app = App::new();
    app.insert_resource(ClearColor(Color::srgb(0.025, 0.03, 0.04)))
        .insert_resource(Time::<Fixed>::from_hz(f64::from(
            CASTLE_FIGHT_SIMULATION_HZ,
        )))
        .insert_resource(SelectedMatch {
            content: demo.content,
            direct_buildings: demo.direct_buildings,
            local_player,
        })
        .insert_resource(authoritative)
        .init_resource::<SimulationPlayback>()
        .insert_resource(PresentationSamples::new(initial_snapshot))
        .insert_resource(demo.metrics)
        .insert_resource(TerrainSurface::new(demo.terrain))
        .insert_resource(terrain_texture_layout)
        .insert_resource(terrain_textures)
        .add_plugins(
            DefaultPlugins
                .set(AssetPlugin {
                    file_path: client_asset_root().to_string_lossy().into_owned(),
                    ..default()
                })
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: window_title,
                        resolution: (1440, 900).into(),
                        present_mode,
                        ..default()
                    }),
                    ..default()
                }),
        )
        .add_plugins(FrameTimeDiagnosticsPlugin::default())
        .add_plugins((
            CastlePresentationPlugin::new(options.health_bars),
            DoodadPresentationPlugin,
            BuildUiPlugin,
            CursorPresentationPlugin,
            ResourceUiPlugin,
            InspectionPlugin,
            BuilderControlPlugin,
            DebugMenuPlugin,
        ))
        .add_systems(Startup, setup_simulation_pause_ui)
        .add_systems(
            Update,
            (
                toggle_simulation_pause,
                update_simulation_pause_ui,
                sync_local_command_feedback,
            )
                .chain(),
        )
        .add_systems(FixedUpdate, advance_authoritative_simulation);

    if options.perf_log {
        app.insert_resource(PerfTelemetry(Timer::from_seconds(
            1.0,
            TimerMode::Repeating,
        )))
        .add_systems(Update, print_perf_telemetry);
    }

    app.run();
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
    health_bars: bool,
    perf_log: bool,
    map_version: MapVersion,
    release_revision: String,
    match_seed: u64,
    team_size: usize,
    server: Option<std::net::SocketAddr>,
    list_map_versions: bool,
}

impl ClientOptions {
    fn parse() -> Self {
        let mut options = Self {
            stress_units: None,
            health_bars: true,
            perf_log: false,
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
                "--no-health-bars" => options.health_bars = false,
                "--perf-log" => options.perf_log = true,
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
                        "Usage: cargo run -p castle-fight-client -- [--server 127.0.0.1:6112] [--map-version 9.27] [--map-revision r1] [--seed N] [--team-size 1|2|3] [--list-map-versions] [--stress-units N] [--no-health-bars] [--perf-log]"
                    );
                    std::process::exit(0);
                }
                unknown => panic!("unknown client option: {unknown}"),
            }
        }
        options
    }
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

fn toggle_simulation_pause(
    keys: Res<ButtonInput<KeyCode>>,
    authoritative: Res<AuthoritativeSimulation>,
    mut playback: ResMut<SimulationPlayback>,
) {
    if authoritative.is_networked() {
        playback.paused = false;
        return;
    }
    if keys.just_pressed(KeyCode::Space) || keys.just_pressed(KeyCode::KeyP) {
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
) {
    if authoritative.is_networked() {
        if let Err(error) = process_network_events(&mut authoritative, &mut presentation) {
            panic!("network canonical stream invariant failed: {error}");
        }
        return;
    }
    if playback.paused {
        return;
    }
    match advance_authoritative_simulation_once(&mut authoritative, &mut presentation) {
        Ok(_) | Err(CanonicalStreamError::MatchNotRunning(_)) => {}
        Err(error) => panic!("local canonical stream invariant failed: {error:?}"),
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
) -> Result<(), String> {
    let events = match &authoritative.authority {
        AuthorityMode::Local => return Ok(()),
        AuthorityMode::Network { client, .. } => client.drain_events(),
    };
    for event in events {
        match event {
            NetworkEvent::Disconnected(reason) => {
                if let AuthorityMode::Network { connected, .. } = &mut authoritative.authority {
                    *connected = false;
                }
                authoritative
                    .pending_status
                    .push(format!("Disconnected from server: {reason}"));
            }
            NetworkEvent::Message(message) => match message {
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
                    let applied = authoritative
                        .driver
                        .apply_stream_record(&mut authoritative.simulation, canonical)
                        .map_err(|error| format!("canonical stream error: {error:?}"))?;
                    if let Some(result) = applied {
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
                    presentation.publish(PresentationSnapshot::capture(&authoritative.simulation));
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
                        return Err(format!(
                            "authoritative checksum mismatch at tick {}: server {:#018x}, client {:#018x}",
                            checkpoint.completed_tick, checkpoint.checksum, local_checksum
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

fn sync_local_command_feedback(
    selected_match: Res<SelectedMatch>,
    mut authoritative: ResMut<AuthoritativeSimulation>,
    mut action_panel: ResMut<ActionPanelState>,
) {
    for status in std::mem::take(&mut authoritative.pending_status) {
        action_panel.status = status;
    }
    let feedback = std::mem::take(&mut authoritative.pending_feedback);
    for execution in feedback {
        if execution.scheduled.player != selected_match.local_player {
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
        PlayerCommand::UpgradeBuilding { .. } => "Upgrade",
        PlayerCommand::AttackWithBuilding { .. } => "Attack",
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
