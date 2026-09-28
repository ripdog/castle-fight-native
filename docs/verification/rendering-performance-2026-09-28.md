# Rendering architecture investigation — 2026-09-28

Status: investigation and opt-in measurement tools, plus six measured renderer changes: ordinary additive particles share buffered render state, the common static unit team-colour layer is composited in one pass, exported binary geoset visibility excludes truly hidden geosets from submission, terrain-conforming splats use persistent shared material state, alpha-only animated model layers keep alpha in per-instance GPU records, and texture-ID animated layers now switch among immutable shared material variants instead of mutating material assets. Per-instance attachment indices also bound repeated effect attachment lookup; that change is tail-latency/complexity work rather than a measured throughput gain. The broader model-envelope/off-screen pose lifecycle work remains proposed.

## Scope and reproducibility

The simulation is excluded from the primary comparison by keeping it paused. The user supplied
screenshots showing 29.28 ms/frame with 13.073 ms main CPU and 54.25 ms/frame with 14.907 ms main
CPU. The latter had no simulation ticks, 9,108 visible mesh entities, 5,503 skins and 1,107 animation
players. Its two largest reported GPU passes were 4.317 ms transparent and 3.321 ms opaque.
These are different scenes, not a controlled before/after. The unaccounted frame interval cannot
be labelled GPU time: it includes extraction, waiting for the render thread, and presentation.
Likewise the displayed “3D CPU” covers application Update systems, not all Bevy rendering work.

The requested repeatable fixture is `--stress-units 500`. Primary measurements use the real window,
release build, fixed map-centred camera and paused simulation, ten seconds of warm-up, and ten seconds of
capture. Ambient effects and presentation animation continue. Each experiment starts a fresh
process; runs are sequential. The existing local changes to presentation/effect binding and asset
export were preserved, so results describe that working tree rather than pristine HEAD.

Reference hardware: AMD Ryzen 5 5600 (6 cores / 12 threads), Radeon RX 6900 XT, Mesa RADV 26.2.3.
The dependency source inspected is Bevy 0.19.1 from Cargo.lock. The source base is
`b885a20b686eacd83276178b1f8e16275489e4d6` plus the preserved working-tree changes and this instrumentation.

Generated asset manifest SHA-256 fingerprints:

| Pack | SHA-256 |
| --- | --- |
| units | `ca2cfc8297ab9724dfcbd1d3439621947456df6974c7ffd3a15ee63fed4cb773` |
| buildings | `8527ff209292c16797c991b0e5def2daa5d3108fec037b2454e100f28923d8ac` |
| doodads | `8996de6895208a184286d1b738c5c5fd89295f488449cc5207e79a215ff403a7` |
| effects | `82990941e2730c9eb5cdd0a9d7f7784e7171e5ff0ae6224d904b9afe15d852a6` |


```sh
tools/cargo-interactive build --release -p castle-fight-client
# Run the built binary separately, so compiler CPU affinity does not constrain the game.
target/release/castle-fight-client --stress-units 500 --profile --profile-paused --profile-warmup 10 --profile-duration 10
# Repeat with one of:
# --render-experiment freeze-bounds
# --render-experiment hide-skinned
# --render-experiment hide-particles
# --render-experiment hide-transparent
# --render-experiment legacy-team-color
# --render-experiment legacy-geoset-visibility
# --render-experiment legacy-attachment-index
# --render-experiment legacy-splat-material-state
# --render-experiment legacy-animated-alpha-state
# --render-experiment legacy-animated-texture-state
# Texture-ID-heavy visual fixture used for that comparison:
# target/release/castle-fight-client --stress-visual h02W 500 --profile --profile-paused --profile-warmup 10 --profile-duration 10
```

The ordinary quicksave could not be used: snapshot decoding reported a missing
`resurrection_mana_cost` field. The original save was not modified or migrated.

An exploratory run before the camera fix was discarded: the default view started at the builder
and was manually panned. All comparison runs below must use the automatically centred fixed camera.

## Controlled results

All seven runs used a visible 3840×2160 window with `AutoNoVsync`, camera position
`(0, 3394.0002, 3060)` and forward direction `(0, -0.7270132, -0.6866236)`.
Every run ended at simulation tick 0 with exactly 500 units, 2 builders, 2 buildings, no corpses and
no projectiles. Camera/window identity and population were checked across the reports.
No compiler ran during the timed captures. Ordinary desktop activity was not isolated.

| Experiment | FPS | Frame ms | p95 ms | Main ms | Extract ms | Prepare ms | Render/submit/present ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Baseline 1 | 46.51 | 21.503 | 23.936 | 11.468 | 2.553 | 3.167 | 9.339 |
| Freeze bounds | 48.24 | 20.731 | 23.039 | 10.396 | 2.627 | 3.130 | 9.248 |
| Hide particles | 91.20 | 10.964 | 14.340 | 8.876 | 2.011 | 2.194 | 2.272 |
| Hide skinned meshes | 110.31 | 9.065 | 11.381 | 8.478 | 0.299 | 1.485 | 2.485 |
| Hide transparent meshes | 102.85 | 9.723 | 12.436 | 8.490 | 1.160 | 1.313 | 1.071 |
| Baseline 2 | 46.53 | 21.490 | 23.877 | 11.487 | 2.562 | 3.160 | 9.346 |
| Hide particles repeat | 87.85 | 11.383 | 14.494 | 9.272 | 2.033 | 2.235 | 2.348 |

[Complete captured reports](rendering-performance-2026-09-28-results.txt) include all render stages,
CPU/GPU pass measurements, scene counts, 1% lows and camera identity. Render stages run in parallel
with the main schedule; the columns are not additive. “Render/submit/present” includes driver and
presentation waits and is not a GPU timer. Its exact internal split still needs a driver/CPU trace.

The two baselines differ by only 0.02 FPS. Disabling ordinary particle visibility improves frame
time by 47–49% in two runs, despite continuing to spawn and update those particles. The number of
sampled transparent draw-function calls falls from 1,947–2,026 to 427–447. Recent transparent-pass
CPU time falls from about 1.5 ms to 0.28–0.30 ms, and GPU time from about 3.1 ms to 1.32–1.34 ms.
Render/submit/present falls from about 9.34 ms to 2.27–2.35 ms. This implicates both submission work
and GPU work; the pass timings alone do not account for the whole frame improvement.

Freezing bounds saves only about 0.77 ms/frame here (3.6%). It also changes which meshes pass
culling, so that is not a pure bound-update cost. Prioritize particle representation and the model
rendering path before spending the whole effort on this smaller cost.

Hiding all skinned meshes reaches 110 FPS while retaining their skeleton/animation entities;
extraction falls from 2.55 ms to 0.30 ms and the skin upload disappears. This removes most imported
geometry, including doodads, so it does not measure skin arithmetic alone. Hiding transparent meshes
reaches 103 FPS; that overlaps heavily with the particle and skin probes and is not an additional
independent saving.

Baseline census: approximately 71,400 entities, 8,800 mesh entities, 5,115 skinned meshes, 929 animation
players, and 3,500 ordinary particles. Skins contain 203,780 joint references for 26,016 distinct joint
entities (7.83× references); the renderer uploads 13,578,880 bytes of palette staging data each frame.
A palette per model instance can remove repeated joint work and storage. This byte count alone does
not establish PCIe bandwidth saturation, and the reduction from sharing palettes is not yet measured.

There are also 2,007 frustum-visible meshes with zero-determinant transforms. See the shader caveat
below: these are candidates for a geoset-visibility audit, not proof that all are visually absent.

These are deliberately destructive visual probes, not equivalent-quality optimizations. Their FPS
numbers are not promised gains from the proposed designs. Particle populations vary modestly with
presentation clocks and capture phase; animation throttling and CPU/render-thread overlap also mean
that differences between main-thread averages are not exclusive subsystem costs.

## Implementation result 1 — buffered additive ordinary particles

The first production change takes the safe subset of the particle-buffer proposal. Ordinary WC3
particles using additive blend mode (`filter_mode == 1`) and a texture now share one unit quad and
one custom material per texture. A persistent storage buffer holds per-particle lifecycle colour,
alpha and atlas UV rectangle, indexed through Bevy's per-instance `MeshTag`. Slots are recycled and
the storage buffer grows geometrically. Particle transforms and lifetime integration remain on the
CPU for now.

This deliberately does **not** collapse the transparent phase into one unsorted draw. Each particle
still has its own render entity/transform, so Bevy retains the authored depth ordering and
interleaving with other transparent geometry; adjacent compatible additive particles can then use
normal automatic instancing. Alpha-blended, multiply and masked ordinary particles remain on the
reference `StandardMaterial` path. Ribbons and legacy model-particle scenes are unchanged. The old
31-step lifecycle-colour quantization is also retained, so this change does not silently alter the
sampled colour/alpha curve.

A fresh baseline was captured immediately before implementation, then the same release fixture was
captured twice after implementation. These runs use the same 500-unit, paused, map-centred,
3840×2160 `AutoNoVsync` setup as the controlled investigation above.

| Run | FPS | Frame ms | p95 ms | Main ms | Effects ms | Specialize ms | Queue ms | Transparent draws | Transparent GPU ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Fresh baseline | 47.75 | 20.944 | 23.761 | 11.010 | 2.144 | 2.351 | 1.735 | 1,965 | 3.060 |
| Buffered additive 1 | 51.72 | 19.336 | 22.008 | 9.695 | 1.046 | 1.115 | 0.880 | 1,524 | 2.674 |
| Buffered additive 2 | 52.26 | 19.137 | 21.778 | 9.541 | 1.040 | 1.111 | 0.843 | 1,555 | 2.656 |

Averaging the two implementation runs gives 19.237 ms/frame, **8.2% lower** than the fresh baseline,
and 51.99 FPS, **8.9% higher**. The application-side effects slice falls by about **51%**,
render specialization by about **53%**, render queueing by about **50%**, sampled transparent draw
calls by about **22%**, and recent transparent-pass GPU time by about **13%**. The ordinary particle
population was 3,543 in the fresh baseline and 3,486 / 3,471 in the implementation captures; this is
the modest presentation-clock variation already noted above, not a particle-count reduction
optimization.

`render/submit/present` did not fall in these captures (9.148 ms baseline versus 9.783 / 9.662 ms).
That scope overlaps CPU/render-thread work and presentation waiting, so it is not additive with the
other stages and should not be interpreted as contradicting the lower whole-frame time. The useful
structural evidence is the repeated reduction in effects work, specialization, queueing, draw calls
and transparent GPU time.

The profiler's `hide-transparent` experiment and scene census now recognize the custom additive
particle material as transparent, so future attribution runs continue to measure the intended set.

## Implementation result 2 — single-pass static unit team colour

The second production change takes the first material-layer slice of the shared-model-state proposal.
The common classic unit team-colour representation previously rendered the same skinned geoset twice:
an opaque team-colour underlay plus a textured `Blend` overlay. The new path uses an extended PBR
material and composites those two authored layers in the fragment shader, so the geometry and skin
are submitted only once. The shader keeps the existing texture, PBR lighting, priority plane and
owner colour while making the final composite opaque, matching the fact that the old underlay made
the combined result opaque.

The path is intentionally narrow. An audit of the current unit asset pack finds **161** materials
matching the exact static two-layer `Blend` case. Materials with alpha animation, texture animation,
team glow, additive/nonstandard blend modes, vertex tint, different layer counts, or non-unit asset
packs retain the old two-pass implementation. Building team colour also retains its existing
pre-flattened texture path. A profiling-only `legacy-team-color` experiment forces the old unit path
so the same release binary can make a controlled A/B comparison.

Three production-path captures bracket one legacy-path capture on the same 500-unit, paused,
map-centred, 3840×2160 `AutoNoVsync` fixture:

| Run | FPS | Frame ms | p95 ms | Extract ms | Render/submit/present ms | Transparent draws | Transparent GPU ms | Skinned meshes | Skin upload bytes |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Single-pass 1 | 61.10 | 16.365 | 18.733 | 2.054 | 7.640 | 1,167 | 1.752 | 4,683 | 12,432,064 |
| Single-pass 2 | 60.05 | 16.652 | 19.222 | 2.109 | 7.647 | 1,171 | 1.758 | 4,683 | 12,432,064 |
| Legacy two-pass | 52.03 | 19.218 | 21.660 | 2.412 | 9.743 | 1,598 | 2.668 | 5,115 | 13,578,880 |
| Single-pass 3 | 58.02 | 17.236 | 19.982 | 2.172 | 7.943 | 1,222 | 1.755 | 4,683 | 12,432,064 |

Averaging the three production-path runs gives 16.751 ms/frame, **12.8% lower** than the legacy
path, and 59.72 FPS, **14.8% higher**. The structural reductions are deterministic across the runs:
**432 fewer skinned meshes/dynamic bounds**, **17,041 fewer joint references**, and **1,146,816 fewer
skin-palette staging bytes per frame** (about 8.4%). Against the three-run production average,
world extraction falls about **12.5%**, sampled transparent draw calls about **25.7%**,
transparent-pass CPU about **22.3%**, transparent-pass GPU about **34.2%**, and the overlapping
`render/submit/present` scope about **20.5%**. Opaque-pass GPU time remains essentially unchanged at
about 3.06 ms.

The third production capture is noisier than the first two but remains clearly separated from the
legacy run. Particle populations also vary with presentation-clock phase as noted above; the skin,
joint and palette reductions do not vary and directly reflect removal of the duplicate team-colour
skin layer.

## Implementation result 3 — explicit exported geoset visibility

The third production change takes the safe hidden-geoset slice of the model-level culling proposal.
The converter already marks every Warcraft geoset node with `wc3Geoset` metadata and exports geoset
alpha as an exact binary STEP animation on node scale: `[0,0,0]` for hidden and `[1,1,1]` for visible.
The client now recognizes only those marked nodes and maps that authored binary state to Bevy
`Visibility` immediately after animation sampling. Animation clocks, timeline events, attachments,
root movement and the original transform animation continue unchanged; this change only stops an
authored-hidden geoset from entering view submission. This also fixes the old representation hazard
where Bevy skinning could ignore the zero mesh transform and still shade a supposedly hidden geoset.

A profiling-only `legacy-geoset-visibility` experiment leaves the old scale-only representation in
place, allowing a same-binary A/B. Bevy still runs dynamic skinned-bound recomputation for hidden
entities, so this slice is expected to reduce view extraction, skin-palette allocation/upload and
submission rather than eliminate the remaining bounds cost. Conservative whole-model envelopes,
off-screen pose suppression, settled-corpse pose caching and static doodad grouping remain separate
parts of proposal 3.

Two production runs bracketed one legacy run on the paused 500-unit fixture:

| Run | FPS | Frame ms | Extract ms | Handoff ms | Render/submit/present ms | Transparent draws | Transparent GPU ms | Skin upload bytes | Collapsed visible |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Explicit visibility 1 | 67.20 | 14.882 | 1.338 | 5.202 | 6.746 | 1,014 | 1.523 | 11,327,552 | 0 |
| Legacy scale-only | 56.52 | 17.694 | 2.344 | 7.388 | 7.996 | 1,237 | 1.759 | 12,432,064 | 1,936 |
| Explicit visibility 2 | 68.01 | 14.703 | 1.303 | 5.119 | 6.674 | 960 | 1.526 | 11,327,552 | 0 |

Averaging the two production runs gives 14.793 ms/frame, **16.4% lower** than the legacy path, and
67.61 FPS, **19.6% higher**. World extraction falls about **43.7%**, the overlapping handoff scope
about **30.1%**, render/submit/present about **16.1%**, sampled transparent draws about **20.2%**,
transparent-pass GPU time about **13.3%**, and skin-palette staging by **1,104,512 bytes/frame**
(**8.9%**). The production census has zero frustum-visible zero-determinant candidates versus 1,936
on the legacy path. Dynamic-bound component count remains 4,683 in both paths, consistent with the
known Bevy bounds limitation above.

The same release binary was also run with the simulation active, because combat continuously changes
sequences and creates corpses/effects. The production and legacy captures finished at nearly the same
authoritative state (295/294 ticks, 115 living units and 330 corpses; 4/3 projectiles):

| Active run | FPS | Frame ms | Extract ms | Handoff ms | Render/submit/present ms | Transparent draws | Transparent GPU ms | Skin upload bytes | Collapsed visible |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Explicit visibility | 19.72 | 50.709 | 2.985 | 25.157 | 16.130 | 1,230 | 2.841 | 11,327,552 | 0 |
| Legacy scale-only | 18.22 | 54.892 | 4.194 | 28.646 | 17.772 | 1,490 | 3.201 | 15,131,136 | 1,307 |

That active comparison is **7.6% lower frame time / 8.2% higher FPS**. Extraction falls **28.8%**,
handoff **12.2%**, render/submit/present **9.2%**, sampled transparent draws **17.4%**, transparent
GPU time **11.2%**, and skin-palette staging **25.1%**. The active benchmark is intentionally noisy
and contains large combat-time asset/scene churn, so the structural visibility/palette/draw-count
changes are the stronger attribution signal.

## Active-combat follow-up: attachment search and material updates

The simulation remains a small part of the regression: the original 30-second combat capture
averaged **1.329 ms/tick**, versus **46.939 ms/frame**. Real-window measurements below use the
same 500-unit fixture, centred/locked camera, 3840×2160, AutoNoVsync and ten-second paused warm-up.
The original binary was built from `4c05a22`; the updated binary includes the changes accompanying
this section. No compilation overlapped the captures. Full reports are in
[the combat results](rendering-combat-performance-2026-09-28-results.txt).

| First ten seconds of combat | FPS | Mean frame ms | p99 ms | Max ms | Scene setup ms | Prepare assets ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Original binary | 19.91 | 50.213 | 261.945 | 317.643 | 3.338 | 11.269 |
| Updated, run 1 | 23.83 | 41.965 | 78.439 | 106.530 | 1.028 | 6.948 |
| Restore global attachment search only | 23.00 | 43.473 | 116.697 | 312.092 | 2.969 | 6.823 |
| Restore unconditional splat writes only | 21.09 | 47.415 | 78.672 | 103.335 | 1.028 | 11.283 |
| Freeze material animation (diagnostic only) | 34.51 | 28.978 | 65.884 | 102.662 | 0.769 | 0.579 |
| Updated, run 2 | 23.98 | 41.694 | 77.853 | 106.744 | 0.993 | 6.834 |

The two updated runs average **23.91 FPS**, about **20% higher** than the original. The original
finishes at tick 297 and the updated runs at 300; all have 115 units and 330 corpses. The production
changes improve the first-ten-second 1% low from **3.45 FPS to 10.35–10.48 FPS**. The individual
legacy switches and material-freeze experiment use the same updated binary.

A longer, single 30-second comparison improves from **21.30 to 37.97 FPS** (46.939 to 26.336 ms),
with p99 falling from 92.685 to 68.335 ms. That result includes the quieter period after the battle;
it is not the FPS of the initial pitched battle. The runs end at ticks 891/901 with different
corpse-expiry populations, so the repeated ten-second captures are the cleaner comparison.
Render-stage timings include scheduling/waiting and overlap; they are not additive CPU budgets.

### Changes implemented

1. **Owner-local attachment resolution.** `resolve_wc3_visual_attachments` previously scanned all
   named entities and allocated a normalized name for each, for every pending attachment on every
   frame. Missing or not-yet-ready attachment points repeated that work indefinitely. CPU sampling
   during combat attributed 3.39% of sampled cycles to name normalization alone. The resolver now
   walks only the owner's hierarchy with a reusable stack, excludes the effect's own subtree,
   preserves `Ref`/exact/prefix priority, and retries asynchronous loads. The legacy switch
   restores the global scan and brings back the large frame spikes. Equally ranked duplicate
   names now resolve in hierarchy order instead of ECS query order.
2. **Avoid unchanged ground-splat material writes.** Blood/footprint/other event splats previously
   performed a mutable material lookup and colour write every frame, including constant-colour
   portions of their authored lifecycle. This produces asset modification events. Bevy's
   `PreparedMaterial::prepare_asset` frees and reallocates the material binding even for a change
   confined to uniform data. Read-only colour comparison now suppresses redundant writes while
   retaining the exact sampled colour, alpha, atlas frame and lifetime. The legacy switch raises
   asset preparation from about 6.9 to 11.3 ms/frame and lowers FPS to 21.09.

### Implementation follow-up: persistent terrain-splat material state

The first architectural follow-up now moves terrain-conforming splats off mutable per-instance
`StandardMaterial` assets. Each splat keeps its existing terrain-sampled mesh entity, source-event
transform, authored lifetime and sorted transparent submission. Colour/alpha and atlas-frame UV
selection instead live in a compact shared storage buffer indexed by `MeshTag`; materials are
immutable and shared by texture/blend mode. Released slots are recycled, buffer capacity grows
geometrically, and uploads occur only when a record changes. The current effects manifest uses only
Blend and Additive splat modes; the custom shader preserves both, including additive premultiplication.

A profiling-only `legacy-splat-material-state` switch restores the previous per-splat StandardMaterial
and mesh-UV mutation path in the same release binary. Because the paused tick-0 fixture emits no combat
splats, this comparison uses the active 500-unit first-ten-seconds combat capture. All three runs
finish at tick 300 with 115 living units and 330 corpses; each has two projectiles at capture end.

| Active combat | FPS | Mean frame ms | Main CPU ms | Effects ms | Prepare assets ms | Prepare meshes ms | Specialize ms | Queue ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Buffered splats, run 1 | 29.74 | 33.621 | 18.039 | 2.030 | 2.041 | 1.397 | 1.976 | 2.577 |
| Legacy splat material state | 24.63 | 40.606 | 19.918 | 2.332 | 6.668 | 2.239 | 3.231 | 4.093 |
| Buffered splats, run 2 | 30.42 | 32.869 | 17.254 | 1.941 | 2.176 | 1.279 | 1.885 | 2.566 |

The two production runs average **30.08 FPS / 33.245 ms**, versus **24.63 FPS / 40.606 ms** on the
legacy path: **18.1% lower mean frame time and 22.1% higher FPS**. Asset preparation falls from
6.668 to an average **2.109 ms/frame (68.4%)**. Main-thread CPU falls about **11.4%**, the measured
effects slice about **14.9%**, mesh preparation about **40.2%**, specialization about **40.2%**, and
queueing about **37.2%**. Transparent-pass GPU time averages 2.264 ms versus 2.474 ms legacy. The
render/submit/present scope is noisier and overlaps scheduling/waiting, so it is not used as the main
attribution metric here.

This implements the splat-first portion of persistent material state.

### Implementation follow-up: persistent animated-alpha model state

The next slice targets model-layer alpha tracks while deliberately leaving texture-ID animation on
the reference path. An asset audit across the converted classic model packs found **549 animated
materials: 534 alpha-only, 14 texture-only, and 1 with both alpha and texture-ID tracks**. That makes
alpha-only state the dominant safe slice.

Eligible alpha-only layers now share an `ExtendedMaterial` keyed by their immutable processed
`StandardMaterial` state. Each model-layer entity receives a compact alpha slot via `MeshTag`; the
fragment shader reads that slot from a shared storage buffer immediately after normal StandardMaterial
sampling, then runs the same alpha discard, lighting, unlit, priority-plane and blend-mode logic as the
reference PBR path. Team colour, vertex tint, depth bias, Mask/Blend/Add/Multiply modes and existing
transparent ordering remain part of the immutable base material. Texture-ID animated layers still use
the old cloned StandardMaterial path. Slots are reclaimed when animated-alpha components disappear.

A profiling-only `legacy-animated-alpha-state` switch restores the old per-entity material clone and
base-colour-alpha mutation path in the same release binary. The comparison uses active 500-unit combat,
ten seconds warm-up and ten seconds capture. The final fidelity-corrected shader preserves sampled
texture/vertex alpha by normalizing immutable base-material alpha to one and multiplying the sampled
alpha by the authored animated value. Both production runs and the intervening legacy run end at tick
300 with 115 living units and 330 corpses.

| Active combat | FPS | Mean frame ms | Main CPU ms | Handoff ms | Prepare assets ms | Specialize ms | Queue ms | Render/submit/present ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Buffered alpha, run 1 | 36.62 | 27.305 | 15.428 | 11.788 | 0.264 | 1.554 | 1.854 | 15.060 |
| Legacy animated alpha | 30.80 | 32.464 | 16.888 | 15.478 | 2.117 | 1.973 | 2.476 | 16.697 |
| Buffered alpha, run 2 | 35.95 | 27.817 | 15.615 | 12.111 | 0.264 | 1.551 | 1.902 | 15.336 |

The two production runs average **36.29 FPS / 27.561 ms**, versus **30.80 FPS / 32.464 ms** on the
legacy path: **15.1% lower mean frame time and 17.8% higher FPS**. Asset preparation falls from 2.117
to **0.264 ms/frame (87.5%)**. The overlapping handoff scope falls **22.8%**, world extraction
**8.9%**, specialization **21.3%**, queueing **24.2%**, render/submit/present **9.0%**, and main-thread
CPU **8.1%**. Transparent-pass GPU time falls about **7.1%** while opaque GPU time is unchanged.

The scene census gives the clearest structural signal: production averages **290 material assets and
386 visible mesh/material pairs**, versus **929 materials and 796 visible pairs** on legacy—reductions
of **68.8%** and **51.5%** respectively. Skin-palette staging remains 11.33 MB/frame in both paths, as
expected; this optimization changes material state rather than skeleton work.

### Implementation follow-up: persistent animated texture-ID state

The remaining material-state slice covers the 15 converted materials with texture-ID tracks: 14
texture-only and one combined alpha/texture layer. Some Naga water materials select among as many as
46 authored textures, so a small fixed texture binding set is not sufficient. The production path
instead prebuilds immutable shared material variants for the authored texture IDs and animates by
switching mesh material handles. Texture assets, samplers, blend/depth state, tint, priority plane and
all other material properties stay immutable. The combined alpha/texture case reuses the existing
per-instance alpha buffer and switches among `Wc3AnimatedAlphaMaterial` variants that share that same
alpha storage. Unknown or absent texture IDs retain the authored static texture fallback.

A profiling-only `legacy-animated-texture-state` switch restores the old behavior: each affected mesh
owns a cloned StandardMaterial and mutates its base-colour texture as the track changes. Because the
normal 500-unit battle does not densely exercise these rare tracks, this comparison uses the dedicated
presentation fixture `--stress-visual h02W 500`, which instantiates 500 copies of the converted model
while leaving the simulation paused. The clean baseline and legacy captures both report zero simulation
ticks during the timed ten-second window and exactly **1,000 animated texture tracks**.

| 500 × h02W visual fixture | FPS | Mean frame ms | Main CPU ms | Handoff ms | Prepare assets ms | Specialize ms | Queue ms | Render/submit/present ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Shared immutable variants | 163.64 | 6.111 | 5.047 | 1.000 | 0.068 | 0.131 | 0.112 | 1.548 |
| Legacy mutable materials | 146.34 | 6.833 | 5.136 | 1.639 | 0.532 | 0.176 | 0.152 | 1.771 |

The shared-variant path is **10.6% lower in mean frame time and 11.8% higher in FPS**. Asset
preparation falls **87.2%**, the overlapping handoff scope **39.0%**, specialization **25.6%**, queueing
**26.3%**, and render/submit/present **12.6%**. World extraction falls about **9.0%**. GPU pass timing
is effectively unchanged (transparent 0.635 vs 0.640 ms; opaque 1.457 vs 1.469 ms), which is consistent
with this optimization targeting CPU-side material lifecycle rather than pixel cost.

The structural result is larger than the frame-time delta: material assets fall from **1,179 to 181**
and visible mesh/material pairs from **1,129 to 131**, while transparent draw-function calls remain
essentially unchanged (123 legacy vs 124 production). This confirms that the gain comes from removing
per-instance mutable material state, not from hiding geometry or changing rendered ordering.

Two later production repeats in the pasted benchmark log also land near 161–166 FPS, but their capture
reports contain nonzero simulation ticks despite `--profile-paused`; they are therefore excluded from
the controlled table above. The clean zero-tick pair is the attribution result.

### Implementation follow-up: per-instance attachment indices

Each imported unit/building/corpse model now builds a normalized attachment-node index when its
`WorldInstance` becomes ready. Dynamic effects resolve against that owner-local index, cache both
hits and misses for repeated attachment names, and never walk unrelated scene hierarchies. A
profiling-only `legacy-attachment-index` switch restores the immediately previous owner-local walk,
so this comparison isolates indexing from the older scene-wide attachment-search optimization.

Two production captures bracketed two legacy-index captures on the active 500-unit first-ten-seconds
combat fixture. All runs ended at tick 300/301 with 115 living units and 330 corpses.

| Attachment path | FPS | Mean frame ms | 1% low FPS | p99 ms | Main CPU ms | Scene setup ms |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Indexed, 2-run average | 35.60 | 28.097 | 13.31 | 61.342 | 15.744 | 0.723 |
| Owner-local walk, 2-run average | 35.58 | 28.109 | 12.00 | 64.921 | 15.526 | 0.688 |

Mean frame time is effectively identical (about **0.04% lower** on the indexed path), so this is not
claimed as an average-FPS optimization on the current fixture. The indexed runs do show a better
average 1% low (13.31 vs 12.00 FPS) and p99 (61.3 vs 64.9 ms), but max-frame results move the other
way and the sample is too small to claim a robust tail win. The production value is therefore the
bounded lookup cost and cached misses/repeated names; the retained-effect work below remains the
expected way to reduce the larger scene/material activation spikes.

### Next architectural changes, in priority order

- **Retain and pool effect instances/resources.** Pool frequently spawned effect instances/resources
  to reduce the remaining initial scene/material activation spikes; reset animation, event cursors,
  bindings and lifetime on reuse. Benchmark the transition separately from later combat, since the
  updated worst frame still exceeds 100 ms.
- **One skeleton evaluation and palette per model.** CPU samples also show animation evaluation,
  transform propagation and skinned bounds among the dominant consumers. Updated combat still
  averages about 10.5 ms in PostUpdate and stages 11.3 MB of palettes per sampled frame. The shared
  palette/model-level bounds design above remains relevant. Retain settled corpse poses only when
  their authored phase actually stops changing; do not freeze death or decay sequences or alter
  authoritative corpse lifetime. Measure this after material state is fixed, since the costs overlap.

The release build, client Clippy with warnings denied, formatting, the client test suite and live
captures pass. The material-freeze path is profiling-only and intentionally changes the picture;
owner-local attachment search, unchanged-write suppression, persistent terrain-splat material
state, buffered alpha-only model material state, and immutable texture-ID material variants ship by default.

## Architectural findings from source

### Repeated skeleton work per mesh layer

`wc3-assets/src/export.rs::build_gltf` exports geosets as separate mesh nodes sharing one source
skin. Bevy expands these into separate SkinnedMesh entities. `wc3_effects.rs::fix_wc3_scene_materials`
adds another skinned child for non-building team-colour underlays and requests dynamic bounds for
it. The model's joint list is therefore referenced repeatedly across geosets and layers.

Bevy's `bevy_camera::visibility::update_skinned_mesh_bounds` visits every dynamically bounded skin,
without a pose-change or visibility filter. Its PBR `extract_skins` scans all skin components and
checks changed joints for each registered skin. `prepare_skins` uploads the current staging buffer
every frame, even if a pose did not change. Throttling animation sampling does not eliminate those
costs. Stock animation evaluation also visits animation targets independently of view visibility.

Desktop Bevy DOES support skin batching through storage buffers. Its `no_automatic_skin_batching`
only disables batching on platforms that require uniform buffers. Thus “one skin = one draw” is not
a valid premise for this redesign.

### Rendering state expanded into assets and extra geometry

Originally, animated material alpha/texture tracks cloned a StandardMaterial for each affected mesh
in `fix_wc3_scene_materials`, then modified that asset as its track changed. Alpha-only tracks now use
the shared GPU-record path measured above. Texture-ID tracks switch among immutable shared variants,
and the single combined alpha/texture case shares the alpha buffer across those variants. Distinct
material handles do not necessarily mean separate draw calls because Bevy supports bindless material
slabs, but removing per-instance mutation still avoids asset lifecycle, extraction and preparation
work. Transparent sorting continues to limit which compatible items can be batched.

Geoset visibility is still exported as binary zero/nonzero node scale (`visibility_scale`) for glTF
animation, but the client now translates that authored state into explicit Bevy visibility before
render submission. This is required because Bevy's `mesh.wgsl` SKINNED branch obtains
`world_from_local` from `skinning::skin_model` instead of the mesh transform, so zero mesh scale alone
was not a reliable hide operation. The dedicated per-geoset visibility path measured above removes
both that fidelity ambiguity and needless submission.

### One general-purpose mesh entity per ordinary particle

`emit_wc3_particles` spawns a Mesh3d/StandardMaterial entity for each particle. `update_wc3_particles`
updates its transform and swaps mesh handles for atlas frames and material handles for lifecycle
colour/alpha steps. The material cache builds formatted string keys. This converts cheap particle
parameters into entity churn, geometry/material identity changes, extraction and transparent-phase
sorting. A hidden-particle experiment retains the particle update/emission cost, so its improvement
only attributes the work removed by hiding them.

Ribbons allocate fresh attribute/index vectors and replace a Mesh each frame. Legacy model particles
instantiate entire imported model scenes; those are separate from the ordinary quad-particle test.

### Imported scenes retained as full hierarchies

Doodads, units, corpses and effects all retain general scene graphs. Some composed-model features
walk ancestors repeatedly. The non-inheritance correction pass traverses and writes entire affected
subtrees after normal transform propagation, including unchanged values. Corpse animation lookup
also linearly searches the corpse collection for each unmatched controller. These are real CPU
costs, but their priority should follow measurements rather than imply they explain all frame time.

## Proposed designs

Recommended order: dedicated particle buffers first, shared model palettes and material-layer
programs second, model-level culling and retained-pose lifecycle third. Keep the current path as a
visual reference during each migration. The initial 500-unit scene contains no corpses, so corpse
optimizations require a separate compatible saved-scene benchmark before making performance claims.

### 1. Dedicated particle and ribbon buffers

Represent ordinary particles in compact arrays with persistent capacity. One shared quad and a
per-particle record carry position, size, rotation, atlas rectangle, colour and alpha. Update/upload
contiguous records rather than spawn general scene hierarchies or swap assets. Evaluate sprite
animation and lifecycle colour in the shader where faithful; CPU integration can remain initially.
Replace ribbon mesh recreation with reusable vertex/index or procedural strip buffers.

Partition by blend/depth/texture state. Batch compatible additive particles where ordering permits;
for ordinary alpha preserve back-to-front order and required interleaving with other transparent
geometry. One giant unsorted particle draw is not an acceptable substitute. Keep legacy model
particles on a distinct model-instance path. View culling must account for particle lifetime,
trajectory and trail extent, not only the emitter's current location.

### 2. One model instance, one palette, shared material state

Introduce an immutable, version-scoped ModelDefinition containing geometry sections, material-layer
programs, joint hierarchy, animation clips, attachment bindings and conservative bounds. Runtime
instances carry only stable presentation/SimId mapping, root transform, sequence/time/blend state,
owner colour, visibility mask and a palette offset. Evaluate a skeleton once per visible/dirty model
instance and let all its geosets/layers reference the same persistent GPU palette allocation.
Upload changed ranges; preserve previous-pose data for any motion-vector use.

Compose team-colour underlay plus textured overlay in a single material pass wherever equivalent.
Keep animated alpha, texture selection, owner tint and per-geoset visibility as instance data rather
than cloned assets. Group opaque/masked geometry by compatible mesh/pipeline/material resources;
retain authored priority planes and correct ordering for genuine blend/add/multiply layers. Never
convert all transparency to opaque or mask merely to improve a benchmark.

Start with the material/underlay path and palette sharing, retaining the existing renderer as a
comparison path. A later crowd path can evaluate clips on the GPU or cache quantized poses for
far units, after measuring memory/quality costs. CPU attachment transforms must use the same pose
math for the needed joint ancestor closure; avoid a GPU readback dependency for attachments.

### 3. Model-level culling and pose-aware lifecycle

Cull a conservative model envelope before animation and geoset expansion. Use versioned offline
clip envelopes with conservative interpolation bounds, or a union of conservative transformed bone
bounds; arbitrary sparse animation samples are not proof of containment. Root movement updates the
world envelope cheaply. Recompute pose-dependent bounds once per instance when the pose changes,
then share them across sections and underlays. Exclude truly hidden geosets from submission while
retaining their timeline events and independently visible attachments.

Separate visible, off-screen, and settled-pose presentation states. Off-screen models retain logical
sequence/event clocks but can avoid unnecessary pose evaluation and GPU uploads. Re-entering the view
seeks directly to the correct pose. Settled corpses can retain a cached pose/geometry while their
authoritative identity, lifetime and decay phase remain unchanged. Never shorten gameplay corpse
lifetime to reduce render load. Static doodads should use compact spatial instance groups; animated
or emitting doodads retain only the dynamic components they need.

The freeze-bounds experiment is an attribution tool, not this implementation: it can falsely cull
later poses. Production acceptance requires camera-edge, movement, scale, transition, attachment,
death/decay and resurrection comparisons with the current dynamic-bounds path.

## Acceptance gates

- Compare full-scene frame distribution, extraction/handoff, render stages, pass CPU/GPU timing,
  transparent draw calls, palette bytes and scene populations on the same fixture and camera.
- Re-run paused, moving/combat, corpse-heavy and effect-heavy cases. A 500-unit initial fixture alone
  does not validate the screenshots' corpse workload or every model type.
- Compare animation transitions, team colour, multilayer blending, geoset visibility, texture tracks,
  global sequences, attachments, non-inheritance and particle/ribbon lifetimes visually. Use the
  existing per-entity fidelity checklist when validating affected Warcraft content.
- Keep authoritative snapshots/checksums identical under all production rendering paths. Selection
  and inspection preserve stable identity when rendering instances are pooled or grouped.
- Do not present geometry-hiding gains as shipped optimizations. Every shipped change must preserve
  the intended picture and include its own before/after measurement.

## Validation

The client test suite passes (204 tests); Clippy passes for all client targets with warnings denied;
formatting passes; the release client builds, the original seven bounded investigation profiles
complete, and the measured production renderer changes have repeat or same-binary A/B captures. The
profiling plugin and experiments are only registered in automated profiling mode. Normal gameplay
rendering is changed for textured additive ordinary particles, the narrowly gated static two-layer
unit team-colour case, exported hidden-geoset submission, terrain-splat material state, alpha-only
animated model material state, and texture-ID animated material variants; excluded cases retain their
reference paths. Interactive stress startup
centres the camera, while automated
profiles centre and lock it.
