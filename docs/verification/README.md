# Verification Stage

This directory records reproducible evidence for the early architecture before full gameplay/content is built.

## Current slice

Implemented:

- Rust 2024 workspace pinned to Rust 1.98.1;
- authoritative `bevy_ecs` simulation state;
- integer/fixed-subunit 2D positions;
- monotonic `SimId` identity;
- deterministic fixed ticks and canonical checksums;
- explicit building footprints as hard navigation topology;
- connected-component reachability and shared objective distance fields;
- uniform-grid broad phase partitioned by team + navigation component for melee targeting;
- sticky individual target acquisition with retaliation-on-attack behavior;
- reachable attack-position filtering and passive-building fallback around cages;
- deterministic greedy pursuit with A* fallback around blockers;
- deterministic crowd steering from immutable movement intents plus a hard non-overlap reservation/commit pass;
- melee attack cooldown/damage resolution;
- spawn-tick attack suppression and death-before-later-actions ordering;
- production buildings with deterministic bounded expanding-spiral spawn search;
- failed spawn attempts are lost rather than backlogged;
- dedicated Rayon worker pool configurable per simulation instance;
- cross-worker determinism tests;
- phase-level tick timing diagnostics;
- open-lane, dense-cage, and crossing-crowd release benchmarks;
- Bevy debug viewer using procedural placeholder units, building footprints, and target-link gizmos;
- a separate playable verification game with mirrored production-building placement and procedural placeholder visuals.

Not implemented yet:

- richer unit collision shapes / physically stronger crowd response beyond the current hard circle-distance exclusion;
- builder control/items;
- building attacks, mana, automatic abilities, or legendary abilities;
- ranged/ballistic/bounce delivery;
- air/ground movement and attack classes;
- invisibility/invulnerability/status effects;
- snapshots/networking;
- production art/assets.

The benchmark remains an architectural scaling probe, not a final game performance claim.

## Verification commands

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p castle-fight-sim-bench -- \
  --scenario lane,cage,crowd --units 1000,5000,10000 \
  --workers 1,2,4,8 --warmup 5 --ticks 20
```

The benchmark exits non-zero if different worker counts produce different final canonical checksums for the same fixture.

Run the placeholder viewer with the cage workload:

```bash
cargo run -p castle-fight-debug-viewer
```

or the open-lane fixture:

```bash
cargo run -p castle-fight-debug-viewer -- --scenario lane
```

The viewer is presentation-only. Disabling or changing it must not change simulation checksums.

## Playable verification game

Run:

```bash
cargo run -p castle-fight-verification-game
```

The harness intentionally contains only enough game structure to exercise the simulation interactively:

- map: 2,000 × 750 world units;
- player base: left third;
- enemy base: right third;
- center third is blocked above/below a centered 350-unit-high lane;
- one castle is centered in each base;
- **left click** in the player base queues a melee production building;
- **right click** queues a ranged production building;
- every player placement is mirrored horizontally into the enemy base;
- production starts after 10 seconds and repeats every 10 seconds;
- both unit types have 10 HP and deal exactly 1 damage every 30 simulation ticks (1 DPS at 30 Hz);
- melee and ranged attacks are shown as short-lived source→target lines;
- building/unit art is entirely procedural placeholder geometry.

This is deliberately **not** the production input/game-rule layer. It bypasses the builder, resources, network command scheduling, and normal construction UI so we can rapidly generate symmetric battles and inspect pathing, crowd behavior, targeting, production, and combat. That bypass does not change the normative builder-only control rules in `docs/spec`.

The current ranged verification attack uses the simulation's guaranteed-hit ranged targeting semantics but resolves damage immediately at the attack tick; authoritative projectile travel/interpolation is a later combat-delivery verification slice.

## Initial baseline — 2026-09-12

Hardware:

- AMD Ryzen 5 5600
- 6 cores / 12 hardware threads
- reported max clock ~4.7 GHz

Software:

- Rust 1.98.1
- Bevy / `bevy_ecs` 0.19.1
- release profile with thin LTO and one codegen unit

The first minimal open-lane fixture, before building topology, measured 10,000 units at 16.658 ms/tick on one worker and 5.468 ms/tick on eight workers with identical checksums. This established that the deterministic parallel phase model scaled before harder gameplay was added.

## Topology/cage baseline — 2026-09-12

The second verification slice deliberately introduced a pathological cage fixture: half the units are densely packed inside a disconnected building enclosure while the opposing half are close enough that a naive spatial-radius query can enumerate the trapped population.

Before reachability partitioning, the 10,000-unit cage case spent almost the entire tick rejecting unreachable targets:

| Units | Workers | ms/tick | Targeting ms/tick | Final checksum |
| ---: | ---: | ---: | ---: | --- |
| 10,000 | 1 | 213.579 | 209.697 | `c17f2d7efcc674e2` |
| 10,000 | 8 | 44.291 | 40.439 | `c17f2d7efcc674e2` |

The fix was architectural rather than additional brute-force parallelism: melee target buckets are partitioned by hostile team and navigation connected-component. Units outside a cage therefore never enumerate units in the disconnected interior.

After that change, with the **same fixture and same canonical checksum**:

| Scenario | Units | Workers | ms/tick | Spatial ms | Targeting ms | Movement/commit ms | Checksum ms | Final checksum |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| lane | 10,000 | 1 | 4.114 | 0.723 | 0.965 | 0.850 | 1.448 | `75615e490b189e22` |
| lane | 10,000 | 8 | 3.461 | 0.684 | 0.280 | 0.870 | 1.467 | `75615e490b189e22` |
| cage | 10,000 | 1 | 6.198 | 0.672 | 2.241 | 1.542 | 1.479 | `c17f2d7efcc674e2` |
| cage | 10,000 | 8 | 4.373 | 0.653 | 0.511 | 1.542 | 1.446 | `c17f2d7efcc674e2` |

This is roughly a **34x improvement** in single-worker cage targeting time and about **79x** on eight workers versus the naive disconnected-candidate scan. It also demonstrates why spatial partitioning and data reduction matter more than simply adding worker threads.

At this point checksum generation itself is a visible part of the synthetic tick budget (~1.45 ms at 10,000 units). That is acceptable for verification but suggests production builds should not necessarily compute a full canonical checksum every live tick; periodic checkpoints or incremental/subsystem hashes should be evaluated later without weakening determinism testing.

## Crowd-separation baseline — 2026-09-12

The original crowd fixture placed two dense opposing populations into an intentionally interpenetrating central region. That was useful while crowd handling was only soft separation, but it is not a legal game state once non-overlap is a hard invariant. The current crowd fixture therefore starts as two legally packed opposing formations and drives them into one another. Movement is computed from one immutable snapshot, a data-parallel pass applies bounded deterministic steering, and a final deterministic reservation/commit pass rejects overlapping final positions.

| Units | Workers | ms/tick | Crowd separation ms/tick | Final checksum |
| ---: | ---: | ---: | ---: | --- |
| 1,000 | 1 | 0.843 | 0.450 | `f11921aedc30f294` |
| 1,000 | 8 | 0.583 | 0.176 | `f11921aedc30f294` |
| 5,000 | 1 | 5.321 | 2.930 | `5156a03137d21ba7` |
| 5,000 | 8 | 2.982 | 0.966 | `5156a03137d21ba7` |
| 10,000 | 1 | 10.574 | 6.057 | `ed78153b189f4ee3` |
| 10,000 | 8 | 5.382 | 1.711 | `ed78153b189f4ee3` |

Those historical numbers predate the hard collision invariant and are retained only as an implementation baseline. The old intentionally overlapping initial state is no longer considered a valid gameplay benchmark.

A later regression sweep after crowd separation was enabled for every combat unit and the checksum projection was expanded to include unit attack/movement profiles produced the following 10,000-unit totals:

| Scenario | 1 worker | 8 workers |
| --- | ---: | ---: |
| lane | 7.766 ms/tick | 5.440 ms/tick |
| cage | 18.640 ms/tick | 6.588 ms/tick |
| crowd | 10.786 ms/tick | 5.651 ms/tick |

The cage fixture is now dominated by crowd separation rather than targeting. It remains comfortably inside a 30 Hz tick budget on the eight-worker reference run, but extreme enclosed density is the first place to revisit if richer collision rules materially increase cost.

## Engagement/defense clarification — 2026-09-12

The targeting verification now includes canonical one-tick defense alerts. An actual hit can cause idle nearby allies, or allies fighting a target that is not fighting them back, to switch to the attacker on the following targeting phase. Mutual engagements remain sticky, direct self-retaliation outranks ally defense, passive castles/buildings are defenceless targets, and the alert survives a lethal hit so nearby allies may still react to the killer.

Crowd steering also now adds a deterministic lateral sidestep when a moving unit is queued directly behind a stationary engagement. A mirrored three-on-three melee-column fixture verifies that the front pair remain engaged while rear units leave the centerline and eventually reach attack range rather than forming a permanent queue.

A 10,000-unit regression sweep after these rules were enabled remained worker-count deterministic:

| Scenario | 1 worker | 8 workers |
| --- | ---: | ---: |
| lane | 8.011 ms/tick | 5.657 ms/tick |
| cage | 19.732 ms/tick | 8.543 ms/tick |
| crowd | 11.486 ms/tick | 6.190 ms/tick |

The defense-alert spatial query is deliberately lazy: units already in a mutual engagement do not query nearby ally alerts. The pathological cage case nevertheless shows a measurable increase because many units are fighting passive/unreciprocating targets; this is recorded as a future optimization target rather than hidden behind additional threading.

## Hard non-overlap and terminal-match regression — 2026-09-12

Leaving the playable verification game running after castle destruction exposed two separate issues. First, the client continued stepping the simulation after victory, so production never stopped and surviving units continued following the now-ownerless static objective field. Second, crowd separation was only a bounded steering force and therefore could not guarantee non-overlap under sustained compression.

The verification match now becomes terminal immediately after the victory phase: the final state remains visible, but no further production, movement, combat, or placement ticks advance. Simultaneous castle loss resolves as a draw in this harness.

Movement now has a hard deterministic collision commit after the parallel steering pass. Proposed positions are reserved in stable unit order using a dense intrusive spatial grid; an overlapping move is rejected or replaced with a legal local sidestep, and a legal state must always finish the movement phase with non-overlapping live combat-unit collision footprints. A dedicated fixture drives 100 friendly units toward one objective for 300 ticks and checks every pair after every commit.

The previous tiny-cage and interpenetrating-crowd stress fixtures intentionally created states that are no longer legal under the hard collision invariant. They remain useful historical evidence, but the active `cage` and `crowd` benchmarks now start from legal non-overlapping layouts; the cage enclosure was enlarged to hold its population physically.

After replacing an initial hash-map reservation prototype (~15–23 ms/tick of collision work at 10k) with the bounded dense intrusive grid, the current 10,000-unit legal-state regression is:

| Scenario | 1 worker | 8 workers |
| --- | ---: | ---: |
| lane | 9.460 ms/tick | 7.766 ms/tick |
| cage | 13.820 ms/tick | 9.122 ms/tick |
| crowd | 10.765 ms/tick | 8.749 ms/tick |

The hard collision commit is intentionally canonical and currently sequential, so it reduces worker scaling compared with pure soft steering. It costs roughly a few milliseconds at 10,000 units on the reference machine while enforcing a gameplay invariant that soft repulsion cannot guarantee. This is acceptable for the verification stage but remains a clear optimization target if realistic matches approach these densities.

### Production/congestion panic regression

A later long-running playable match exposed a panic in the hard commit (`legal simulation state had no non-overlapping unit position`). The root cause was production placement: it rejected only an occupied **navigation cell**, so a spawn at the center of an empty cell could still be inside the collision radius of a unit standing near the edge of a neighboring cell. Dense production could therefore inject an already-invalid position into the movement phase. Collision was also unnecessarily partitioned by navigation component, even though physical bodies exist regardless of path connectivity.

The fix makes production use the same real collision-distance reservation test as movement, makes physical unit collision global across navigation components, and lets emergency collision repair search the reachable map rather than assuming a free point exists within one navigation cell. The dense reservation grid was simplified accordingly so its hot path no longer stores or compares component IDs.

Regression coverage now includes the neighboring-cell spawn case, collision across disconnected topology, and a verification-game stress run with 50 production buildings for 1,800 ticks. That run builds a few hundred units converging on one objective and finishes without a panic or pairwise collision violation.

The 10,000-unit regression after this fix remains worker-count deterministic:

| Scenario | 1 worker | 8 workers |
| --- | ---: | ---: |
| lane | 9.812 ms/tick | 8.105 ms/tick |
| cage | 13.796 ms/tick | 7.828 ms/tick |
| crowd | 9.556 ms/tick | 7.360 ms/tick |

## Current interpretation

The core deterministic architecture remains viable under deliberately hostile topology and crowd workloads. The next meaningful risks are repeated topology mutations, arbitrary-target pursuit/A* fallback frequency, production churn, and attack/projectile/ability density. Each should receive a deliberately adversarial benchmark before broader game content is built.
