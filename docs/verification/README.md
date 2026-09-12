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
- melee attack cooldown/damage resolution;
- spawn-tick attack suppression and death-before-later-actions ordering;
- production buildings with deterministic bounded expanding-spiral spawn search;
- failed spawn attempts are lost rather than backlogged;
- dedicated Rayon worker pool configurable per simulation instance;
- cross-worker determinism tests;
- phase-level tick timing diagnostics;
- open-lane and dense-cage release benchmarks;
- Bevy debug viewer using procedural placeholder units, building footprints, and target-link gizmos.

Not implemented yet:

- dynamic unit collision/separation and dense crowd resolution;
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
  --scenario lane,cage --units 1000,5000,10000 \
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

## Current interpretation

The core deterministic architecture remains viable under the first deliberately hostile topology workload. The next meaningful risks are dynamic crowd separation/collision, repeated topology mutations, arbitrary-target pursuit/A* fallback frequency, production churn, and attack/projectile/ability density. Each should receive a deliberately adversarial benchmark before broader game content is built.
