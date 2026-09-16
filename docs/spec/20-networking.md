# Multiplayer Networking Architecture

Status: **normative architecture, provisional transport details**

## 1. Purpose

This document defines the multiplayer authority model and the relationship between the authoritative server and deterministic client simulations.

The design combines deterministic command replication with an authoritative canonical server so that high unit counts do not require streaming every unit transform while disconnect/rejoin and desync recovery remain possible.

## 2. Authority model

The server is the sole canonical authority for:

- admitted/scheduled player commands;
- canonical stream (finalized tick inputs plus boundary-control records) and command execution outcomes;
- current canonical match state;
- match lifecycle;
- reconnect/snapshot state;
- desync resolution;
- victory result.

Clients may run the same simulation for responsiveness and bandwidth efficiency, but client state never overrides server state.

## 3. Shared deterministic simulation

Server and clients MUST execute the same compatible simulation version/content bundle.

The authoritative simulation crate SHOULD be shared source/code, not independently reimplemented.

Clients normally advance from:

```text
canonical starting snapshot/state
+ ordered canonical stream
```

rather than receiving continuous per-unit position updates.

## 4. Client commands

Players issue explicit validated gameplay commands, never direct state mutations. The sole direct unit-control command in the standard ruleset is movement of the player's own builder; ordinary combat units have no player-order surface.

The protocol deliberately exposes specific game actions rather than a generic RTS unit-order envelope. Ordinary combat units are not commandable.

The current executable development slice uses this explicit vocabulary:

```rust
pub enum PlayerCommand {
    MoveBuilder { builder: SimId, destination: SimPoint },
    FollowWithBuilder { builder: SimId, target: SimId },
    StopBuilder { builder: SimId },
    BlinkBuilder { builder: SimId, destination: SimPoint },
    RepairWithBuilder { builder: SimId, target: SimId },
    SetBuilderRepairAutocast { builder: SimId, enabled: bool },
    PlaceBuilding {
        builder: SimId,
        building: CastleFightBuildingId,
        position: BuildPosition,
    },
    CancelBuildingConstruction { building: SimId },
    UpgradeBuilding {
        building: SimId,
        target: CastleFightBuildingId,
    },
    AttackWithBuilding { building: SimId, target: SimId },
}
```

`BuildPosition` contains canonical integer grid coordinates only. Footprint size, cost, stats, production profile, attack profile, and other building properties are resolved from the selected authoritative content bundle; they are never caller-authored command payload. Discrete rotation may be added later if/when an implemented building requires it. Active items and explicitly manual building abilities extend this same vocabulary when their mechanics are implemented in step 11.

There MUST NOT be a general `MoveUnit`, `AttackUnit`, `StopUnit`, `CastUnitAbility`, or `OrderUnit` command for ordinary combat units in the standard ruleset.

A client command includes protocol metadata sufficient to associate it with:

- connection/player identity;
- client command sequence/nonce;
- optional client-observed tick for latency diagnostics/prediction;
- command payload.

The server first performs **admission validation** and, if the request is admissible, schedules it for a canonical future tick. Scheduling does not guarantee that the gameplay action will still be valid when that tick executes.

## 5. Command admission and execution validation

The server MUST perform admission validation before scheduling obviously invalid or unauthorized requests. Admission checks include protocol shape, ownership/permission, content identity, target coordinate encoding, match phase, and abuse/rate limits.

Gameplay-state checks whose truth may change before execution MUST be evaluated again by the shared deterministic simulation at the scheduled tick, in canonical command order.

Admission examples:

- requested content/command type exists and is permitted;
- command is issued by the owning player/session, or by a currently delegated teammate for a disconnected player's builder where the rules allow it;
- no command attempts to direct an ordinary combat unit;
- coordinates/identifiers are well-formed and in representable bounds;
- command is allowed in the current match phase;
- rate/abuse limits are satisfied.

Execution-time examples:

- sufficient resources still exist;
- the builder/building/item still exists and the issuing player still has owner or delegated control permission at that canonical tick;
- a build position is still legal and unoccupied;
- an item/ability is still ready and affordable;
- an entity target still exists and remains eligible where the ability requires that;
- a sale target still exists and is sellable.

The server MUST NOT trust a client's local prediction or resource count.

Execution failure is an ordinary deterministic simulation outcome. It MUST leave no partial gameplay mutation and MUST produce a structured `CommandOutcome` suitable for UI feedback.

## 6. Tick assignment

The server assigns scheduled commands to a canonical simulation tick and deterministic order within that tick.

This policy must balance:

- player input latency;
- network jitter;
- deterministic replication;
- ability for all clients to receive commands before execution when practical.

The exact input-delay policy is open, but the result must be explicit:

```rust
pub struct ScheduledCommand {
    pub tick: Tick,
    pub order: CommandOrder,
    pub player: PlayerId,
    pub client_sequence: ClientCommandSequence,
    pub command: PlayerCommand,
}

pub enum CommandOutcome {
    Executed(CommandExecutionResult),
    Rejected(CommandRejectReason),
}
```

`CommandOrder` MUST be stable/canonical and generated by the server. For the same initial state and scheduled command stream, execution outcomes MUST be identical on server, clients, and replay.

## 7. Late commands

A command arriving after its permissible scheduling window MUST NOT be retroactively inserted into an already-finalized tick on some peers.

The server must either:

- schedule it for a later valid tick;
- or reject it according to protocol/gameplay rules.

Clients receive the authoritative scheduled tick/order.

## 8. Local prediction

The client MAY predict user-facing command effects, especially:

- placement ghost acceptance;
- resource/UI feedback;
- building appearance;
- command latency hiding.

Prediction is presentation/local speculative state until server acceptance.

When scheduled command details differ from prediction, or execution later rejects the action, the client MUST reconcile to canonical state and surface the deterministic outcome.

Early implementations MAY avoid gameplay prediction and simply wait for scheduled/executed outcomes if latency is acceptable.

## 9. Finalized tick input stream

The server MUST provide positive proof that each simulation tick's input set is complete. Silence is not proof of an empty tick.

The canonical stream contains both finalized simulation-tick bundles and explicit between-tick control records:

```rust
pub enum CanonicalStreamRecord {
    Tick(FinalizedTickInputs),
    Control(BoundaryControlRecord),
}

pub struct FinalizedTickInputs {
    pub tick: Tick,
    pub stream_position: InputStreamPosition,
    pub commands: Vec<ScheduledCommand>, // canonical CommandOrder
    pub server_events: Vec<ServerAuthoredInput>,
}

pub struct BoundaryControlRecord {
    pub after_tick: Tick,
    pub stream_position: InputStreamPosition,
    pub event: MatchControlEvent,
}
```

An empty simulation tick is represented explicitly. The wire format MAY compact consecutive empty finalized ticks into a range, provided the logical stream is identical.

Clients MUST NOT execute tick `T` until they possess proof that all authoritative inputs for `T` are finalized and complete. They also MUST process all preceding boundary-control records before advancing. Sequence/stream-position gaps MUST be recovered before crossing the gap.

Between-tick control records exist because some canonical lifecycle events occur while simulation ticks are stopped. Examples include disconnect/reconnect permission changes, team-wide pause, resume, and disconnect-timeout match termination. A pause record after completed tick `T` prevents `T+1` from executing until a later canonical resume record exists (or the match-end record terminates the match).

Transport messages may arrive out of order; the simulation-facing canonical stream MUST not.

`InputStreamPosition` is the monotonic logical ordering key used for reconnect, duplicate suppression, and history retention. It is protocol continuity state and MUST NOT by itself enter gameplay checksums; the gameplay-relevant state produced by applying control records does.

## 10. State checksums

At configured intervals, server and clients compute the canonical simulation checksum defined by `11-determinism.md`.

The server may broadcast:

```text
(tick, state_checksum)
```

Clients compare once they have reached the same tick.

A mismatch triggers a desync recovery path rather than allowing divergent simulations to continue indefinitely.

## 11. Desync recovery

The server wins all state disputes.

On mismatch, protocol behavior SHOULD support:

1. client reports/recognizes mismatch;
2. client pauses presentation of speculative future authoritative state as needed;
3. server selects an appropriate canonical snapshot/checkpoint;
4. client loads the snapshot;
5. client replays canonical stream records to the live boundary;
6. checksum is revalidated;
7. normal pacing resumes.

Repeated mismatch after resync is an implementation/version integrity fault and should be reported with diagnostics.

## 12. Disconnect behavior

A single client disconnect does not stop the match. The server continues simulating that player's autonomous state, and a canonical disconnect event grants temporary builder-control permission to still-connected teammates as defined by `41-match-gameplay.md` and `42-builder-items.md`.

If every player on one team is disconnected, the server finalizes the current tick, records a canonical team-disconnected pause event, and stops advancing simulation ticks while a configured wall-clock reconnect timeout runs. A reconnect before expiry records a canonical resume event; expiry records a canonical match-end/forfeit event.

The paused wall-clock interval itself is operational time, not simulation time. Replay applies the recorded control events at their stream positions without waiting.

## 13. Reconnect identity

Reconnect requires secure restoration of the correct player/session identity.

Authentication/session details are outside the simulation crate, but the protocol must prevent a reconnecting client from claiming another player's slot merely by sending a `PlayerId`.

Server-issued reconnect/session credentials should be separate from deterministic gameplay state.

## 14. Transport

The initial native multiplayer prototype uses **TCP** as its reliable ordered transport. This is an operational transport choice, not part of canonical gameplay identity: protocol messages and canonical stream semantics remain transport-independent so a later QUIC or other transport can replace TCP without changing authoritative simulation ordering.

Each TCP connection carries a sequence of bounded protocol frames:

```text
[u32 payload length, big-endian][payload bytes]
```

Step 9 payloads use the protocol crate's schema-versioned JSON encoding and are limited to 1 MiB per frame. The decoder MUST reject zero-length and oversized frames before allocating the declared body. Snapshot/reconnect transfer introduced in Step 10 may define a separate bounded bulk-transfer/chunking policy rather than increasing ordinary command/control message limits without review.

TCP's byte-stream ordering does not replace canonical logical ordering. Every finalized tick/control record still carries `InputStreamPosition`, and clients MUST NOT infer an empty tick from transport silence, connection liveness, or lack of immediately available bytes. A missing/disconnected TCP stream therefore never means "advance with no commands"; only an explicit finalized tick record proves that input set complete.

Server implementations SHOULD enable `TCP_NODELAY` for latency-sensitive command/control traffic. Large reconnect snapshots MUST NOT be allowed to indefinitely head-of-line block live canonical traffic; Step 10 must address this through bounded chunking, a separate bulk channel/connection, or a transport revision such as QUIC streams.

Because authoritative gameplay mostly transmits low-rate commands, finalized-input records, outcomes, and checkpoints rather than frame-by-frame unit transforms, correctness and reconnect semantics matter more initially than minimizing every packet's transport latency. QUIC/reliable-UDP remain valid future choices if measured behavior justifies the extra transport complexity.

The simulation/protocol types MUST not be tightly coupled to one transport library.

## 15. Reliable vs ephemeral data

The following is authoritative/reliable:

- scheduled player commands;
- deterministic command execution outcomes;
- snapshots;
- reconnect state;
- simulation/content compatibility metadata;
- match start/config;
- final result;
- checksum checkpoints.

Pure telemetry/presence/ping data may use weaker delivery semantics.

No gameplay command may be considered executed merely because it was submitted or scheduled. The client applies only commands present in finalized tick inputs and derives the same execution outcome as the authoritative simulation.

## 16. Match startup

Before tick 0/first active tick, all participants MUST agree on or receive:

- protocol version;
- simulation compatibility version;
- content bundle hash(es);
- map hash/version;
- match configuration;
- player/team assignments;
- match seed;
- initial canonical state or enough data to construct it deterministically.

A client with incompatible simulation/content MUST be rejected before joining as an active deterministic participant.

## 17. Server ticking and command horizon

The server owns the canonical current tick.

A command scheduling horizon/input delay may be used so clients receive scheduled commands and finalized input proof before execution. The exact value should adapt to expected latency but MUST be represented as protocol/game configuration rather than inferred separately by each client.

The design should avoid hard coupling to LAN-level latency.

## 18. Spectators

Spectators are a natural extension of the snapshot + command-stream architecture.

A spectator can:

1. authenticate/authorize;
2. receive a recent/current canonical snapshot;
3. receive subsequent canonical stream records;
4. fast-forward to live tick;
5. continue deterministic simulation locally.

Spectator support is not required for the first playable milestone but should not be architecturally blocked.

## 19. Bandwidth model

The intended steady-state gameplay bandwidth scales primarily with:

- player command rate;
- checksum/checkpoint rate;
- protocol overhead;

not with total unit count.

High unit count increases CPU/GPU cost but should not require sending thousands of transforms every network frame.

Snapshots are larger but infrequent and compressible.

## 20. Security boundaries

Clients are untrusted.

The server MUST NOT accept:

- client-provided entity state as canonical;
- client-computed resource balances;
- client-selected outcome of random events;
- client-selected target/combat results;
- arbitrary past tick insertion;
- commands referencing inaccessible/invalid entities without validation.

Determinism is not an anti-cheat mechanism by itself; authority remains server-side.

## 21. Operational failure handling

The protocol should distinguish:

- transient transport loss;
- reconnecting/catching up;
- incompatible version;
- invalid command;
- desync detected;
- snapshot/resync in progress;
- server overload/shutdown;
- match completed.

These should be explicit states/errors, not inferred from timeout alone wherever possible.

## 22. Required multiplayer tests

The integration suite MUST eventually include:

1. two clients + server receive same command stream and maintain matching checksums;
2. different client worker counts remain synchronized;
3. packet reordering does not reorder canonical commands;
4. duplicate client command is not applied twice;
5. invalid build command is rejected without desync;
6. disconnecting player leaves autonomous army/buildings simulated;
7. reconnect catches up to current tick and matches checksum;
8. deliberately corrupted client state is repaired from server snapshot;
9. incompatible content/simulation version cannot join;
10. high unit count does not materially increase steady-state command bandwidth in absence of more player actions.
