# Snapshots, Rejoin, Replays, and State Continuity

Status: **normative architecture, provisional retention/encoding details**

## 1. Purpose

This document defines how canonical match state is captured and restored so that players can rejoin live matches, clients can recover from desync, spectators can join late, and matches can be replayed deterministically.

These capabilities intentionally share one state-continuity mechanism.

## 2. Canonical snapshot

A snapshot is a complete logical representation of authoritative state at a specific completed simulation tick.

A snapshot MUST contain enough information to resume the simulation with identical future results when supplied the same subsequent accepted commands.

It includes, directly or transitively:

- simulation compatibility version;
- protocol/snapshot schema version;
- content/map hashes or canonical references;
- completed tick number;
- match seed;
- players/resources/game-mode state;
- all authoritative entities sorted or encoded by stable `SimId`;
- canonical components/state;
- deterministic allocators/counters;
- pending authoritative timers/projectiles/effects required across ticks;
- builder position/movement and inventory state;
- item charges/cooldowns/automatic-effect sequences;
- building mana, ability cooldowns/charges, and cast sequences;
- captured ballistic impact positions and bounce/guaranteed-hit projectile state where still live;
- any canonical global resources.

Derived caches SHOULD be excluded if safely reconstructable.

## 3. Snapshot loading

Loading a snapshot MUST be deterministic regardless of ECS insertion/archetype order.

The loader SHOULD:

1. validate format/version/content compatibility;
2. clear/replace the target authoritative simulation world;
3. restore canonical resources;
4. recreate entities retaining original `SimId`s;
5. restore canonical component state;
6. restore deterministic counters/allocators;
7. rebuild `SimId -> Entity` lookup;
8. rebuild spatial/navigation/other derived indexes;
9. run invariant validation;
10. verify the snapshot's stored state checksum if present.

No new gameplay RNG values or IDs may be consumed merely because a snapshot was loaded.

## 4. Snapshot cadence

The server SHOULD retain periodic snapshots during a live match.

Cadence is provisional and should be selected from measurements balancing:

- snapshot generation CPU cost;
- memory/disk storage;
- network transfer size;
- reconnect fast-forward length;
- replay seek granularity.

A likely initial design is a snapshot every several seconds plus a complete accepted-command log after the oldest retained live snapshot.

## 5. Snapshot creation and simulation stalls

Snapshot generation SHOULD minimize disruption to authoritative ticking.

Possible implementations include:

- serialize a canonical immutable/checkpoint view after a tick;
- incremental/copy-on-write snapshot preparation;
- copy compact canonical state then compress asynchronously;
- maintain serialized component tables optimized for checkpointing.

Compression and disk/network encoding may run asynchronously because encoded byte order need not influence gameplay. However, the logical state being encoded MUST correspond to one exact completed tick.

## 6. Live command history

The server retains accepted commands with canonical tick/order for at least the period needed to advance from the oldest reconnect/desync snapshot to current time.

Conceptually:

```text
Snapshot at T=10000
AcceptedCommand T=10003 #0
AcceptedCommand T=10007 #0
AcceptedCommand T=10007 #1
...
Current T=11200
```

Command history MUST be complete and ordered.

## 7. Rejoin flow

A reconnecting player does not reconstitute the world from their stale local process state.

Recommended flow:

1. client reconnects/authenticates to the existing player/session slot;
2. server reports canonical current tick/version;
3. server selects a suitable snapshot `S <= current`;
4. server sends snapshot plus accepted commands after `S`;
5. client replaces its authoritative local world with snapshot;
6. client disables or minimizes presentation work;
7. client replays commands and advances as fast as possible;
8. client reaches a server-defined near-live tick/checkpoint;
9. checksum is verified;
10. normal real-time pacing/presentation resumes.

The player's units/buildings continued running on the server throughout disconnection.

## 8. Catch-up strategy

During catch-up, the client SHOULD:

- skip rendering frames;
- suppress most audio/cosmetic event playback;
- avoid replaying historical UI notifications unless relevant;
- retain authoritative simulation behavior exactly;
- process ticks as quickly as CPU allows.

The client MAY join presentation slightly behind the newest server tick with a normal input-delay buffer rather than repeatedly chasing a moving exact tick.

## 9. Fresh snapshot vs old snapshot + replay

The server MAY choose between:

- sending a recent/current snapshot with minimal replay;
- reusing an older cached snapshot plus more command history.

The choice is operational and MUST NOT affect final state.

A freshly encoded current snapshot may be beneficial for very long disconnects.

## 10. Desync resynchronization

Desync recovery uses the same machinery as reconnect.

The server identifies a trusted canonical snapshot/checkpoint and instructs the client to replace divergent state and replay forward.

A client MUST NOT attempt to merge arbitrary divergent entity state into the server snapshot.

## 11. Replay file model

A replay SHOULD be representable as:

```text
ReplayHeader
InitialSnapshot (or deterministic match-construction data)
AcceptedCommand stream
Optional periodic seek snapshots
Optional checksum checkpoints
Optional metadata/chat/events not affecting simulation
```

The accepted command stream is canonical gameplay history.

A replay player runs the same deterministic simulation code rather than storing every entity transform for every frame.

## 12. Replay seeking

To seek to tick `T`:

1. choose nearest compatible snapshot/checkpoint `S <= T`;
2. load `S`;
3. replay accepted commands;
4. simulate unpaced to `T`;
5. render state.

Additional seek snapshots trade replay size for faster seeking.

## 13. Replay compatibility

A replay MUST declare the simulation/content versions/hashes needed to interpret it.

Long-term replay compatibility is not guaranteed automatically.

Options to evaluate later:

- ship/version old simulation implementations;
- provide explicit migration for selected versions;
- mark unsupported old replays clearly;
- archive standalone simulation build/container metadata for tournament matches.

The first implementation only needs strict same-compatible-version playback.

## 14. Snapshot format

The on-wire/on-disk encoding is provisional.

Requirements:

- schema/versioned;
- bounded decoding with defensive validation;
- efficient enough for large matches;
- compressible;
- independent of raw memory layout/padding;
- independent of Bevy internal ECS serialization;
- stable explicit discriminants for persistent enums/IDs.

Potential Rust serialization choices should be benchmarked rather than selected solely for convenience.

## 15. Compression

Snapshots are expected to contain repeated component arrays/IDs and SHOULD compress well.

Compression is non-authoritative and MAY vary by platform/level. Decompressed logical state plus compatibility metadata is what matters.

Compression should occur off the critical simulation path where practical.

## 16. Persistence and server restart

A later dedicated-server milestone SHOULD support surviving server process restart by persisting:

- a recent canonical snapshot;
- accepted commands after it;
- match/session metadata needed to resume ownership/authentication.

This is separate from ordinary player reconnect but deliberately uses the same canonical state machinery.

Crash-consistency policy is open.

## 17. Partial state is not enough for authoritative restore

A reconnect snapshot MUST NOT omit state merely because it is not currently visible to the reconnecting player.

Fog-of-war/privacy, if introduced, complicates this. The first architecture assumes deterministic clients have sufficient state to run the simulation. If hidden information becomes a game requirement, the networking/simulation model must be revisited deliberately rather than sending incomplete state that cannot reproduce the server.

## 18. Snapshot integrity

Snapshots SHOULD carry integrity metadata such as:

- state checksum;
- uncompressed length;
- content/version identifiers;
- transport-level hash/checksum.

Untrusted snapshot bytes MUST be validated before allocation/use to avoid denial-of-service or malformed-state attacks.

## 19. Replay/debug value

Because reproduction is deterministic, bug reports SHOULD eventually be able to attach a compact reproduction consisting of:

- compatible build/content version;
- snapshot or match seed/config;
- command log;
- first unexpected tick/checksum.

This is expected to be one of the project's strongest debugging tools.

## 20. Required tests

The continuity suite MUST eventually include:

1. snapshot -> reload -> next 10,000 ticks matches uninterrupted simulation;
2. snapshot loaded with different ECS insertion order still matches checksum;
3. reconnect after short disconnect catches up and matches server;
4. reconnect after long disconnect using fresher snapshot matches server;
5. corrupt local client state then resync snapshot restores equality;
6. replay from initial state reproduces final checksum;
7. seek snapshot + replay produces same state as replay from tick zero;
8. snapshot compression/decompression does not alter logical state;
9. incompatible content/simulation snapshot is rejected;
10. deterministic allocators continue with identical next `SimId` after restore.
