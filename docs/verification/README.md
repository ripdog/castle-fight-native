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
- authoritative `RangedGuaranteedHit` projectiles with integer travel time, retained target identity, source-death independence, and deterministic target-death invalidation;
- projectile/targeting-density diagnostics including live/peak projectiles, launches/impacts/invalidations, target retentions/changes, and ally-defense candidate counts;
- spawn-tick attack suppression and death-before-later-actions ordering;
- production buildings with deterministic bounded expanding-spiral spawn search;
- failed spawn attempts are lost rather than backlogged;
- dedicated Rayon worker pool configurable per simulation instance;
- cross-worker determinism tests;
- phase-level tick timing diagnostics split across topology, timers, production, spatial rebuild, targeting, combat, movement intent, collision/commit, structural commit, and checksum;
- pursuit diagnostics for total pursuit steps, deterministic A* fallback frequency, fallback-cache hits, and expanded A* nodes;
- open-lane, dense-cage, crossing-crowd, adversarial pursuit, repeated-topology-mutation, production-churn, and projectile-density release benchmarks;
- Bevy debug viewer using procedural placeholder units, building footprints, and target-link gizmos;
- a separate playable verification game with mirrored production-building placement and procedural placeholder visuals.

Not implemented yet:

- richer unit collision shapes / physically stronger crowd response beyond the current hard circle-distance exclusion;
- builder control/items;
- building attacks, mana, automatic abilities, or legendary abilities;
- ballistic and bounce delivery;
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

# Architecture-risk probes (use smaller counts first for deliberately adversarial pathing)
cargo run --release -p castle-fight-sim-bench -- \
  --scenario pathing,topology,production --units 100,500,1000 \
  --workers 1,8 --warmup 2 --ticks 10

# Guaranteed-hit projectile/targeting density
cargo run --release -p castle-fight-sim-bench -- \
  --scenario projectile --units 1000,5000,10000 \
  --workers 1,8 --warmup 2 --ticks 20
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

## Current interpretation

Repeated full topology rebuilding and bounded spawn churn are not architectural bottlenecks on the current verification map. Arbitrary-target A* fallback and dense ally-defense alert processing both exposed architecture/correctness risks; sustained regression fixtures plus deterministic derived indexes/caches now keep those costs bounded enough for continued verification while preserving canonical outcomes across worker counts.

The next high-risk delivery mechanics are ballistic and bounce attacks. Ballistic work must stress post-movement impact-zone spatial queries and effect density; bounce work must stress deterministic subsequent-target selection without turning chains into candidate-scan explosions. Longer playable verification matches should continue in parallel so congestion, topology, targeting, production, and projectile failures discovered interactively become focused regressions.
