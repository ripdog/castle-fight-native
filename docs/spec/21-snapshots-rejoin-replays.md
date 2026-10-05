# Snapshots, Rejoin, Replays, and State Continuity

Status: **normative architecture, provisional retention/encoding details**

## 1. Purpose

This document defines how canonical match state is captured and restored so that players can rejoin live matches, clients can recover from desync, spectators can join late, and matches can be replayed deterministically.

These capabilities intentionally share one state-continuity mechanism.

## 2. Canonical snapshot

A snapshot is a complete logical representation of authoritative state at a specific completed simulation tick.

A snapshot labeled tick `T` means **all authoritative phases of tick `T`, including structural commit, victory evaluation, and checksum-visible state, have completed**. The snapshot also records a canonical stream position `P` through which all between-tick control records are included. Loading resumes from the first canonical stream record after `P`; tick `T` is never executed again, and the next simulation tick is `T + 1` once lifecycle state permits it.

A snapshot MUST contain enough information to resume the simulation with identical future results when supplied the same subsequent canonical stream.

It includes, directly or transitively:

- simulation compatibility version;
- protocol/snapshot schema version;
- content/map hashes or canonical references;
- completed tick number;
- exact finalized input-stream boundary included in the snapshot state;
- match seed;
- players/resources/game-mode state;
- canonical disconnect/delegated-builder-control state where gameplay-relevant;
- canonical match pause/abandonment state and the stream boundary at which it began;
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

`castle-fight-sim` exposes the logical authoritative schema identified by `AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION` through `SimulationSnapshot`. The schema is deliberately independent of Bevy entity handles and storage order. It records the completed boundary as `None` for the initial pre-tick state or `Some(T)` for a completed tick, stores allocator/player/lifecycle/objective/defense-alert state plus the complete canonical entity projection, and carries the authoritative state checksum. Restoration occurs into an already constructed compatible match instance: the immutable configuration/content identity must match, while execution-only settings such as worker count may differ. Restoration replaces the ECS world, preserves stable `SimId`s, rebuilds derived topology/caches, clears presentation-only event buffers, and verifies the stored checksum before accepting the state.

Step 10 adds a bounded JSON wire encoding of that same logical revision rather than a second authoritative-state schema. Static presentation names attached to content identities are omitted from the encoded state; map versions and rawcodes remain canonical and names are re-resolved from the already selected/version-checked content bundle before restoration. The decoder rejects a different identity version even inside cold production/construction/upgrade definitions; it MUST NOT replace that version with the selected bundle's default. Unknown rawcodes or malformed/beyond-bound snapshot data are rejected. Replay-file encoding remains separate/provisional.

A delayed action that consults versioned script/content definitions MUST retain its originating version independently of optional host/replacement identity. Removing its source entity or restoring an unlabelled synthetic body MUST NOT cause a process-default lookup. Wire decoding validates that retained version against the selected content bundle.

The checksum and snapshot capture paths MUST share the same canonical entity projection so adding an authoritative entity field cannot silently update one persistence mechanism without the other. Snapshot loading is intentionally valid under a different ECS insertion order than capture.

`MatchDriverSnapshot` schema revision 1 layers stream continuity on top of `SimulationSnapshot`. It captures the exact next canonical stream position, admitted pending commands, per-player admission sequence cursors, submission/deduplication records, applied canonical command sequences, canonical history, and replay-recording checkpoints. Restoring the driver validates the selected content identity, history positions, pending command tick/order, deduplication invariants, and simulation snapshot before replacing state. This is what prevents an accepted paid command from being lost or executed twice across reconnect/restore. Presentation-only command execution feedback is cleared rather than persisted.

`22-authoritative-state-inventory.md` is the field-level coverage checklist for the current implementation. Snapshot work MUST reconcile that inventory against the implementation before declaring restore complete, and MUST extend it for content-bundle, player/lifecycle, and command-stream state introduced by steps 3–6.

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

The server SHOULD retain periodic snapshots during a live match once measurements justify reuse. The current Step 10 TCP path instead captures a fresh snapshot at each reconnect/resynchronization handoff, minimizing required history suffix length and avoiding an unbounded retained-snapshot cache.

Cadence is provisional and should be selected from measurements balancing:

- snapshot generation CPU cost;
- memory/disk storage;
- network transfer size;
- reconnect fast-forward length;
- replay seek granularity.

A likely initial design is a snapshot every several seconds plus complete canonical stream history (tick bundles and boundary-control records) after the oldest retained live snapshot.

## 5. Snapshot creation and simulation stalls

Snapshot generation SHOULD minimize disruption to authoritative ticking.

Possible implementations include:

- serialize a canonical immutable/checkpoint view after a tick;
- incremental/copy-on-write snapshot preparation;
- copy compact canonical state then compress asynchronously;
- maintain serialized component tables optimized for checkpointing.

Compression and disk/network encoding may run asynchronously because encoded byte order need not influence gameplay. However, the logical state being encoded MUST correspond to one exact completed tick.

## 6. Live command history

The server retains canonical stream records with monotonic input-stream positions. Any snapshot/history handoff MUST pin a finite suffix from the snapshot stream position to a recorded handoff position before transfer starts. The current fresh-snapshot reconnect path normally needs only the canonical reconnect control after the captured boundary, and enforces an explicit maximum history-record count rather than growing a transfer backlog without bound.

Every snapshot records the exact input-stream position through which its state is complete. Inputs after that boundary are replayed exactly once.

Conceptually:

```text
Snapshot at completed T=10000, input boundary P=8301
Finalized T=10001, P=8302, no commands
Finalized T=10002, P=8303, no commands
Finalized T=10003, P=8304, command #0
...
Current finalized T=11200, P=9506
```

History MUST be complete and ordered, including logically empty finalized ticks (which may be wire-compressed).

## 7. Rejoin flow

A reconnecting player does not reconstitute the world from their stale local process state.

Current protocol revision 3 flow:

1. client reconnects/authenticates to the existing player/session slot using the server-issued session/token pair;
2. while that session is still canonically disconnected, the server captures a fresh snapshot and records its exact next-stream position `P_s`;
3. server emits the canonical `Connected` control and records the resulting handoff position `P_h`;
4. server queues the accepted session assignment, bounded snapshot metadata/chunks, and every canonical record in `[P_s, P_h)` to the replacement socket;
5. server queues `CatchUpComplete(P_h, completed_tick, checksum)` and only then adds the socket to live broadcast recipients;
6. client disables gameplay command submission while handoff is incomplete, replaces its local authoritative world with the snapshot, and creates a replica driver starting exactly at `P_s`;
7. client consumes the pinned canonical suffix strictly by stream position and rejects gaps, duplicate/overflowing chunks, or records beyond `P_h`;
8. presentation events generated by historical catch-up are cleared instead of replayed;
9. client verifies the completion tick/checksum at `P_h`, resets presentation/interpolation samples to the restored current state, and reenables gameplay command submission using the server-supplied next client sequence;
10. subsequent FIFO live records continue from `P_h` without changing canonical stream identity.

The handoff MUST have neither a gap nor an ambiguous overlap between snapshot history and live subscription. Required retained history MUST NOT be discarded while a reconnect transfer depends on it.

The player's units/buildings continued running on the server throughout disconnection.

## 8. Catch-up strategy

During catch-up, the client SHOULD:

- skip or minimize rendering work where practical;
- suppress most audio/cosmetic event playback;
- avoid replaying historical UI notifications unless relevant;
- retain authoritative simulation behavior exactly;
- process authoritative history as quickly as CPU allows.

The current client clears simulation presentation-event buffers after each catch-up record and publishes no intermediate presentation snapshots; completion replaces the interpolation sample pair with one snapshot of the recovered authoritative state. Thus historical attacks/spells do not become duplicate cosmetic effects after reconnect.

The client MAY join presentation slightly behind the newest server tick with a normal input-delay buffer rather than repeatedly chasing a moving exact tick.

If replay catch-up cannot close the gap quickly enough, the server SHOULD prefer a fresher snapshot rather than forcing a client to replay an impractically large backlog. Catch-up throughput is therefore an early measurable performance requirement, not an assumption.

## 9. Fresh snapshot vs old snapshot + replay

The server MAY choose between:

- sending a recent/current snapshot with minimal replay;
- reusing an older cached snapshot plus more command history.

The choice is operational and MUST NOT affect final state. The initial Step 10 implementation deliberately chooses a freshly encoded current snapshot for every replacement, so catch-up work is bounded by snapshot size plus a small pinned history suffix rather than disconnection duration. Snapshot caching can be added later if measurement shows encoding cost dominates.

## 10. Desync resynchronization

Desync recovery uses the same snapshot/chunk/history/completion machinery as reconnect. The client-side replacement path accepts an authenticated snapshot transfer while already connected, disables command submission after detecting/reporting a checkpoint mismatch, replaces divergent state, clears historical presentation events, verifies the supplied authoritative boundary/checksum, and then resumes live processing.

The TCP server initiates replacement only after comparing the client's checkpoint report to the latest authoritative checkpoint and proving a checksum mismatch. It then captures a fresh snapshot at the current canonical stream boundary and sends a bounded transfer on that same authenticated socket; the current implementation therefore uses an empty history suffix for live desync recovery. The socket remains the same live session, and subsequent canonical messages queue after `CatchUpComplete` in FIFO order. A client MUST NOT attempt to merge arbitrary divergent entity state into the server snapshot.

## 11. Replay file model

A replay SHOULD be representable as:

```text
ReplayHeader
InitialSnapshot (or deterministic match-construction data)
FinalizedTickInputs stream
Optional periodic seek snapshots
Optional checksum checkpoints
Optional metadata/chat/events not affecting simulation
```

The ordered canonical stream of finalized tick bundles plus between-tick control records is canonical gameplay history.

A replay player runs the same deterministic simulation code rather than storing every entity transform for every frame.

The current logical implementation is `MatchReplay` schema revision 1. `MatchDriver` retains its creation-time `SimulationSnapshot`, every canonical tick/control record, and a checksum checkpoint after each record; `export_replay()` packages those with explicit replay/snapshot/checksum schema revisions, map/release identity, content gameplay identity, and configuration identity. Playback restores the initial state and feeds the canonical stream back through `MatchDriver`, verifying checkpoints as it advances. Optional `MatchDriverSnapshot` seek points can be attached only at command-free canonical boundaries; playback chooses the nearest one not beyond the requested stream position and resumes from there. This remains an in-memory logical replay model: bounded file decoding, byte encoding, compression, and storage retention are intentionally deferred.

## 12. Replay seeking

To seek to tick `T`:

1. choose nearest compatible snapshot/checkpoint `S <= T`;
2. load `S`;
3. replay canonical stream records;
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

The on-disk encoding remains provisional. The current on-wire reconnect/resynchronization encoding is schema-versioned JSON over the existing bounded protocol frames: one logical snapshot is capped at 8 MiB and split into 48 KiB chunks, with explicit transfer ID, byte/chunk counts, snapshot stream/checksum metadata, and a separately bounded canonical history suffix.

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
- canonical stream records after it;
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
