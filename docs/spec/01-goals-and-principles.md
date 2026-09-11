# Goals and Architectural Principles

Status: **normative unless marked otherwise**

## 1. Product goal

Castle Fight Native is a native multiplayer autobattler inspired by the Warcraft III Castle Fight map family. Players construct production and attack buildings; production buildings autonomously create units; units autonomously advance, acquire targets, fight, and die; the opposing castle is the strategic objective.

The project is not intended to emulate Warcraft III internally. It SHOULD reproduce the strategically important behaviours of the game while using an architecture designed for modern multicore CPUs, robust online play, deterministic replays, and very large unit counts.

## 2. Primary goals

### 2.1 High unit-count simulation

The engine SHOULD remain playable with unit counts well above the range where Warcraft III becomes CPU-bound. The simulation architecture MUST support parallel execution of expensive per-entity work across multiple CPU cores without changing game outcomes.

No fixed maximum unit count is specified yet. Performance targets will be established through benchmarks rather than chosen from intuition.

### 2.2 Deterministic multiplayer

Given:

- the same simulation build and content version,
- the same initial match state,
- the same match seed,
- and the same ordered stream of accepted player commands,

every conforming simulation instance MUST produce the same authoritative world state and checksum at every synchronization checkpoint.

Determinism MUST hold independently of:

- worker-thread count,
- worker scheduling,
- rendering frame rate,
- machine load,
- wall-clock timing,
- and whether the simulation is running interactively, headlessly, or during replay fast-forward.

### 2.3 Authoritative server with reconnect

The server MUST be the canonical authority for match state and accepted player commands.

A client MAY predict/run the complete deterministic simulation locally, but server state wins on disagreement.

A player MUST be able to disconnect and later rejoin while the match continues. Rejoin MUST NOT require the other players to pause.

### 2.4 Preserve emergent strategy

The engine MUST avoid convenience rules that erase strategically meaningful emergent behaviour.

In particular, the game MUST support **caging**: a player may intentionally construct an enclosed region using buildings and trap friendly units inside. Those trapped units may remain useful as targets that attract otherwise valid enemy attacks.

The engine therefore MUST NOT globally enforce that every spawned unit has a navigable path to the enemy castle.

### 2.5 Individual autonomous units

Every combat unit MUST make its own deterministic target-selection decision.

The engine MUST NOT assume that nearby units share a squad target, formation controller, or collective combat decision merely as an optimization.

Shared acceleration structures such as spatial grids, flow fields, or navigation maps are allowed and encouraged, provided they do not collapse individual decision-making.

## 3. Technology direction

### 3.1 Rust

Rust is the preferred implementation language.

The main reasons are architectural rather than stylistic:

- compile-time aliasing and mutability rules support safe parallel simulation;
- `Send`/`Sync` constraints help prevent accidental cross-thread state hazards;
- enums and exhaustive `match` are well suited to explicit command/effect protocols;
- Cargo workspaces provide clean crate boundaries;
- serialization, property testing, benchmarking, and tracing ecosystems are strong;
- headless Linux server deployment is straightforward;
- memory safety is achieved without a garbage collector.

### 3.2 Bevy

Bevy is the preferred client engine and `bevy_ecs` is the preferred ECS foundation for the simulation prototype.

The authoritative simulation SHOULD depend on `bevy_ecs` rather than the entire Bevy application stack unless a broader dependency is justified.

Bevy scheduling is an implementation mechanism, not a source of gameplay ordering semantics. Authoritative ordering MUST be defined by this specification and enforced explicitly.

## 4. Architectural principles

### 4.1 Simulation and presentation are separate worlds

Authoritative state is the state that determines game outcomes. Presentation state exists only to show authoritative state to a player.

Examples of authoritative state:

- simulation position,
- health,
- cooldowns,
- target identity,
- building occupancy,
- production timers,
- buffs/debuffs,
- projectiles that have gameplay consequences,
- player resources.

Examples of non-authoritative presentation state:

- interpolated render transforms,
- particles,
- animation blending,
- camera shake,
- UI transitions,
- purely cosmetic ragdolls,
- sound playback timing.

Presentation MUST NOT feed nondeterministic results back into the authoritative simulation.

### 4.2 Read, decide, then commit

Where practical, a simulation phase SHOULD follow this pattern:

```text
immutable phase input
        ↓
parallel per-entity decisions
        ↓
thread-local / partition-local intents
        ↓
deterministic merge or keyed reduction
        ↓
authoritative mutation
```

Workers SHOULD NOT race to mutate shared gameplay state.

### 4.3 Optimize the problem before parallelizing it

The engine SHOULD reduce algorithmic complexity before adding threads.

Examples:

- use a spatial grid instead of all-pairs proximity checks;
- use flow/integration fields or equivalent shared navigation data instead of running a full path search for every unit;
- update expensive low-frequency decisions less often where game rules permit it;
- keep hot component data compact and contiguous.

### 4.4 Stable game identity is not ECS identity

ECS entity handles are runtime implementation details.

Every authoritative game object that needs stable identity MUST have a deterministic simulation identifier (`SimId` or equivalent). Stable IDs are used for:

- deterministic tie-breaking,
- network references,
- snapshots,
- replays,
- checksums,
- deterministic random-number keys,
- diagnostics.

### 4.5 Explicit synchronization boundaries

Parallel work may execute in any order within a phase only when that ordering cannot affect the authoritative result.

If ordering can affect gameplay, the rule MUST be represented explicitly by one of:

- a phase dependency,
- a deterministic sort key,
- a deterministic reduction,
- or a documented simultaneous-resolution rule.

Thread completion order MUST NEVER become an implicit game mechanic.

## 5. Gameplay compatibility principles

### 5.1 Navigation is not targetability

Whether entity A can navigate to entity B MUST NOT automatically determine whether A may attack B.

Target eligibility is governed by combat rules such as:

- team relationship,
- acquisition range,
- attack range,
- target category,
- visibility/line-of-sight if applicable,
- attack restrictions,
- priority and retention rules.

A unit trapped in a cage can therefore remain a valid target for enemy ranged units or attack buildings.

### 5.2 Buildings are real world geometry

Player buildings MUST affect navigation occupancy according to their footprints.

The engine MUST permit legal placement that creates enclosed or disconnected traversable regions unless a later gameplay rule explicitly forbids a particular placement for reasons independent of connectivity.

### 5.3 Standard combat units are autonomous and never player-commandable

This is a core game rule, not merely an optimization opportunity.

Players MUST NOT be able to issue move, attack, stop, hold-position, focus-target, ability, formation, or other direct orders to ordinary combat units. Standard units move, acquire targets, attack, and use any autonomous abilities solely according to deterministic simulation rules.

The client MAY allow standard units to be selected or inspected for information, but selection MUST NOT expose a control surface that can alter that unit's behavior.

The network protocol and authoritative simulation SHOULD therefore avoid a generic `OrderUnit`/RTS command API. Any later game mode that introduces commandable combat units would be a deliberate ruleset extension rather than latent capability of the core game.

### 5.4 The builder is the player's sole directly controlled unit

Each active player controls exactly one builder unit for normal gameplay.

The builder exists for two purposes:

1. move around the player's owned build region and construct buildings there;
2. hold and use items.

The builder itself is non-combat. It MUST NOT:

- attack;
- be a valid combat target;
- take ordinary combat damage;
- block combat-unit pathing;
- participate in local combat-unit separation/collision;
- alter combat-unit target choice merely by proximity or by being the origin/carrier of an item effect.

The builder's authoritative position remains gameplay state because building interaction, item range, and active item targeting may depend on it. Item effects carried or activated by the builder are separate gameplay effects; they do not make the builder itself a combat participant.

Building placement MUST be restricted to the player's canonical owned build region (the player's third of the battlefield in the standard ruleset).

## 6. Multiplayer philosophy

The network model SHOULD combine the bandwidth efficiency of deterministic command replication with the recoverability of an authoritative server.

Clients SHOULD normally receive accepted player commands and simulate the resulting world locally rather than receiving continuous transforms for every unit.

The server MUST retain enough canonical state/history to recover a client from:

- temporary disconnect,
- process restart,
- desynchronization,
- late join as spectator if supported.

## 7. Non-goals

The first implementation is not required to provide:

- a general-purpose RTS engine;
- arbitrary per-unit player orders;
- a general rigid-body physics simulation;
- a Unity/Godot-style scene editor;
- deterministic cosmetic effects;
- compatibility between arbitrary simulation versions;
- peer-to-peer authority;
- anti-cheat based solely on trusting client simulation results.

## 8. Specification discipline

The project SHOULD prefer explicit invariants over clever implementation shortcuts.

When an optimization conflicts with an observable gameplay behaviour, the observable behaviour wins unless the design is deliberately changed and documented.

Performance-sensitive shortcuts MUST be covered by determinism and behavioural tests so that optimization cannot silently alter match outcomes.
