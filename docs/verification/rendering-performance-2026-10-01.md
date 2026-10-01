# Rendering performance investigation — consolidated

Last consolidated: 2026-10-01. Current target: a faithful 60 FPS presentation at the original
game's roughly 700-unit population.

This is the canonical renderer-performance note. It intentionally keeps only benchmark history,
current conclusions, rejected approaches that are important not to repeat, and the next measured
work. Detailed profiler output remains in the raw capture files:

- [2026-09-28 raw captures](rendering-performance-2026-09-28-results.txt)
- [2026-09-28 combat captures](rendering-combat-performance-2026-09-28-results.txt)
- [2026-09-29 raw captures](rendering-performance-2026-09-29-results.txt)
- [2026-10-01 raw captures](rendering-performance-2026-10-01-results.txt)

Do not append long investigation diaries here. When a new optimization is measured, add one row to
the benchmark ledger, update the current conclusions/next work if the result changes them, and keep
the full report in a raw-results file.

## Benchmark fixtures

All comparison runs use a visible 3840×2160 window, AutoNoVsync, release builds, and a centred
locked camera unless the row says otherwise. Builds and CPU sampling do not overlap captures.

**500 paused** is the original 500-unit tick-0 presentation fixture: 10 s paused warm-up + 10 s
capture. **500 combat** uses the same population/camera but captures 10 s of live combat after a
10 s paused warm-up. **h02W visual** is 500 copies of that model with simulation paused, used only
for texture-ID animation. **700 combat** is the current 700-unit target fixture with 10 s paused
warm-up + 10 s combat.

The combat fixtures rapidly lose living units, so they measure activation, battle, effects and
corpse rendering rather than a sustained field of 500/700 living attackers. The 700-unit run starts
with 700 living units, reaches 330 living / 370 corpses in the first telemetry interval, and ends
near 121 living / 529 corpses. A 60 FPS result there would still not prove 60 FPS with 700
continuously living units.

Timed render scopes overlap and must not be summed. Draw counts mentioned below are sampled frames,
not capture-wide averages.

## Benchmark ledger

"Reference" is the same-binary legacy/control path where one exists. Percentages are reductions in
mean frame time, which is the least misleading compact comparison. Rows use means when repeated
captures were recorded. The raw files above retain every individual run and every profiler field.

| Step | Change / experiment | Status | Fixture | Result FPS / ms | Reference FPS / ms | Mean-frame change | Source |
| ---: | --- | --- | --- | ---: | ---: | ---: | --- |
| 0 | Initial controlled renderer baseline | historical baseline | 500 paused | 46.51 / 21.503 | — | — | 2026-09-28 raw |
| 1 | Buffered additive ordinary particles | shipped, 396367f | 500 paused | 51.99 / 19.237 | 47.75 / 20.944 | -8.2% | 2026-09-28 note/raw |
| 2 | Single-pass static unit team colour | shipped, 1384d6f | 500 paused | 59.72 / 16.751 | 52.03 / 19.218 legacy | -12.8% | 2026-09-28 note |
| 3 | Explicit exported geoset visibility | shipped, 4c05a22 | 500 paused | 67.61 / 14.793 | 56.52 / 17.694 legacy | -16.4% | 2026-09-28 note |
| 4 | Owner-local attachment resolution | shipped, ba3d546 | 500 combat | 23.91 / 41.830 | 23.00 / 43.473 global search | -3.8% | 2026-09-28 combat raw |
| 5 | Suppress unchanged splat-material writes | shipped, ba3d546 | 500 combat | 23.91 / 41.830 | 21.09 / 47.415 unconditional writes | -11.8% | 2026-09-28 combat raw |
| 6 | Persistent buffered terrain-splat state | shipped, 1a5fb45 | 500 combat | 30.08 / 33.245 | 24.63 / 40.606 legacy | -18.1% | 2026-09-28 note |
| 7 | Buffered animated-alpha model state | shipped, 978ee12 | 500 combat | 36.29 / 27.561 | 30.80 / 32.464 legacy | -15.1% | 2026-09-28 note |
| 8 | Immutable shared texture-ID variants | shipped, 36dfb7f | h02W visual | 163.64 / 6.111 | 146.34 / 6.833 legacy | -10.6% | 2026-09-28 note |
| 9 | Per-instance attachment-node index | shipped, f575ae2 | 500 combat | 35.60 / 28.097 | 35.58 / 28.109 owner-local walk | -0.04% | 2026-09-28 note |
| 10 | Retained timed-effect instances | shipped, 4744b1c | 500 combat | 36.39 / 27.486 | 35.03 / 28.549 legacy | -3.7% | 2026-09-28 note |
| — | Shared skin-palette alias hack | **reverted**, cfa78f5 → f319148 | 500 combat | ~0.4% frame-time gain | prior path | ~-0.4% | 2026-09-29 note |
| 11 | Cap StandardMaterial bindless slabs at 64 | shipped, 7929ff6 | 500 combat | 46.32 / 21.589 | 36.17 / 27.656 Bevy Auto | -21.9% | 2026-09-29 raw |
| 12 | Compact dedicated billboard renderer | shipped, 5e15f47 | 500 combat | 52.49 / 19.054 | 46.54 / 21.488 Mesh3d particles | -11.3% | 2026-09-29 note/raw |
| — | Disable bindless StandardMaterial | attribution/control | 500 combat | 44.24 / 22.605 | 37.73 / 26.509 baseline | -14.7% | 2026-09-29 raw |
| — | Freeze poses | attribution/control | 500 combat | 39.43 / 25.362 | 37.73 / 26.509 baseline | -4.3% | 2026-09-29 raw |
| — | Freeze bounds | attribution/control | 500 combat | 38.70 / 25.841 | 37.73 / 26.509 baseline | -2.5% | 2026-09-29 raw |
| — | Hide ordinary particles | destructive attribution | 500 combat | 75.56 / 13.234 | 37.73 / 26.509 baseline | -50.1% | 2026-09-29 raw |
| — | 700-unit current baseline, bracketing mean | current baseline | 700 combat | 46.80 / 21.372 | — | — | 2026-10-01 raw |
| 13? | Partially populated particle texture arrays | candidate | 700 combat | 50.18 / 19.932 | 46.80 / 21.372 baseline | -6.7% | 2026-10-01 raw |
| — | Shared PBR view bindings | candidate, smaller gain | 700 combat | 48.74 / 20.519 | 46.80 / 21.372 baseline | -4.0% | 2026-10-01 raw |
| — | Conservative particle culling | candidate, low value | 700 combat | 47.25 / 21.165 | 46.80 / 21.372 baseline | -1.0% | 2026-10-01 raw |
| — | Hide ordinary particles | destructive attribution | 700 combat | 67.21 / 14.879 | 50.45 / 19.823 baseline | -24.9% | 2026-10-01 raw |
| — | Freeze poses | attribution/control | 700 combat | 50.42 / 19.832 | 50.45 / 19.823 baseline | +0.05% | 2026-10-01 raw |

The progression rows are not one continuous synthetic benchmark: fixture changes are explicit above,
and each percentage is relative only to its recorded control. In particular, the h02W texture test
must not be compared numerically with combat runs. The two changes in ba3d546 were measured with
independent same-binary legacy switches; their gains are not additive.

## Current conclusions

The target is not met. On the controlled final 700-unit series, the current renderer averages about
46.8 FPS / 21.37 ms. Partially populated particle bindings are the best small measured follow-up at
50.18 FPS / 19.93 ms, but they remain opt-in pending visual parity and fallback/device validation.

Particle submission is still the dominant steady-state opportunity. The compact renderer removed
most per-particle ECS/material preparation cost, but strict interleaving with transparent model
layers still leaves roughly 2,500 draw-function calls in a representative late 700-unit frame.
Oversized resource arrays are also expensive on the tested RADV driver: 64-slot StandardMaterial
slabs beat Bevy's 2,048-slot Linux Auto policy, and particle partial binding improves the current
64-entry padded particle group by binding only its populated prefix (8 actual textures in the
measured sample).

Combat activation is a separate tail-latency problem. The current 700-unit capture repeatedly shows
the worst frame around 88–90 ms. Roughly 64–65 ms of that frame is SpawnScene; the following frame
spends about 40–41 ms in Update. A later ~2.05 s burst again spends about 18–19 ms in SpawnScene.
This survives the particle-binding experiment, so steady submission work alone cannot remove it.

The cold-root census identifies concrete burst sources. In the measured 10 s capture: Gryphon
missiles requested 242 new roots (peak 50/frame, 27 entities/template); Resurrect caster and target
each requested 50 roots (peak 36/frame, 78 and 49 entities/template); Mortar missiles requested 53
(peak 50, 36 entities/template); Inner Fire targets 53 (peak 50, 28 entities/template); Defend caster
50 (peak 50, 23 entities/template); and a later Rifleman burst requested 18 roots (peak 15,
80 entities/template). These are root requests, not individually timed completed spawns.

Animation is expensive in CPU samples but is not currently a throughput priority. A combat sample
put animation-target evaluation at 14.1% of sampled cycles and descendant propagation at 9.25%, yet
freezing poses changed the final 700-unit throughput essentially not at all. Reassess it after
submission and activation churn are lower.

## Next measured work

1. Validate partial particle bindings on fallback/device paths and with image parity; promote only if
   it remains faithful.
2. Prewarm/retain the high-burst resurrection and missile effect templates first. Hidden reserves
   must be allowed to finish WorldAsset spawning and WC3 setup before entering the pool. Size pools
   from concurrent occupancy/lifetime, not only one-frame request peaks.
3. Instrument how often non-overlapping transparent items split otherwise compatible particle/model
   runs. Only if the count is material, prototype overlap-aware reordering that preserves precedence
   for every potentially overlapping item and treats unknown/side-effecting draws as barriers.
4. Cache stable particle texture bind groups and compile immutable WC3 event-playback metadata once
   per source model. Re-measure before spending effort on a larger animation architecture.

Scene preparation also repeats avoidable immutable work: node/material extras are deserialized per
instance, source material normalization can trigger redundant changes, and Bevy may rescan immutable
mesh vertices for rest AABBs on newly spawned shared meshes. Prefer prepared per-model metadata and
per-mesh cached rest bounds; keep clocks, cursors, GPU slots, emitter counters and child ownership
per instance. Asset reload/replacement must invalidate caches.

## Approaches not to repeat

The reverted shared-palette implementation shortened each mesh's SkinnedMesh and patched the GPU
skin index after extraction. It produced only about a 0.4% mean-frame improvement and caused
exploding team-colour geometry plus instance-dependent missing resurrection geometry. Any future
palette sharing must be owned by the Bevy skin allocator/render path (mesh membership → shared
palette allocation) while preserving full main-world skin identity, previous-frame palette history,
all bind-group users, layer creation, visibility and pool lifecycles.

Do not globally reorder Alpha/Add/Multiply transparency. A forced premultiplied combined pipeline
reduced draw count but regressed badly to 35.56 FPS / 28.124 ms with 15.062 ms in
render/submit/present. Reordering is only acceptable when conservative coverage proves items cannot
interact.

Do not treat particle culling, frozen bounds, frozen poses, hidden geometry, or hidden particles as
production wins. They are attribution controls unless a faithful implementation is separately
designed and measured. Do not disable wgpu validation/resource initialization to recover binding
overhead.

Do not infer that the SpawnScene hitch is shader compilation. Pooling prepared instances is the
direct measured direction because it avoids both hierarchy copying and repeated setup; broader
pipeline warm-up can be tested separately.

## Relevant code

Primary client areas are particle_renderer.rs (particle records, phase queuing and bindings),
wc3_effects.rs (effect events, animation clocks and WC3 scene setup), and
presentation.rs::spawn_or_reuse_timed_wc3_visual (timed-effect pooling). The shared-palette design,
if revisited, needs changes around Bevy 0.19.1 PBR skin allocation/extraction rather than another
late per-instance skin-index patch.
