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
- optional `ContentIdentity.rawcode`; `ContentIdentity.name` is presentation metadata and is not gameplay identity;
- owning `PlayerId`, plus `Team`, `Position`, and `Health`;
- `AttackProfile` including delivery parameters, `AttackTargetMask`, `DamageType`, and `ArmorProfile`;
- `PassiveUnitEffects` and every nested ability/effect parameter;
- `MovementClass`, `MovementProfile`, optional `CollisionRadius`, and `MechanicalUnit` marker presence;
- optional `BuildTimeTicks` and `RepairTimeTicks` component presence and values;
- `AttackCooldown` and `AttackSequence`;
- `TargetState` including both lock flags;
- `RetaliationState` including optional attacker and attacked tick;
- complete `StatusState`: stun expiry, active movement/attack-speed/armor modifiers, reactive slow parameters, damage-over-time pulse state, and active counts;
- `NavigationState`: goal kind/reference, bypass side, and clear-tick continuity. This is stored movement continuity, not a disposable route cache;
- `SpawnTick`;
- optional `CorpseProducer` definition and optional lifetime;
- optional `SpellcastingProfile`, plus `ManaState` current/remainder and `AutomaticAbilityState` ready tick/cast sequence when spellcasting is present.

Optional component presence is canonical. `None` must not alias a present zero-valued component.

### 5.2 Buildings and construction

The complete current building state is:

- `SimId`, optional content rawcode, optional owning `PlayerId`, `Team`, `BuildingFootprint`, and `Health`;
- optional `BuildingConstruction`: start/complete ticks plus the complete pending authored building definition/properties. The current checksum hashes that definition canonically; snapshots must preserve the logical definition or an exact compatible definition reference;
- optional `BuildingEconomyProfile` and optional `RepairTimeTicks`;
- `DamageType` and `ArmorProfile`;
- optional production definition and `ProductionState.next_spawn_tick`;
- production content rawcode, corpse profile, collision radius, movement class, mechanical/build/repair metadata, target mask, damage type, armor, passive effects, and spellcasting profile;
- optional attack profile plus target mask, cooldown, target-lock state, and spawn tick;
- optional `StatusState`;
- optional building spellcasting profile plus mana remainder/current value and automatic-ability ready/cast sequence.

Production metadata is authoritative because it defines future spawned units. It must remain complete even when no produced unit is currently alive.

### 5.3 Builders and paid build orders

The complete current builder state is:

- `SimId`, owning `PlayerId`, `Team`, `Position`, and builder marker presence;
- `BuilderProfile` movement/build/repair/autocast/blink values;
- `BuilderConfiguration`: appearance rawcode, locomotion, and ordered build catalog;
- `BuilderState`: optional destination, follow target, repair target, repair-progress remainder, and repair-autocast toggle;
- optional `BuilderBuildOrder`, including the full pending `BuildingSpawn` and `BuildingGameplayProperties` purchased for the order.

A paid build order is persistent authoritative state. Restoration must neither refund it nor charge it again.

### 5.4 Projectiles and persistent combat effects

**Guaranteed-hit projectile:** `SimId`, source/team, target, damage, pending on-hit effects, damage type, launch position/tick, and impact tick.

**Ballistic projectile:** `SimId`, source/team, target mask, damage, optional Burning Oil profile, damage type, captured launch/destination positions, impact radius, launch tick, and impact tick.

**Bounce projectile:** `SimId`, source/team, target mask/current target, damage/type, launch position/timing, speed/range, remaining bounces, bounce index, damage falloff, repeat-target rule, hit count, and hit-target history.

**Burning Oil zone:** `SimId`, source/team, center, complete effect profile, creation tick, and pulse index.

**Chain Lightning state:** `SimId`, source/team, complete profile, start tick, next jump index, current target, last position, next damage, hit count, and hit history.

All of these survive tick boundaries and therefore must be restored exactly.

### 5.5 Corpses

A corpse stores `SimId`, `Position`, source unit/player/team, corpse definition, creation tick, and optional expiry tick. Source ownership is retained after the live unit disappears so presentation/statistical provenance does not fall back to team identity. Optional expiry presence is authoritative; an immortal corpse must not alias an expiry sentinel.

## 6. State reconstructed inside a tick

Per-tick unit/building snapshots, spatial partitions/reservation grids, movement intents, target candidate buffers, damage/commit buffers, projectile-impact work lists, and phase metrics are transient. They are reconstructed from the tick-boundary state and MUST NOT be captured as snapshot state.

A snapshot is taken only at a defined completed boundary. Step 7 must not attempt to capture the simulation halfway through `step()` unless a separate mid-tick schema and phase-resume contract is deliberately introduced.

## 7. Checksum coverage revision 5

`CANONICAL_CHECKSUM_SCHEMA_VERSION = 5` is the current compatibility boundary. Revision 2 added immutable configuration/combat identity, allocator state, audited optional presence, content rawcodes, canonical entity-shape validation, and duplicate-`SimId` rejection. Revision 3 retains that coverage and adds the authoritative state introduced by the Defender/production-upgrade slice:

- fixed-point unit health-regeneration rate/remainder state;
- persistent reflected-projectile state;
- in-progress building-upgrade identity and the complete saved precursor runtime required for deterministic cancellation, including production timer, attack cooldown/target state, spawn tick, mana/ability state, and status state;
- the associated production-unit regeneration/profile data needed to preserve future spawn semantics.

Revision 4 additionally incorporates the immutable resolved gameplay-bundle schema/hash into the cached match-configuration identity. This makes content-definition or implementation-binding differences incompatible even when the live ECS happens to contain the same entities.

Revision 5 adds the Step 5 player/lifecycle model: stable entity ownership, canonical per-player resource and connection records, team-objective `SimId`s, running/paused/finished lifecycle state and terminal outcome/tick, plus corpse source ownership. All previously covered resources, entities, timers/effect state, navigation continuity, ordinary projectiles, build orders, and defense alerts remain included. `ContentIdentity.name`, worker count, derived caches, presentation events, and diagnostic timings remain deliberately excluded.

Changing the canonical encoding or the semantics of a field requires a deliberate checksum/simulation compatibility revision. Checksums from revisions 1 through 5 are mutually incompatible.

## 8. Required additions before step 7 completion

Step 6 introduces the remaining authoritative command-stream concepts that are not present in the current `Simulation` yet. Before snapshot/replay sign-off, this inventory must be extended for:

- finalized command-stream position, admitted future commands, client/player sequence tracking, and any scheduler history owned by the match driver.

Those additions must be represented in checksum/snapshot coverage before they can be considered restorable.
