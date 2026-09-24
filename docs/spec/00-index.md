# Castle Fight Native — Specification Index

Status: **living design specification**

This directory defines the architecture and game rules for Castle Fight Native. The documents are deliberately split by layer so that low-level invariants can remain stable while higher-level gameplay and presentation evolve.

The key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT**, and **MAY** describe normative requirements unless a section is explicitly marked as exploratory.

## Layering

### Layer 0 — Project invariants

- [01-goals-and-principles.md](01-goals-and-principles.md) — product goals, non-goals, architectural constraints, terminology.

### Layer 1 — Deterministic simulation engine

- [10-simulation-core.md](10-simulation-core.md) — fixed-step authoritative world simulation and phase model.
- [11-determinism.md](11-determinism.md) — deterministic arithmetic, ordering, IDs, random numbers, checksums, and forbidden sources of nondeterminism.
- [12-ecs-data-model.md](12-ecs-data-model.md) — ECS ownership, canonical entities/components, stable simulation identity, and mutation boundaries.
- [13-time-and-scheduling.md](13-time-and-scheduling.md) — ticks, schedules, worker parallelism, barriers, and presentation interpolation.
- [14-spatial-navigation.md](14-spatial-navigation.md) — map occupancy, spatial indexing, pathing, flow fields, local steering, and caging.
- [15-targeting-combat.md](15-targeting-combat.md) — individual target selection, attack delivery modes, projectiles, effects, deaths, and deterministic tie-breaking.
- [16-abilities-spellcasting.md](16-abilities-spellcasting.md) — mana, autonomous spellcasting, player-targeted legendary abilities, auras, and deterministic ability execution.

### Layer 2 — Multiplayer and continuity

- [20-networking.md](20-networking.md) — authoritative server, deterministic clients, command transport, validation, tick assignment, and desync handling.
- [21-snapshots-rejoin-replays.md](21-snapshots-rejoin-replays.md) — snapshots, reconnect, fast-forward, spectators, replay logs, and compatibility/versioning.
- [22-authoritative-state-inventory.md](22-authoritative-state-inventory.md) — current authoritative/immutable/derived/presentation state boundary used by checksums and future restore.

### Layer 3 — Client/presentation

- [30-client-presentation.md](30-client-presentation.md) — Bevy client, render extraction, interpolation, animation, audio, effects, and simulation/presentation separation.
- [31-input-building-ui.md](31-input-building-ui.md) — player commands, building placement, occupancy preview, UI, camera, and command acknowledgement.

### Layer 4 — Game/content model

- [40-content-data.md](40-content-data.md) — data-driven unit/building definitions, factions, upgrades, maps, validation, and mod/content boundaries.
- [41-match-gameplay.md](41-match-gameplay.md) — players/teams, resources, buildings, autonomous production, objectives, and victory semantics.
- [42-builder-items.md](42-builder-items.md) — the sole commandable builder, owned build regions, inventory, passive item effects, and active item use.

### Layer 5 — Verification and delivery

- [50-testing-observability-performance.md](50-testing-observability-performance.md) — determinism testing, property tests, soak tests, profiling, tracing, and performance budgets.
- [60-roadmap-open-questions.md](60-roadmap-open-questions.md) — implementation sequence, unresolved design choices, and explicit experiments.
- [61-implementation-plan.md](61-implementation-plan.md) — current ordered handoff for Sol: versioned content, ownership, commands, continuity, multiplayer, and roster expansion.
- [62-wc3-asset-fidelity-plan.md](62-wc3-asset-fidelity-plan.md) — active plan for full Castle Fight presentation-asset closure, extractor fidelity gates, and WC3 model/VFX correctness.

## Dependency direction

Higher-numbered layers may depend on lower-numbered layers. Lower layers MUST NOT depend on presentation or UI concerns.

In particular:

```text
content/game rules
        ↓
multiplayer + persistence
        ↓
deterministic simulation
        ↓
math / IDs / scheduling primitives

presentation reads simulation output but is not authoritative
```

The authoritative server and deterministic client simulation MUST share the same simulation and protocol crates. The server MUST NOT link client rendering/UI code.

## Proposed workspace shape

The final crate layout is not yet normative, but the intended dependency boundaries are:

```text
castle-fight-native/
├── crates/
│   ├── sim/          # deterministic authoritative game simulation
│   ├── protocol/     # commands, snapshots, network/replay wire types
│   ├── content/      # validated gameplay definitions
│   ├── server/       # headless authoritative server
│   └── client/       # Bevy application and presentation
└── docs/spec/
```

`sim` SHOULD depend on `bevy_ecs`, deterministic math utilities, and validated content types, but SHOULD avoid dependencies on the rest of Bevy unless a concrete need is demonstrated.

## Core invariants summary

The following requirements are foundational and are expanded in later documents:

1. The simulation MUST be deterministic for a given initial state and ordered command stream.
2. Simulation results MUST NOT depend on worker-thread count, task completion order, wall-clock time, pointer/address layout, hash-table iteration order, or presentation frame rate.
3. Multiplayer MUST use an authoritative server running the same deterministic simulation as clients.
4. Clients MUST be recoverable from desync by replacing or replaying authoritative state.
5. Players MUST be able to disconnect and rejoin a live match without pausing the match.
6. Units MUST select attack targets individually according to deterministic rules. Group/squad targeting MUST NOT be an architectural assumption.
7. Player buildings MUST participate in navigation occupancy and MAY intentionally create enclosed areas. The engine MUST preserve caging as a valid emergent mechanic.
8. A trapped unit MUST remain a valid combat target if ordinary targeting rules make it eligible; navigation reachability MUST NOT imply attack eligibility.
9. Simulation state SHOULD use integer/fixed-point representations for authoritative quantities. Floating-point values MUST NOT affect authoritative outcomes unless later proven bit-stable across all supported targets.
10. Expensive per-unit work SHOULD be data-parallel within explicit deterministic simulation phases.
11. Ordinary combat units MUST NOT accept player orders. Each player directly controls only their builder; standard units remain fully autonomous.
12. The builder itself is non-combat and non-blocking. Its inventory may create combat effects, but those effects MUST NOT turn the builder into a valid target or alter ordinary unit behavior toward it.
13. Buildings are not limited to production: they may attack, hold mana, autonomously cast abilities, provide auras, or expose explicitly player-targeted legendary abilities.

## Status convention

Each specification should label decisions as one of:

- **Normative** — required unless the specification is deliberately revised.
- **Provisional** — preferred direction, pending measurement or prototype validation.
- **Open** — unresolved question with constraints recorded.

When documents conflict, the more foundational layer wins until the conflict is intentionally resolved and both documents are updated.
