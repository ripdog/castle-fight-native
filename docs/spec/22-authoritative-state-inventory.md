# Authoritative State Inventory

Status: **normative inventory for the current simulation; update whenever future-affecting state is added**

## 1. Purpose

This document classifies the state currently owned by `castle-fight-sim` so checksums, snapshots, replay restoration, and multiplayer compatibility use the same boundary. A value is authoritative when changing it at a tick boundary can change a future gameplay result, including future entity identity or deterministic random keys.

The inventory is intentionally implementation-specific. Step 7 snapshot work MUST use it as a coverage checklist and MUST update it when the command stream, player/lifecycle model, or content bundle introduces additional persistent state.

## 2. Classification rules

Every simulation value belongs to one of four classes:

1. **Mutable authoritative state** — must be represented by logical snapshots and state checksums.
2. **Immutable match/content input** — fixed at match construction; snapshots/replays may reference it by a validated immutable identity rather than duplicate all bytes, but state compatibility must include that identity.
3. **Rebuildable derived state** — may be omitted only when deterministic reconstruction from authoritative state is defined and reconstruction consumes no gameplay IDs or randomness.
4. **Presentation/diagnostics** — must not feed future gameplay and is excluded from authoritative snapshots/checksums.

A cache is not automatically derived. If retaining or clearing it can change a future gameplay result, it is authoritative until the algorithm is changed or equivalence is proven.

## 3. Simulation-global state

| Field / concept | Class | Snapshot/checksum rule |
| --- | --- | --- |
| `next_tick` | Mutable authoritative | Store/hash exactly. It selects timer phases, income timing, keyed randomness, spawn ticks, and effect timing. |
| `next_id` | Mutable authoritative | Store/hash exactly. Allocator history is gameplay state even when the current live-entity set is identical. |
| `players` | Mutable authoritative | Store/hash every canonical player record in stable `PlayerId` order: player ID, team, connection status, gold, lumber, and legendary-point usage/cap. Ownership/delegation rules resolve against these records; there is no longer a team-indexed player-resource array. |
| `defense_alerts` | Mutable authoritative | Store/hash the complete alert set: attacked tick, victim team/id/position, and attacker id. Order is canonicalized for hashing/serialization. |
| `lifecycle` | Mutable authoritative | Store/hash `Running`, paused-for-disconnect team mask, or terminal outcome plus finished tick. A paused or finished match must restore without advancing ordinary gameplay. |
| `team_objectives` | Mutable authoritative | Store/hash the optional stable `SimId` registered for each team objective. Victory evaluation depends on these exact identities, not a content-name search. |
| `shrine_death_generation` | Mutable authoritative | Store/hash exactly. Delayed revival callbacks compare their retained generation before creating a replacement. |
| `debug_buildings_invulnerable` | Mutable authoritative | Store/hash exactly. Host debug immunity changes future building damage and must survive replay/rejoin. |
| `config` / `SimulationConfig` | Immutable match input | Compatibility identity covers every field listed in section 4. Step 3 replaces the temporary direct configuration identity with the resolved content/mode identity where appropriate. |
| `combat_rules` / `CombatRules` | Immutable match input | Compatibility identity covers terrain elevation, uphill miss chance, armor factor, and the complete damage-type/armor-type table. |
| `configuration_identity` | Rebuildable derived identity | Cached canonical hash of the immutable inputs above. It is not an independent gameplay value; restoration must recompute/validate it from selected compatible inputs. |
| `topology`, `air_topology` | Rebuildable derived state | Rebuild from immutable navigation configuration plus live blocking buildings. Do not serialize Bevy/internal grid layout. |
| `topology_dirty` | Rebuildable coordination state | A restore may normalize this by rebuilding topology immediately and setting the dirty state consistently. It must not resume with stale topology. |
| `pursuit_cache` | Rebuildable derived cache | Clear on restore. Cache hits/misses must not alter chosen canonical movement. |
| `radius_objective_fields` | Rebuildable derived cache | Clear/rebuild on restore from canonical topology/configuration. |
| Rayon `pool` / worker count | Diagnostics/execution strategy | Never snapshot/hash. Worker count must not affect gameplay identity. |
| `last_attacks`, `last_ability_casts`, `last_chain_lightnings` | Presentation events | Exclude from authoritative state. Reset/suppress around restore and replay fast-forward. |
| `TickResult`, `TickTimings`, phase metrics | Diagnostics | Exclude. Wall-clock duration and metric counters do not feed gameplay. |

## 4. Immutable match/configuration inputs

Until the canonical content bundle from step 3 exists, the checksum compatibility identity explicitly encodes:

- checksum schema revision and simulation tick rate;
- `match_seed`;
- spatial and navigation cell sizes and navigation min/max cells;
- target-pursuit extra range, unit-separation distance, and per-tick separation cap;
- ground static blockers, air-only blockers, placement-only blockers, and both teams' build regions;
- optional targetless-lane bounds;
- both team objective points;
- all economy rules: starting resources/legendary cap, base income, income interval, and tax bracket;
- optional terrain elevation map dimensions/origin/tile size and every cliff/height sample;
- uphill miss chance;
- armor factor and every damage-type × armor-type multiplier.

Order-insensitive footprint collections are canonically sorted before hashing. The Rayon worker count, presentation settings, and diagnostic configuration are intentionally absent.

The selected content bundle introduced in step 3 MUST eventually carry the remaining immutable gameplay definitions and behavior-binding identity. A release label by itself is insufficient.

## 5. Entity state

Every authoritative ECS entity has exactly one stable `SimId`. Canonical traversal sorts entities by `SimId`; duplicate IDs or an entity shape that cannot be projected into one of the supported logical entity kinds is invalid state and must fail validation rather than disappear from a checksum/snapshot.

### 5.1 Units

The complete current unit state is:

- `SimId`;
- optional `ContentIdentity` (map version and rawcode); `ContentIdentity.name` is presentation metadata and is not gameplay identity;
- owning `PlayerId`, plus `Team`, `Position`, and `Health`;
- `AttackProfile` including delivery parameters, `AttackTargetMask`, `DamageType`, and `ArmorProfile`;
- `PassiveUnitEffects` and every nested ability/effect parameter;
- `MovementClass`, `MovementProfile`, optional `CollisionRadius`, and `MechanicalUnit` marker presence;
- intrinsic `UnitClassifications` values (hero, summoned, spell-immune, combat-sapper, invulnerable, legendary, summoned marker, illusion, invisible), including in original resurrection definitions. Absent component storage represents the all-false logical value; only non-default flags need an ECS component;
- optional `BuildTimeTicks` and `RepairTimeTicks` component presence and values;
- `ActionTimingProfile`: independent primary/secondary point-plus-backswing and damage-point tick durations, plus cast duration; retained in production/resurrection definitions as well;
- `AttackCooldown` and `AttackSequence`;
- `TargetState` including both lock flags;
- `RetaliationState` including optional attacker and attacked tick;
- complete `StatusState`: action-animation kind/start/end, optional committed ordinary-attack target and absolute release tick, native stun expiry, independent order-recovery expiry, ability-retreat deadlines, primary secondary-resurrection ability identity/deadline/readiness, active movement/attack-speed/armor modifiers, reactive slow parameters, optional revealing team on timed armor effects, damage-over-time pulse state, and active counts;
- `NavigationState`: goal kind/reference, bypass side, and clear-tick continuity. This is stored movement continuity, not a disposable route cache;
- `SpawnTick`;
- optional `CorpseProducer` definition and optional lifetime;
- optional `SpellcastingProfile`, plus shared `ManaState` current/remainder and primary `AutomaticAbilityState` ready tick/cast sequence/control flags when spellcasting is present;
- optional `AdditionalAutomaticAbilities`: ability-ID-ordered profiles, independent ready ticks/cast sequences/control flags, and each slot's delayed secondary-resurrection due/ready ticks. Its bounded representation is carried only by multi-ability entities, not the ordinary per-tick unit snapshot;
- original resolved resurrection definition, including additional ability definitions, before the unit dies. The common case references the already captured current unit definition; an explicit original definition is retained when live mutations differ from the resurrection baseline. Presentation names MUST NOT choose a different authoritative representation.

Optional component presence is canonical. `None` must not alias a present zero-valued component.

### 5.2 Buildings and construction

The complete current building state is:

- `SimId`, optional content identity (map version and rawcode), optional owning `PlayerId`, `Team`, `BuildingFootprint`, and `Health`;
- intrinsic `UnitClassifications`, independent of produced-child flags, in live structures and all cold building properties (including paid orders, pending construction and saved upgrade precursors). Absent ECS storage means all-false; each flag is canonically hashed and restored. Construction shells immediately use their declared classifications, upgrade shells use the target definition, successful completion keeps that definition, and cancellation restores the precursor's saved live flags, including runtime changes;
- optional `BuildingConstruction`: start/complete ticks plus the complete pending authored building definition/properties. The current checksum hashes that definition canonically; snapshots must preserve the logical definition or an exact compatible definition reference;
- optional `BuildingEconomyProfile` and optional `RepairTimeTicks`;
- `DamageType` and `ArmorProfile`;
- optional production definition and `ProductionState.next_spawn_tick`;
- production content identity (map version and rawcode), corpse profile, collision radius, movement class, mechanical/build/repair metadata, intrinsic classifications, target mask, damage type, armor, passive effects, primary spellcasting profile, and optional additional automatic-ability definitions;
- optional attack profile plus target mask, cooldown, target-lock state, and spawn tick;
- optional `StatusState`;
- optional building spellcasting profile plus shared mana remainder/current value and primary automatic-ability ready/cast sequence/control flags;
- optional `AdditionalAutomaticAbilities`, with the same profile/state coverage as units. In-progress upgrade cancellation preserves the complete precursor ability set and state; a successful definition replacement discards obsolete slots.

Content identity includes its retained map version before activation, not a version inferred from a live rawcode or a process default. This identity participates in every live/cold hash and snapshot location, including original resurrection bodies, paid build orders, pending construction, production children, and saved upgrade precursors. Native carrier/regeneration state and delayed building-spell controls consume this identity; Hex profiles/callbacks also retain the source effect version.

Production metadata is authoritative because it defines future spawned units. It must remain complete even when no produced unit is currently alive, including inside pending construction and saved upgrade precursors. Every ordinary or companion production spawn receives the complete definition set with fresh per-ability runtime state; producer metadata MUST NOT carry a previous child's cooldowns, sequences, or pending actions.

### 5.3 Builders and paid build orders

The complete current builder state is:

- `SimId`, owning `PlayerId`, `Team`, `Position`, and builder marker presence;
- `BuilderProfile` movement/build/repair/autocast/blink values;
- `BuilderConfiguration`: appearance identity (map version and rawcode), locomotion, and ordered build catalog;
- `BuilderState`: optional destination, follow target, repair target, repair-progress remainder, and repair-autocast toggle;
- optional `BuilderBuildOrder`, including the full pending `BuildingSpawn` and `BuildingGameplayProperties` purchased for the order.

A paid build order is persistent authoritative state. Restoration must neither refund it nor charge it again.

### 5.4 Projectiles and persistent combat effects

**Guaranteed-hit projectile:** `SimId`, source/team, target, damage, pending on-hit effects, damage type, launch position/tick, and impact tick.

**Ballistic projectile:** `SimId`, source/team, target mask, damage, optional Burning Oil profile, damage type, captured launch/destination positions, impact radius, launch tick, and impact tick.

Directed on-hit state also retains any Feedback ability identity, mana-drain limit, damage ratio, summoned bonus, and target mask. It reads the target's live resource/classification state at impact; it MUST NOT capture an already computed mana burn at launch or derive combustion from critical weapon damage.

**Bounce projectile:** `SimId`, source/team, target mask/current target, damage/type, launch position/timing, speed/range, remaining bounces, bounce index, damage falloff, repeat-target rule, hit count, and hit-target history.

**Line projectile:** `SimId`, source/team and retained source-art rawcode, primary target, current damage/type, complete typed delivery (speed, minimum range, spill length/half-width, per-collision retention and spill mask), launch position/tick, primary/final impact ticks, optional fixed spill origin, destination, and ordered hit-target history. The fixed segment and collision history MUST survive restoration during spill; source-art identity MUST survive removal of the source.

**Burning Oil zone:** `SimId`, source/team, center, complete effect profile, creation tick, and pulse index.

**Chain Lightning state:** `SimId`, source/team, complete profile, start tick, next jump index, current target, last position, next damage, hit count, and hit history.

**Native Healing Wave action:** `SimId`, source/team, complete profile (including distinct trigger healing), start tick, next jump index, current target, last position, next healing, hit count, and hit history. Hop deadlines derive from the original cast rather than accumulated rounded intervals.

**Delayed Shrine revival:** `SimId`, supporting script map version, source unit identity, owner/team, position, due tick, death generation, and the complete cold replacement definition. The script version is independent of that definition's optional content identity: generic/unlabelled bodies must not cause a default-version lookup or lose the originating script context after supporting buildings disappear. Wire decoding validates this version against the selected bundle even when the replacement has no content identity. Supporting buildings, the live unit and any retained original identity must agree on version; mixed source contexts are rejected before allocating a callback.

**Native directed bolt action:** `SimId`, source/team, target, complete native effect profile, original launch position/tick, authoritative current homing position/update tick, and projected impact tick. Moving the target can change arrival; a presentation interpolation or an obsolete launch-time arrival estimate MUST NOT determine damage.

Timed damage-over-time state includes whether a final pulse is permitted at exact expiry. That policy MUST survive restoration and participate in checksums; an inclusive final pulse MUST resolve before buff removal/target reacquisition at the same boundary. Passive status can exist on a structure without an attack or active spell.

All of these survive tick boundaries and therefore must be restored exactly.

### 5.5 Corpses

A corpse stores `SimId`, `Position`, source unit/player/team, corpse definition, creation tick, decay-start/eligibility tick, and optional expiry tick. Source ownership is retained after the live unit disappears so presentation/statistical provenance does not fall back to team identity. The decay-start tick is authoritative because corpse selection, resurrection, and consumption must reject a fresh death until its imported Warcraft `death_time` has elapsed. Optional expiry presence is authoritative; an immortal corpse must not alias an expiry sentinel. A resurrection-capable corpse also retains the complete resolved unit definition, including additional automatic-ability definitions. Dead-source cooldowns, sequences, control flags, and delayed actions are not carried into the revived body; runtime state is initialized as for a fresh spawn.

## 6. State reconstructed inside a tick

Per-tick unit/building snapshots, spatial partitions/reservation grids, movement intents, target candidate buffers, damage/commit buffers, projectile-impact work lists, and phase metrics are transient. They are reconstructed from the tick-boundary state and MUST NOT be captured as snapshot state.

A snapshot is taken only at a defined completed boundary. Step 7 must not attempt to capture the simulation halfway through `step()` unless a separate mid-tick schema and phase-resume contract is deliberately introduced.

## 7. Checksum coverage

`CANONICAL_CHECKSUM_SCHEMA_VERSION` in `crates/sim/src/simulation.rs` is the current compatibility boundary. The multi-ability design revision includes optional additional ability profiles/state, independent delayed secondary actions, and the originating ability identity for primary delayed resurrection. All such state is also covered inside saved upgrade-precursor runtime. The historical additions below remain included. Revision 2 added immutable configuration/combat identity, allocator state, audited optional presence, content rawcodes, canonical entity-shape validation, and duplicate-`SimId` rejection. Revision 3 retains that coverage and adds the authoritative state introduced by the Defender/production-upgrade slice:

- fixed-point unit health-regeneration rate/remainder state;
- persistent reflected-projectile state;
- in-progress building-upgrade identity and the complete saved precursor runtime required for deterministic cancellation, including production timer, attack cooldown/target state, spawn tick, mana/ability state, and status state;
- the associated production-unit regeneration/profile data needed to preserve future spawn semantics.

Revision 4 additionally incorporates the immutable resolved gameplay-bundle schema/hash into the cached match-configuration identity. This makes content-definition or implementation-binding differences incompatible even when the live ECS happens to contain the same entities.

Revision 5 adds the Step 5 player/lifecycle model: stable entity ownership, canonical per-player resource and connection records, team-objective `SimId`s, running/paused/finished lifecycle state and terminal outcome/tick, plus corpse source ownership. All previously covered resources, entities, timers/effect state, navigation continuity, ordinary projectiles, build orders, and defense alerts remain included. `ContentIdentity.name`, worker count, derived caches, presentation events, and diagnostic timings remain deliberately excluded.

Changing the canonical encoding or the semantics of a field requires a deliberate checksum/simulation compatibility revision. Checksums from distinct schema revisions are not comparable.

## 8. Step 6 command-stream state outside `Simulation`

Step 6 introduces a `MatchDriver` beside `Simulation`. It owns authoritative scheduling/continuity state that is not ECS gameplay state:

- the next monotonic `InputStreamPosition`;
- admitted commands not yet finalized/executed, including their assigned tick and canonical within-tick order;
- the next expected `ClientCommandSequence` for each player;
- admission dispositions retained for idempotent retry/conflicting-duplicate detection;
- the set/history boundary needed to prevent a finalized client sequence from executing twice;
- the canonical finalized tick/control history retained by the local driver.

The driver's most recent execution-outcome list is UI/protocol feedback and does not affect future gameplay; it need not be persisted if the same acknowledgement can be reconstructed/resupplied from retained canonical history.

`InputStreamPosition` and client transport/admission sequence counters are continuity state, not gameplay checksum inputs; `20-networking.md` deliberately excludes stream position from the simulation checksum. Admitted future commands likewise have not affected the completed gameplay state yet, so ordinary `(tick, state_checksum)` checkpoints continue to hash `Simulation`, not the future-input queue. Step 7 snapshots, however, MUST capture the exact stream/history boundary plus every admitted future command and sufficient sequence/deduplication state so restoration neither loses an accepted command nor executes one twice. A later combined driver/snapshot integrity hash may cover those fields separately without changing the gameplay checksum definition.

## 9. Step 7 logical simulation snapshot coverage

`SimulationSnapshot` at `AUTHORITATIVE_SNAPSHOT_SCHEMA_VERSION` captures all mutable authoritative `Simulation` state named in sections 3 and 5: `next_tick`, allocator `next_id`, canonical players/resources/connections, lifecycle, objective identities, defense alerts, and the complete canonical entity projection. In-progress construction stores its full authored target definition and optional precursor runtime rather than only the definition hash used by the gameplay checksum, so cancellation/completion semantics survive restoration exactly. The snapshot also stores the immutable `configuration_identity` as a compatibility requirement and the resulting gameplay checksum as an integrity check.

Checksum traversal and snapshot capture share one canonical entity projection. Restoration deliberately omits and rebuilds topology/pathing caches, clears presentation events, retains the destination simulation's worker pool, and may recreate entities in a different insertion order.

`MatchDriverSnapshot` schema revision 1 now captures the section 8 continuity state as well: next stream position, pending admitted commands, per-player next client sequence, retained submission dispositions, applied command sequences, complete canonical history, and replay checkpoint history. Restore validates content identity, canonical stream/history positions, pending tick/order, sequence/deduplication consistency, replay checkpoint boundaries, and the nested `SimulationSnapshot` before replacing live driver state. The driver's last execution-outcome list remains deliberately excluded and is cleared on restore.

`MatchReplay` schema revision 1 retains the driver's creation-time logical simulation snapshot, canonical tick/control record history, and one `(stream position, completed tick boundary, checksum)` checkpoint per record. Replay headers declare replay/snapshot/checksum schema revisions plus map/release, content gameplay identity, and simulation configuration identity. Optional seek points are full `MatchDriverSnapshot`s captured only at command-free canonical boundaries, so seeking restores both gameplay state and stream/deduplication continuity before replaying later records.
