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
- deterministic attack-envelope pursuit with relative-closing crowd anticipation, canonical bypass continuity, per-unit authoritative collision radii, radius-aware topology/objective fields, and a hard non-overlap reservation/commit pass; melee/ranged groups can flow around occupied unit/building attack regions without assigned slots;
- melee attack cooldown/damage resolution;
- authoritative `RangedGuaranteedHit` projectiles with integer travel time, retained target identity, source-death independence, and deterministic target-death invalidation;
- authoritative `RangedBallistic` projectiles with fixed captured destinations, integer travel time, post-movement hostile splash queries, and canonical projectile/target effect ordering;
- authoritative `Bounce` projectiles with persistent chain identity, bounded hit history, integer travel/falloff, and keyed deterministic subsequent-target selection;
- attack-capable buildings with independent target/cooldown state, footprint-based static acquisition, shared canonical unit/building combat ordering, and ordinary projectile delivery;
- automatic spellcasting buildings with authoritative integer mana, cooldown/cast-sequence state, keyed deterministic random enemy-unit targeting, canonical map-wide enemy-unit targeting, atomic cast commitment, immediate non-retaliatory damage, and timed stun effects;
- authoritative timed stun state with exclusive absolute expiry, max-expiry refresh, and exact suppression of fresh targeting, ordinary attacks, and intentional movement while active;
- a bounded timed movement-speed modifier verification primitive with stable modifier identity, exact expiry, same-ID refresh, distinct-ID additive stacking, and derived integer effective movement speed without mutating authored base stats;
- authoritative corpse entities with stable identity/source metadata, explicit corpse definitions, optional tick-exact expiry, direct- and production-spawn corpse profiles, canonical checksumming, and exclusion from ordinary unit targeting/collision/building occupancy;
- projectile/targeting/ability/status-density diagnostics including live/peak projectiles, launches/impacts/effects/invalidations, ballistic impact candidates, bounce jumps/candidates, automatic evaluations/casts/effects/candidates, current/average/peak stunned units, active/average/peak timed movement modifiers, target retentions/changes, and ally-defense candidate counts;
- spawn-tick attack suppression and death-before-later-actions ordering;
- production buildings with deterministic bounded expanding-spiral spawn search;
- failed spawn attempts are lost rather than backlogged;
- dedicated Rayon worker pool configurable per simulation instance;
- cross-worker determinism tests;
- phase-level tick timing diagnostics split across topology, timers, production, spatial rebuild, automatic abilities, targeting, combat, movement intent, collision/commit, post-movement ballistic impact, structural commit, and checksum;
- pursuit diagnostics for total pursuit steps, deterministic A* fallback frequency, fallback-cache hits, and expanded A* nodes;
- open-lane, dense-cage, crossing-crowd, mixed-radius collision, adversarial pursuit, repeated-topology-mutation, production-churn, guaranteed-hit projectile-density, ballistic splash-density, bounce-chain-density, long-range attack-building, automatic-spellcasting, global-stun/status-density, and long mixed-combat release benchmarks;
- Bevy debug viewer using procedural placeholder units, building footprints, and target-link gizmos;
- a separate playable verification game with mirrored production-building placement and procedural placeholder visuals.

Not implemented yet:

- richer non-circular unit collision shapes / physically stronger crowd response beyond the current authoritative circle-distance exclusion;
- builder control/items;
- broader imported buff/debuff/aura semantics beyond the narrow movement-speed verification primitive, manual/legendary ability activation, and multi-ability buildings;
- corpse-query/consume effects such as raise dead and corpse explosion, plus imported per-unit corpse-profile assignment;
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
  --scenario lane,cage,crowd --units 700,1000,5000,10000 \
  --workers 1,2,4,8 --warmup 5 --ticks 20

# Architecture-risk probes (use smaller counts first for deliberately adversarial pathing)
cargo run --release -p castle-fight-sim-bench -- \
  --scenario pathing,topology,production --units 100,500,1000 \
  --workers 1,8 --warmup 2 --ticks 10

# Guaranteed-hit projectile/targeting density
cargo run --release -p castle-fight-sim-bench -- \
  --scenario projectile --units 700,1000,5000,10000 \
  --workers 1,8 --warmup 2 --ticks 20

# Ballistic post-movement splash density
cargo run --release -p castle-fight-sim-bench -- \
  --scenario ballistic --units 700,1000,5000,10000 \
  --workers 1,8 --warmup 2 --ticks 20

# Bounce chain / candidate density
cargo run --release -p castle-fight-sim-bench -- \
  --scenario bounce --units 700,1000,5000,10000 \
  --workers 1,8 --warmup 2 --ticks 20

# Long-range attack-building density
cargo run --release -p castle-fight-sim-bench -- \
  --scenario tower --units 700,1000,5000,10000 \
  --workers 1,8 --warmup 2 --ticks 20

# Automatic spellcasting / random-target density
cargo run --release -p castle-fight-sim-bench -- \
  --scenario ability --units 700,1000,5000,10000 \
  --workers 1,8 --warmup 2 --ticks 20

# Global timed-stun/status density
cargo run --release -p castle-fight-sim-bench -- \
  --scenario stun --units 700,1000,5000,10000 \
  --workers 1,8 --warmup 2 --ticks 20

# Timed movement-modifier density
cargo run --release -p castle-fight-sim-bench -- \
  --scenario slow --units 700,1000,5000 \
  --workers 1,8 --warmup 2 --ticks 20

# Two-minute mixed verification battle at the compatibility ceiling
cargo run --release -p castle-fight-sim-bench -- \
  --scenario mixed --units 700 --workers 1,8 --warmup 30 --ticks 3600
```

The benchmark exits non-zero if different worker counts produce different final canonical checksums for the same fixture.

## Compatibility-scale ceiling — 700 units

The original Castle Fight compatibility target caps total units at **700**, so 700-unit cases are now first-class benchmark points. The 1k/5k/10k workloads remain deliberate architecture stress probes rather than implied gameplay support targets.

On the Ryzen 5 5600 reference machine, the current attack-envelope/objective-flow solver keeps the ordinary 700-unit fixtures far below a 30 Hz tick budget even on one worker: lane `1.187 ms/tick`, cage `1.230 ms/tick`, and crossing crowd `1.149 ms/tick`. The corresponding eight-worker runs are `0.979`, `1.010`, and `0.993 ms/tick`. These numbers include the full canonical checksum every tick. Hard collision rejects only `0.26%`, `0.05%`, and `0.14%` of movement intents respectively. The current per-unit objective tie distribution and proposed-final collision commitment intentionally change authoritative movement outcomes, and worker-count checksums match for every fixture (`0286150dd84dff49`, `1c8ca2c16f869459`, and `a7fdc182ca65fd23` respectively).

Focused client-scale regressions now cover the observed 3D movement failures. A ranged unit entering behind an occupied firing line reaches range without repeated side-to-side reversal; the same continuity rule applies to melee congestion. A 12-unit melee group converging on one unit target produces the same result on one/eight workers while filling multiple sides of the legal attack region. A 24-unit melee group approaching a 7×7-cell building gets at least 20 distinct units into legal attack positions across at least three building faces, with no perimeter-slot assignment. A ranged unit spawned well inside its maximum range remains at that closer legal distance and attacks rather than backing away to the outer boundary.

The intentionally extreme delivery-density fixtures also have substantial headroom at 700 total units, despite every attacker being allowed to launch every tick:

| Delivery stress | 1 worker ms/tick | 8 workers ms/tick | Peak live projectiles | Extra density work | Final checksum |
| --- | ---: | ---: | ---: | --- | --- |
| Guaranteed-hit | 1.634 | 1.898 | 4,000 | 535 impacts/tick | `e33e6a41bdbed0dc` |
| Ballistic splash | 2.657 | 3.016 | 4,198 | 525.1 impacts and 6,746.9 effects/tick | `c6077c6b4df6ecac` |
| Bounce chain | 4.472 | 4.733 | 7,170 | 1,442.7 extra jumps/tick, 104.39 candidates/jump | `b3157f85a005905c` |

These numbers are **full simulation wall-clock time per tick**, including the deliberately expensive full canonical checksum performed every tick by the benchmark. They put the larger 5k/10k results in context: those larger cases are useful for finding eventual architectural limits, but the current delivery implementations are not near the compatibility-scale unit ceiling's real-time budget.

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
- both unit types have 10 HP and launch/deal exactly 1 damage every 30 simulation ticks (1 DPS at 30 Hz before travel delay);
- melee attacks are shown as short-lived source→target lines;
- ranged units launch authoritative guaranteed-hit projectiles at 10 world units/tick (300 world units/sec at 30 Hz), and the viewer interpolates the live authoritative projectile population toward each retained target;
- building/unit art is entirely procedural placeholder geometry.

This is deliberately **not** the production input/game-rule layer. It bypasses the builder, resources, network command scheduling, and normal construction UI so we can rapidly generate symmetric battles and inspect pathing, crowd behavior, targeting, production, and combat. That bypass does not change the normative builder-only control rules in `docs/spec`.

The ranged verification attack now uses authoritative guaranteed-hit travel. Launch creates canonical projectile state with source/target IDs, damage, launch position, launch tick, and due impact tick. Moving targets remain hit at the scheduled tick; source death does not cancel an already-launched projectile; a target that has already died or been removed before impact causes deterministic projectile invalidation instead of retargeting.

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

The targeting verification now includes canonical one-tick defense alerts. The first implementation allowed ally-defense alerts to pre-empt a valid current target when that target was not fighting back; later interactive verification showed that rule could make idle melee units revolve rapidly between different nearby attackers. That behavior is superseded by the sticky-target regression described below. Defense alerts still survive lethal hits so an idle nearby ally may react to the killer.

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

## Pursuit fallback, topology mutation, and production churn — 2026-09-12

Adding explicit fallback instrumentation immediately exposed a correctness and scaling problem in arbitrary-target pursuit. The previous implementation took one deterministic A* step when greedy pursuit hit a local minimum, then resumed ordinary greedy pursuit on the next tick. Around a straight wall this could select the previous cell because it was geometrically closer to the target, producing a two-cell oscillation. A sustained wall-detour regression now advances repeatedly until the target is reached rather than checking only the first fallback step.

The verifier now uses a greedy step only when repeated deterministic greedy descent can be proven to reach the requested target cell. Otherwise it uses canonical A*. Exact fallback results are cached by `(source cell, target cell)` and the cache is cleared on every topology rebuild. The cache is deliberately derived/performance-only: capacity or eviction can change work performed but cannot change the selected canonical next cell.

The first adversarial `pathing` benchmark deliberately placed opposing units across a long wall so every pursuit required a detour. Before caching, fallback was the dominant cost:

| Units | Workers | ms/tick | Movement intent ms/tick | A* fallback rate | A* expanded nodes/tick | Final checksum |
| ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 100 | 1 | 8.905 | 8.788 | 100% | 166,971.6 | `16de9b1972445c23` |
| 100 | 8 | 1.759 | 1.632 | 100% | 166,971.6 | `16de9b1972445c23` |
| 500 | 1 | 32.163 | 31.599 | 100% | 567,409.2 | `40a9e48ae36313e5` |
| 500 | 8 | 6.159 | 5.694 | 100% | 567,409.2 | `40a9e48ae36313e5` |
| 1,000 | 1 | 70.041 | 68.822 | 100% | 1,200,125.1 | `8cd598c576fde6ad` |
| 1,000 | 8 | 15.212 | 14.091 | 100% | 1,200,125.1 | `8cd598c576fde6ad` |

With exact-pair fallback caching, the same fixtures produced the **same canonical checksums** while removing most repeated searches:

| Units | Workers | ms/tick | Movement intent ms/tick | A* cache-hit rate | A* expanded nodes/tick | Final checksum |
| ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 100 | 1 | 0.154 | 0.053 | 99.40% | 559.0 | `16de9b1972445c23` |
| 100 | 8 | 0.145 | 0.031 | 99.40% | 559.0 | `16de9b1972445c23` |
| 500 | 1 | 2.240 | 1.660 | 94.54% | 26,650.3 | `40a9e48ae36313e5` |
| 500 | 8 | 0.858 | 0.378 | 94.54% | 26,650.3 | `40a9e48ae36313e5` |
| 1,000 | 1 | 3.629 | 2.485 | 95.99% | 40,792.9 | `8cd598c576fde6ad` |
| 1,000 | 8 | 1.431 | 0.551 | 95.99% | 40,792.9 | `8cd598c576fde6ad` |

At 1,000 units this reduces one-worker movement-intent time by about **27.7x** and A* node expansion by about **29.4x**. The adversarial fallback rate remains 100%, which is intentional for this fixture; the important result is that sticky pursuit no longer repeats the same global search each tick. Maps with rapidly changing target cells or topology can still defeat this cache and remain candidates for cached target fields/local search if realistic verification shows that pattern.

The `topology` benchmark separately forces a building removal/rebuild every tick while 10,000 ordinary lane units remain active. Full connected-component and both objective-field rebuilds currently cost about 0.7 ms/tick on the reference map:

| Units | Workers | ms/tick | Topology ms/tick | Final checksum |
| ---: | ---: | ---: | ---: | --- |
| 10,000 | 1 | 10.119 | 0.725 | `e1c6a790567ade` |
| 10,000 | 8 | 8.488 | 0.707 | `e1c6a790567ade` |

A separate cage-opening regression seals a 24-unit group behind a complete building wall, verifies that the group remains stationary while disconnected, removes one gate building, observes the topology rebuild, and verifies that at least half the group begins leaving on the next tick. This covers the semantic reaction to topology change while the benchmark above isolates repeated rebuild cost.

The `production` benchmark creates up to 100 production buildings that attempt to spawn every tick into bounded two-cell search regions. At the 100-building scale, congestion makes most attempts fail as intended, but the production phase remains small:

| Scale arg | Workers | ms/tick | Production ms/tick | Spawns/tick | Failed attempts/tick | Final checksum |
| ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1,000 | 1 | 0.167 | 0.023 | 5.50 | 4.50 | `8b8d0e6d645e21be` |
| 1,000 | 8 | 0.177 | 0.023 | 5.50 | 4.50 | `8b8d0e6d645e21be` |
| 5,000 | 1 | 0.550 | 0.073 | 15.70 | 34.30 | `66f28f520639f55f` |
| 5,000 | 8 | 0.539 | 0.081 | 15.70 | 34.30 | `66f28f520639f55f` |
| 10,000 | 1 | 1.077 | 0.130 | 24.40 | 75.60 | `f173780ae3f8e98` |
| 10,000 | 8 | 0.888 | 0.143 | 24.40 | 75.60 | `f173780ae3f8e98` |

The ordinary 10,000-unit regression remained in its prior performance band after the stronger pursuit rule: lane 9.343/7.715 ms/tick, cage 13.972/8.174 ms/tick, and crowd 9.616/7.560 ms/tick for 1/8 workers respectively, with matching checksums at both worker counts.

## Guaranteed-hit projectile and defense-alert density — 2026-09-12

`RangedGuaranteedHit` now has real authoritative travel rather than immediate damage. Launch distance is converted to integer travel ticks with an upward-rounded Euclidean subunit distance and upward division by positive projectile speed, minimum one tick. Live projectiles retain source/target identity, damage, launch position/tick, and impact tick in canonical ECS/checksum state. A moving target remains guaranteed to be hit on the due tick; source death after launch does not cancel the missile; a target already dead/removed at impact invalidates the projectile without retargeting. Focused regressions cover all three cases, including hitting a target across disconnected cage topology.

The first projectile-density benchmark also exposed a targeting architecture failure before projectile storage itself became expensive. With 5,000 stationary ranged units launching every tick, about 33,550 projectiles were live on average and 38,600 at peak. The initial one-alert-per-hit ally-defense query enumerated dense alert populations inside each defender's large acquisition radius:

| Units | Workers | ms/tick | Targeting ms/tick | Avg live projectiles | Peak live projectiles | Final checksum |
| ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 5,000 | 1 | 487.823 | 477.005 | 33,550 | 38,600 | `f1c6b9556d7a2193` |
| 5,000 | 8 | 105.984 | 94.971 | 33,550 | 38,600 | `f1c6b9556d7a2193` |

Adding threads was plainly not a solution. The fix preserves the documented ally-defense ordering exactly while changing candidate discovery: one-tick alerts are grouped by attacked victim, attacked victims are traversed in nearest-distance layers, and each victim's actual attacker relation is indexed in a separate derived spatial partition. The resolver therefore evaluates nearest valid attackers only for victims tied at the nearest relevant ally distance rather than scanning every hit/attacker combination. A regression with two attacked allies and multiple attackers verifies the required ordering: nearest attacked ally first, nearest valid attacker second, then stable IDs.

With that architecture, the final density sweep is worker-count deterministic:

| Units | Workers | ms/tick | Targeting ms/tick | Checksum ms/tick | Avg live | Peak live | Launches/tick | Impacts/tick | Final checksum |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1,000 | 1 | 7.377 | 5.237 | 0.997 | 5,340 | 5,800 | 1,000 | 760 | `7bb78283b5d5a389` |
| 1,000 | 8 | 3.551 | 1.267 | 0.963 | 5,340 | 5,800 | 1,000 | 760 | `7bb78283b5d5a389` |
| 5,000 | 1 | 49.685 | 37.579 | 6.271 | 33,550 | 38,600 | 5,000 | 3,320 | `f1c6b9556d7a2193` |
| 5,000 | 8 | 18.609 | 7.560 | 5.792 | 33,550 | 38,600 | 5,000 | 3,320 | `f1c6b9556d7a2193` |
| 10,000 | 1 | 112.706 | 88.635 | 13.587 | 80,490 | 100,600 | 10,000 | 5,470 | `6fbe379dc6549cac` |
| 10,000 | 8 | 41.453 | 18.108 | 13.427 | 80,490 | 100,600 | 10,000 | 5,470 | `6fbe379dc6549cac` |

At 5,000 units the final one-worker targeting phase is about **12.7x faster** than the naive defense-alert scan with the same checksum. In the 10,000-unit eight-worker case, the separately timed verification checksum accounts for ~13.4 ms of the 41.5 ms total; the other timed simulation phases sum to roughly 28.0 ms. That is useful architectural evidence, not a 10,000-unit support promise: production checksum cadence may be lower than every tick, while richer projectile/effect behavior will add work that this synthetic guaranteed-hit case does not contain.

Projectile entity count and impact/launch structural work were not the dominant cost in this fixture. At the time of this sweep, long-range target/defense evaluation remained the largest phase even after removing the pathological alert scan. The later sticky-target correction below removes most of that work because units with valid engagements no longer re-query ally-defense every tick. The playable verification game now renders the authoritative in-flight guaranteed-hit population instead of drawing ranged attacks as immediate hit lines.

## Sticky target / first-attacker retaliation regression — 2026-09-12

An interactive verification match exposed rapid target oscillation in melee units positioned between two separate fights. The affected units had initially chosen attackers through nearby-ally defense, then alternated between enemies on opposite sides as fresh ally-defense alerts arrived. The root cause was intentional selector logic that let ally defense pre-empt any valid current target that was not actively targeting the defender.

The corrected rule distinguishes combat-unit engagements from building objectives. Once ordinary acquisition or ally defense selects an enemy **unit**, that engagement remains sticky until normal invalidation (death/despawn, untargetability, unreachable attack position, or pursuit-leash escape). A direct attack on the unit may pre-empt an ordinary/ally-defense target once. The first valid hostile attacker in canonical combat-event order becomes a persistent direct-retaliation target; later attackers cannot replace it while it remains valid. The retaliation-lock bit is authoritative and included in the canonical checksum. A current **building** target is intentionally interruptible by ally defense: after an actual nearby allied unit is attacked, a building attacker may peel off to engage that ally's attacker. Mere defender arrival/proximity does not trigger the switch.

Focused regressions cover the observed pattern and the edge cases behind it: a target chosen through ally defense stays fixed when a different ally is attacked later; the first personal attacker remains locked while another enemy continues hitting the unit; an idle unit still uses ally defense and preserves nearest-ally/nearest-attacker ordering; a nearby ally attack does pre-empt a building target; two units pounding a castle both remain on the castle when a defender merely arrives, then the directly struck attacker and a nearby castle attacker both peel to that defender after it actually attacks; and a lethal hit still leaves a one-tick alert that an otherwise idle nearby ally can consume.

Re-enabling ally-defense checks specifically for building targets does not recreate the earlier target-density spike in the 10,000-unit cage fixture: the post-exception run measured 14.909 ms/tick on one worker and 9.632 ms/tick on eight workers, with matching final checksum `a34462b08b8572ea`.

The corrected rule also removes the repeated defense-query cost from the synthetic projectile-density fixture while leaving live/launch/impact projectile counts unchanged. Re-running the same sweep on the exact committed sticky-target state produced:

| Units | Workers | ms/tick | Targeting ms/tick | Checksum ms/tick | Avg live | Peak live | Final checksum |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1,000 | 1 | 2.259 | 0.052 | 1.050 | 5,340 | 5,800 | `1346998528d5c1e9` |
| 1,000 | 8 | 2.566 | 0.136 | 1.020 | 5,340 | 5,800 | `1346998528d5c1e9` |
| 5,000 | 1 | 11.761 | 0.276 | 6.154 | 33,550 | 38,600 | `50672cccc8bb831b` |
| 5,000 | 8 | 11.250 | 0.307 | 6.153 | 33,550 | 38,600 | `50672cccc8bb831b` |
| 10,000 | 1 | 24.739 | 0.542 | 13.948 | 80,490 | 100,600 | `499530785e31dc12` |
| 10,000 | 8 | 22.886 | 0.338 | 13.714 | 80,490 | 100,600 | `499530785e31dc12` |

At 10,000 units this cuts one-worker targeting from 88.635 to 0.542 ms/tick and eight-worker targeting from 18.108 to 0.338 ms/tick. The changed checksum is expected because target-state semantics now include the authoritative direct-retaliation lock. The result reinforces the intended architecture: ally-defense indexing remains necessary for idle units and units currently attacking buildings, but active unit-vs-unit engagements should not pay that query cost or change targets in response to unrelated nearby fights.

## Ballistic post-movement splash density — 2026-09-12

`RangedBallistic` now has executable authoritative semantics rather than only a design placeholder. Launch captures the selected target's current pre-movement position as a fixed destination and computes integer travel time from launch distance/speed. The due projectile survives source death, does not follow its original target, and resolves after movement against a fresh spatial index. The provisional verification splash is a hostile-only circle: living enemy units are tested by post-movement center position and enemy buildings by footprint/radius intersection. Due projectiles resolve by projectile `SimId`, and affected entities within each impact resolve by target `SimId`.

Focused regressions prove the two rule-significant movement cases: an original target can move out of the captured impact zone and take no damage, while another unit that was outside the zone at launch can move into it and be damaged at impact. The second fixture also verifies that splash membership is using the post-movement position rather than the launch-time snapshot.

The first 10,000-unit ballistic run exposed a derived-data cost rather than an impact-query problem. Roughly 70,000 splash damage effects per tick generated the same order of one-tick defense alerts. Every unit already had a valid sticky unit engagement (`defense-q/t=0`), but snapshot/spatial preparation still grouped and spatially indexed those alerts unconditionally. Before making that index lazy, the 10,000-unit fixture measured 49.241/46.336 ms/tick for 1/8 workers, including 12.965/12.028 ms/tick in snapshot/spatial preparation. The canonical checksum was `823987be85591108` at both worker counts.

Defense-alert grouping/index construction is now skipped unless at least one unit satisfies the exact selector condition that can reach an ally-defense query: no retained valid unit engagement/direct-retaliation outcome, or a retained building target eligible for the building-target ally-defense exception. The alert events themselves remain authoritative and stay in the checksum; only the derived lookup structures are omitted when provably unused. The same 10,000-unit ballistic fixture keeps checksum `823987be85591108` while snapshot/spatial preparation falls to roughly 1.7 ms/tick.

Final ballistic-density sweep:

| Units | Workers | ms/tick | Spatial ms/tick | Ballistic impact ms/tick | Avg live | Peak live | Impacts/tick | Effects/tick | Candidates/impact | Final checksum |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1,000 | 1 | 3.352 | 0.191 | 0.401 | 5,439 | 5,998 | 750.1 | 9,640.4 | 60.42 | `fcc0a2b7e85bc7f5` |
| 1,000 | 8 | 3.368 | 0.223 | 0.359 | 5,439 | 5,998 | 750.1 | 9,640.4 | 60.42 | `fcc0a2b7e85bc7f5` |
| 5,000 | 1 | 17.326 | 0.910 | 1.936 | 33,649 | 38,798 | 3,310.1 | 42,562.0 | 128.36 | `dd3f01ad89105b45` |
| 5,000 | 8 | 16.613 | 0.851 | 1.967 | 33,649 | 38,798 | 3,310.1 | 42,562.0 | 128.36 | `dd3f01ad89105b45` |
| 10,000 | 1 | 35.906 | 1.658 | 3.272 | 80,589 | 100,798 | 5,460.1 | 70,211.0 | 128.36 | `823987be85591108` |
| 10,000 | 8 | 35.003 | 1.756 | 3.425 | 80,589 | 100,798 | 5,460.1 | 70,211.0 | 128.36 | `823987be85591108` |

At 10,000 units the full every-tick verification checksum itself costs ~22.7 ms/tick on eight workers; the other timed phases total roughly 12.3 ms/tick. The ballistic impact query is therefore not the limiting subsystem in this synthetic splash-heavy fixture. The current implementation still checks buildings linearly per ballistic impact; that is acceptable for the current unit-heavy probe but should be revisited if a future many-building siege benchmark makes building splash membership material.

## Bounce chain / candidate density — 2026-09-12

`Bounce` now has executable authoritative chain state. The initial ordinary target is guaranteed-hit, then one persistent projectile `SimId` is retained across every hop. The projectile stores bounded hit history and authored range/repeat/falloff/travel rules; source death cannot cancel an already-launched chain. A due hop whose retained target already died/disappeared invalidates the chain. Successful impacts apply damage first, then optionally choose another living hostile unit inside the authored bounce radius. Repeats are excluded when disabled, subsequent buildings are excluded in the provisional verifier, and hop damage uses integer floor scaling.

Subsequent target selection introduces the simulation's first executable keyed-random gameplay primitive. Each valid candidate receives a SplitMix64-derived rank keyed by match seed, impact tick, persistent projectile `SimId`, stable bounce-target purpose plus candidate `SimId`, and next bounce index; minimum `(rank, SimId)` wins. This result is independent of spatial enumeration and worker order. Focused regressions verify stable projectile identity across the chain, no-repeat history, `8 -> 4 -> 2` damage at 50% falloff, chain completion/removal, and identical final checksums on 1/2/8 workers.

The first density version materialized and sorted the full valid candidate list for every hop. At 5,000 units that spent ~53–55 ms/tick in combat and ~78 ms/tick overall. Replacing the allocation/sort with an enumeration-order-independent minimum keyed rank preserves the same rule class while removing the intermediate vector/sort; the 5,000-unit final combat phase is ~28 ms/tick. The remaining cost is real candidate evaluation rather than sorting: this deliberately dense fixture reaches ~553 broad-phase candidates per successful hop.

Final bounce-density sweep:

| Units | Workers | ms/tick | Combat ms/tick | Checksum ms/tick | Avg live | Peak live | Impacts/tick | Jumps/tick | Candidates/jump | Final checksum |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 1,000 | 1 | 7.227 | 2.721 | 3.323 | 8,400 | 10,559 | 2,541.1 | 2,019.0 | 146.25 | `c360765063672360` |
| 1,000 | 8 | 7.586 | 2.926 | 3.311 | 8,400 | 10,559 | 2,541.1 | 2,019.0 | 146.25 | `c360765063672360` |
| 5,000 | 1 | 52.528 | 28.434 | 18.660 | 47,932.2 | 64,629 | 10,468.4 | 8,449.9 | 552.84 | `36efd04adf1da8fd` |
| 5,000 | 8 | 51.175 | 27.814 | 18.278 | 47,932.2 | 64,629 | 10,468.4 | 8,449.9 | 552.84 | `36efd04adf1da8fd` |
| 10,000 | 1 | 100.095 | 45.658 | 43.156 | 103,195.6 | 152,372 | 16,494.3 | 13,613.0 | 552.28 | `bae9ffc39ab04c2c` |
| 10,000 | 8 | 104.762 | 48.271 | 44.527 | 103,195.6 | 152,372 | 16,494.3 | 13,613.0 | 552.28 | `bae9ffc39ab04c2c` |

This is intentionally a pathological architecture probe: every unit launches a bounce attack every tick and every chain may add three more guaranteed-hit hops. It is **not** a 10,000-unit support target. The worker-count hashes match at every scale, but extra threads do not help the canonically ordered chain-resolution phase. At 10,000 units roughly half the wall time is the full verification checksum and roughly half is pre-movement bounce combat. The candidate count plateaus with local density rather than total population, but total work still scales with the number of simultaneous hops. A future optimization must preserve the keyed-random candidate rule and remain independent of spatial bucket configuration; making gameplay choose “the first grid entry” merely to reduce this benchmark is explicitly rejected.

## Long-range attack-building density — 2026-09-12

Attack-capable buildings now use ordinary authoritative combat state rather than a special effect path. Each one has its own attack profile, cooldown, target state, and spawn tick. Static acquisition ignores navigation and measures exact range from the authoritative source footprint; hostile units are preferred over buildings for fresh acquisition, with squared distance and stable `SimId` as tie-breaks. Attack-building intents are merged with unit intents and sorted by source/target IDs, so an earlier canonical unit strike can kill a tower and cancel its later unresolved attack. Already-launched tower projectiles remain independent of the source. Tower damage uses the tower `SimId` as its source, allowing ordinary unit retaliation/ally-defense rules to react to it.

Focused regressions verify independent target choice by adjacent towers, ranged targeting of a unit inside a disconnected cage, same-tick unit-before-tower death cancellation, and personal retaliation against a tower after its projectile actually hits.

The `tower` stress fixture uses the requested unit count as the unit population and adds long-range towers separately: one tower per four units, capped at 500 total towers. Towers fire guaranteed-hit projectiles every tick at stationary durable enemies. Thus the compatibility-scale case is **700 units + 175 attack buildings**, while the 5k/10k architecture probes use 500 towers.

| Units | Towers | Workers | ms/tick | Targeting ms/tick | Checksum ms/tick | Peak projectiles | Launches/tick | Final checksum |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 700 | 175 | 1 | 1.179 | 0.351 | 0.333 | 1,005 | 175 | `b8af0d7ccb046cf3` |
| 700 | 175 | 8 | 0.979 | 0.116 | 0.331 | 1,005 | 175 | `b8af0d7ccb046cf3` |
| 1,000 | 250 | 1 | 1.970 | 0.665 | 0.496 | 1,427 | 250 | `91ec3c22e5f4377e` |
| 1,000 | 250 | 8 | 1.539 | 0.258 | 0.500 | 1,427 | 250 | `91ec3c22e5f4377e` |
| 5,000 | 500 | 1 | 10.757 | 6.095 | 1.618 | 2,687 | 500 | `2990c09fae6fb168` |
| 5,000 | 500 | 8 | 5.557 | 1.331 | 1.624 | 2,687 | 500 | `2990c09fae6fb168` |
| 10,000 | 500 | 1 | 20.883 | 12.182 | 2.718 | 2,687 | 500 | `4d78726f1e519e7c` |
| 10,000 | 500 | 8 | 10.219 | 2.670 | 2.705 | 2,687 | 500 | `4d78726f1e519e7c` |

At the actual 700-unit compatibility ceiling, the tower-heavy fixture is ~1.2 ms/tick including the full checksum. The larger cases expose a predictable targeting cost from thousands of stationary passive units receiving tower hits: because the distant tower is not a valid retaliation/pursuit target for those units, they can perform cheap unsuccessful ally-defense queries. At 10,000 units this becomes the dominant one-worker targeting cost, but it is not material at the compatibility scale and does not justify weakening target semantics. Worker-count checksums match at every scale.

## Automatic spellcasting / mana density — 2026-09-12

The first executable ability slice implements one automatic ability on a spellcasting building. Mana regenerates with deterministic integer arithmetic during the timer phase and clamps at the authored maximum. Eligibility/target evaluation runs before ordinary combat targeting. The initial target policy chooses a random hostile unit within footprint-based range by assigning each eligible candidate a keyed deterministic rank from match seed, caster `SimId`, stable `AbilityId`, cast sequence, and candidate `SimId`; minimum `(rank, SimId)` wins independently of grid enumeration and worker order.

Cast commitment is atomic and canonical. Intents resolve by source `SimId`, ability ID, cast sequence, then target `SimId`; source/resource/readiness and target liveness/team/range are revalidated before mana is spent. A successful cast subtracts mana, sets `ready_tick = cast_tick + cooldown_ticks`, increments cast sequence, then applies the immediate effect. Focused regressions verify exact regen/cost/cooldown cadence, 1/2/8-worker checksum identity, and that an ability-phase lethal hit suppresses the victim's later ordinary attack in the same tick. Building spell damage is deliberately non-retaliatory: it does not populate `last_attacker` or nearby-ally defense alerts, unlike an ordinary attack-building strike.

The `ability` density fixture uses the requested unit count as durable stationary combat units and adds one automatic caster per four units, capped at 500 casters. Every caster is mana-ready, casts every tick, has 100-world-unit range, and chooses among the full hostile unit population. This is deliberately harsher than realistic content frequency and directly exercises deterministic random candidate evaluation.

| Units | Casters | Workers | ms/tick | Ability ms/tick | Evaluations/tick | Casts/tick | Candidates/evaluation | Final checksum |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 700 | 175 | 1 | 2.682 | 1.719 | 175 | 175 | 350.00 | `599654a192e81d81` |
| 700 | 175 | 8 | 1.145 | 0.381 | 175 | 175 | 350.00 | `599654a192e81d81` |
| 1,000 | 250 | 1 | 4.507 | 2.909 | 250 | 250 | 500.00 | `b37b601337166f17` |
| 1,000 | 250 | 8 | 1.663 | 0.584 | 250 | 250 | 500.00 | `b37b601337166f17` |
| 5,000 | 500 | 1 | 26.752 | 16.639 | 500 | 500 | 2,500.00 | `5a57e2c7cb716dc8` |
| 5,000 | 500 | 8 | 7.984 | 3.275 | 500 | 500 | 2,500.00 | `5a57e2c7cb716dc8` |
| 10,000 | 500 | 1 | 46.574 | 27.831 | 500 | 500 | 4,725.00 | `2abd5030f88d220d` |
| 10,000 | 500 | 8 | 15.175 | 5.306 | 500 | 500 | 4,725.00 | `2abd5030f88d220d` |

At the actual 700-unit compatibility ceiling, even this all-casters-every-tick fixture is only 2.68 ms/tick on one worker and 1.15 ms on eight, including the full canonical checksum. Unlike the smaller ordinary-combat fixtures, this phase contains enough independent candidate-query work to amortize Rayon overhead: eight workers reduce ability evaluation from 1.72 to 0.38 ms at 700 units and from 27.83 to 5.31 ms in the 10k torture case. This validates the intended architecture choice of parallel evaluation plus canonical serial commitment rather than requiring all simulation phases to exhibit multicore speedup.

## Global timed-stun density — 2026-09-12

Timed stun is now executable authoritative status state rather than only a scheduling requirement. A stun cast on tick `T` for duration `D > 0` stores exclusive expiry `T + D`; the unit is disabled while `current_tick < stunned_until_tick`. Reapplication keeps the later expiry, so a shorter stun cannot truncate a longer one. While stunned, units retain a still-valid existing target but do not perform fresh target acquisition/retaliation/ally-defense switching, ordinary attacks, or intentional movement. Active buildings with status state likewise suppress retargeting, ordinary attacks, and automatic casts while stunned. Cooldowns and mana regeneration continue normally.

Focused regressions verify that a one-tick global stun cancels an already-established melee attack on its cast tick and releases the attacker on the next tick; a two-tick stun suppresses acquisition and movement for exactly two ticks; overlapping shorter/longer stuns preserve the longer expiry; and a global-stun battle produces identical checksums on 1/2/8 workers.

The `stun` stress fixture uses the requested combat-unit count plus one global stun caster per 40 units, capped at 64 casters. Casters have a three-tick cooldown and apply a two-tick map-wide stun to every living enemy combat unit. This intentionally creates a high status duty cycle and mass canonical effect application while repeatedly taking thousands of units in and out of active targeting/movement.

| Units | Casters | Workers | ms/tick | Ability ms/tick | Casts/tick | Stun effects/tick | Avg stunned | Peak stunned | Final checksum |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 700 | 17 | 1 | 1.353 | 0.017 | 6.0 | 2,082.5 | 455 | 700 | `42fdbcb6fb482313` |
| 700 | 17 | 8 | 0.957 | 0.037 | 6.0 | 2,082.5 | 455 | 700 | `42fdbcb6fb482313` |
| 1,000 | 25 | 1 | 2.214 | 0.028 | 8.8 | 4,375.0 | 650 | 1,000 | `5d0b18ae53073f39` |
| 1,000 | 25 | 8 | 1.285 | 0.062 | 8.8 | 4,375.0 | 650 | 1,000 | `5d0b18ae53073f39` |
| 5,000 | 64 | 1 | 16.304 | 0.188 | 22.4 | 56,000 | 3,250 | 5,000 | `8b910535430532c0` |
| 5,000 | 64 | 8 | 6.805 | 0.200 | 22.4 | 56,000 | 3,250 | 5,000 | `8b910535430532c0` |
| 10,000 | 64 | 1 | 46.819 | 0.455 | 22.4 | 112,000 | 6,500 | 10,000 | `f224d2396bb187b0` |
| 10,000 | 64 | 8 | 18.719 | 0.501 | 22.4 | 112,000 | 6,500 | 10,000 | `f224d2396bb187b0` |

Mass stun application itself is inexpensive even in the torture cases; the larger cost comes from thousands of units repeatedly transitioning back into active target/movement evaluation. At the actual 700-unit ceiling the entire deliberately extreme fixture remains below 1.4 ms/tick on one worker and below 1.0 ms on eight, including full canonical checksumming. The matching hashes confirm that status expiry/refresh and action suppression are worker-count independent.

## Timed movement-modifier density — 2026-09-12

The `slow` fixture exercises the first generic timed-stat architecture without claiming to reproduce every Warcraft III buff rule. Each unit retains immutable authored movement speed; active `ModifierId` entries are canonical status state with exclusive expiry ticks. Reapplying the same ID refreshes rather than adding another copy, while distinct IDs stack additively. Effective speed uses deterministic integer `base * clamp(100 + sum(percent), 0..1000) / 100`. The current fixed-capacity representation allows eight simultaneous movement-modifier identities per combat unit; exceeding that bound is a content/simulation error rather than allocating an unbounded collection in the movement hot path.

The stress fixture uses the requested combat-unit count plus one global modifier caster per 40 units, capped at 64 casters. Casters alternate between two modifier identities (`-20%` and `-15%`), cast every three ticks, and refresh five-tick effects across every hostile combat unit. This deliberately keeps two simultaneous modifiers active on essentially the entire population, stressing canonical refresh/expiry and effective-speed derivation more heavily than representative Castle Fight content is expected to.

| Units | Casters | Workers | ms/tick | Ability ms/tick | Effects/tick | Avg active modifiers | Peak active modifiers | Final checksum |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 700 | 17 | 1 | 1.668 | 0.024 | 2,082.5 | 1,400 | 1,400 | `16d2b4f6f9e62eea` |
| 700 | 17 | 8 | 1.049 | 0.037 | 2,082.5 | 1,400 | 1,400 | `16d2b4f6f9e62eea` |
| 1,000 | 25 | 1 | 2.312 | 0.041 | 4,375.0 | 2,000 | 2,000 | `3d53320ea76db3a1` |
| 1,000 | 25 | 8 | 1.409 | 0.053 | 4,375.0 | 2,000 | 2,000 | `3d53320ea76db3a1` |
| 5,000 | 64 | 1 | 14.112 | 0.456 | 56,000 | 10,000 | 10,000 | `c7f1f9e27d0d59c6` |
| 5,000 | 64 | 8 | 7.826 | 0.529 | 56,000 | 10,000 | 10,000 | `c7f1f9e27d0d59c6` |

At the real 700-unit compatibility ceiling, two continuously refreshed modifiers on every combat unit still leave the full simulation at about 1.67 ms/tick on one worker and 1.05 ms on eight, including checksum. The focused regressions additionally verify exact duration/expiry, same-ID refresh without duplicate stacking, distinct-ID additive stacking, and worker-count independence. These semantics remain provisional import/runtime primitives until original-map extraction establishes the precise Warcraft stacking/source/aura rules for each content effect.

## Long mixed compatibility battle — 2026-09-12

The `mixed` fixture is the first sustained cross-system battle rather than an isolated subsystem torture test. Its 700-unit scale reserves four unit slots for four slow production buildings, starts 696 combat units split evenly between teams, then lets each producer emit one melee unit during the standard run so the no-death population cannot exceed the 700-unit compatibility reference. The initial army is evenly mixed across melee, guaranteed-hit, ballistic, and bounce delivery. The map also contains twelve independently targeted ranged attack buildings and eight automatic mana spell buildings: six random-target damage casters plus one infrequent global stunner per team. Movement, hard non-overlap collision, sticky targeting/retaliation, ally defense, projectile travel/impact, bounce chains, splash, production placement, mana/cooldown scheduling, timed stun suppression/expiry, deaths, and the full canonical checksum all run together.

The standard long-run probe uses 30 warmup ticks plus 3,600 measured ticks, equivalent to two simulated minutes at 30 Hz. It completes without panic or overlap failure, resolves substantial attrition, and produces the same final checksum on one and eight workers:

| Units ceiling | Workers | ms/tick | Avg live projectiles | Peak projectiles | Launches/tick | Impacts/tick | Effects/tick | Bounce jumps/tick | Ability casts/tick | Avg / peak stunned | Final live units | Final buildings | Final checksum |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 700 | 1 | 0.963 | 95.1 | 502 | 22.2 | 33.7 | 47.0 | 11.6 | 0.2 | 52.6 / 697 | 309 | 24 | `48e744f0c5315c58` |
| 700 | 8 | 0.965 | 95.1 | 502 | 22.2 | 33.7 | 47.0 | 11.6 | 0.2 | 52.6 / 697 | 309 | 24 | `48e744f0c5315c58` |

With the current attack-envelope, objective-flow, and proposed-final collision commit enabled, the run averages 524.8 retained targets/tick with only 1.4 target changes/tick while processing 6.4 ally-defense queries/tick. It averages 55.2 movement intents/tick, including 17.2 objective-following intents, and only 1.1 movement intents/tick are rejected by the hard collision pass (1.91%). Periodic global stuns still reach 697 simultaneous disabled units without stale-target churn, overlap failure, or worker-count divergence. The entire mixed simulation remains below 1 ms/tick including the full checksum on the reference run, leaving very large headroom against a 30 Hz budget at the original unit ceiling; this particular mixed workload remains too small/sequential for eight workers to improve wall time.

## Mixed collision-radius verification — 2026-09-12

The sim now supports an optional authoritative `CollisionRadius` per combat unit and a matching production-unit property. Unit-unit clearance is the sum of both radii; explicit radii also constrain map-boundary/static-blocker clearance, production spawn legality, local avoidance, hard reservation, attack-envelope fallback positions, and path traversal. Units without imported geometry retain the historical fallback radius (`unit_separation_distance / 2`) so existing placeholder/client fixtures remain behaviorally and checksum compatible. Attack range remains its separately authored center/footprint-distance rule; collision radius is not silently added to weapon range.

The `radius` fixture drives 700 units using representative extracted Warcraft collision radii of 8, 16, 24, and 31 world units into sustained opposing flow. Radius-aware objective fields are lazily cached per `(team, radius)` and exclude cells where that circle cannot physically fit, while retaining the same strategic objective. Focused regressions also prove that large units detour around radius-inflated blocker clearance, can find alternate legal attack positions around both unit and building targets, and produce identical mixed-radius outcomes across worker counts.

| Units | Workers | ms/tick | Movement intent | Crowd/collision | Final checksum |
| ---: | ---: | ---: | ---: | ---: | --- |
| 700 | 1 | 1.457 | 0.133 | 0.784 | `d6e517a1eb84cc3a` |
| 700 | 8 | 1.246 | 0.063 | 0.622 | `d6e517a1eb84cc3a` |

The ordinary 700-unit lane/cage/crowd fixtures still retain their pre-radius hashes when no explicit radius is authored, confirming that the imported-geometry path is opt-in rather than a silent rules change for verification placeholders.

## High-density traffic-flow verification — 2026-09-12

A playable ~2,000-unit stalemate exposed two distinct movement failures that smaller fixtures did not reveal. First, equal-cost objective-field choices used coordinate ordering, so symmetric route choices consistently favored one side of the lane and accumulated a macroscopic one-sided traffic bias. Second, the hard collision pass initially reserved every unit's old position before considering movement, so a packed convoy could not advance into space that the unit ahead was simultaneously vacating. Under sustained compression that rejection propagated all the way back toward production buildings.

Objective following now resolves equal-cost downhill choices with a stable per-unit lateral preference derived from `SimId`, while retaining the same integration cost as the primary rule. Local bypass continuity also applies to objective movement, not only retained combat targets. The hard non-overlap pass begins from proposed final positions and canonically repairs only actual final-position conflicts, allowing coherent rows/columns to translate together while preserving the no-overlap invariant. Focused regressions cover two-sided objective tie distribution, symmetric blocker routing, objective movers flowing around stationary allies, and packed convoys using simultaneously vacated space.

The client-scale `traffic` fixture places 2,000 units into a symmetric wide-lane stalemate. Before the proposed-final commit, the fixture averaged about 1,924 movement intents/tick but rejected about 1,681 of them in hard collision (`87.35%`). With the current solver the 240-tick post-warmup run is:

| Units | Workers | ms/tick | Movement intents/tick | Objective intents/tick | Hard-blocked/tick | Blocked % | Final lower / upper lane halves | Final checksum |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | --- |
| 2,000 | 1 | 4.439 | 1649.1 | 4.7 | 7.6 | 0.46% | 1090 / 910 | `045f0928ef8201f4` |
| 2,000 | 8 | 3.774 | 1649.1 | 4.7 | 7.6 | 0.46% | 1090 / 910 | `045f0928ef8201f4` |

The later 1090/910 split occurs after almost the entire population is mutually target-locked, so it is dominated by combat packing rather than objective routing. A 25-tick approach-phase sample remains close to symmetric at 1017/983 while rejecting only 0.55% of movement intents, confirming that the previous coordinate-driven one-sided objective bias is gone. The traffic fixture is intentionally above the original 700-unit compatibility ceiling and exists to catch emergent macroscopic flow failures.

## Current interpretation

Repeated full topology rebuilding and bounded spawn churn are not architectural bottlenecks on the current verification map. Arbitrary-target A* fallback, dense ally-defense processing, and unnecessary derived alert-index construction all exposed architecture/correctness risks; sustained regression fixtures plus deterministic derived indexes/caches and exact lazy construction now keep those costs bounded enough for continued verification while preserving canonical outcomes across worker counts. Guaranteed-hit projectile storage, ballistic post-movement splash queries, independently targeted long-range attack buildings, automatic mana/random-target casting, and mass timed-stun application all remain viable at the 700-unit compatibility scale. Bounce is functionally/deterministically viable, but extreme simultaneous chain density exposes canonically ordered candidate evaluation as a measurable scaling risk that should be revisited only with realistic content frequency/support targets or an acceleration structure that provably preserves the keyed rule.

All four ordinary delivery architecture classes, attack-building source semantics, the base automatic ability/mana scheduler, and exact timed-stun precedence now have executable deterministic verification coverage. The automatic-caster and status-transition probes also show where multicore execution is materially useful: expensive independent target/movement evaluation scales well while canonical effect commitment remains ordered. The two-minute mixed battle provides corresponding cross-system evidence at the actual unit ceiling. The next rule-heavy compatibility slice should be chosen from observed map behavior—preferably a representative buff/debuff or area effect that exercises modifier stacking/expiry—rather than expanding into production UI/networking prematurely.
