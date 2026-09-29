# Determinism Contract

Status: **normative**

## 1. Purpose

Determinism is a multiplayer, replay, testing, and recovery requirement rather than an implementation preference.

For a fixed simulation version, content bundle, initial state, match seed, and ordered canonical stream of finalized tick inputs plus gameplay-relevant boundary-control records, every conforming simulation instance MUST produce identical authoritative state at every tick checkpoint.

## 2. Determinism boundary

The authoritative simulation includes all values that can affect future gameplay outcomes.

The following need not be deterministic:

- rendering;
- animation interpolation;
- particle systems with no gameplay effect;
- audio playback;
- UI transitions;
- camera state;
- diagnostic timestamps;
- log ordering from parallel workers.

No nondeterministic presentation value may feed back into authoritative state.

## 3. Numeric representation

### 3.1 Floating point

Authoritative gameplay SHOULD NOT use IEEE floating-point values for state or calculations unless a specific use is demonstrated to be bit-identical on every supported target and compiler configuration.

The default is integer or fixed-point arithmetic.

### 3.2 Fixed point

Authoritative position, velocity, ranges, and other fractional quantities SHOULD use explicit fixed-point types.

The exact representation is provisional. Candidate forms include:

- signed integer map subunits;
- Q-format fixed point backed by `i32` or `i64`;
- a small project-local fixed-point wrapper with checked/saturating operations where appropriate.

Conversions from content data into runtime fixed-point values MUST be validated before a match begins.

### 3.3 Overflow

Overflow semantics MUST be explicit.

Authoritative code MUST NOT depend on Rust debug/release overflow differences.

Each arithmetic domain MUST choose one of:

- values proven not to overflow by validation/invariants;
- checked arithmetic with deterministic failure handling;
- saturating arithmetic where saturation is part of the game rule;
- deliberately wrapping arithmetic for hash/RNG primitives only where specified.

### 3.4 Rounding, division, and geometry intermediates

The initial arithmetic contract is:

- signed integer/fixed-point division truncates toward zero;
- fixed-point multiplication uses a widened intermediate before rescaling, then truncates toward zero;
- code MUST NOT use right-shift as a substitute for signed division where negative values are possible;
- squared distance/dot-product intermediates use a width proven sufficient for the validated coordinate/range bounds; the public `SimPoint` squared-distance helper uses a wider intermediate and saturates to `u64::MAX` for untrusted coordinates outside those validated bounds so malformed command input cannot trigger debug/release-dependent overflow;
- normalization uses deterministic integer/fixed-point math with an explicitly specified integer square-root/length routine rather than floating point;
- content/map loading validates coordinate, velocity, range, and radius bounds so those intermediate-width proofs remain true;
- conversions between fixed-point scales use explicit documented rounding rather than casts whose intent is unclear.

If the selected fixed-point crate/library does not expose these exact semantics, the simulation MUST wrap or replace the relevant operations rather than inherit different behavior implicitly.

These arithmetic rules are part of the simulation version and require fixture tests around zero, negative values, half-unit boundaries, and maximum supported coordinates.

## 4. Stable identity

Every authoritative entity that may be referenced across ticks MUST have a stable simulation ID.

Provisional form:

```rust
#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SimId(pub u64);
```

`SimId` allocation MUST be deterministic.

The allocator state is canonical simulation state and MUST be included in snapshots/checksums as appropriate.

Bevy `Entity` IDs MUST NOT be used as:

- network identity;
- replay identity;
- cross-run serialized identity;
- deterministic RNG key;
- final tie-break criterion.

## 5. Canonical ordering

Any operation whose result can depend on iteration order MUST define canonical ordering.

Potentially dangerous sources include:

- `HashMap` / `HashSet` iteration;
- ECS query ordering;
- parallel iterator completion order;
- thread-local buffer concatenation order;
- filesystem enumeration;
- serialization map ordering;
- network receive ordering for commands intended for the same simulation tick.

Where order affects the result, values MUST be sorted or reduced using an explicit stable key.

`SimId` is the default final tie-breaker unless a game rule specifies another canonical key.

## 6. Target-selection ordering

Individual target acquisition MUST never depend on spatial-index traversal order.

A target-selection rule MUST compare all relevant target properties explicitly and use a deterministic final tie-break.

Conceptually:

```rust
struct TargetScore {
    priority_class: u16,
    retention_rank: u8,
    distance_sq: u64,
    // optional game-specific terms
    sim_id: SimId,
}
```

The exact ordering is a gameplay decision specified in `15-targeting-combat.md` and content/rules documents.

## 7. Deterministic random numbers

### 7.1 No shared mutable RNG in parallel gameplay

Parallel gameplay code MUST NOT consume from one shared sequence where consumption count/order depends on worker scheduling.

Forbidden pattern:

```rust
let roll = global_rng.next_u32();
```

when called from independently scheduled entity work.

### 7.2 Keyed/counter-style random values

Gameplay randomness SHOULD be derived from an explicit key such as:

```text
match seed
+ tick
+ SimId
+ random-purpose discriminator
+ local sequence/index
```

Conceptual API:

```rust
fn deterministic_random(
    seed: MatchSeed,
    tick: Tick,
    entity: SimId,
    purpose: RandomPurpose,
    index: u32,
) -> u64;
```

`RandomPurpose` MUST use stable explicit discriminants when serialized/hashed. Source-code enum declaration order alone SHOULD NOT define persistent wire values.

The random algorithm is part of the simulation version and MUST NOT change silently.

The current verification simulation introduces its first executable keyed-random use for bounce target selection. It uses a SplitMix64-based stateless mixer over the match seed, impact tick, persistent projectile `SimId`, a stable bounce-target purpose discriminator combined with candidate `SimId`, and bounce index. Every valid candidate receives a keyed rank and the minimum `(rank, SimId)` wins. This makes the result independent of candidate enumeration, spatial bucket layout, hash-map iteration, and worker order while avoiding a shared mutable RNG stream.

## 8. Parallel reductions

Parallel systems MUST avoid non-associative reductions whose grouping can change results.

Integer addition is generally safe when overflow is impossible/defined. Fixed-point multiplication/division, clamping, priority selection, status stacking, and movement resolution may not be safely reorderable.

A reduction MUST either:

- be mathematically order-independent under the domain constraints;
- use a canonical sorted order;
- or specify an equivalent deterministic aggregate operation.

Example: damage can often be accumulated by target as exact integer total if game rules treat same-tick damage as simultaneous. If attack order has side effects (lifesteal, shields, on-hit triggers), attacks require canonical event ordering or a more explicit staged resolution model.

## 9. ECS determinism

ECS storage order is not a gameplay guarantee.

Systems MUST NOT assume query order corresponds to `SimId`, insertion order, archetype creation order, or spatial order.

When a system produces independent per-entity outputs keyed by `SimId`, query order may be arbitrary. When the order of interactions matters, outputs MUST be canonicalized before resolution.

## 10. Navigation determinism

Path/flow generation MUST define deterministic behavior for equal-cost alternatives.

Requirements include:

- fixed neighbor visitation order where it can affect results;
- integer/fixed-point costs;
- no dependence on thread completion order during parallel field construction;
- deterministic tie-break for equal integration values;
- deterministic handling of building topology changes.

Local steering MUST likewise avoid accumulation in arbitrary neighbor iteration order when the result can differ due to rounding.

## 11. Serialization canonicality

Snapshots do not necessarily need byte-for-byte canonical serialization for transport, but state checksums require canonical logical ordering.

Checksum generation MUST NOT hash raw ECS memory or implementation-dependent serialization bytes.

Instead it SHOULD hash a canonical projection such as:

```text
simulation version
current tick
match seed
players sorted by PlayerId
entities sorted by SimId
  canonical component presence/type order
  canonical component field encoding
canonical global resources
```

Derived caches MAY be excluded if they are guaranteed to be reconstructable and cannot influence authoritative behavior except through their canonical inputs.

The concrete current classification and field-level coverage is maintained in `22-authoritative-state-inventory.md`. New future-affecting fields MUST be added to that inventory at the same time they are introduced; checksum and snapshot coverage must not be maintained as unrelated ad-hoc lists.

## 12. State checksums

The simulation MUST expose a reproducible state checksum for desync detection.

Checksums SHOULD be computed periodically in production and MAY be computed every tick in tests/debug builds.

The checksum mechanism MUST distinguish at least:

- simulation version mismatch;
- content hash mismatch;
- state mismatch.

The current executable checksum projection is schema revision **11** (`CANONICAL_CHECKSUM_SCHEMA_VERSION`). Revision 2 deliberately invalidated comparison with the prototype projection by adding deterministic allocator state, immutable simulation configuration/combat-rule identity (including the match seed), audited optional component presence, and live content rawcodes. Revision 3 additionally covers the authoritative state introduced by the Defender/production-upgrade slice, including health-regeneration accumulators, reflected projectiles, and the complete saved precursor runtime held by an in-progress building upgrade. Revision 4 adds the immutable resolved gameplay-bundle identity to match compatibility: two otherwise identical worlds constructed from different canonical content bundles do not share an authoritative checksum. Revision 5 adds stable player ownership, per-player resources/connection state, authoritative team-objective identities, match pause/terminal lifecycle state, and corpse owner provenance. Revision 6 adds each production building's two-entry queue, including the queue preserved inside an in-progress upgrade's saved precursor runtime. Later revisions extend coverage for subsequently added authoritative Human/runtime state; revision 11 adds the corpse-profile decay-start delay and each live corpse's absolute decay-start/eligibility tick. Explicit configuration/rule encoding remains alongside the bundle identity until shared match bootstrap owns those inputs. Worker count, derived navigation caches, presentation events, and diagnostic timings remain outside the checksum.

For diagnostics, the engine SHOULD support hierarchical checksums, e.g. per subsystem/component/entity range, so a desync can be localized without diffing an entire world dump.

## 13. Build/version compatibility

Determinism is guaranteed only within an explicitly compatible simulation version.

The network/replay protocol MUST carry identifiers sufficient to reject incompatible simulation/content combinations before they participate in a match.

A source-compatible code change is not automatically simulation-compatible. Changes to any of the following normally require a simulation compatibility version bump:

- phase ordering;
- arithmetic representation;
- RNG algorithm or keys;
- entity allocation rules;
- navigation tie-break rules;
- targeting tie-break rules;
- combat resolution order;
- serialized gameplay definition semantics.

## 14. Determinism test matrix

The project MUST test the same scripted/seeded simulation across multiple worker counts.

Minimum local/CI matrix should eventually include:

```text
workers = 1
workers = 2
workers = 4
workers = logical CPU count (where practical)
```

All runs MUST produce identical checkpoint hashes.

Where CI coverage permits, deterministic fixtures SHOULD also be exercised across supported operating systems/architectures.

## 15. Forbidden hidden dependencies

Authoritative results MUST NOT depend on:

- wall-clock time;
- OS scheduling;
- CPU thread count;
- memory addresses;
- randomized hash seeds;
- locale;
- filesystem ordering;
- render frame timing;
- audio state;
- nondeterministic physics engines;
- network packet arrival order after commands have been assigned canonical ticks/order;
- uninitialized/padding bytes.

## 16. Determinism failures

A detected client checksum mismatch MUST NOT be treated as proof of cheating. It is a synchronization fault until classified.

The server remains authoritative and SHOULD support resynchronizing the client from a canonical snapshot.

Development builds SHOULD be able to capture:

- last matching tick;
- first mismatching tick;
- canonical stream history around the mismatch;
- simulation/content version;
- hierarchical state hashes;
- optionally canonical state dumps for offline comparison.
