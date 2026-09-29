# Renderer follow-up — 2026-09-29

Base revision: `f319148`, with the unsafe palette-sharing attempt reverted. This investigation adds
profiling switches; it does **not** re-enable shared palettes or change the production renderer.

## Decision

A clean shared-palette implementation using Bevy's existing mesh pipeline needs a change in
`bevy_pbr`'s skin allocator/extraction code. It does not require rewriting the whole renderer.
However, the next measured priorities are **material binding policy and particle submission**.
Their costs are much larger than the palette-sharing benefit measured before the revert.

## Why the palette workaround was unsafe

The reverted `cfa78f5` changed every participating mesh's `SkinnedMesh.joints` to a one-joint stub,
retained the full skin on a synthetic anchor, and patched `MeshInputUniform.current_skin_index`
just before uploading instance buffers. Vertex joint indices and weights still addressed the
original full skeleton.

In Bevy 0.19.1, `SkinUniforms` owns a private allocator and a private mesh-entity-to-allocation map.
That map feeds more than the one patched field:

- `RenderMeshInstanceGpuBuilder::prepare` supplies GPU-preprocessed mesh input;
- the CPU mesh preparation/batching paths call `skin_index` directly;
- `SetMeshBindGroup` calls `skin_byte_offset` to select skinned bindings and uniform offsets;
- `set_mesh_motion_vector_flags` visits `all_skins`;
- `prepare_skins` swaps current/previous palette buffers. `skinning.wgsl` reads both using the
  current skin offset, so allocation history must remain coherent across frames.

The workaround left those consumers believing the mesh owned a one-joint allocation. If its late
alias lookup fails, it silently leaves that allocation selected even though vertex indices require
more joints. The shader then addresses unrelated palette entries. This is a concrete failure mode
consistent with exploding geometry; it is **not a captured diagnosis of every reported artifact**.
There is also a source-level cloning hazard: `fix_wc3_scene_materials` copies `skin.clone()` into
team-colour underlays, while the workaround declines to alias skins of length one. A layer created
from an already-shortened source can therefore inherit a stub without its alias/full-skin metadata.
Tying the full palette anchor to one mesh child also gives one mesh ownership of a model-wide
resource. The broken path was not re-enabled to reproduce the artifacts.

### Narrow, clean Bevy patch

Keep all main-world `SkinnedMesh` components and vertex data unchanged. Internally split
`SkinUniforms` into `mesh entity -> palette ID` membership and `palette ID -> allocation/pose` storage.
The sharing key must compare the **entire ordered joint entity list and inverse-bindpose asset ID**;
asset modifications invalidate the pose. Sharing only a first joint is insufficient.

Each visible/enabled mesh acquires a reference; losing visibility, despawning, changing skins or
entering/leaving a disabled effect pool updates membership. The palette belongs to the group, not
an arbitrarily selected mesh. Compute and stage each palette once. Existing `skin_index` and
`skin_byte_offset` accessors resolve membership; `all_skins` still enumerates mesh consumers.
This keeps GPU preprocessing, CPU fallback, uniform bindings, shadows and motion flags consistent
without late buffer edits or new mesh shader semantics.

Keep palette addresses stable while previous-frame data can be referenced, and initialize history
when a palette appears or changes identity. Partial uploads must account for **both alternating
buffers**, including buffer growth; updating only this frame's dirty range can leave the other
buffer stale. First implement sharing with the existing full upload, then optimize dirty ranges.
Leave normal dynamic bounds in place initially: geosets have different bound data even when their
palettes are equal. Shared conservative model bounds are a separate change.

The current public `SkinUniforms` API does not expose membership/allocation mutation. A pinned,
narrow `bevy_pbr` patch is appropriate for retaining the stock pipeline; a separate custom renderer
could also own its palettes, but is a much larger project. Validate layer creation, visibility
transitions, pool reuse, resurrection, palette growth and current/previous poses before enabling it.
The previous attempt's ~0.4% frame-time gain is not a reason to prioritize this fork now.

## New controlled measurements

Real visible 3840×2160 window, AutoNoVsync, 500 units, centred locked camera. Each run warms up paused
for 10 seconds and captures 10 seconds of combat. All runs finish at tick 300/301 with 115 units and
330 corpses. No builds or CPU sampling overlap the comparison runs. Baseline runs bracket the other
experiments; binding fallback also has two runs. [Raw reports](rendering-performance-2026-09-29-results.txt).

| Mode | FPS | Mean frame ms | PostUpdate ms | Render/submit/present ms | Transparent draws* |
| --- | ---: | ---: | ---: | ---: | ---: |
| Baseline, two-run mean | 37.73 | 26.509 | 7.487 | 14.884 | 1,515 |
| No bindless, two-run mean | 44.24 | 22.605 | 7.581 | 10.289 | 1,628 |
| Freeze poses | 39.43 | 25.362 | 6.233 | 14.236 | 1,579 |
| Freeze bounds | 38.70 | 25.841 | 7.033 | 14.700 | 1,518 |
| Hide ordinary particles | 75.56 | 13.234 | 5.893 | 5.074 | 521 |

*Draw-function invocations from the latest sampled frame, not whole-capture averages. Timed scopes
include scheduling/waiting and overlap; do not sum them. Frozen/hidden modes change the picture and
are attribution experiments, not optimizations. Their gains cannot be added together.

### 1. Bindless resource groups are too expensive for this workload

Disabling `TEXTURE_BINDING_ARRAY` exercises Bevy's normal non-bindless material fallback. The logs
confirm `standard_material_bindless=false`; skinning, geometry and authored material parameters stay
intact. FPS rises **17.3%**, frame time falls **14.7%**, and render submission falls **30.9%**, despite
slightly more draws. p99 improves from 61.90 to 49.34 ms. This remains an opt-in experiment; visual
parity across the full content set and other GPUs has not been established.

A separate CPU sample of active combat attributes 5.99% of sampled cycles to wgpu texture-memory
initialization tracking and 2.93% to texture usage merging. In wgpu 29.0.4, `flush_bindings_helper`
iterates a rebound group's `used_texture_ranges`, and setting render bind groups merges their
resource usage. Bevy 0.19.1's `AUTO_BINDLESS_SLAB_RESOURCE_LIMIT` is **2,048 on Linux**, versus 64
on macOS/iOS; its own comment identifies wgpu resource-count costs as the reason for this limit.
This supports resource-tracking overhead as the explanation, rather than a GPU pixel bottleneck.

Next design: compare smaller material slabs (64/128/256) with the measured non-bindless fallback.
Use a game material wrapper with a capped `bindless_slot_count`, or expose the allocator's existing
capacity setting cleanly. `ExtendedMaterial` already selects the smaller base/extension limit.
Keep shader semantics and material state unchanged. This work does not require skinning changes.
A configurable non-bindless path is the simplest candidate if smaller slabs do not beat it.
Do not disable wgpu validation or memory initialization to achieve the gain.

### 2. Buffered particle state has not removed particle submission overhead

`emit_wc3_particles` still creates a `Mesh3d`, material component, transform and entity for each
ordinary particle, including additive particles. `update_wc3_particles` still changes each particle's
transform. Existing buffers eliminate colour/UV asset churn, but not ECS traversal, mesh extraction,
visibility processing or the many interleaved transparent draws.

Hiding particles retains emission, integration and lifetimes, yet removes roughly **1,000 draws**,
halves frame time and lowers submission from 14.88 to 5.07 ms. Transparent GPU time falls only from
about 2.08 to 1.27 ms in the recent pass sample, while opaque GPU time remains about 2.5 ms. The large
opportunity is the CPU path for issuing particle geometry.

Next design: compact particle arrays and a dedicated billboard draw path with position, size,
colour and atlas coordinates in instance records. Avoid general Mesh3d entities and PBR bindings
for individual quads. Merge particle items into the existing transparent ordering and batch only
compatible contiguous runs; retain interleaving with alpha-blended model layers. Additive blending
does not permit arbitrary reordering across alpha geometry. Texture arrays grouped by compatible
size/format or carefully authored atlases may reduce bindings further without giant arrays of
separate texture resources. Preserve blend/depth rules, priority, atlas timing and residual lifetimes.
This is the unfinished geometry/submission half of the original particle design, not another colour
buffer optimization. Legacy model particles and ribbons need separate handling.

### Lower-priority findings

Freezing all poses improves frame time only 4.3%; frozen bounds improve it 2.5% on this fixture.
CPU samples still show animation evaluation/propagation and skin extraction, but they are not the
best immediate route to higher FPS. Skin extraction itself is 2.72% of sampled cycles; a cycle share
across worker threads is not a direct estimate of frame-time savings.

The read-only `python3 tools/audit-animation-rest-channels.py` scan also rejects broad constant-curve
pruning as a major next project: only 1,792 of 91,887 unit animation channels (1.95%) are exactly at
the rest value across every clip for that target/property. Only 50 of 9,304 animated unit nodes have
all their properties at rest. These are unweighted catalog counts, not runtime savings estimates.
Do not remove a rest-valued channel from one clip if another clip animates that property: it may be
needed to reset the pose on transition.

The initial worst frame remains around 100 ms even in the particle-hidden and binding-fallback
captures. Removing steady submission overhead will not alone eliminate that activation hitch.

## Changes and validation

Added profiling-only `freeze-poses` and `no-bindless`, actual bindless-mode logging, the read-only
channel audit, this design report and raw measurements. Normal gameplay rendering remains unchanged.
Release build, client-binary Clippy with warnings denied, formatting and all seven live captures
passed. **No tests were run, as requested.**
