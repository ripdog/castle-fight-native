# Verification Stage

This directory records reproducible evidence for the early architecture before full gameplay/content is built.

## Current slice

Implemented:

- Rust 2024 workspace pinned to Rust 1.98.1;
- authoritative `bevy_ecs` simulation state;
- integer/fixed-subunit 2D positions;
- monotonic `SimId` identity;
- deterministic fixed ticks;
- uniform-grid broad phase;
- autonomous individual target acquisition with stable `SimId` tie-breaks;
- melee attack cooldown/damage resolution;
- spawn-tick attack suppression;
- death-before-later-actions ordering;
- simple target pursuit/objective movement;
- canonical state checksum;
- dedicated Rayon worker pool configurable per simulation instance;
- cross-worker determinism tests;
- release-mode benchmark CLI;
- Bevy debug viewer using procedural placeholder sprites and target-link gizmos.

Not implemented yet:

- building topology, flow fields, caging, or reachability-aware attack positions;
- collision/separation;
- buildings/production/builders/items/abilities;
- ranged/ballistic/bounce delivery;
- stun/status effects;
- snapshots/networking;
- production art/assets.

The current benchmark is therefore an architectural scaling probe, not a final game performance claim.

## Verification commands

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo run --release -p castle-fight-sim-bench -- \
  --units 1000,5000,10000 --workers 1,2,4,8 --warmup 10 --ticks 100
```

The benchmark exits non-zero if different worker counts produce different final canonical checksums for the same fixture.

Run the placeholder viewer with:

```bash
cargo run -p castle-fight-debug-viewer
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

Fixture: two opposing autonomous unit populations using the uniform-grid target query, target pursuit, melee cooldown/damage logic, and canonical checksum. 10 warm-up ticks followed by 100 measured ticks.

| Units | Workers | ms/tick | ticks/sec | Final checksum |
| ---: | ---: | ---: | ---: | --- |
| 1,000 | 1 | 0.787 | 1271.5 | `5abeedbe721273bd` |
| 1,000 | 2 | 0.592 | 1690.6 | `5abeedbe721273bd` |
| 1,000 | 4 | 0.466 | 2145.1 | `5abeedbe721273bd` |
| 1,000 | 8 | 0.438 | 2283.9 | `5abeedbe721273bd` |
| 5,000 | 1 | 7.616 | 131.3 | `e583cd3aecb9fcad` |
| 5,000 | 2 | 4.521 | 221.2 | `e583cd3aecb9fcad` |
| 5,000 | 4 | 2.981 | 335.5 | `e583cd3aecb9fcad` |
| 5,000 | 8 | 2.612 | 382.9 | `e583cd3aecb9fcad` |
| 10,000 | 1 | 16.658 | 60.0 | `f297ed830d3f1b29` |
| 10,000 | 2 | 9.689 | 103.2 | `f297ed830d3f1b29` |
| 10,000 | 4 | 7.311 | 136.8 | `f297ed830d3f1b29` |
| 10,000 | 8 | 5.468 | 182.9 | `f297ed830d3f1b29` |

The 10,000-unit fixture shows about a 3.0x wall-clock speedup from one worker to eight while preserving the exact checksum. Future verification must add deliberately pathological dense/caged workloads and phase-level profiling before interpreting these numbers as capacity guidance.
