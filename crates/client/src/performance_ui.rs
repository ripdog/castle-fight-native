use std::{
    collections::VecDeque,
    fmt::Write as _,
    time::{Duration, Instant},
};

use bevy::{
    app::{MainScheduleOrder, RunFixedMainLoop, SpawnScene},
    camera::visibility::{DynamicSkinnedMeshBounds, NoFrustumCulling, ViewVisibility},
    diagnostic::DiagnosticsStore,
    ecs::schedule::ScheduleLabel,
    mesh::skinning::SkinnedMesh,
    prelude::*,
    render::diagnostic::RenderDiagnosticsPlugin,
    time::Fixed,
};
use castle_fight_sim::{CASTLE_FIGHT_SIMULATION_HZ, TickTimings};

use crate::{bridge::PresentationSamples, resource_ui::TOP_BAR_HEIGHT};

const PANEL_LEFT: f32 = 72.0;
const PANEL_TOP: f32 = TOP_BAR_HEIGHT + 10.0;
const PANEL_WIDTH: f32 = 350.0;
const PANEL_PADDING: f32 = 10.0;
const PANEL_BACKGROUND: Color = Color::srgba(0.025, 0.030, 0.040, 0.94);
const PANEL_BORDER: Color = Color::srgba(0.28, 0.32, 0.38, 0.96);
const HEADING_COLOR: Color = Color::srgb(0.92, 0.94, 0.98);
const FRAME_COLOR: Color = Color::srgb(0.76, 0.80, 0.86);
const SMOOTHING_WINDOW: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct PresentationTimings {
    pub(crate) camera: Duration,
    pub(crate) model_prep: Duration,
    pub(crate) entity_sync: Duration,
    pub(crate) scene_setup: Duration,
    pub(crate) animations: Duration,
    pub(crate) transforms: Duration,
    pub(crate) effects: Duration,
    pub(crate) overlays: Duration,
    pub(crate) total: Duration,
}

#[derive(Debug, Clone, Copy, Default)]
struct FrameWork {
    fixed_runs: u32,
    sim_ticks: u32,
    fixed_wall: Duration,
    sim_step: Duration,
}

#[derive(Debug, Clone, Copy, Default)]
struct MainScheduleTimings {
    first: Duration,
    pre_update: Duration,
    fixed_loop: Duration,
    update: Duration,
    spawn_scene: Duration,
    post_update: Duration,
    last: Duration,
}

impl MainScheduleTimings {
    fn total(self) -> Duration {
        self.first
            + self.pre_update
            + self.fixed_loop
            + self.update
            + self.spawn_scene
            + self.post_update
            + self.last
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct CompletedMainFrame {
    main_cpu: Duration,
    work: FrameWork,
    schedules: MainScheduleTimings,
}

#[derive(Debug, Clone, Copy, Default)]
struct FrameSample {
    wall: Duration,
    main_cpu: Duration,
    fixed_runs: f64,
    sim_ticks: f64,
    fixed_wall: Duration,
    sim_step: Duration,
    schedules: MainScheduleTimings,
}

#[derive(Debug, Clone, Copy, Default)]
struct CollisionFallbackSample {
    searches: usize,
    candidate_checks: usize,
    max_ring: u32,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct PerformanceCounters {
    sim_tick: Option<u64>,
    sim_samples: VecDeque<(Instant, TickTimings)>,
    collision_fallback_samples: VecDeque<(Instant, CollisionFallbackSample)>,
    presentation: PresentationTimings,
    presentation_samples: VecDeque<(Instant, PresentationTimings)>,
    presentation_started: Option<Instant>,
    step_started: Option<Instant>,
    main_frame_started: Option<Instant>,
    main_phase_started: Option<Instant>,
    completed_main_frame: Option<CompletedMainFrame>,
    current_frame_work: FrameWork,
    current_schedule_timings: MainScheduleTimings,
    frame_samples: VecDeque<(Instant, FrameSample)>,
}

impl PerformanceCounters {
    pub(crate) fn record_sim_tick(
        &mut self,
        tick: u64,
        timings: TickTimings,
        collision_fallback_searches: usize,
        collision_fallback_candidate_checks: usize,
        collision_fallback_max_ring: u32,
    ) {
        let now = Instant::now();
        self.sim_tick = Some(tick);
        push_recent_sample(&mut self.sim_samples, now, timings);
        push_recent_sample(
            &mut self.collision_fallback_samples,
            now,
            CollisionFallbackSample {
                searches: collision_fallback_searches,
                candidate_checks: collision_fallback_candidate_checks,
                max_ring: collision_fallback_max_ring,
            },
        );
        self.current_frame_work.sim_ticks = self.current_frame_work.sim_ticks.saturating_add(1);
        self.current_frame_work.sim_step += timings.total;
    }

    pub(crate) fn record_fixed_update_wall(&mut self, elapsed: Duration) {
        self.current_frame_work.fixed_runs = self.current_frame_work.fixed_runs.saturating_add(1);
        self.current_frame_work.fixed_wall += elapsed;
    }

    fn begin_main_frame(&mut self) {
        let now = Instant::now();
        if let (Some(previous_start), Some(completed)) = (
            self.main_frame_started.replace(now),
            self.completed_main_frame.take(),
        ) {
            push_recent_sample(
                &mut self.frame_samples,
                now,
                FrameSample {
                    wall: now.duration_since(previous_start),
                    main_cpu: completed.main_cpu,
                    fixed_runs: f64::from(completed.work.fixed_runs),
                    sim_ticks: f64::from(completed.work.sim_ticks),
                    fixed_wall: completed.work.fixed_wall,
                    sim_step: completed.work.sim_step,
                    schedules: completed.schedules,
                },
            );
        }
        self.current_frame_work = FrameWork::default();
        self.current_schedule_timings = MainScheduleTimings::default();
        self.main_phase_started = Some(now);
    }

    fn finish_main_schedule_phase(&mut self) -> Duration {
        let now = Instant::now();
        self.main_phase_started
            .replace(now)
            .map_or(Duration::ZERO, |started| now.duration_since(started))
    }

    fn finish_main_frame(&mut self) {
        let Some(started) = self.main_frame_started else {
            return;
        };
        self.completed_main_frame = Some(CompletedMainFrame {
            main_cpu: started.elapsed(),
            work: self.current_frame_work,
            schedules: self.current_schedule_timings,
        });
        self.main_phase_started = None;
    }

    fn begin_presentation(&mut self) {
        let now = Instant::now();
        self.presentation = PresentationTimings::default();
        self.presentation_started = Some(now);
        self.step_started = Some(now);
    }

    fn finish_step(&mut self) -> Duration {
        let now = Instant::now();
        self.step_started
            .replace(now)
            .map_or(Duration::ZERO, |started| now.duration_since(started))
    }

    fn finish_presentation(&mut self) {
        self.presentation.total = self
            .presentation_started
            .take()
            .map_or(Duration::ZERO, |started| started.elapsed());
        self.step_started = None;
        let now = Instant::now();
        push_recent_sample(&mut self.presentation_samples, now, self.presentation);
    }
}

#[derive(Component)]
struct PerformancePanelText;

#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
struct PerfFrameStart;
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
struct PerfAfterFirst;
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
struct PerfAfterPreUpdate;
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
struct PerfAfterFixedLoop;
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
struct PerfAfterUpdate;
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
struct PerfAfterSpawnScene;
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
struct PerfAfterPostUpdate;
#[derive(ScheduleLabel, Debug, Hash, PartialEq, Eq, Clone)]
struct PerfAfterLast;

pub(crate) struct PerformanceUiPlugin;

impl Plugin for PerformanceUiPlugin {
    fn build(&self, app: &mut App) {
        if !app.is_plugin_added::<RenderDiagnosticsPlugin>() {
            app.add_plugins(RenderDiagnosticsPlugin);
        }

        app.init_resource::<PerformanceCounters>()
            .init_schedule(PerfFrameStart)
            .init_schedule(PerfAfterFirst)
            .init_schedule(PerfAfterPreUpdate)
            .init_schedule(PerfAfterFixedLoop)
            .init_schedule(PerfAfterUpdate)
            .init_schedule(PerfAfterSpawnScene)
            .init_schedule(PerfAfterPostUpdate)
            .init_schedule(PerfAfterLast)
            .add_systems(Startup, setup_performance_panel)
            .add_systems(PerfFrameStart, begin_main_frame_profile)
            .add_systems(PerfAfterFirst, finish_first_profile)
            .add_systems(PerfAfterPreUpdate, finish_pre_update_profile)
            .add_systems(PerfAfterFixedLoop, finish_fixed_loop_profile)
            .add_systems(PerfAfterUpdate, finish_update_profile)
            .add_systems(PerfAfterSpawnScene, finish_spawn_scene_profile)
            .add_systems(PerfAfterPostUpdate, finish_post_update_profile)
            .add_systems(PerfAfterLast, finish_main_frame_profile)
            .add_systems(
                Update,
                update_performance_panel.after(finish_presentation_profile),
            );

        let mut order = app.world_mut().resource_mut::<MainScheduleOrder>();
        order.insert_before(First, PerfFrameStart);
        order.insert_after(First, PerfAfterFirst);
        order.insert_after(PreUpdate, PerfAfterPreUpdate);
        order.insert_after(RunFixedMainLoop, PerfAfterFixedLoop);
        order.insert_after(Update, PerfAfterUpdate);
        order.insert_after(SpawnScene, PerfAfterSpawnScene);
        order.insert_after(PostUpdate, PerfAfterPostUpdate);
        order.insert_after(Last, PerfAfterLast);
    }
}

fn setup_performance_panel(mut commands: Commands) {
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                left: px(PANEL_LEFT),
                top: px(PANEL_TOP),
                width: px(PANEL_WIDTH),
                padding: UiRect::all(px(PANEL_PADDING)),
                border: UiRect::all(px(1.0)),
                border_radius: BorderRadius::all(px(6.0)),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(PANEL_BACKGROUND),
            BorderColor::all(PANEL_BORDER),
            ZIndex(940),
            Pickable::IGNORE,
        ))
        .with_children(|panel| {
            panel.spawn((
                Text::new("PERFORMANCE"),
                TextFont::from_font_size(13.0),
                TextColor(HEADING_COLOR),
            ));
            panel.spawn((
                Text::new("collecting timings..."),
                TextFont::from_font_size(10.0),
                TextColor(FRAME_COLOR),
                Node {
                    width: percent(100.0),
                    margin: UiRect::top(px(4.0)),
                    ..default()
                },
                PerformancePanelText,
            ));
        });
}

type MeshVisibilityDiagnostics<'w, 's> = Query<
    'w,
    's,
    (
        &'static ViewVisibility,
        Has<NoFrustumCulling>,
        Has<SkinnedMesh>,
        Has<DynamicSkinnedMeshBounds>,
    ),
    With<Mesh3d>,
>;

fn update_performance_panel(
    counters: Res<PerformanceCounters>,
    presentation: Res<PresentationSamples>,
    diagnostics: Res<DiagnosticsStore>,
    fixed_time: Res<Time<Fixed>>,
    mesh_visibility: MeshVisibilityDiagnostics<'_, '_>,
    animation_players: Query<(), With<AnimationPlayer>>,
    mut text: Single<&mut Text, With<PerformancePanelText>>,
) {
    let now = Instant::now();
    let frame = average_frame_samples(&counters.frame_samples, now);
    let sim = average_sim_samples(&counters.sim_samples, now);
    let collision_fallback =
        summarize_collision_fallback_samples(&counters.collision_fallback_samples, now);
    let render = average_presentation_samples(&counters.presentation_samples, now);
    let gpu_passes = gpu_pass_timings(&diagnostics);
    let (mesh_count, visible_meshes, no_cull_meshes, skinned_meshes, dynamic_skinned_bounds) =
        mesh_visibility.iter().fold(
            (0usize, 0usize, 0usize, 0usize, 0usize),
            |(total, visible, no_cull, skinned, dynamic_bounds),
             (
                view_visibility,
                no_frustum_culling,
                skinned_mesh,
                dynamic_skinned_bounds,
            )| {
                (
                    total + 1,
                    visible + usize::from(view_visibility.get()),
                    no_cull + usize::from(no_frustum_culling),
                    skinned + usize::from(skinned_mesh),
                    dynamic_bounds + usize::from(dynamic_skinned_bounds),
                )
            },
        );
    let effective_sim_hz = 1.0 / fixed_time.timestep().as_secs_f64();
    let sim_speed = effective_sim_hz / f64::from(CASTLE_FIGHT_SIMULATION_HZ);

    let output = &mut text.0;
    output.clear();
    if output.capacity() < 1400 {
        output.reserve(1400 - output.capacity());
    }

    if let Some(frame) = frame {
        let fps = if frame.wall.is_zero() {
            0.0
        } else {
            1.0 / frame.wall.as_secs_f64()
        };
        writeln!(
            output,
            "FRAME  1s avg {:>7.2}ms   FPS {:>5.1}",
            duration_ms(frame.wall),
            fps
        )
        .unwrap();
        push_timing(output, "main CPU", frame.main_cpu);
        push_timing(
            output,
            "outside main",
            frame.wall.saturating_sub(frame.main_cpu),
        );
        writeln!(
            output,
            "  fixed/frame       {:>4.1}x {:>7.3}ms",
            frame.fixed_runs,
            duration_ms(frame.fixed_wall)
        )
        .unwrap();
        writeln!(
            output,
            "  sim ticks/frame   {:>4.1}x {:>7.3}ms",
            frame.sim_ticks,
            duration_ms(frame.sim_step)
        )
        .unwrap();
        writeln!(
            output,
            "  sim rate          {:>4.2}x {:>7.1}Hz",
            sim_speed, effective_sim_hz
        )
        .unwrap();
        if let Some(render) = render {
            push_timing(
                output,
                "other main",
                frame
                    .main_cpu
                    .saturating_sub(frame.fixed_wall.saturating_add(render.total)),
            );
        }
        output.push_str("  main schedules\n");
        push_timing(output, "    First", frame.schedules.first);
        push_timing(output, "    PreUpdate", frame.schedules.pre_update);
        push_timing(output, "    fixed loop", frame.schedules.fixed_loop);
        push_timing(output, "    Update", frame.schedules.update);
        push_timing(output, "    SpawnScene", frame.schedules.spawn_scene);
        push_timing(output, "    PostUpdate", frame.schedules.post_update);
        push_timing(output, "    Last", frame.schedules.last);
        push_timing(
            output,
            "    unaccounted",
            frame.main_cpu.saturating_sub(frame.schedules.total()),
        );
    } else {
        output.push_str("FRAME  collecting 1s average...\n");
    }

    writeln!(
        output,
        "\nENTITIES  units {}   buildings {}",
        presentation.current.units.len(),
        presentation.current.buildings.len()
    )
    .unwrap();
    writeln!(
        output,
        "          builders {} corpses {} projectiles {}",
        presentation.current.builders.len(),
        presentation.current.corpses.len(),
        presentation.current.projectiles.len()
    )
    .unwrap();

    if let (Some(tick), Some(sim)) = (counters.sim_tick, sim) {
        writeln!(
            output,
            "\nSIM  tick {tick}  1s avg/tick {:>7.3}ms",
            duration_ms(sim.total)
        )
        .unwrap();
        push_timing(output, "topology", sim.topology);
        push_timing(output, "timers/build", sim.timers);
        push_timing(output, "production", sim.production);
        push_timing(output, "snapshot/spatial", sim.snapshot_and_spatial);
        push_timing(output, "abilities", sim.abilities);
        push_timing(output, "targeting", sim.targeting);
        push_timing(output, "combat", sim.combat);
        push_timing(output, "move intent", sim.movement_intent);
        push_timing(output, "crowd/collision", sim.crowd_and_collision);
        push_timing(output, "crowd separate", sim.crowd_separation);
        push_timing(output, "hard collision", sim.hard_collision);
        push_timing(output, "fallback search", sim.collision_fallback_search);
        let (fallback_searches, fallback_checks, fallback_max_ring) =
            collision_fallback.unwrap_or_default();
        writeln!(
            output,
            "  fallback calls      {:>7.2}/tick",
            fallback_searches
        )
        .unwrap();
        writeln!(
            output,
            "  fallback checks     {:>7.1}/tick",
            fallback_checks
        )
        .unwrap();
        writeln!(output, "  fallback ring max   {:>7}", fallback_max_ring).unwrap();
        push_timing(output, "ballistic", sim.ballistic_impact);
        push_timing(output, "commit", sim.structural_commit);
        push_timing(output, "checksum", sim.checksum);
    } else {
        output.push_str("\nSIM  no completed tick in last 1s\n");
    }

    if let Some(render) = render {
        writeln!(
            output,
            "\n3D CPU  1s avg/frame {:>7.3}ms",
            duration_ms(render.total)
        )
        .unwrap();
        push_timing(output, "camera", render.camera);
        push_timing(output, "model prep", render.model_prep);
        push_timing(output, "entity sync", render.entity_sync);
        push_timing(output, "scene setup", render.scene_setup);
        push_timing(output, "animations", render.animations);
        push_timing(output, "transforms", render.transforms);
        push_timing(output, "effects", render.effects);
        push_timing(output, "overlays/gizmos", render.overlays);
    } else {
        output.push_str("\n3D CPU  collecting 1s average...\n");
    }

    writeln!(
        output,
        "\nRENDER  meshes {:>5}/{:<5} visible  no-cull {:>4}",
        visible_meshes, mesh_count, no_cull_meshes
    )
    .unwrap();
    writeln!(
        output,
        "        skinned {:>5}  dynamic {:>5}  anim players {:>5}",
        skinned_meshes,
        dynamic_skinned_bounds,
        animation_players.iter().count()
    )
    .unwrap();
    if gpu_passes.is_empty() {
        output.push_str("GPU PASS  collecting render diagnostics...\n");
    } else {
        output.push_str("GPU PASS  recent avg\n");
        for (name, milliseconds) in gpu_passes {
            writeln!(output, "  {name:<24} {milliseconds:>7.3}ms").unwrap();
        }
    }
}

fn gpu_pass_timings(diagnostics: &DiagnosticsStore) -> Vec<(&str, f64)> {
    const MAX_GPU_PASSES: usize = 8;

    let mut passes = diagnostics
        .iter()
        .filter_map(|diagnostic| {
            let name = diagnostic
                .path()
                .as_str()
                .strip_prefix("render/")?
                .strip_suffix("/elapsed_gpu")?;
            let milliseconds = diagnostic.average()?;
            milliseconds.is_finite().then_some((name, milliseconds))
        })
        .collect::<Vec<_>>();
    passes.sort_by(|left, right| {
        right
            .1
            .partial_cmp(&left.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.0.cmp(right.0))
    });
    passes.truncate(MAX_GPU_PASSES);
    passes
}

fn push_timing(output: &mut String, label: &str, duration: Duration) {
    writeln!(output, "  {label:<18} {:>7.3}ms", duration_ms(duration)).unwrap();
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn push_recent_sample<T>(samples: &mut VecDeque<(Instant, T)>, now: Instant, sample: T) {
    samples.push_back((now, sample));
    while samples
        .front()
        .is_some_and(|(recorded, _)| now.duration_since(*recorded) > SMOOTHING_WINDOW)
    {
        samples.pop_front();
    }
}

fn recent_sample_count<T>(samples: &VecDeque<(Instant, T)>, now: Instant) -> usize {
    samples
        .iter()
        .filter(|(recorded, _)| now.duration_since(*recorded) <= SMOOTHING_WINDOW)
        .count()
}

fn summarize_collision_fallback_samples(
    samples: &VecDeque<(Instant, CollisionFallbackSample)>,
    now: Instant,
) -> Option<(f64, f64, u32)> {
    let mut count = 0usize;
    let mut searches = 0usize;
    let mut candidate_checks = 0usize;
    let mut max_ring = 0u32;
    for (_, sample) in samples
        .iter()
        .filter(|(recorded, _)| now.duration_since(*recorded) <= SMOOTHING_WINDOW)
    {
        count += 1;
        searches = searches.saturating_add(sample.searches);
        candidate_checks = candidate_checks.saturating_add(sample.candidate_checks);
        max_ring = max_ring.max(sample.max_ring);
    }
    if count == 0 {
        return None;
    }
    let divisor = count as f64;
    Some((
        searches as f64 / divisor,
        candidate_checks as f64 / divisor,
        max_ring,
    ))
}

fn average_sim_samples(
    samples: &VecDeque<(Instant, TickTimings)>,
    now: Instant,
) -> Option<TickTimings> {
    let count = recent_sample_count(samples, now);
    if count == 0 {
        return None;
    }

    let mut total = TickTimings::default();
    for (_, sample) in samples
        .iter()
        .filter(|(recorded, _)| now.duration_since(*recorded) <= SMOOTHING_WINDOW)
    {
        total.topology += sample.topology;
        total.timers += sample.timers;
        total.production += sample.production;
        total.snapshot_and_spatial += sample.snapshot_and_spatial;
        total.abilities += sample.abilities;
        total.targeting += sample.targeting;
        total.combat += sample.combat;
        total.movement_intent += sample.movement_intent;
        total.crowd_separation += sample.crowd_separation;
        total.hard_collision += sample.hard_collision;
        total.collision_fallback_search += sample.collision_fallback_search;
        total.crowd_and_collision += sample.crowd_and_collision;
        total.ballistic_impact += sample.ballistic_impact;
        total.structural_commit += sample.structural_commit;
        total.checksum += sample.checksum;
        total.total += sample.total;
    }

    let divisor = u32::try_from(count).expect("one-second timing sample count fits u32");
    Some(TickTimings {
        topology: total.topology / divisor,
        timers: total.timers / divisor,
        production: total.production / divisor,
        snapshot_and_spatial: total.snapshot_and_spatial / divisor,
        abilities: total.abilities / divisor,
        targeting: total.targeting / divisor,
        combat: total.combat / divisor,
        movement_intent: total.movement_intent / divisor,
        crowd_separation: total.crowd_separation / divisor,
        hard_collision: total.hard_collision / divisor,
        collision_fallback_search: total.collision_fallback_search / divisor,
        crowd_and_collision: total.crowd_and_collision / divisor,
        ballistic_impact: total.ballistic_impact / divisor,
        structural_commit: total.structural_commit / divisor,
        checksum: total.checksum / divisor,
        total: total.total / divisor,
    })
}

fn average_presentation_samples(
    samples: &VecDeque<(Instant, PresentationTimings)>,
    now: Instant,
) -> Option<PresentationTimings> {
    let count = recent_sample_count(samples, now);
    if count == 0 {
        return None;
    }

    let mut total = PresentationTimings::default();
    for (_, sample) in samples
        .iter()
        .filter(|(recorded, _)| now.duration_since(*recorded) <= SMOOTHING_WINDOW)
    {
        total.camera += sample.camera;
        total.model_prep += sample.model_prep;
        total.entity_sync += sample.entity_sync;
        total.scene_setup += sample.scene_setup;
        total.animations += sample.animations;
        total.transforms += sample.transforms;
        total.effects += sample.effects;
        total.overlays += sample.overlays;
        total.total += sample.total;
    }

    let divisor = u32::try_from(count).expect("one-second timing sample count fits u32");
    Some(PresentationTimings {
        camera: total.camera / divisor,
        model_prep: total.model_prep / divisor,
        entity_sync: total.entity_sync / divisor,
        scene_setup: total.scene_setup / divisor,
        animations: total.animations / divisor,
        transforms: total.transforms / divisor,
        effects: total.effects / divisor,
        overlays: total.overlays / divisor,
        total: total.total / divisor,
    })
}

fn average_frame_samples(
    samples: &VecDeque<(Instant, FrameSample)>,
    now: Instant,
) -> Option<FrameSample> {
    let count = recent_sample_count(samples, now);
    if count == 0 {
        return None;
    }

    let mut total = FrameSample::default();
    for (_, sample) in samples
        .iter()
        .filter(|(recorded, _)| now.duration_since(*recorded) <= SMOOTHING_WINDOW)
    {
        total.wall += sample.wall;
        total.main_cpu += sample.main_cpu;
        total.fixed_runs += sample.fixed_runs;
        total.sim_ticks += sample.sim_ticks;
        total.fixed_wall += sample.fixed_wall;
        total.sim_step += sample.sim_step;
        total.schedules.first += sample.schedules.first;
        total.schedules.pre_update += sample.schedules.pre_update;
        total.schedules.fixed_loop += sample.schedules.fixed_loop;
        total.schedules.update += sample.schedules.update;
        total.schedules.spawn_scene += sample.schedules.spawn_scene;
        total.schedules.post_update += sample.schedules.post_update;
        total.schedules.last += sample.schedules.last;
    }

    let divisor = u32::try_from(count).expect("one-second timing sample count fits u32");
    let scalar = f64::from(divisor);
    Some(FrameSample {
        wall: total.wall / divisor,
        main_cpu: total.main_cpu / divisor,
        fixed_runs: total.fixed_runs / scalar,
        sim_ticks: total.sim_ticks / scalar,
        fixed_wall: total.fixed_wall / divisor,
        sim_step: total.sim_step / divisor,
        schedules: MainScheduleTimings {
            first: total.schedules.first / divisor,
            pre_update: total.schedules.pre_update / divisor,
            fixed_loop: total.schedules.fixed_loop / divisor,
            update: total.schedules.update / divisor,
            spawn_scene: total.schedules.spawn_scene / divisor,
            post_update: total.schedules.post_update / divisor,
            last: total.schedules.last / divisor,
        },
    })
}

fn begin_main_frame_profile(mut counters: ResMut<PerformanceCounters>) {
    counters.begin_main_frame();
}

fn finish_first_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_main_schedule_phase();
    counters.current_schedule_timings.first = elapsed;
}

fn finish_pre_update_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_main_schedule_phase();
    counters.current_schedule_timings.pre_update = elapsed;
}

fn finish_fixed_loop_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_main_schedule_phase();
    counters.current_schedule_timings.fixed_loop = elapsed;
}

fn finish_update_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_main_schedule_phase();
    counters.current_schedule_timings.update = elapsed;
}

fn finish_spawn_scene_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_main_schedule_phase();
    counters.current_schedule_timings.spawn_scene = elapsed;
}

fn finish_post_update_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_main_schedule_phase();
    counters.current_schedule_timings.post_update = elapsed;
}

fn finish_main_frame_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_main_schedule_phase();
    counters.current_schedule_timings.last = elapsed;
    counters.finish_main_frame();
}

pub(crate) fn begin_presentation_profile(mut counters: ResMut<PerformanceCounters>) {
    counters.begin_presentation();
}

pub(crate) fn finish_camera_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_step();
    counters.presentation.camera = elapsed;
}

pub(crate) fn finish_model_prep_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_step();
    counters.presentation.model_prep = elapsed;
}

pub(crate) fn finish_entity_sync_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_step();
    counters.presentation.entity_sync = elapsed;
}

pub(crate) fn finish_scene_setup_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_step();
    counters.presentation.scene_setup = elapsed;
}

pub(crate) fn finish_animation_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_step();
    counters.presentation.animations = elapsed;
}

pub(crate) fn finish_transform_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_step();
    counters.presentation.transforms = elapsed;
}

pub(crate) fn finish_effects_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_step();
    counters.presentation.effects = elapsed;
}

pub(crate) fn finish_presentation_profile(mut counters: ResMut<PerformanceCounters>) {
    let elapsed = counters.finish_step();
    counters.presentation.overlays = elapsed;
    counters.finish_presentation();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn duration_format_keeps_sub_millisecond_resolution() {
        assert_eq!(duration_ms(Duration::from_micros(125)), 0.125);
    }

    #[test]
    fn one_second_average_excludes_older_samples() {
        let now = Instant::now();
        let mut samples = VecDeque::new();
        samples.push_back((
            now - Duration::from_millis(1_100),
            TickTimings {
                total: Duration::from_millis(90),
                ..default()
            },
        ));
        samples.push_back((
            now - Duration::from_millis(500),
            TickTimings {
                total: Duration::from_millis(10),
                ..default()
            },
        ));
        samples.push_back((
            now - Duration::from_millis(100),
            TickTimings {
                total: Duration::from_millis(30),
                ..default()
            },
        ));

        let average = average_sim_samples(&samples, now).unwrap();
        assert_eq!(average.total, Duration::from_millis(20));
    }

    #[test]
    fn collision_fallback_summary_averages_work_and_keeps_peak_ring() {
        let now = Instant::now();
        let mut samples = VecDeque::new();
        samples.push_back((
            now - Duration::from_millis(400),
            CollisionFallbackSample {
                searches: 2,
                candidate_checks: 120,
                max_ring: 7,
            },
        ));
        samples.push_back((
            now - Duration::from_millis(100),
            CollisionFallbackSample {
                searches: 4,
                candidate_checks: 280,
                max_ring: 31,
            },
        ));

        let summary = summarize_collision_fallback_samples(&samples, now).unwrap();
        assert_eq!(summary, (3.0, 200.0, 31));
    }

    #[test]
    fn gpu_pass_summary_filters_and_orders_render_gpu_timings() {
        use bevy::diagnostic::{Diagnostic, DiagnosticMeasurement, DiagnosticPath};

        let mut diagnostics = DiagnosticsStore::default();
        for (path, value) in [
            ("render/main_opaque_pass_3d/elapsed_gpu", 8.0),
            ("render/ui/elapsed_gpu", 2.0),
            ("render/main_opaque_pass_3d/elapsed_cpu", 20.0),
            ("other/not_render_gpu", 99.0),
        ] {
            let mut diagnostic = Diagnostic::new(DiagnosticPath::new(path.to_owned()));
            diagnostic.add_measurement(DiagnosticMeasurement {
                time: Instant::now(),
                value,
            });
            diagnostics.add(diagnostic);
        }

        assert_eq!(
            gpu_pass_timings(&diagnostics),
            vec![("main_opaque_pass_3d", 8.0), ("ui", 2.0)]
        );
    }

    #[test]
    fn frame_average_preserves_sim_work_per_rendered_frame() {
        let now = Instant::now();
        let mut samples = VecDeque::new();
        samples.push_back((
            now,
            FrameSample {
                wall: Duration::from_millis(16),
                main_cpu: Duration::from_millis(8),
                fixed_runs: 1.0,
                sim_ticks: 1.0,
                fixed_wall: Duration::from_millis(6),
                sim_step: Duration::from_millis(5),
                schedules: MainScheduleTimings {
                    update: Duration::from_millis(4),
                    post_update: Duration::from_millis(2),
                    ..default()
                },
            },
        ));
        samples.push_back((
            now,
            FrameSample {
                wall: Duration::from_millis(20),
                main_cpu: Duration::from_millis(12),
                fixed_runs: 3.0,
                sim_ticks: 3.0,
                fixed_wall: Duration::from_millis(10),
                sim_step: Duration::from_millis(8),
                schedules: MainScheduleTimings {
                    update: Duration::from_millis(8),
                    post_update: Duration::from_millis(4),
                    ..default()
                },
            },
        ));

        let average = average_frame_samples(&samples, now).unwrap();
        assert_eq!(average.wall, Duration::from_millis(18));
        assert_eq!(average.main_cpu, Duration::from_millis(10));
        assert_eq!(average.fixed_runs, 2.0);
        assert_eq!(average.sim_ticks, 2.0);
        assert_eq!(average.fixed_wall, Duration::from_millis(8));
        assert_eq!(average.sim_step, Duration::from_micros(6_500));
        assert_eq!(average.schedules.update, Duration::from_millis(6));
        assert_eq!(average.schedules.post_update, Duration::from_millis(3));
    }
}
