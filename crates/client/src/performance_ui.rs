use std::{
    fmt::Write as _,
    time::{Duration, Instant},
};

use bevy::{
    diagnostic::{DiagnosticsStore, FrameTimeDiagnosticsPlugin},
    prelude::*,
};
use castle_fight_sim::TickTimings;

use crate::resource_ui::TOP_BAR_HEIGHT;

const PANEL_LEFT: f32 = 72.0;
const PANEL_TOP: f32 = TOP_BAR_HEIGHT + 10.0;
const PANEL_WIDTH: f32 = 294.0;
const PANEL_PADDING: f32 = 10.0;
const PANEL_BACKGROUND: Color = Color::srgba(0.025, 0.030, 0.040, 0.94);
const PANEL_BORDER: Color = Color::srgba(0.28, 0.32, 0.38, 0.96);
const HEADING_COLOR: Color = Color::srgb(0.92, 0.94, 0.98);
const FRAME_COLOR: Color = Color::srgb(0.76, 0.80, 0.86);

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

#[derive(Resource, Debug, Default)]
pub(crate) struct PerformanceCounters {
    sim_tick: Option<u64>,
    sim: Option<TickTimings>,
    presentation: PresentationTimings,
    presentation_started: Option<Instant>,
    step_started: Option<Instant>,
}

impl PerformanceCounters {
    pub(crate) fn record_sim_tick(&mut self, tick: u64, timings: TickTimings) {
        self.sim_tick = Some(tick);
        self.sim = Some(timings);
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
    }
}

#[derive(Component)]
struct PerformancePanelText;

pub(crate) struct PerformanceUiPlugin;

impl Plugin for PerformanceUiPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PerformanceCounters>()
            .add_systems(Startup, setup_performance_panel)
            .add_systems(
                Update,
                update_performance_panel.after(finish_presentation_profile),
            );
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
    diagnostics: Res<DiagnosticsStore>,
    mut text: Single<&mut Text, With<PerformancePanelText>>,
) {
    let fps = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FPS)
        .and_then(|diagnostic| diagnostic.smoothed());
    let frame_ms = diagnostics
        .get(&FrameTimeDiagnosticsPlugin::FRAME_TIME)
        .and_then(|diagnostic| diagnostic.smoothed());

    let output = &mut text.0;
    output.clear();
    if output.capacity() < 1024 {
        output.reserve(1024 - output.capacity());
    }

    match (frame_ms, fps) {
        (Some(frame_ms), Some(fps)) => {
            writeln!(output, "FRAME  {frame_ms:>6.2}ms   FPS {fps:>5.1}\n").unwrap();
        }
        (Some(frame_ms), None) => {
            writeln!(output, "FRAME  {frame_ms:>6.2}ms   FPS    --\n").unwrap();
        }
        (None, Some(fps)) => {
            writeln!(output, "FRAME       --   FPS {fps:>5.1}\n").unwrap();
        }
        (None, None) => output.push_str("FRAME       --   FPS    --\n\n"),
    }

    if let (Some(tick), Some(sim)) = (counters.sim_tick, counters.sim) {
        writeln!(
            output,
            "SIM  tick {tick}   {:>6.3}ms",
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
        output.push_str("SIM  no completed tick yet\n");
    }

    let render = counters.presentation;
    writeln!(
        output,
        "\n3D CPU              {:>6.3}ms",
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
}

fn push_timing(output: &mut String, label: &str, duration: Duration) {
    writeln!(output, "  {label:<18} {:>6.3}ms", duration_ms(duration)).unwrap();
}

fn duration_ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
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
}
