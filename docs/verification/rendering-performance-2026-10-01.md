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
| — | Earlier 700-unit baseline, bracketing mean | prior series | 700 combat | 46.80 / 21.372 | — | — | 2026-10-01 raw |
| — | Partially populated particle texture arrays | candidate | 700 combat | 50.18 / 19.932 | 46.80 / 21.372 baseline | -6.7% | 2026-10-01 raw |
| — | Shared PBR view bindings | candidate, smaller gain | 700 combat | 48.74 / 20.519 | 46.80 / 21.372 baseline | -4.0% | 2026-10-01 raw |
| — | Conservative particle culling | candidate, low value | 700 combat | 47.25 / 21.165 | 46.80 / 21.372 baseline | -1.0% | 2026-10-01 raw |
| — | Hide ordinary particles | destructive attribution | 700 combat | 67.21 / 14.879 | 50.45 / 19.823 baseline | -24.9% | 2026-10-01 raw |
| — | Freeze poses | attribution/control | 700 combat | 50.42 / 19.832 | 50.45 / 19.823 baseline | +0.05% | 2026-10-01 raw |
| 13 | Persistent particle texture bindings | shipped, 85458c4; gain inconclusive | 700 combat | 42.63 / 23.457 | 44.71 / 22.365 uncached | single noisy pair | 2026-10-01 follow-up raw |
| 14 | Prepared timed/looping effects and retained missiles | shipped, de46e1b | 700 combat | 43.69 / 22.887 | 43.24 / 23.126 | -1.0% | 2026-10-01 follow-up raw |
| — | Stricter reserve readiness confirmation | shipped, ebaaea2 | 700 combat | 43.98 / 22.736 | — | validation only | 2026-10-01 follow-up raw |
| — | Partial particle arrays with prepared reserves | candidate | 700 combat | 45.94 / 21.767 | 43.69 / 22.887 | -4.9% | 2026-10-01 follow-up raw |
| — | Conservative overlap regrouping | prototype, small draw reduction | 700 combat | 42.44 / 23.561 | 43.69 / 22.887 | +2.9% | 2026-10-01 follow-up raw |
| — | StandardMaterial slabs 16 / 32 | rejected default change | 700 combat | 44.12 / 22.666; 42.93 / 23.294 | 44.30 / 22.575 timed-reserve mean | no repeatable gain | 2026-10-01 follow-up raw |

The progression rows are not one continuous synthetic benchmark: fixture changes are explicit above,
and each percentage is relative only to its recorded control. In particular, the h02W texture test
must not be compared numerically with combat runs. The two changes in ba3d546 were measured with
independent same-binary legacy switches; their gains are not additive.

## Current conclusions

The target is not met. The latest same-binary bracketing comparison with prepared reserves averages
43.69 FPS / 22.887 ms, versus 43.24 FPS / 23.126 ms with reserve preparation disabled. The earlier
46.8 FPS series is historical; compare each optimization with its own contemporary control. The latest partial particle
array capture reaches 45.94 FPS / 21.767 ms, and remains opt-in pending exact parity and broader device
validation. A final readiness-guard confirmation reaches 43.98 FPS / 22.736 ms with a 47.045 ms
worst frame and all 1,024 reserves prepared. Native single-texture fallback successfully retained/reused all eight sampled texture
bindings; one device/path does not establish complete fallback coverage.

Prepared hierarchies remove cold resurrection, Defend and missile roots from the first activation
burst. Final reserves also select persistent status art by the applied modifier ID, which can differ
from the casting ability ID. One-shot and looping Stand instances have distinct keys. Hidden roots
finish WC3/material/animation/emitter setup before entering a reserve; explicit guards wait for
material processing and emitter-node binding as well as scene/animation readiness. Authored emissions/events are
suppressed, and acquisition rewinds clocks/cursors while preserving node bindings and full skins.
Reserves grow from population high-water marks and versioned concurrency estimates, with budgets of
8 new roots/frame, 128/playback template and 1,024 total. The finite budget covers activation
waves across templates before allocating extra concurrent occupancy. Ribbon/attachment scenes get
prepared one-use instances; shortages retain ordinary spawning. Scene replacement invalidates free/pending
reserves. The default bracketing runs have worst frames of 43.252 and 47.588 ms,
versus 90.378 ms with cold reserves (47–52% lower worst-frame time). SpawnScene in the cold
activation frame costs 64.327 ms; the corresponding prepared activation frames cost 1.915 and
2.065 ms. Residual startup Update still reaches 20–24 ms, so activation remains over budget.

Particle/model submission remains the main steady-state opportunity: roughly 2,500–2,600 draw
functions in sampled late frames. A bounded conservative overlap search found 322 safe moves in its
dry audit; its paired applying capture removed only about 3% of sampled draws without a clear
throughput win. In the final series it reached 42.44 FPS / 23.561 ms (+2.9% frame time versus the
bracketing mean), with 2,437 sampled draws versus 2,523–2,583 in default captures. Those are different
sampled frames, not an exact draw saving. The prototype stays opt-in. It preserves overlapping
precedence, uses guarded billboard/current mesh bounds, and treats unknown, multi-instance, ribbon and morph draws as barriers. Its broader
bounds/fidelity validation remains unfinished.

Oversized resource arrays remain costly on the tested RADV driver. Default StandardMaterial slabs
stay at 64; fresh 16/32 captures showed no repeatable gain. Partial particle arrays bind only the
populated prefix (eight textures in the measured sample). Stable particle groups are now cached by
ordered GPU view/sampler identity, bound count and fallback identity, including single-texture mode.
This eliminated repeated group creation in steady sampled frames, but the isolated early cache
comparison was noisy and does not establish a throughput gain. Source-material normalization also
avoids unchanged writes, and inherited sequence clocks are borrowed instead of cloning their names
for each event/emitter lookup.

Animation was expensive in prior CPU samples (target evaluation 14.1% of sampled cycles, descendant
propagation 9.25%), yet freezing poses changed 700-unit throughput essentially not at all. Reassess
it after submission and activation churn are lower. The fixture still rapidly loses living units;
these averages do not prove 60 FPS with 700 continuously living attackers or 60 FPS throughout combat.

## Next measured work

1. Reduce faithful transparent submission/resource tracking cost. The current 32-item overlap
   prototype has limited value; measure whether larger safely bounded opportunities exist before
   expanding it. Keep potentially overlapping precedence and unknown-draw barriers.
2. Validate partial particle binding arrays across unsupported-feature/device paths and exact image
   parity when visual verification is requested; promote only after that evidence exists.
3. Address remaining cold sources identified by the final census, especially resurrected Rifleman
   unit hierarchies and event-spawned children. Unit reserves need owner/team/material/animation
   setup and full skin identity, not timed-effect reuse assumptions. Re-measure whole-battle tail
   latency and reserve growth during ordinary live production as well as paused warm-up fixtures.
4. Compile immutable WC3 node/material/event metadata once per source and cache shared rest bounds.
   Existing node/material extras are still deserialized per new instance. Keep clocks, cursors, GPU
   slots, emitter counters and child ownership per instance; invalidate caches on asset replacement.

No further tests were run after the user's instruction to skip them. Follow-up validation used
formatting, Clippy with warnings denied, release builds and native frame/census captures. Early
particle-cache tests ran before that instruction. Full reports, including intermediate rejected
reserve-selection policy and controls, are in the raw capture file.

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

Do not exhaust the finite reserve budget in asset-path order: long-lived status occupancy can
otherwise displace whole missile waves and move the activation hitch to a different template.

Do not infer that the SpawnScene hitch is shader compilation. Pooling prepared instances is the
direct measured direction because it avoids both hierarchy copying and repeated setup; broader
pipeline warm-up can be tested separately.

## Relevant code

Primary client areas are particle_renderer.rs (particle records, phase queuing and bindings),
wc3_effects.rs (effect events, animation clocks and WC3 scene setup), and
presentation.rs::spawn_or_reuse_timed_wc3_visual (timed-effect pooling). The shared-palette design,
if revisited, needs changes around Bevy 0.19.1 PBR skin allocation/extraction rather than another
late per-instance skin-index patch.
