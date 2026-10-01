# Rendering investigation: 700 units at 60 FPS

Base revision: `5e15f47`. The 64-slot StandardMaterial policy and compact billboard renderer are
already enabled. Target: frames at or below 16.67 ms with the original game's 700-unit population.

## Workload and interpretation

Visible 3840×2160 window, AutoNoVsync, centred locked camera, `--stress-units 700 --profile
--profile-warmup 10 --profile-duration 10`. Warm-up is paused; capture runs combat. No tests were
run, as requested. Builds and CPU sampling do not overlap comparison captures.

This fixture starts with 700 living units, but its interleaved opposing teams cause immediate mass
casualties: the first telemetry interval has 330 living units and 370 corpses; capture ends near
121 living units and 529 corpses. It is a useful activation/battle/corpse workload, **not proof of
60 FPS with 700 continuously living, moving, attacking units**. The paused warm-up is around
82–99 smoothed FPS; the opening combat interval is around 30. Aggregate FPS hides both differences.
The report now includes unsmoothed frame windows and the fraction exceeding the 60 FPS budget.

## Initial attribution

| Mode | Average FPS | Mean frame ms | PostUpdate ms | Render/submit/present ms |
| --- | ---: | ---: | ---: | ---: |
| Baseline | 50.45 | 19.823 | 7.491 | 10.988 |
| Hide ordinary particles | 67.21 | 14.879 | 6.974 | 4.834 |
| Freeze poses | 50.42 | 19.832 | 5.980 | 11.223 |

Hidden/frozen modes change the picture and are attribution controls, not acceptable optimizations.
The initial maximum frame remains 88–89 ms in all three runs. Scope times overlap and cannot be
summed. Final sampled transparent draw-function calls are 2,410 / 448 / 2,921 respectively; these
are neither capture-wide averages nor perfectly identical particle populations.

A separate CPU sample restricted to approximately the combat portion found 14.10% in Bevy's
animation-target parallel loop, 9.25% in descendant transform propagation, 3.75% in skinned bounds,
2.88% in skin extraction, and 2.59% in `wc3_event_crossings`. Texture initialization tracking is
down to 1.96%, texture usage merging to 1.12%. These are cycle shares across threads, not frame
budgets. Despite substantial animation CPU, freezing poses barely improves throughput here:
particle submission remains the first measured steady-state target.

## New experiments

The final comparison series brackets the experiments with baseline runs. All six use the same
release binary; [raw reports](rendering-performance-2026-10-01-results.txt) also retain the initial
attribution and exploratory captures. These are short comparisons on one GPU, not an acceptance
claim or cross-platform visual validation.

| Mode | Average FPS | Mean frame ms | p99 ms | Render/submit/present ms |
| --- | ---: | ---: | ---: | ---: |
| Baseline A | 46.53 | 21.491 | 38.287 | 12.465 |
| Partial texture bindings A | 50.26 | 19.898 | 35.143 | 11.091 |
| Conservative particle culling | 47.25 | 21.165 | 36.801 | 12.455 |
| Shared PBR view bindings | 48.74 | 20.519 | 36.653 | 11.724 |
| Partial texture bindings B | 50.09 | 19.965 | 35.432 | 11.061 |
| Baseline B | 47.06 | 21.252 | 37.999 | 12.496 |

**Partial binding is the best small follow-up:** two-run mean 50.18 vs 46.80 FPS, **7.2% higher
FPS**, **6.7% less frame time**, and **11.3% less submission time**. Sampled particle groups contain
8 actual textures: baseline binds 64 entries, the experiment binds 8. Both queue every particle.
This isolates padding/resource tracking without changing blend math, shader texture indices or
sorting. It remains opt-in; check visual parity and fallback/device coverage before promotion.

Shared camera bindings show a smaller gain (48.74 FPS in the final series; 48.05 in the earlier
exploratory run). Culling is not a useful centred-benchmark priority: it rejects 113 zero-alpha and
345 offscreen quads out of 3,337 in the final sampled frame, but hardly changes submission time.
Those counts are a single snapshot, not a whole-run culling percentage.

Even partial binding starts at only 33.1 FPS in the first window and reaches 67.8 FPS in the last
window of run A. The maximum remains about 89 ms. The target has **not** been achieved.

`particle-shared-view` keeps the stock PBR view bind groups in slots 0 and 1 and moves particle
textures to slot 2. It obtains the exact per-view layout key from Bevy's `ViewKeyCache`, including
prepass/MSAA/fog/other view features. Model/particle interleaving then need not replace the view
groups each time. Geometry, blend operations and sorting remain unchanged. This uses public Bevy
APIs and does not need a renderer fork. It deliberately remains opt-in pending measurement and
visual coverage across view configurations.

`particle-cull` skips exactly zero-alpha quads and quads wholly outside the view frustum. A sphere
with radius `abs(scale) / sqrt(2)` contains the billboard square at every camera angle. The far
plane is deliberately not used, making rejection conservative for infinite projection. Simulation,
emission and lifetimes continue; all surviving items retain their original sorting. Missing frustum
data conservatively keeps a quad. This also remains opt-in.

`particle-partial-bindings` retains the existing array layout and shader but supplies only the
populated texture/sampler prefix when the device has `PARTIALLY_BOUND_BINDING_ARRAY`. Unsupported
devices retain padding. The measured 8/8 versus 8/64 slot report confirms this hardware exercised
the intended path. All emitted indices address the populated prefix; empty slabs are not emitted.

## Other architectural opportunities

### Batch transparent items that cannot overlap

This is the larger submission redesign to investigate after the small binding experiments. The
sorted phase uses one total depth order for the entire screen. A model in the left of the image can
therefore split a run of particles on the right, even though their pixel coverage cannot interact.
The final sampled phase still needs roughly 2,500 draw-function calls. Global grouping by texture
or blend mode is incorrect, but **reordering provably disjoint coverage is safe** for the known
depth-read-only, side-effect-free transparent shaders.

Start with the existing depth-sorted order. Obtain conservative per-view screen rectangles for
billboard quads and eligible model geosets. Retain the original precedence for every overlapping
pair, then schedule ready items to extend compatible particle/material runs. Use a screen grid to
find possible overlaps and a stable topological ordering; cap candidate work and fall back to the
original order for crowded regions. Unknown bounds, near-plane crossings, unsupported shaders,
stencil/storage side effects and unknown draw functions must act as ordering barriers. Bounds must
cover skinning, shader displacement, rasterization/MSAA footprint and the current pose. Include
every potentially interacting model layer, not only particle/particle overlaps.

This preserves blend operations and all potentially interacting draw order, unlike globally moving
particles after models or merging Alpha/Add blindly. Run it before either PBR or particle batching.
Measure CPU scheduling cost, compatible run lengths, actual draws, submission time and image parity;
the heavily overlapping centre may still offer little freedom. A first diagnostic should count the
split runs caused by disjoint items before implementing the full scheduler. This is an unmeasured
design, not a promised FPS gain, and does not intrinsically require a Bevy fork.

### Reduce texture resources touched by each particle draw

The new particle renderer fills every texture/sampler array to 64 entries with fallback bindings,
even if fewer textures are used. In the pinned wgpu 29.0.4 source, `create_texture_binding` appends
a `TextureInitTrackerAction` for every entry, including repeated fallback views; rebound groups
visit this list during command encoding. Consequently a short instanced run still pays for a large
resource list. Potential designs:

- Use partially populated arrays when `PARTIALLY_BOUND_BINDING_ARRAY` is actually enabled, ensuring
  every instance index remains within the populated range. Retain a fully populated fallback.
- Compare 4/16-entry slabs, not only 64/128/256. More texture groups can still win if each draw
  touches fewer resources. Report draw counts and submission time together.
- Longer term, pack compatible images into `texture_2d_array` layers. Group by exact dimensions,
  format, mip count and sampler semantics so no resampling or atlas-edge filtering changes the
  authored image. One image-array binding avoids a bindless resource per texture. Converted unit
  manifests reference 83 unique particle texture paths; 34 are 64×64, 16 are 32×32, and 16 are
  128×128. This is a catalog count, not a measured runtime texture working set.

The texture bind groups are also destroyed/recreated each frame. Persistent texture-slot allocation
and cached groups can avoid this setup work. Cache keys must include actual GPU texture-view and
sampler identity, handle image readiness/reload/removal, and retire empty slabs. Asset IDs alone do
not detect replaced GPU resources. This is separate from changing transparent order.

### Compile effect playback metadata once per model

Every sound/spawn/splat event repeatedly finds its inherited sequence clock, clones its sequence
name, linearly finds the named sequence window, then scans event times. The 2.59% cycle share makes
this more worthwhile now. At scene preparation, bind an event to its owning model and compile
sequence IDs to event ranges. Borrow the clock and keep per-instance previous-time state. Use
ordered event ranges/cursors for crossings, retaining initial time-zero events, non-looping ends,
global sequences, repeated loops, sequence changes and pool resets. Rebinding must follow hierarchy
changes and nested model ownership. Avoid changing authoritative ability timing.

### Smooth pose work before replacing the animation architecture

`throttle_gameplay_animation_poses` currently uses one global 30 Hz accumulator, so all eligible
players sample together. Stagger independent presentation sampling phases, while forcing a fresh
sample on activation/sequence changes and retaining logical clocks. This may reduce spikes rather
than mean CPU; measure frame distributions before claiming a gain. Root movement still propagates
descendants, so staggering alone does not eliminate transform work.

For a larger redesign, resolve the active graph/clip once per model, evaluate contiguous local
joint poses, and keep root motion separate from the local skin palette. All material layers share
that model's palette; attachments request the relevant joint world transforms. This removes
repeated graph lookups per bone and propagation through every bone solely for root movement.
Retain exact transitions, non-inheritance, attachment transforms and bounds. The earlier unsafe
skin-index override must not return; stock-pipeline palette sharing requires the allocator change
described in the [previous investigation](rendering-performance-2026-09-29.md).

### Treat combat activation separately

The new slow-frame breakdown localizes the worst hitch. In baseline A, the frame beginning 0.076 s
into capture takes **87.665 ms**, with **64.066 ms in SpawnScene** and 83.965 ms in main CPU. The
next frame takes 74.397 ms, including **39.430 ms in Update** and another 13.025 ms in SpawnScene.
The partial-binding capture repeats this pattern: 65.151 ms in SpawnScene on its worst frame and
39.747 ms in Update on the following frame. A later spike near 2.05 s again spends about 18.5 ms
in SpawnScene. This is concrete evidence to prioritize cold hierarchy creation and subsequent
setup; it does not identify every effect asset or every Update system yet. The first simulation
activation also has a roughly 20 ms fixed-loop spike, separate from the worst scene-spawn frame.

The current effect pool only retains instances after their first use. `Wc3VisualModel` marks models
with ribbons or attachments non-poolable, so they still use fresh hierarchies on every activation.
Prepare reusable effect instances and immutable parsed model metadata during loading, with bounded
pool sizes and incremental refill. Resettable nested attachments/ribbons need explicit lifecycle
ownership before broadening pool eligibility. Preserve onset timing, authored lifetimes, animation
selection, event cursors, team/material state, particle ownership and visibility when borrowing an
instance. Start by counting cold spawns and setup time by model asset to choose pool capacities.
Pipeline warm-up may also help, but the captured hitch should not be attributed primarily to shader
compilation. Do not simply extend warm-up until combat activation is excluded from the results.

## Recommended implementation order

1. Finish device/fallback and image-parity validation for partial particle bindings, then promote
   that small change independently. The opt-in implementation is available for comparison.
2. Instrument cold spawns by model; prewarm the high-burst effect templates and prepared metadata.
   This directly targets the observed 64–65 ms SpawnScene frame and following setup work.
3. Count non-overlapping items that split compatible transparent runs, then prototype the conservative
   overlap-aware scheduler. This addresses submission scale without sacrificing blend correctness.
4. Cache stable particle binding groups and compile event playback metadata. Reassess animation
   after these changes; its substantial CPU share currently overstates its throughput impact.

Changes in this investigation are instrumentation and opt-in experiments only. Default blend,
sorting, simulation and gameplay policy remain unchanged. Validation: release builds, live windowed
captures, warning-free client-binary Clippy, formatting and diff checks. Tests were deliberately
skipped. No Bevy patch or unsafe palette override was added.

## Follow-up: preparation and pool coverage

The timed-effect pool is only one instantiation path. Moving projectiles, persistent status visuals,
stun visuals, and the special Defend transition currently create fresh `WorldAssetRoot` hierarchies.
Consequently warming only `TimedWc3EffectPool` cannot remove every initial or repeated combat burst.
The profiling census now counts newly added scene roots by asset during capture, with peak requests
per frame, first-request time and template entity counts. It runs before world-instance spawning.
Requests can wait for assets, so the table deliberately does not call them completed spawns or
attribute individual durations to them. Paused warm-up roots and reactivated pooled roots are
excluded. This lets a subsequent implementation target the expensive paths instead of allocating
an arbitrary reserve of every loaded effect.
Prewarming must let hidden instances finish world spawning and WC3 setup before disabling and
reserving them; disabling a root immediately can exclude it from the systems meant to prepare it.
Reserve sizing must consider concurrent lifetimes, not only one-frame request peaks.

A final live census on `440a68d` (the intervening hosted-lobby commit) plus this instrumentation
uses the same centred 700-unit workload and partial particle bindings. It ends at tick 300 with
121 units and 529 corpses. The worst frame takes 90.199 ms, including 63.712 ms in SpawnScene;
the following frame spends 41.006 ms in Update. The later 2.06 s spike includes 19.093 ms in
SpawnScene. This is an attribution capture, not a new controlled FPS comparison against the
earlier baseline. Its raw report is appended to the results file.

| Requested model | New roots in 10 s | Peak roots/frame | Entities/template | First observed |
| --- | ---: | ---: | ---: | ---: |
| Gryphon rider missile | 242 | 50 | 27 | 0.093 s |
| Resurrect caster | 50 | 36 | 78 | 0.093 s |
| Resurrect target | 50 | 36 | 49 | 0.093 s |
| Mortar missile | 53 | 50 | 36 | 0.093 s |
| Inner Fire target | 53 | 50 | 28 | 0.093 s |
| Rifleman unit | 18 | 15 | 80 | 2.085 s |
| Defend caster | 50 | 50 | 23 | 0.720 s |

Counts cover newly added roots, including instances that might wait for asset readiness. Template
counts sum occupied non-resource archetype entities, matching the world spawner's entity-copy
scope; they are not the allocator's capacity. Do not sum independent per-model peak counts as
though every peak necessarily occurs in the same frame. Start prewarming with resurrection and
missile templates; expand the pool to the relevant persistent/moving visual lifecycles, and
measure concurrent occupancy before deciding reserve sizes.

Two additional setup inefficiencies are visible directly in source:

- `setup_wc3_model_composed_features` and `setup_wc3_model_lights` each deserialize the complete
  `Wc3NodeExtras` for every new node. `resolve_wc3_emitter_nodes` also reads node extras. Even nodes
  without a light take the parsing path in the light setup system. Material setup separately
  deserializes its tracks/windows for each instance. Prepare typed immutable model/node/material
  metadata once per source asset, including negative results, then attach or reference it from
  instances. Keep mutable clocks, event cursors, GPU slots, emitter counters and child ownership
  per instance. Asset reload must invalidate the prepared template. Version/asset identity must
  remain part of the cache key. Do not accidentally share mutable playback state between models.
- `fix_wc3_scene_materials` takes a mutable source `StandardMaterial` and writes alpha mode and
  depth bias on every newly prepared mesh, including instances whose shared source already has
  those values. That generates redundant asset-change work during bursts. Normalize immutable
  source/material variants once, or compare through an immutable borrow and write only when
  values actually differ. Cache identity must still distinguish authored layer state, team tint
  and the existing building/unit underlay policy; team-specific state must not leak into another
  instance.

A further activation cost is redundant rest-bounds calculation. The glTF loader inserts authored
primitive AABBs, but Bevy's `calculate_bounds` still rescans mesh vertices for each newly added
`Mesh3d` because additions satisfy `Changed<Mesh3d>`. Many instances share the same immutable mesh.
The short initial CPU sample includes 10.29% in that bounds-update loop; this is a coarse sample,
not a measured saving. Cache rest bounds once per mesh asset, or retain loader bounds with a scoped
`NoAutoAabb` policy for converted immutable geometry. Asset/handle changes must refresh the cache;
dynamic skinned-pose bounds must continue normally. Applying `NoFrustumCulling` or freezing pose
bounds would not be a faithful substitute. This can complement pooling without changing skin IDs.

Bevy's world-asset spawner copies component data through its reflection-based serialization path
before these setup systems run. Pooling prepared instances avoids both this hierarchy copy and
the repeated setup. Typed template instantiation is another possible long-term route, but requires
correct entity remapping for skins, animation targets and nested relationships; the established
spawner is the reference for fidelity. These source findings do not by themselves quantify savings.

The later unit-model burst also suggests retaining prepared unit instances through resurrection.
The simulation currently removes a corpse and creates a unit with a fresh ID; presentation removes
the old corpse hierarchy and instantiates a new unit hierarchy. The snapshot does not expose a
corpse-to-new-unit mapping. A safe reuse design needs an explicit authoritative presentation hint
or a prepared unit-instance pool; matching disappeared corpses to new units by position is ambiguous.
Reset every template-local transform and authored material/visibility state before starting the
live sequence, since a death clip can animate channels that Stand never resets. Preserve complete
skin joint lists, reset owner/attachment bindings and controller IDs, and retain ordinary bounds.
This is a lifecycle optimization, not another late skin-index alias. The timing of the measured
rifleman burst is consistent with resurrection, but the census alone is not an event correlation.

## Source anchors

- Client: `particle_renderer.rs` (slabs, queuing, binding), `wc3_effects.rs` (event crossings and
  animation throttling), `presentation.rs::spawn_or_reuse_timed_wc3_visual` (cold pool path).
- Bevy 0.19.1: `bevy_pbr/src/render/mesh.rs::ViewKeyCache`, `mesh_view_bindings.rs`,
  `bevy_animation/src/lib.rs::animate_targets`.
- wgpu-core 29.0.4: `device/resource.rs::create_texture_binding` and `check_array_binding`,
  `command/pass.rs::flush_bindings_helper`.
