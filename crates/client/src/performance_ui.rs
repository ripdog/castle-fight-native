use std::{
    collections::VecDeque,
    fmt::Write as _,
    time::{Duration, Instant},
};

use bevy::prelude::*;
use castle_fight_sim::TickTimings;

use crate::{bridge::PresentationSamples, resource_ui::TOP_BAR_HEIGHT};

const PANEL_LEFT: f32 = 72.0;
const PANEL_TOP: f32 = TOP_BAR_HEIGHT + 10.0;
const PANEL_WIDTH: f32 = 314.0;
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
struct CompletedMainFrame {
    main_cpu: Duration,
    work: FrameWork,
}

#[derive(Debug, Clone, Copy, Default)]
struct FrameSample {
    wall: Duration,
    main_cpu: Duration,
    fixed_runs: f64,
    sim_ticks: f64,
    fixed_wall: Duration,
    sim_step: Duration,
}

#[derive(Resource, Debug, Default)]
pub(crate) struct PerformanceCounters {
    sim_tick: Option<u64>,
    sim_samples: VecDeque<(Instant, TickTimings)>,
    presentation: PresentationTimings,
    presentation_samples: VecDeque<(Instant, PresentationTimings)>,
    presentation_started: Option<Instant>,
    step_started: Option<Instant>,
    main_frame_started: Option<Instant>,
    completed_main_frame: Option<CompletedMainFrame>,
    current_frame_work: FrameWork,
    frame_samples: VecDeque<(Instant, FrameSample)>,
}

impl PerformanceCounters {
    pub(crate) fn record_sim_tick(&mut self, tick: u64, timings: TickTimings) {
        let now = Instant::now();
        self.sim_tick = Some(tick);
        push_recent_sample(&mut self.sim_samples, now, timings);
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
                },
            );
        }
        self.current_frame_work = FrameWork::default();
    }

    fn finish_main_frame(&mut self) {
        let Some(started) = self.main_frame_started else {
            return;
        };
        self.completed_main_frame = Some(CompletedMainFrame {
            main_cpu: started.elapsed(),
            work: self.current_frame_work,
        });
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

pub(crate) struct PerformanceUiPlugin;

impl Plugin for PerformanceUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PerformanceCounters>()
            .add_systems(Startup, setup_performance_panel)
            .add_systems(First, begin_main_frame_profile)
            .add_systems(
                Update,
                update_performance_panel.after(finish_presentation_profile),
            )
            .add_systems(Last, finish_main_frame_profile);
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

fn update_performance_panel(
    counters: Res<PerformanceCounters>,
    presentation: Res<PresentationSamples>,
    mut text: Single<&mut Text, With<PerformancePanelText>>,
) {
    let now = Instant::now();
    let frame = average_frame_samples(&counters.frame_samples, now);
    let sim = average_sim_samples(&counters.sim_samples, now);
    let render = average_presentation_samples(&counters.presentation_samples, now);

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
    })
}

pub(crate) fn begin_main_frame_profile(mut counters: ResMut<PerformanceCounters>) {
    counters.begin_main_frame();
}

pub(crate) fn finish_main_frame_profile(mut counters: ResMut<PerformanceCounters>) {
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
            },
        ));

        let average = average_frame_samples(&samples, now).unwrap();
        assert_eq!(average.wall, Duration::from_millis(18));
        assert_eq!(average.main_cpu, Duration::from_millis(10));
        assert_eq!(average.fixed_runs, 2.0);
        assert_eq!(average.sim_ticks, 2.0);
        assert_eq!(average.fixed_wall, Duration::from_millis(8));
        assert_eq!(average.sim_step, Duration::from_micros(6_500));
    }
}
