mod bridge;
mod build_ui;
mod builder_controls;
mod building_models;
mod cursor;
mod debug_menu;
mod demo;
mod doodads;
mod inspection;
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
use castle_fight_sim::{
    CASTLE_FIGHT_SIMULATION_HZ, CastleFightContentAvailability, CastleFightContentBundle,
    MapVersion, PlayerId, Simulation, castle_fight_registered_releases,
};

use bridge::{PresentationSamples, PresentationSnapshot};
use build_ui::BuildUiPlugin;
use builder_controls::BuilderControlPlugin;
use cursor::CursorPresentationPlugin;
use debug_menu::DebugMenuPlugin;
use demo::{BuildKind, create_demo_world_for_version};
use doodads::DoodadPresentationPlugin;
use inspection::InspectionPlugin;
use presentation::CastlePresentationPlugin;
use resource_ui::{ResourceUiPlugin, TOP_BAR_HEIGHT};
use terrain::{TerrainSurface, TerrainTextureLayout, TerrainTextureSet, client_asset_root};

const ASSET_IO_STACK_BYTES: usize = 8 * 1024 * 1024;

#[derive(Resource)]
pub(crate) struct AuthoritativeSimulation {
    simulation: Simulation,
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
    let demo = create_demo_world_for_version(
        default_worker_count(),
        options.stress_units,
        options.map_version,
        &options.release_revision,
    )
    .unwrap_or_else(|error| {
        eprintln!(
            "cannot start Castle Fight {}/{}: {error}",
            options.map_version, options.release_revision
        );
        std::process::exit(2);
    });
    let local_player = demo
        .match_config
        .participants
        .first()
        .expect("playable Castle Fight match must contain a local participant")
        .id;
    let initial_snapshot = PresentationSnapshot::capture(&demo.simulation);
    let present_mode = if options.stress_units.is_some() {
        PresentMode::AutoNoVsync
    } else {
        PresentMode::AutoVsync
    };
    let window_title = format!(
        "Castle Fight Native 3D — CF {}/{} ({})",
        demo.match_config.release.map_version,
        demo.match_config.release.release_revision,
        demo.match_config
            .release
            .content_revision
            .unwrap_or("archived-only"),
    );

    let terrain_texture_layout =
        TerrainTextureLayout::from_wc3_terrain_json(demo.terrain_source_json)
            .expect("selected Warcraft terrain texture layout must be valid");
    let terrain_textures = TerrainTextureSet::load_default();

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
        .insert_resource(AuthoritativeSimulation {
            simulation: demo.simulation,
        })
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
            (toggle_simulation_pause, update_simulation_pause_ui).chain(),
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
                "--list-map-versions" => options.list_map_versions = true,
                "-h" | "--help" => {
                    println!(
                        "Usage: cargo run -p castle-fight-client -- [--map-version 9.27] [--map-revision r1] [--list-map-versions] [--stress-units N] [--no-health-bars] [--perf-log]"
                    );
                    std::process::exit(0);
                }
                unknown => panic!("unknown client option: {unknown}"),
            }
        }
        options
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
    mut playback: ResMut<SimulationPlayback>,
) {
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
    if playback.paused {
        return;
    }
    advance_authoritative_simulation_once(&mut authoritative, &mut presentation);
}

pub(crate) fn advance_authoritative_simulation_once(
    authoritative: &mut AuthoritativeSimulation,
    presentation: &mut PresentationSamples,
) {
    authoritative.simulation.step();
    presentation.publish(PresentationSnapshot::capture(&authoritative.simulation));
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
    use crate::build_ui::ActionPanelState;

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
    fn paused_fixed_update_does_not_advance_authoritative_tick() {
        let simulation = Simulation::new(Default::default(), 1);
        let initial_snapshot = PresentationSnapshot::capture(&simulation);
        let mut app = App::new();
        app.insert_resource(SimulationPlayback { paused: true })
            .insert_resource(AuthoritativeSimulation { simulation })
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
