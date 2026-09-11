# Time, Scheduling, and Parallel Execution

Status: **normative architecture, provisional rates/budgets**

## 1. Purpose

This document defines how simulation time advances, how Bevy schedules are constrained, and how multicore execution is used without making gameplay depend on thread scheduling.

## 2. Simulation tick

The authoritative simulation advances only through integer ticks.

```rust
pub struct Tick(pub u64);
```

One tick represents a fixed duration chosen by the simulation version.

The initial target rate is provisional, likely **20 or 30 ticks per second**. The final choice must be measured against:

- combat responsiveness;
- projectile/movement fidelity;
- CPU budget at high entity counts;
- network command latency requirements;
- fast-forward/replay throughput.

Changing tick rate after compatibility is established is a simulation-version change.

## 3. Pacing vs simulation

Real-time pacing is external to simulation semantics.

Conceptually:

```text
server/client wall clock
        ↓
decide how many simulation ticks should be advanced
        ↓
Simulation::step() one or more times
        ↓
presentation consumes completed states
```

`Simulation::step()` MUST NOT read wall-clock delta time.

A headless benchmark may call it continuously. A reconnecting client may call it hundreds of times as quickly as possible. The resulting state MUST be identical to real-time execution.

## 4. Schedule structure

The simulation SHOULD use explicit Bevy `SystemSet`s or equivalent barriers for rule-significant phase ordering.

Conceptually:

```rust
#[derive(SystemSet, Debug, Clone, Eq, PartialEq, Hash)]
pub enum SimPhase {
    Commands,
    Topology,
    Navigation,
    Production,
    Timers,
    AbilityIntent,
    Targeting,
    CombatIntent,
    AbilityResolve,
    CombatResolve,
    ProjectileResolve,
    MovementIntent,
    MovementResolve,
    StructuralCommit,
    Victory,
    Checksum,
}
```

Exact names/order may evolve with gameplay rules.

Any dependency that affects authoritative results MUST be explicit. Default scheduler ordering MUST NOT be treated as a gameplay guarantee.

## 5. Parallelism within a phase

Systems and entity work MAY execute in parallel if they do not expose scheduling order to authoritative results.

Good candidates include:

- target validation/acquisition for independent units;
- automatic ability eligibility/target evaluation;
- attack intent calculation;
- movement intent calculation;
- spatial-index population via partitioned buffers plus deterministic assembly;
- local steering queries;
- many content-rule evaluations;
- deterministic per-entity timer advancement.

Less suitable work includes:

- stateful global allocators without partitioning;
- operations requiring ordered side effects;
- direct contested mutation of targets/health/occupancy;
- arbitrary ECS structural changes.

## 6. Worker-count independence

The scheduler MAY use any available number of workers.

The simulation MUST produce identical state for at least:

```text
1 worker
2 workers
N workers
```

where N is bounded only by implementation/environment.

Increasing worker count may change:

- completion time;
- temporary work partitioning;
- diagnostic/log ordering;
- which CPU executes which entity.

It MUST NOT change authoritative values.

## 7. Partition-local buffers

Parallel producers SHOULD write into isolated buffers rather than synchronize on one global mutable vector.

Example:

```text
worker/partition A -> intents A
worker/partition B -> intents B
worker/partition C -> intents C

barrier

canonical merge/reduction
```

The merge MUST NOT rely on buffer completion order. Either:

- concatenate partitions in a predefined partition order when that order itself is deterministic and semantically valid;
- or sort/reduce by canonical keys before resolution.

## 8. Tick-local scratch memory

Hot systems SHOULD reuse scratch storage to avoid allocator churn.

Scratch storage is non-authoritative and MAY differ by worker count. It MUST be cleared/reinitialized so no stale nondeterministic contents affect simulation decisions.

Arena/bump allocation MAY be useful for phase-local intent buffers.

## 9. Decision cadence

Not every decision needs to be recomputed every tick.

A unit MAY retain a target until a deterministic invalidation condition occurs. Expensive target searches or behavior evaluations MAY be staggered if the cadence is itself deterministic.

For example, an optional deterministic cohort schedule could use:

```text
(SimId + tick) mod K == 0
```

for low-priority reevaluation, provided gameplay semantics explicitly allow the added latency.

Staggering MUST NOT use worker/thread assignment.

## 10. Rendering cadence

The client MAY render at any frame rate independently of simulation tick rate.

Presentation SHOULD retain at least two completed authoritative snapshots/states sufficient to interpolate visual position between ticks.

Conceptually:

```text
Tick N               Tick N+1
  |----------------------|
          ^
          render alpha
```

Interpolation is cosmetic. Hit tests, target acquisition, placement validation, guaranteed-hit impact timing, ballistic captured destinations/impact zones, and other authoritative projectile outcomes use simulation state, not interpolated transforms.

## 11. Input scheduling

Local player input occurs in wall-clock/presentation time, but becomes authoritative only after conversion to a protocol command assigned to a simulation tick/order by the authoritative server.

Normal commands may move the player's builder, place/sell buildings, use builder-held active items, purchase upgrades, or activate explicitly player-targeted building abilities. Ordinary combat units have no direct player-order command path.

A client MAY provide immediate visual movement/placement/targeting feedback, but a command is not authoritative merely because local UI accepted it.

## 12. Server catch-up

The server SHOULD monitor wall-clock pacing to avoid unbounded drift, but MUST NOT alter the simulation timestep to catch up.

If the server falls behind, it should execute fixed ticks back-to-back until caught up, within operational safeguards.

Dynamic authoritative `dt` is forbidden.

Persistent inability to maintain the target tick rate is an overload condition that should be surfaced through observability and matchmaking/capacity policy rather than hidden by changing game physics.

## 13. Client catch-up after snapshot

A reconnecting client may receive a snapshot older than the current server tick plus the finalized tick-input history after it.

It MUST be possible to run the simulation without presentation during catch-up:

```text
load snapshot T
apply finalized tick inputs
step T+1 ... current
publish current state to presentation
```

Catch-up SHOULD use available multicore simulation exactly as normal execution does. Disabling rendering/audio/UI work is expected to make it substantially faster than real time.

## 14. Background/foreground behavior

Client rendering may be throttled or paused when minimized without changing local authoritative simulation progression if the client remains connected.

If local simulation intentionally stops, networking/rejoin logic must treat the client as behind and resynchronize/catch up rather than trying to simulate a large wall-clock `dt` in one step.

## 15. Performance measurement

Simulation timing metrics MUST be observational only.

Useful measurements include:

- total tick duration;
- duration per phase;
- number of entities processed per phase;
- spatial query candidate counts;
- navigation rebuild cost;
- intents/effects emitted;
- worker utilization;
- catch-up ticks/second.

No production game outcome may branch on profiling/timing results.

## 16. Deterministic barriers vs excessive serialization

A barrier should exist because there is a semantic dependency, not because it is convenient.

The implementation SHOULD combine phases or permit more scheduler parallelism when profiling shows overhead, provided the externally defined phase semantics and deterministic outcomes remain unchanged.

Likewise, adding cores cannot repair an O(N²) algorithm. Spatial/algorithmic optimizations remain a prerequisite to useful parallel scaling.
