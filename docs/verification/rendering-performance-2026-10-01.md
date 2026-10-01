# Rendering performance investigation — consolidated

Last consolidated: 2026-10-02. Current target: a faithful 60 FPS presentation at the original
game's roughly 700-unit population.

This is the canonical renderer-performance note. It intentionally keeps only benchmark history,
current conclusions, rejected approaches that are important not to repeat, and the next measured
work. Detailed profiler output remains in the raw capture files:

- [2026-09-28 raw captures](rendering-performance-2026-09-28-results.txt)
- [2026-09-28 combat captures](rendering-combat-performance-2026-09-28-results.txt)
- [2026-09-29 raw captures](rendering-performance-2026-09-29-results.txt)
- [2026-10-01 raw captures](rendering-performance-2026-10-01-results.txt)
- [2026-10-02 binding-prefix captures](rendering-performance-2026-10-02-results.txt)
- [2026-10-02 compact-material captures](rendering-performance-2026-10-02-compact-material-results.txt)
- [2026-10-02 compiled-event captures](rendering-performance-2026-10-02-event-metadata-results.txt)

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
| — | Retained alpha-slot ownership and submission census | shipped, a5010e4 / 4f91fd1 | 700 combat | 44.90 / 22.274 | — | correctness/attribution; no gain claimed | 2026-10-01 discovery raw |
| — | Compatible particle/model mesh-layout prefix, A/B mean | implemented, opt-in; no repeatable gain | 700 combat | 44.56 / 22.441 | 44.59 / 22.427 default A/B/C mean | +0.06% | 2026-10-02 raw |
| — | Mesh-layout prefix + partial particle arrays, A/B mean | opt-in; initial gain did not repeat | 700 combat | 42.68 / 23.431 | 45.34 / 22.058 partial-only A/B mean | +6.2% | 2026-10-02 raw |
| — | Compact WC3 alpha/team material bindings, A/B/C mean | implemented, opt-in; no repeatable gain | 700 combat | 47.21 / 21.180 | 47.20 / 21.188 default A/B/C/D mean | -0.04% | 2026-10-02 compact raw |
| — | Shared compiled event metadata, A/B/C mean | implemented, opt-in; CPU reduction, no throughput gain | 700 combat | 45.92 / 21.777 | 46.20 / 21.644 default A/B/C/D mean | +0.61% | 2026-10-02 event raw |

The progression rows are not one continuous synthetic benchmark: fixture changes are explicit above,
and each percentage is relative only to its recorded control. In particular, the h02W texture test
must not be compared numerically with combat runs. The two changes in ba3d546 were measured with
independent same-binary legacy switches; their gains are not additive.

The 2026-10-02 means cover the planned control series: three default and two of each experiment.
Their FPS values are reciprocals of mean frame time. An additional initial mesh-prefix validation
capture reached only 36.24 FPS / 27.593 ms; it is retained in the raw file separately from the planned
series and supplies no evidence of a gain. Every capture, including slow repeats, is retained.
The separate compact-material series alternates four defaults with three candidate captures, plus
one accepted validation capture (47.93 FPS / 20.863 ms) outside those means. Its initial shader-import
failure is explicitly rejected: missing draws invalidate the reported 50.85 FPS. The full failed
report remains in its raw file; all eight corrected captures finish without shader/GPU errors.
The event-metadata series also alternates four defaults with three candidate captures, with one
validation capture outside the means (45.62 FPS / 21.922 ms). Four separate CPU-sampled captures
are excluded from timing means; stack recording dropped data in both paths, so attribution uses
replacement 99 Hz self-cycle samples with zero lost samples. All twelve native event-series captures
complete without shader/GPU errors, and every report remains in the event raw file.

## Current conclusions

The target is not met. The latest alternating same-binary default captures reach 45.54–46.61 FPS /
21.455–21.960 ms, averaging 21.644 ms. Shared compiled event metadata averages 21.777 ms (+0.61%):
no throughput gain. Candidate p95 is 33.710–34.014 ms versus 32.839–34.017 ms in defaults; p99 is
36.885–39.051 ms versus 37.958–39.328 ms. The lower candidate p99 range does not establish a
consistent tail win, with overlapping worst frames and a 40.561 ms p99 in its extra validation.
Update scope falls from 4.954 to 4.051 ms (-18.2%, about 0.903 ms), supported by lower sampled event
CPU work. That is useful CPU headroom, but cannot be treated as a frame-time saving. Keep the cache
opt-in and move to per-primitive skin-influence specialization. These newer default timings do not
establish a gain against earlier series; compare controls from the same binary and sequence.

The preceding compact-material series averages 21.180 ms against 21.188 ms in its defaults (-0.04%).
It likewise remains opt-in; neither material-bindings prototype establishes repeatable throughput.

In the preceding binding-prefix series, default captures average 22.427 ms and mesh-prefix A/B
captures average 22.441 ms (+0.06%). Shared-view controls vary from 21.780 to 24.586 ms, so gains
against a slow control are inconclusive. Combined mesh-prefix/partial arrays average 6.2% slower
than partial arrays alone. Both binding-prefix controls also remain opt-in.

The earlier prepared-reserve comparison averages 43.69 FPS / 22.887 ms versus 43.24 FPS /
23.126 ms with reserve preparation disabled. Its partial-array capture reaches 45.94 FPS /
21.767 ms; the new partial-only repeats average 22.058 ms. Partial arrays remain opt-in pending
broader device/fidelity validation. Earlier 46.8 FPS captures are historical; compare each change
with its contemporary controls. The alpha-slot ownership fix is a correctness change, with no
throughput gain claimed. Native single-texture fallback retained/reused all eight sampled texture
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
Retained disabled hierarchies now keep their animated-alpha GPU slots: Bevy's default query filtering
previously made those owners appear absent and recycled slots still referenced by their meshes.
Earlier prepared-reserve captures predate this correction and do not validate alpha-state fidelity.

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

The new submission census finds 2,466–2,488 calls, split into 1,167–1,176 particle and 1,299–1,312
mesh/other calls in two late sampled frames. They use only 20–21 distinct pipelines but change
pipelines 2,308–2,309 times, including 1,842 particle/mesh transitions. Nearly every draw switches
pipeline; cached texture-group creation alone cannot eliminate this cost. A source audit of the
initial roster's 12 unique models finds 48 materials, all with only a base-color texture. That
supports investigating compact faithful material bindings, while preserving lighting, alpha modes,
team layers, skinning and runtime material features. It is not a measured FPS improvement.
The new `particle-shared-mesh` experiment keeps compatible PBR groups 0/1/2 and moves particle
textures to group 3. It selects final prepared draw representatives by view + particle item identity,
matches exact engine layouts, and resolves current mesh bindings through Bevy's normal command.
It supports model, lightmapped, skin/motion and morph storage layouts; unsupported uniform-offset
paths, missing/unknown layouts and compiling variants retain the shared-view draw. No skin identity,
palette, particle math, blending, sorting or batch membership changes. All 12 native captures finish
without GPU validation errors; the new paths use 1,198–1,315 compatible particle batches and zero
fallback batches in their sampled final frames. Other device/layout paths and visual parity remain
unverified. This implements the binding-prefix part of the first candidate, not a compact replacement
for StandardMaterial. The planned mesh-prefix captures raise prepare-resources CPU time to
4.312–4.496 ms from 3.873–3.965 ms in defaults; transparent-pass CPU samples also show no repeatable
reduction. These overlapping timings and different sampled frames do not establish causal costs,
but the whole-frame measurements give no reason to promote or expand this prototype.

`compact-material` now targets the non-bindless WC3 animated-alpha and team-colour extensions.
Render-only proxies bind the complete StandardMaterial uniform, base-colour texture/sampler and
original WC3 extension data in four bindings. Main-world handles, alpha-buffer ownership, full skin
identity and ordinary StandardMaterial's bindless path stay unchanged. Eligibility rejects every
additional image input, including feature-gated fields. Source material events replace/invalidate
cached proxies; unprepared proxies retain original materials, and substitutions explicitly refresh
mesh material bindings and pipeline specialization. The loaded Bevy PBR helper is reused with only
known-false texture branches specialized away; lighting, scalar properties, UV transforms, culling,
alpha modes and extension calculations are retained. The final sampled frames select 707–709
extension meshes with zero pending/unsupported fallbacks. All eight corrected captures validate
without shader/GPU errors, but neither whole-frame nor tail timing improves repeatably. Keep this
shader/proxy path opt-in; broader pass/device, replacement and visual parity verification remains
unfinished. Do not spend further effort expanding either binding experiment without new evidence.

A prior 99 Hz CPU stack sample attributes 16.74% of sampled cycles to the largest animation-target
symbol, 7.34% to descendant propagation, 3.52% to skinned bounds, 3.14% to wgpu render-pass encoding
and 1.92% to event crossings. These are exclusive CPU-cycle shares across threads, not frame-time
budgets. Its preceding 199 Hz sample lost 5.58% of events and is less useful for attribution.
Authored roster metadata contains 5,300 event objects across the initial 700 copies; 57,550 of
63,050 event/sequence windows (91.3%) contain no timestamps.

`compiled-event-tracks` now shares parsed composed-node extras and immutable event specifications
by exact content, compiles relative sequence/global event phases, and caches the selected window in
an instance-local cursor. Empty windows still advance that cursor. Original key ordering, duplicates,
endpoints, crossing arithmetic, global fallback and reset-on-reuse behavior are retained. Changed or
removed extras replace/remove event runtimes; changed content selects fresh metadata, while asset
paths/handles remain resolved per instance. Clocks, sound counters and child ownership remain local;
clock lookup still follows the current hierarchy each frame. Other metadata readers are unchanged.
The final census finds 757 unique node-content sources, 108,517–108,567 cumulative cache hits,
119 tracks and 1,176/1,304 empty compiled windows; these cover prepared effects and doodads as well
as units, so they are not the weighted roster counts above. Metadata entries are retained for the
process; repeated hot reloads can accumulate obsolete content. Broader replacement/reuse behavior
has not been exercised. Keep this focused prototype opt-in; do not expand it on source counts alone.

The new 99 Hz CPU self-cycle samples record 8,735 baseline / 8,445 candidate samples with no losses.
Event crossings fall from 3.39% to 0.10% of sampled cycles; inherited clock lookup remains
1.36% / 1.82%. One sample per path supports lower event CPU work, not a precise speedup or
frame-time prediction. The independent timing series' Update reduction agrees with that finding,
while whole-frame measurements remain flat. Its two earlier stack-recording samples dropped data
and are explicitly rejected for attribution. Do not compare the percentages across sampling methods
and binaries as a progression.

Animation was expensive in prior CPU samples (target evaluation 14.1% of sampled cycles, descendant
propagation 9.25%), yet freezing poses changed 700-unit throughput essentially not at all. Reassess
it after submission and activation churn are lower. The fixture still rapidly loses living units;
these averages do not prove 60 FPS with 700 continuously living attackers or 60 FPS throughout combat.
Conservative rest-channel pruning removes zero of the roster's 6,878 channels across its 12 models
(the entire unit pack has only 1,792 removable channels out of 91,887). Deprioritize that avenue.
An independent geometry audit finds 346,700 of 539,600 weighted roster vertices use one influence
(64.3%), but only 105,650 vertices (19.6%) belong to whole primitives whose highest nonzero weight
slot is one. Source Bevy skinning evaluates all four slots. Per-primitive one/two/three-slot shader
specialization is a new candidate; retain full palettes, all nonzero weights, normal transforms,
previous-frame skinning and every material/pass path. These authored counts exclude visibility,
material-layer copies and actual vertex invocations; they predict neither GPU savings nor FPS.

## Next measured work

1. Prototype per-primitive skin-influence specialization behind a profiling control, preserving
   full skin identity, all nonzero weights, normals, previous-frame skinning and every material/pass
   path. Compare GPU pass timing and whole-battle frame tails, not asset counts alone.
2. Address remaining cold sources identified by the final census, especially resurrected Rifleman
   unit hierarchies and event-spawned children. Unit reserves need owner/team/material/animation
   setup and full skin identity, not timed-effect reuse assumptions. Re-measure whole-battle tail
   latency and reserve growth during ordinary live production as well as paused warm-up fixtures.
3. Revisit remaining immutable WC3 material/node metadata and clock-owner caching when CPU work
   limits throughput. The event prototype reduces CPU work without an FPS gain. Preserve initial,
   time-zero, global, skipped-frame, looping and reuse behavior; keep clocks/cursors/GPU slots,
   emitter counters and child ownership per instance. Invalidate metadata on asset replacement and
   cached owners on reparenting/reinstancing; bound obsolete-source retention before promotion.
4. Validate partial particle binding arrays across unsupported-feature/device paths and exact image
   parity when visual verification is requested; promote only after that evidence exists.
5. Revisit transparent submission/resource tracking only with new driver/submission evidence.
   Compatible mesh-layout prefixes and compact WC3 extension bindings both lack a repeatable
   whole-frame gain. The 32-item overlap prototype also has limited value; measure larger safely
   bounded opportunities before expanding it. Keep overlapping precedence and unknown-draw barriers.

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

Primary client areas are compact_material.rs (material-binding experiment), particle_renderer.rs
(particle records, phase queuing and bindings),
wc3_effects.rs (effect events, animation clocks and WC3 scene setup), and
presentation.rs::spawn_or_reuse_timed_wc3_visual (timed-effect pooling). The shared-palette design,
if revisited, needs changes around Bevy 0.19.1 PBR skin allocation/extraction rather than another
late per-instance skin-index patch.
