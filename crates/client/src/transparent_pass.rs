//! Main 3D transparent pass recorded into several command encoders.
//!
//! wgpu defers render-pass encoding (state tracking, bind-group resource
//! merging and the driver's command recording) to `CommandEncoder::finish`, so
//! a single transparent pass with thousands of sorted draws finishes on one
//! render-thread core. This replaces Bevy's `main_transparent_pass_3d` with
//! consecutive sub-passes over contiguous ranges of the same sorted phase.
//! Each sub-pass after the first records and finishes on the compute pool with
//! its own draw-function set (see `vendor/README.md`), concurrently with the
//! first; without those sets they record in order here and finish on the pool.
//! Command buffers are submitted in range order, so draw order and blending are
//! unchanged; only the final sub-pass resolves multisampled colour.

use std::ops::Range;
use std::sync::{Mutex, PoisonError};

use bevy::{
    camera::{MainPassResolutionOverride, Viewport},
    core_pipeline::{
        Core3d, Core3dSystems,
        core_3d::{Transparent3d, main_opaque_pass_3d, main_transparent_pass_3d},
        oit::{OrderIndependentTransparencySettings, resolve::OitResolvePipelineId},
    },
    ecs::schedule::ScheduleCleanupPolicy,
    pbr::main_transmissive_pass_3d,
    prelude::*,
    render::{
        Extract, ExtractSchedule, RenderApp, RenderStartup,
        camera::ExtractedCamera,
        diagnostic::RecordDiagnostics,
        render_phase::{
            DrawFunctions, DrawFunctionsInternal, PhaseItem, TrackedRenderPass,
            ViewSortedRenderPhases,
        },
        render_resource::{CommandEncoderDescriptor, PipelineCache, RenderPassDescriptor, StoreOp},
        renderer::{RenderContext, ViewQuery},
        view::{ExtractedView, ViewDepthTexture, ViewTarget},
    },
    tasks::ComputeTaskPool,
};

use crate::render_audit::RenderExperiment;

/// Upper bound on sub-passes; each one rebinds its first draw's state.
const MAX_TRANSPARENT_CHUNKS: usize = 10;
/// Draws below this per sub-pass do not repay a pass boundary.
const MIN_DRAWS_PER_CHUNK: usize = 384;

pub(crate) struct ChunkedTransparentPassPlugin;

impl Plugin for ChunkedTransparentPassPlugin {
    fn build(&self, app: &mut App) {
        let Some(render_app) = app.get_sub_app_mut(RenderApp) else {
            return;
        };
        render_app
            .init_resource::<TransparentPassChunking>()
            .init_resource::<TransparentDrawLanes>()
            .add_systems(RenderStartup, build_transparent_draw_lanes)
            .add_systems(ExtractSchedule, extract_transparent_pass_chunking)
            .add_systems(
                Core3d,
                chunked_main_transparent_pass_3d
                    // Bevy's pass is removed below; its now-empty system set
                    // still carries the sky/transmission/OIT ordering edges.
                    .after(main_transparent_pass_3d)
                    .after(main_opaque_pass_3d)
                    .after(main_transmissive_pass_3d)
                    .before(bevy::core_pipeline::oit::resolve::node::oit_resolve)
                    .in_set(Core3dSystems::MainPass),
            );
        let removed = render_app
            .remove_systems_in_set(
                Core3d,
                main_transparent_pass_3d,
                ScheduleCleanupPolicy::RemoveSystemsOnly,
            )
            .expect("Core3d schedule must contain Bevy's main transparent pass");
        assert_eq!(removed, 1, "expected exactly one main_transparent_pass_3d");
    }
}

/// Independent draw-function sets, one per sub-pass after the first, so those sub-passes can
/// record concurrently (draw functions keep per-call ECS state and record one pass at a time).
#[derive(Resource, Default)]
struct TransparentDrawLanes {
    lanes: Vec<Mutex<DrawFunctionsInternal<Transparent3d>>>,
    /// Draw functions registered when the lanes were built; a mismatch falls back to serial
    /// recording.
    function_count: usize,
}

fn build_transparent_draw_lanes(world: &mut World) {
    let function_count = world
        .resource::<DrawFunctions<Transparent3d>>()
        .read()
        .draw_functions
        .len();
    let lanes = (1..MAX_TRANSPARENT_CHUNKS)
        .map_while(|_| DrawFunctions::<Transparent3d>::instantiate(world))
        .map(Mutex::new)
        .collect();
    world.insert_resource(TransparentDrawLanes {
        lanes,
        function_count,
    });
}

#[derive(Resource, Clone, Copy)]
struct TransparentPassChunking {
    max_chunks: usize,
}

impl Default for TransparentPassChunking {
    fn default() -> Self {
        Self {
            max_chunks: MAX_TRANSPARENT_CHUNKS,
        }
    }
}

fn extract_transparent_pass_chunking(
    experiment: Extract<Res<RenderExperiment>>,
    mut chunking: ResMut<TransparentPassChunking>,
) {
    chunking.max_chunks = if **experiment == RenderExperiment::LegacyTransparentPass {
        1
    } else {
        MAX_TRANSPARENT_CHUNKS
    };
}

/// Splits a sorted phase, given each item's batch length, into at most
/// `max_chunks` contiguous ranges with similar draw counts. Boundaries fall only
/// where `SortedRenderPhase::render_range` would start a draw, so no multi-item
/// batch is divided.
fn chunk_ranges(
    batch_lens: impl Iterator<Item = usize> + Clone,
    max_chunks: usize,
) -> Vec<Range<usize>> {
    let item_count = batch_lens.clone().count();
    let draws: usize = draw_visits(batch_lens.clone())
        .map(|(_, draws)| draws)
        .sum();
    let chunks = (draws / MIN_DRAWS_PER_CHUNK).clamp(1, max_chunks.max(1));
    if chunks == 1 {
        return std::iter::once(0..item_count).collect();
    }
    let per_chunk = draws.div_ceil(chunks);
    let mut ranges = Vec::with_capacity(chunks);
    let (mut start, mut chunk_draws) = (0, 0);
    for (index, draws) in draw_visits(batch_lens) {
        if chunk_draws == per_chunk && ranges.len() + 1 < chunks {
            ranges.push(start..index);
            (start, chunk_draws) = (index, 0);
        }
        chunk_draws += draws;
    }
    ranges.push(start..item_count);
    ranges
}

/// Yields `(index, draws)` for every item `render_range` visits: an empty batch
/// is stepped over, a non-empty one is drawn and skips its covered items.
fn draw_visits(batch_lens: impl Iterator<Item = usize>) -> impl Iterator<Item = (usize, usize)> {
    let mut next = 0;
    batch_lens.enumerate().filter_map(move |(index, len)| {
        if index != next {
            return None;
        }
        next = index + len.max(1);
        Some((index, usize::from(len > 0)))
    })
}

type TransparentPassView = (
    &'static ExtractedCamera,
    &'static ExtractedView,
    &'static ViewTarget,
    &'static ViewDepthTexture,
    Option<&'static MainPassResolutionOverride>,
    Has<OrderIndependentTransparencySettings>,
    Option<&'static OitResolvePipelineId>,
);

fn chunked_main_transparent_pass_3d(
    world: &World,
    view: ViewQuery<TransparentPassView>,
    transparent_phases: Res<ViewSortedRenderPhases<Transparent3d>>,
    chunking: Res<TransparentPassChunking>,
    draw_functions: Res<DrawFunctions<Transparent3d>>,
    lanes: Res<TransparentDrawLanes>,
    mut ctx: RenderContext,
) {
    let view_entity = view.entity();
    let (
        camera,
        extracted_view,
        target,
        depth,
        resolution_override,
        has_oit,
        oit_resolve_pipeline_id,
    ) = view.into_inner();

    let Some(transparent_phase) = transparent_phases.get(&extracted_view.retained_view_entity)
    else {
        return;
    };
    if transparent_phase.items.is_empty() {
        return;
    }
    if has_oit {
        // Running before the resolve pipeline is ready would corrupt the OIT
        // linked lists (see Bevy's main_transparent_pass_3d).
        let Some(oit_resolve_pipeline_id) = oit_resolve_pipeline_id else {
            return;
        };
        if world
            .resource::<PipelineCache>()
            .get_render_pipeline(oit_resolve_pipeline_id.0)
            .is_none()
        {
            return;
        }
    }

    let viewport =
        Viewport::from_viewport_and_override(camera.viewport.as_ref(), resolution_override);
    let diagnostics = ctx.diagnostic_recorder();
    let diagnostics = diagnostics.as_deref();
    let max_chunks = if has_oit { 1 } else { chunking.max_chunks };
    let ranges = chunk_ranges(
        transparent_phase
            .items
            .values()
            .map(|item| item.batch_range().len()),
        max_chunks,
    );
    let last = ranges.len() - 1;
    let device = ctx.render_device().clone();

    let record = |chunk: usize,
                  range: Range<usize>,
                  lane: Option<&mut DrawFunctionsInternal<Transparent3d>>| {
        let mut encoder = device.create_command_encoder(&CommandEncoderDescriptor {
            label: Some("main_transparent_pass_3d"),
        });
        let mut color = target.get_color_attachment();
        if chunk != last {
            // Keep the multisampled target; resolve once after the last range.
            color.resolve_target = None;
        }
        {
            let render_pass = encoder.begin_render_pass(&RenderPassDescriptor {
                label: Some("main_transparent_pass_3d"),
                color_attachments: &[Some(color)],
                // Stored to work around https://github.com/bevyengine/bevy/issues/3776.
                depth_stencil_attachment: Some(depth.get_attachment(StoreOp::Store)),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            let mut render_pass = TrackedRenderPass::new(&device, render_pass);
            let pass_span = diagnostics.pass_span(&mut render_pass, "main_transparent_pass_3d");
            if let Some(viewport) = viewport.as_ref() {
                render_pass.set_camera_viewport(viewport);
            }
            let result = match lane {
                Some(lane) => transparent_phase.render_range_with(
                    &mut render_pass,
                    world,
                    view_entity,
                    range,
                    lane,
                ),
                None => transparent_phase.render_range(&mut render_pass, world, view_entity, range),
            };
            if let Err(err) = result {
                error!("Error encountered while rendering the transparent phase {err:?}");
            }
            pass_span.end(&mut render_pass);
        }
        encoder
    };

    let parallel = lanes.function_count == draw_functions.read().draw_functions.len()
        && lanes.lanes.len() + 1 >= ranges.len();
    let command_buffers = if ranges.len() == 1 {
        Vec::from([record(0, 0..transparent_phase.items.len(), None).finish()])
    } else if parallel {
        // Every range after the first records and finishes on the pool with its own
        // draw-function lane while the first records here with the world's draw functions.
        let record = &record;
        let mut ranges = ranges.into_iter().enumerate();
        let (_, first_range) = ranges.next().expect("ranges are non-empty");
        let mut first = None;
        let rest = ComputeTaskPool::get().scope_with_executor(false, None, |scope| {
            for ((chunk, range), lane) in ranges.zip(&lanes.lanes) {
                scope.spawn(async move {
                    let mut lane = lane.lock().unwrap_or_else(PoisonError::into_inner);
                    record(chunk, range, Some(&mut lane)).finish()
                });
            }
            first = Some(record(0, first_range, None).finish());
        });
        first.into_iter().chain(rest).collect()
    } else {
        // Each range finishes on the pool while the next one is recorded here; the last one,
        // which nothing else overlaps, finishes inline. Waiting does not tick the shared executor,
        // so an unrelated main-app task cannot delay the render graph.
        let mut last_buffer = None;
        let mut buffers = ComputeTaskPool::get().scope_with_executor(false, None, |scope| {
            for (chunk, range) in ranges.into_iter().enumerate() {
                let encoder = record(chunk, range, None);
                if chunk == last {
                    last_buffer = Some(encoder.finish());
                } else {
                    scope.spawn(async move { encoder.finish() });
                }
            }
        });
        buffers.extend(last_buffer);
        buffers
    };
    for command_buffer in command_buffers {
        ctx.add_command_buffer(command_buffer);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunks_cover_items_without_splitting_batches() {
        // Runs of three items drawn by their first item, skipped items and
        // single draws, interleaved like particle runs among meshes. Six draws per seven
        // iterations, enough to fill every chunk.
        let mut lens = Vec::new();
        for index in 0..2 * MAX_TRANSPARENT_CHUNKS * MIN_DRAWS_PER_CHUNK {
            match index % 7 {
                0 => lens.extend([3, 0, 0]),
                3 => lens.push(0),
                _ => lens.push(1),
            }
        }
        let mut draw_starts = vec![false; lens.len() + 1];
        let mut index = 0;
        while index < lens.len() {
            draw_starts[index] = true;
            index += lens[index].max(1);
        }
        draw_starts[lens.len()] = true;

        let ranges = chunk_ranges(lens.iter().copied(), MAX_TRANSPARENT_CHUNKS);
        assert_eq!(ranges.len(), MAX_TRANSPARENT_CHUNKS);
        assert_eq!(ranges.first().unwrap().start, 0);
        assert_eq!(ranges.last().unwrap().end, lens.len());
        for pair in ranges.windows(2) {
            assert_eq!(pair[0].end, pair[1].start);
        }
        for range in &ranges {
            assert!(draw_starts[range.start] && draw_starts[range.end]);
        }
    }
}
