# Deterministic Simulation Core

Status: **normative core architecture**

## 1. Scope

This document defines the authoritative simulation loop: what constitutes world state, how one simulation tick advances, where parallelism is allowed, and where deterministic mutation occurs.

The simulation is shared by the authoritative server and deterministic clients. It MUST be runnable without graphics, audio, UI, or wall-clock pacing.

## 2. Fixed-step world

The simulation advances in integer ticks.

```rust
pub struct Tick(pub u64);
```

Authoritative game rules MUST be expressed in terms of simulation ticks or deterministic fixed-point durations derived from ticks.

The initial tick rate is **provisional**. A rate in the 20–30 Hz range is expected to be sufficient for autonomous combat, while presentation interpolates independently at the display frame rate. The chosen value MUST be treated as part of the simulation protocol/version once matches are networked or replayed.

The simulation MUST support running:

- slower than real time,
- exactly paced to real time,
- faster than real time,
- and unpaced during tests/replay catch-up.

Wall-clock elapsed time MUST NOT alter authoritative results.

## 3. Canonical world state

The canonical world contains all state required to produce the next tick from the current tick plus that tick's finalized authoritative inputs.

At minimum this includes:

- match tick and deterministic match seed;
- players, teams, alliances if present, and resources;
- all authoritative entities and stable simulation IDs;
- authoritative positions and movement state;
- health and combat state;
- current individual target state;
- cooldowns, attack windups, buffs, debuffs, and status effects;
- each player's non-combat builder position/movement and inventory state;
- item charges/cooldowns/automatic-effect sequences;
- buildings, footprints, ownership, production queues/timers, mana, ability state, and upgrades;
- navigation topology and any deterministically derivable cached navigation state that is persisted by design;
- authoritative projectiles/effects whose future behavior influences gameplay;
- deterministic ID allocators/counters;
- any game-mode or victory-condition state.

Pure caches MAY be excluded from snapshots if they can be rebuilt deterministically from canonical state.

## 4. Tick phase model

The exact final phase list may evolve, but the simulation MUST have explicit ordered phase boundaries.

Initial playable phase order:

```text
Tick N state
    │
    ├─ 1. Apply finalized scheduled player/server commands
    ├─ 2. Apply topology/content changes caused by commands
    ├─ 3. Refresh invalidated navigation data
    ├─ 4. Advance production and perform bounded spawn searches
    ├─ 5. Refresh dynamic spatial indexes needed by post-spawn queries
    ├─ 6. Update timers/cooldowns/mana/status expiration
    ├─ 7. Evaluate and resolve automatic/manual abilities that act before ordinary attacks
    ├─ 8. Apply resulting disables/stuns/deaths; cancel later actions for disabled/dead sources
    ├─ 9. Validate/retain/acquire individual combat targets
    ├─10. Compute ordinary attack intents (excluding units spawned this tick)
    ├─11. Resolve ordinary attacks and non-ballistic impacts in canonical order
    ├─12. Compute movement/steering intents for still-active entities
    ├─13. Resolve movement/occupancy interactions
    ├─14. Refresh dynamic spatial indexes for post-movement impact queries
    ├─15. Resolve due ballistic/siege impacts against post-movement positions
    ├─16. Resolve remaining deaths/despawns and deferred structural changes
    ├─17. Evaluate victory/game-mode state
    ├─18. Emit authoritative tick summary/checksum inputs
    │
Tick N+1 state
```

This order encodes the initial gameplay rules: a newly spawned unit cannot attack on its spawn tick; a stun/disable that becomes active before an ordinary attack resolves suppresses that attack even if an intent was already prepared; once an entity is dead it performs no later action in that tick; and ballistic/siege impacts query positions after movement. Already-launched persistent projectiles are independent entities and are not retroactively removed merely because their source later dies.

The current automatic-spell verification slice makes phase 6/7 concrete: integer mana regeneration is applied during timer advancement before ability eligibility, automatic cast intents are evaluated from the resulting immutable snapshot, and successful immediate ability effects/deaths are visible to target acquisition in phase 9. Ability damage that is explicitly non-retaliatory does not create ordinary combat retaliation/ally-defense alerts even though its health mutation is authoritative immediately.

A later compatibility-driven rules change MAY revise this ordering, but doing so is a simulation-version change and must be covered by executable timing fixtures.

## 5. Phase semantics

### 5.1 Input snapshot

Within a parallel decision phase, systems SHOULD treat the authoritative inputs for that phase as immutable.

Example: target acquisition for all units reads positions, teams, targetability, and current target state as they existed at the start of the targeting phase. One worker discovering a new target MUST NOT immediately mutate another unit's targeting decision.

### 5.2 Intents

Parallel systems SHOULD produce compact intents rather than mutating contested state directly.

Examples:

```rust
pub enum CombatIntent {
    Attack {
        source: SimId,
        target: SimId,
        attack: AttackId,
    },
    Cast {
        source: SimId,
        target: AbilityTarget,
        ability: AbilityId,
    },
}

pub struct MovementIntent {
    pub entity: SimId,
    pub desired_velocity: FixedVec2,
}
```

The exact types are provisional; the architectural requirement is explicit decision output followed by deterministic resolution.

### 5.3 Deterministic resolution

Intent resolution MUST define what happens when multiple intents interact.

Examples requiring explicit rules:

- multiple attacks damage the same target;
- a target dies during a tick in which it also attacks;
- healing and damage affect the same target;
- multiple spawn requests compete for nearby placement;
- units attempt to occupy overlapping space;
- a building is sold/destroyed while it produces a unit;
- a guaranteed-hit projectile reaches a target on the same tick the target dies/is removed;
- a ballistic projectile impacts while units enter/leave its captured target zone;
- an automatic caster regenerates enough mana on the same tick an ability becomes ready;
- a player-targeted legendary artillery command captures a moving target's position.

The initial rules use explicit phase precedence plus canonical ordering within a phase. When otherwise same-tick effects compete, stable event ordering ultimately falls back to `SimId`. Once an entity reaches the death threshold, any later not-yet-resolved attack, cast, or movement by that entity is canceled. No rule may depend on which worker completes first.

## 6. Structural mutation

Entity creation/destruction and component-shape changes are structural mutations.

Structural mutations SHOULD be deferred until designated commit points rather than performed opportunistically inside parallel scans.

Examples:

- spawn unit;
- remove dead unit;
- add/remove a status component;
- transform one unit type into another;
- create/destroy gameplay projectile;
- construct/sell/destroy building;
- add/remove inventory item or persistent aura source where represented structurally.

Where Bevy `Commands` are used internally, their application order MUST NOT be relied upon unless explicitly made deterministic. Gameplay-critical structural changes SHOULD be represented as canonical simulation operations with explicit stable ordering.

## 7. Parallelism model

Parallelism is an implementation detail beneath deterministic phase semantics.

A phase may partition work by:

- ECS archetype/chunk;
- stable entity ranges;
- spatial cells/tiles;
- navigation sectors;
- event batches;
- or another deterministic-independent partition.

A conforming simulation MUST produce identical authoritative state with one worker or many workers.

No gameplay rule may refer to a worker index, task index, thread ID, memory address, or task completion order.

## 8. Read/write ownership

Every system SHOULD have a narrow declared read/write set.

For example:

```text
Target acquisition
Reads:
  Position, Team, AttackProfile, Targetability,
  current Target, spatial index
Writes:
  TargetIntent / next Target

Combat intent
Reads:
  Target, Position, AttackProfile, cooldown/status state
Writes:
  CombatIntent buffers

Combat resolve
Reads:
  CombatIntent buffers
Writes:
  Health, cooldown state, status effects, projectile spawns
```

This is both a safety property and a performance property. Broad `&mut World` access SHOULD be treated as exceptional in hot simulation systems.

## 9. Simulation resources and caches

Global resources MUST be classified as one of:

1. **canonical state** — serialized/checksummed and authoritative;
2. **deterministic derived cache** — rebuildable from canonical state and safe to discard;
3. **presentation/diagnostic state** — excluded from authoritative outcomes.

This classification MUST be clear for navigation fields, spatial indexes, lookup tables, content registries, and event queues.

The spatial index is expected to be a deterministic derived cache. Rebuilding it MUST yield equivalent queries regardless of insertion partitioning.

## 10. Pause and match lifecycle

The server controls match lifecycle.

A single disconnected player does not pause the simulation while at least one teammate remains connected. If an entire team is disconnected, the standard rules enter an explicit canonical pause state at a completed tick boundary and wait for the configured reconnect timeout. A canonical resume or timeout/end control record leaves that state.

Administrative pause, countdown, post-game state, replay pause, and disconnect pause are explicit protocol/game-mode states rather than consequences of client frame timing.

## 11. Simulation API shape

A minimal headless simulation interface SHOULD eventually resemble:

```rust
pub struct Simulation { /* private canonical + derived state */ }

impl Simulation {
    pub fn new(config: MatchConfig, content: ContentHashBundle) -> Result<Self>;

    pub fn enqueue_stream_record(&mut self, record: CanonicalStreamRecord) -> Result<()>;

    pub fn step(&mut self) -> TickResult;

    pub fn tick(&self) -> Tick;

    pub fn checksum(&self) -> StateChecksum;
}
```

The implementation MAY wrap Bevy ECS schedules internally. External callers SHOULD not need to know whether a tick was calculated by 1 or 32 workers.

## 12. No hidden time sources

Authoritative systems MUST NOT read:

- `std::time::Instant` for gameplay decisions;
- operating-system time;
- render delta time;
- network receive timestamp except when converting a command to an explicit server-assigned simulation tick;
- random OS entropy;
- nondeterministic animation state.

All temporal inputs to game rules MUST be explicit canonical values.

## 13. Failure philosophy

Invalid authoritative states SHOULD fail loudly in development/test builds rather than being silently repaired with nondeterministic heuristics.

Examples:

- duplicate `SimId`;
- invalid content ID;
- impossible occupancy state;
- command applied to wrong tick;
- non-canonical serialized ordering where canonical ordering is required.

Network-facing invalid client commands are expected input and MUST be rejected safely rather than crashing the server.
