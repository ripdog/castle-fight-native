# Testing, Observability, and Performance

Status: **normative verification philosophy, provisional budgets/tooling**

## 1. Purpose

The project depends on determinism and high unit-count performance. Both must be continuously verified rather than assumed.

Testing is layered from pure deterministic primitives through full multiplayer/rejoin soak tests.

## 2. Test categories

The project SHOULD maintain distinct suites for:

1. math/ID/RNG unit tests;
2. content validation tests;
3. deterministic simulation rule tests;
4. spatial/navigation behavior tests;
5. targeting/combat/projectile-delivery behavior tests;
6. ability/mana/builder/item behavior tests;
7. snapshot/replay round-trip tests;
8. cross-worker determinism tests;
9. server/client integration tests;
10. reconnect/desync recovery tests;
11. performance benchmarks and long-running soak tests.

## 3. Determinism gate

Determinism is a release-blocking invariant.

Representative fixtures MUST run with multiple worker counts and compare canonical state hashes.

Conceptual test:

```rust
for workers in [1, 2, 4, 8] {
    let result = run_fixture(FIXTURE, workers, 100_000);
    assert_eq!(result.checkpoints, expected_checkpoints);
}
```

Tests SHOULD compare periodic hashes, not only the final state, so the first divergence is localized.

## 4. Golden deterministic fixtures

The repository SHOULD maintain small human-understandable fixtures and large stress fixtures.

Examples:

- two units duel;
- two attackers choose between equal targets;
- cage target soaking;
- building completes a disconnected region;
- cage is destroyed and units leave;
- simultaneous multi-target combat;
- guaranteed-hit ranged target moves during flight;
- ballistic projectile misses original target and hits an entity entering the impact zone;
- deterministic bounce sequence;
- production timing;
- automatic mana-building cast and random-target selection;
- player-targeted legendary artillery captures launch position;
- builder-held offensive item does not provoke targeting of builder;
- map-wide aura item survives snapshot/reload;
- 10,000-unit synthetic lane battle;
- high-density pile/congestion case;
- sustained many-unit convergence on one destination with no committed collision overlap;
- mixed extracted-scale collision radii under sustained opposing flow, including radius-aware topology clearance and pairwise reservation;
- terminal victory state does not advance further production/movement/combat ticks.

Golden hashes may be updated only when an intentional simulation-compatible break/rules change is reviewed and documented.

## 5. Property tests

Property-based testing is strongly encouraged for deterministic primitives and rule invariants.

Examples:

- fixed-point operations remain in validated range;
- target result is unchanged by candidate input permutation;
- spatial grid result set equals brute-force result set;
- snapshot encode/decode preserves canonical state;
- `SimId` allocation never duplicates;
- every completed movement commit preserves the configured live-unit non-overlap invariant;
- building placement/occupancy round-trip agrees;
- deterministic RNG returns same value independent of call order;
- ordinary combat entities cannot be the subject of a player-order command;
- builder presence does not change combat-unit spatial/pathing results;
- ballistic impact result depends on canonical impact-time occupants, not original target identity.

Rust tools such as `proptest` may be evaluated.

## 6. Differential/reference tests

For optimized systems, maintain a slow obviously-correct reference implementation where practical.

Examples:

- spatial query compared against all-entity scan;
- optimized flow-field update compared against full recomputation;
- parallel combat reduction compared against serial canonical resolver;
- compact snapshot loader compared against canonical logical state.

This makes aggressive optimization safer.

## 7. Serial reference mode

The simulation MUST support a one-worker/serial-equivalent execution mode.

This is useful for:

- determinism comparison;
- debugging;
- profiling scheduler overhead;
- easier reproduction;
- validating parallel algorithms against canonical output.

Serial mode should use the same rules, not a separate simulation implementation.

## 8. Replay as regression fixture

A bug reproduced in a real match should ideally become a replay/snapshot regression artifact.

A compact issue fixture can include:

```text
snapshot/seed + canonical stream records + expected checkpoint hashes
```

The test runs headlessly to the failing tick.

This is especially valuable for rare multicore/pathing/combat interactions.

## 9. Navigation/caging tests

Caging is a first-class compatibility invariant.

Tests MUST verify:

- legal final wall placement is accepted;
- trapped units do not escape through topology;
- trapped units remain attackable under ordinary ranged targeting;
- destroying/selling a cage wall updates navigation deterministically;
- navigation cache rebuild does not differ across worker counts.

Any anti-stuck optimization requires specific cage regression tests before merge.

## 10. Targeting tests

Targeting tests MUST deliberately randomize candidate insertion/query ordering while expecting the same selected `SimId`.

Where target rules contain tie-breaks, each term should have focused coverage.

Large randomized worlds can compare optimized target selection against a canonical brute-force selector.

Stable `SimId` ordering is explicitly accepted as the final tie-break for otherwise identical targets/events. Mirrored scenarios remain useful regression coverage, but the engine does not need to add randomized/fairness-neutral tie-breaking solely to remove creation-order bias.

## 11. Snapshot/rejoin tests

Required continuity patterns include:

```text
A: run ticks 0..N uninterrupted
B: run 0..K, snapshot, reload, run K..N
assert state(A,N) == state(B,N)
```

and:

```text
server runs continuously
client disconnects at K
server advances to N
client loads S and fast-forwards
assert client/server checksum at N
```

## 12. Network fault injection

Integration tests SHOULD simulate:

- latency;
- jitter;
- packet/message duplication;
- reordering;
- loss where transport semantics permit;
- abrupt disconnect;
- reconnect;
- stale command retry;
- corrupted client state/checksum mismatch.

The canonical stream must remain unambiguous. Tests MUST include explicit empty-tick finalization, between-tick disconnect/pause/resume records, missing stream-position gaps, duplicate delivery, and snapshot/live handoff overlap.

## 13. Performance philosophy

Performance targets should be empirical and scenario-specific.

The project should benchmark at least:

- ordinary spread-out battle;
- high-density melee pile;
- caged/unreachable-target scenarios that force attack-position filtering and building fallback;
- many long-range attack buildings;
- heavy production/spawn churn including bounded spiral exhaustion;
- repeated building placement/navigation invalidation;
- many disconnected cages/components;
- projectile-heavy battle with guaranteed-hit, ballistic, and bounce deliveries;
- recursive/trigger-heavy effects near the deterministic expansion guard;
- many automatic spellcasting/mana buildings;
- many passive/global item aura sources;
- reconnect fast-forward;
- snapshot generation/serialization.

Average tick time alone is insufficient; high percentiles and worst spikes matter for real-time pacing.

## 14. Primary performance metrics

Simulation telemetry SHOULD expose at least:

```text
current tick
entities by class
units alive
buildings alive
projectiles alive
corpses alive/spawned/expired/consumed
automatic ability evaluations/casts/effects and target candidates examined
currently stunned units, plus average/peak stunned units in status stress probes
active timed stat modifiers, including average/peak movement-modifier instances in modifier stress probes
active aura sources
total tick duration
per-phase duration, including automatic ability evaluation/resolution
spatial candidate queries/counts
navigation rebuild count/duration
navigation-route step count plus A* fallback count/cache-hit count/expanded nodes
movement intents, objective-following intents, and hard-collision-rejected movement intents
target reacquisitions vs retentions
ally-defense query/victim/attacker candidate counts
combat intents/effects
projectile launches/impacts/effects/invalidations, ballistic impact candidates, bounce jumps/candidates, and peak live count
production attempts/spawns/bounded-search failures
structural operations
snapshot generation time/size
replay/catch-up ticks per second
worker utilization where available
```

## 15. Initial performance objective

No normative hardware/unit-count target is set yet.

The first meaningful milestone SHOULD establish a repeatable baseline on named reference client hardware and then set budgets based on measured scaling. Client benchmarking MUST measure both sustained live simulation rate and headless catch-up rate; reconnect design MUST NOT assume that disabling rendering automatically provides enough CPU headroom.

For each reference workload, record catch-up ratio as `headless_ticks_per_second / live_tick_rate`. A ratio at or below 1.0 means replay catch-up cannot reach live state and therefore requires a fresher snapshot or different support target.

The original Castle Fight compatibility target caps total units at **700**, so performance verification SHOULD include 700-unit workloads as the primary compatibility-scale reference. Larger workloads remain architectural stress tests rather than implied gameplay support targets.

A useful early diagnostic stress scenario is **10,000 simultaneously active units**, but it is not a support promise. Minimum hardware and reconnect catch-up requirements will be chosen only after representative prototype measurements exist.

The architecture should then be profiled at larger counts to find the next limiting subsystem.

### 15.1 Automated client quicksave profiling

The development client exposes a bounded profiling run for repeatable investigation of a live
game state. `--profile-quicksave` restores the ordinary offline quicksave before the Bevy app
starts and disables presentation synchronization to vertical refresh. The restored simulation stays
paused during a 5-second presentation warm-up so asset loading, scene instantiation, shader setup,
and other startup work do not contaminate the timed sample or advance the saved game state. It then
unpauses and captures the normal simulation and presentation paths for 10 seconds. The warm-up and
capture durations are configurable and may be fractional; a zero warm-up is supported when startup
behavior itself is the subject of the measurement:

```text
tools/cargo-interactive run --release -p castle-fight-client -- --profile-quicksave
tools/cargo-interactive run --release -p castle-fight-client -- --profile-quicksave --profile-warmup 10 --profile-duration 30
tools/cargo-interactive run --release -p castle-fight-client -- --profile-quicksave-path /path/to/quicksave.json --profile-warmup 0 --profile-duration 2.5
```

The selected map version, release revision, seed, and team size MUST describe the configuration
that created the quicksave; incompatible snapshots fail before the timed run. The mode is offline
and cannot be combined with `--server` or `--stress-units`.

The same bounded capture is available for the generated presentation fixture with
`--stress-units N --profile`. Ordinary `--stress-units N` still runs until the window is closed.
Both modes use a real window with the saved display settings and disable vertical synchronization.
Stress scenes and automated profiling MUST start with the camera focused on the terrain at the map
centre, using the normal initial distance and orientation. Automated captures hold that camera fixed
so pointer edge scrolling, input or portrait tracking cannot change the measured view. Interactive
stress scenes remain freely movable; ordinary matches still start at the local player's builder.
The capture report records the camera position/direction and physical window size.
Rendering investigations can keep the simulation paused for the entire capture with
`--profile-paused`. Presentation clocks, ambient animation and effects still run, as they do during
ordinary simulation pause. Stress profiling also pauses during warm-up to preserve the requested
initial unit population.

`--profile-screenshot PATH` saves the primary game window after completing the capture, then exits.
Screenshot readback/encoding occurs after the measured interval. This is available only in profiling
modes and can be used for visual checks without compositor screenshots or changing the renderer.

The opt-in `--render-experiment` defaults to `baseline`. The experiments below require a profiling
mode and MUST NOT affect the authoritative simulation. They deliberately change presentation for cost attribution:

- `freeze-bounds` keeps each mesh's first available bounds and removes its dynamic skin-bounds
  updates. Those bounds need not contain later poses, so this is not a production culling solution.
- `hide-skinned` hides skinned mesh entities while retaining animation and skeleton processing.
- `hide-particles` hides ordinary quad particles while retaining their emission/update/lifetime
  work; legacy model particles, ribbons and other effects remain.
- `hide-transparent` hides StandardMaterial blended/additive/multiplicative meshes; opaque,
  alpha-masked and alpha-to-coverage geometry remain. Custom materials are not included.
- `legacy-team-color` restores separate team-colour underlay geometry for the static unit
  layers that ordinary rendering composites into a single pass.
- `legacy-geoset-visibility` restores scale-only hiding for authored hidden geosets.
- `legacy-attachment-search` restores the scene-wide name scan for pending visual attachments.
  Ordinary rendering searches only the owning hierarchy, excludes the effect's own subtree,
  and retries unresolved bindings while asynchronous scene creation completes. Authored `Ref`
  names retain priority over exact names and other prefix matches.
- `legacy-splat-updates` restores unconditional material writes for ground splats. Ordinary
  rendering updates the material only when its sampled colour actually changes; authored colour,
  alpha, atlas animation and lifetime remain unchanged.
- `freeze-materials` suppresses model alpha/texture track writes and ground-splat colour writes.
  This deliberately changes the picture to attribute material update costs; splat geometry,
  lifetime and the authoritative simulation continue normally.
- `freeze-poses` sets animation weights to zero immediately before Bevy evaluates animation targets.
  Root motion, logical animation clocks and the simulation continue, but poses and pose-dependent
  attachments are intentionally incorrect. This measures a combined animation/propagation cost,
  not an acceptable production animation policy.
- `no-bindless` disables the device's texture-binding-array feature at renderer creation, exercising
  Bevy's ordinary material-binding fallback. It keeps geometry, skin components, simulation and
  authored visual parameters intact. Profiling logs report whether StandardMaterial actually uses
  bindless resources. This comparison changes batching as well as resource tracking costs.
- `bindless-16` and `bindless-32` use smaller StandardMaterial resource slabs than the default 64.
  These preserve material parameters and compare resource-tracking cost against additional slab
  changes; the renderer's non-bindless fallback remains available.
- `cold-effect-pools` retains completed effects but disables preparation of hidden reserves.
  Ordinary rendering prepares at most 8 roots/frame, 128/template and 1,024 total reserved roots,
  using observed unit-population high-water marks and versioned attack/cast intervals and estimated
  visual/flight lifetimes. Only simulated automatic/proc/Defend abilities, their persistent status
  art and projectile art are candidates. Timed and looping playback have separate reserve keys.
  Persistent art is selected by the applied modifier ID from the versioned ability effect, which
  may differ from the casting ability ID. Status lifetime and cast interval estimate occupancy.
  The finite budget is shared across activation waves before extra concurrent occupancy, so
  stable asset ordering cannot leave whole missile templates unprepared. Existing reservations
  survive population growth. Status instances can return to their looping reserve after expiry;
  ribbon/attachment scenes use
  ordinary cleanup. Free instances wait for scene/WC3/animation setup; node bindings survive acquisition.
  Ribbon scenes receive prepared instances for one use. Source replacement clears free/pending
  reserves and reissues their requests. Reserve counters include warm-up preparation.
- `particle-overlap-audit` estimates particle regrouping opportunities after transparent sorting
  without changing the submitted order. It looks ahead at most 32 items from a compatible run.
- `particle-overlap-batching` applies that prototype: a particle may cross intervening items only
  when its conservative screen rectangle is disjoint from their combined coverage. Billboards use
  their actual four corners; supported ordinary/animated-alpha/team-colour meshes use transformed
  current AABB corners. Two-pixel guards cover edge/MSAA samples. Missing/non-finite bounds,
  eye-plane crossings, morph meshes, ribbons, unknown draws and prebatched model items are ordering
  barriers. Potentially overlapping items retain their sorted precedence. Counters report proposed
  safe moves, overlap rejections and barrier stops; these are search events, not unique draw counts.
- `compact-material` uses render-only proxies for WC3 animated-alpha and team-colour extensions
  whose base material has no image inputs other than base colour. It retains the complete Bevy
  material uniform and delegates material/pipeline properties to StandardMaterial, binding only
  that uniform, base-colour image/sampler and the original WC3 extension data. The PBR input helper
  is derived from the loaded engine shader, specializing only texture branches known to be false
  for eligible materials; unfamiliar shader structure MUST retain the original material path.
  Source handles, animation state, alpha slots, skin/palette identity and main-world setup MUST
  remain unchanged. Unsupported textures and unprepared proxy assets MUST use original materials.
  Source material asset replacement/removal MUST invalidate proxies; substitutions MUST dirty
  pipeline specialization and refresh GPU mesh material bindings. Counters distinguish selected,
  pending and unsupported visible extension instances. This control remains opt-in.
- `particle-shared-view` reuses the exact PBR view layouts and bind groups for compact particles,
  moving their texture group to slot 2. It retains particle sorting and blend operations.
- `particle-shared-mesh` additionally preserves the preceding supported PBR mesh layout in slot 2,
  moving particle textures to slot 3. Prefix selection MUST use final prepared draw representatives
  and associate metadata with the view and particle item identity. The client MUST match the exact
  view/mesh layout and resolve the current mesh bind group through Bevy's normal mesh command; it
  MUST NOT rewrite skin indices or retain GPU skin buffers. Model, lightmapped, skin/motion and morph
  layout variants are supported
  on storage-buffer devices with four bind-group slots. Uniform-offset paths, unknown draws/layouts,
  absent preceding meshes and asynchronously compiling variants MUST use the shared-view fallback.
  Draw order, batch membership, shaders' particle math and blend operations remain unchanged.
- `particle-shared-mesh-partial-bindings` combines that layout prefix with the populated-prefix
  texture arrays of `particle-partial-bindings`, retaining fully padded arrays on unsupported devices.
- `particle-cull` skips compact particles with exactly zero alpha or whose conservative billboard
  sphere is outside the view frustum. It retains emission, simulation and lifetime work. Missing
  frustum data keeps the particle; far-plane rejection is disabled conservatively.
- `particle-partial-bindings` binds only the populated prefix of each particle texture array when
  the device enables `PARTIALLY_BOUND_BINDING_ARRAY`. Instance texture indices MUST stay inside
  that prefix. Unsupported devices retain fully padded arrays. No texture or particle is omitted.

- `particle-uncached-bindings` recreates the compact particle texture bind groups every frame.
  Ordinary rendering retains groups while their ordered GPU texture-view/sampler identities and
  binding counts remain unchanged. Changed, missing, or replaced GPU resources invalidate the
  corresponding group; unused slabs release their cached groups. Both the single-texture fallback
  and padded/partial array paths use the same cache policy. Counters report groups created/reused.

The shared-view, culling, partial-binding and overlap particle experiments remain opt-in; none
changes the default particle ordering/blending policy. The overlap prototype still requires image
parity and broader bounds validation before production adoption. Particle queue
counters report candidate/queued counts, culling rejections and populated/bound texture slots.

Experiments apply throughout warm-up and capture. Their differences are not additive cost budgets:
removing geometry also changes visibility, batching, GPU work and pipeline overlap. Compare the
same save, build, view, resolution and capture duration, and repeat baseline runs.

Automated profiling also reports CPU time around render-world extraction/handoff and render
schedule stages, a scene census, sampled transparent draw-function calls after batching (including
particle/mesh counts, particle batch sizes and pipeline/command transitions), and recent
CPU/GPU pass diagnostics. Main-thread handoff includes extraction and any wait for the render thread;
these overlapping scopes MUST NOT be summed with main or GPU time. Pass diagnostics are recent
rolling samples, not averages over the entire capture. Mesh/material pairs and skin counts are not
draw-call counts. Transparent transition counts MUST reset at each view and exclude its first
binding; they describe submitted phase order, not GPU execution time. A `SYSTEM CPU` report with no
samples does not imply zero engine-system cost; per-system tracing requires Bevy's tracing feature.

```text
tools/cargo-interactive run --release -p castle-fight-client -- --stress-units 500 --profile --profile-paused --profile-warmup 10 --profile-duration 10
tools/cargo-interactive run --release -p castle-fight-client -- --profile-quicksave --profile-paused --render-experiment freeze-bounds --profile-warmup 10 --profile-duration 10
```

The stdout report includes average FPS, 1% low FPS, average/p95/p99/maximum frame and simulation
tick times, average main-schedule and presentation phase costs, collision-fallback work, tick range,
final entity counts, source path, and actual capture duration. The 1% low is the reciprocal of the
mean frame time in the slowest one percent of captured frames (rounded up to at least one frame).
Frame, simulation, and presentation samples come from the same counters as the in-game performance
panel so interactive and automated investigations measure the same work.
The report also groups complete frame samples into approximately one-second windows with FPS,
p95/maximum frame time and the percentage over the 16.67 ms budget, and lists the five slowest
frames with their main-schedule timings. Windows use accumulated captured frame durations; a frame
is never split across windows. A long frame can cross multiple nominal second boundaries.
For 700-unit acceptance, distinguish the initial living population, active battle and settled-corpse
periods. A battle average after mass casualties MUST NOT be presented as sustained 700-living-unit
performance. Include the combat activation hitch, not only the later steady state.
Profiling also records newly added `WorldAssetRoot` requests by asset, including peak requests in a
frame, first observation time and source-template entity counts. These are requests, not confirmed
completed instantiations; unavailable templates report zero entities. Reusing an existing pooled
root does not count as a new request. The census is scoped to capture and excludes paused warm-up.

## 16. Tick budget

If the final simulation rate is 20 Hz, the real-time wall-clock budget is 50 ms/tick; at 30 Hz it is ~33.3 ms/tick.

The server SHOULD target substantial headroom below that budget rather than merely meeting it under average load, because topology changes, mass deaths/spawns, and snapshots create spikes.

Exact budget percentages are open pending benchmark data.

## 17. Scaling tests

Benchmarks SHOULD include the compatibility-scale ceiling and then sweep larger architectural stress points, e.g.:

```text
700
1k
2k
5k
10k
20k
50k
```

where feasible, and record phase scaling.

They should also sweep worker counts:

```text
1
2
4
8
16...
```

This reveals both algorithmic complexity and parallel efficiency.

A phase that stops scaling should be profiled rather than automatically given more threads.

## 18. Profiling

Linux profiling support is important.

The project SHOULD remain friendly to tools such as:

- `perf`;
- flamegraphs;
- Tracy or equivalent frame/tick tracing if adopted;
- Bevy diagnostics where useful;
- allocator profiling;
- custom phase counters/spans via `tracing`.

Release-like optimized builds with debug symbols should be easy to produce for profiling.

## 19. Structured tracing

Server/client logs SHOULD use structured tracing with fields such as:

```text
match_id
player_id where appropriate
tick
phase
SimId where appropriate
command sequence
snapshot tick
checksum
```

High-volume per-unit tracing MUST be disabled by default and selectively filterable.

Diagnostic logging MUST NOT change simulation behavior.

## 20. Desync diagnostics

When a checksum mismatch occurs, development/debug infrastructure SHOULD capture:

- last matching checkpoint;
- first differing checkpoint;
- server/client top-level hash;
- hierarchical subsystem/component hashes;
- command stream around divergence;
- worker count/platform/build metadata;
- optional canonical state dump.

The goal is to answer "which authoritative datum first differed?" rather than merely reporting "desync".

## 21. Invariant checking

Debug/test builds SHOULD optionally validate expensive invariants after selected phases/ticks:

- unique `SimId`;
- valid references;
- health/value ranges;
- occupancy consistency;
- spatial index membership consistency;
- navigation cache generation matches topology generation;
- no forbidden floating-point authoritative components;
- no entity simultaneously live and pending incompatible structural states.

Production builds may reduce these checks for performance.

## 22. Fuzzing

Protocol parsers, snapshot decoders, content loaders, and command validators are good fuzzing targets because they consume untrusted or semi-trusted structured input.

Fuzzing authoritative simulation commands can also uncover invalid state transitions.

## 23. CI expectations

CI SHOULD eventually gate on:

- format/lint;
- unit/property tests;
- deterministic fixture hashes;
- snapshot/replay tests;
- representative networking/reconnect integration tests;
- at least a small performance-regression smoke benchmark where stable runners permit it.

Large soak/performance sweeps may run separately/nightly.

## 24. Performance regressions

Optimization commits must preserve deterministic/behavioral fixtures.

Performance claims SHOULD include reproducible benchmark command/scenario and before/after measurements.

An optimization that changes game outcomes is a gameplay change, not merely an optimization, and must be specified/reviewed as such.
