# ECS and Authoritative Data Model

Status: **normative architecture, provisional component inventory**

## 1. Purpose

This document defines how authoritative game state is represented in ECS form and the boundaries between stable simulation identity, runtime ECS storage, derived caches, and presentation entities.

The prototype SHOULD use `bevy_ecs` for authoritative world storage and scheduling. The design MUST remain independent of Bevy rendering/UI.

## 2. Entity classes

Authoritative entities include at least:

- combat units;
- one non-combat builder per active player;
- production buildings;
- attack/defense/spell/utility/legendary buildings;
- castles/objectives;
- authoritative projectiles;
- gameplay area effects when their lifetime/state affects future outcomes;
- other persistent game-rule objects introduced by content.

Purely cosmetic particles, decals, sound emitters, and animation helpers MUST NOT exist in the authoritative ECS unless they also have gameplay meaning.

## 3. Stable simulation identity

Every authoritative entity MUST have a `SimId` component.

Conceptual form:

```rust
#[derive(Component, Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash)]
pub struct SimId(pub u64);
```

`SimId` MUST be unique within a match and MUST NOT be reused after an entity is destroyed unless the specification is deliberately revised.

Monotonic allocation is preferred because it simplifies debugging, replay identity, and deterministic tie-breaking.

The mapping `SimId -> Entity` is a derived runtime index and MAY be rebuilt.

## 4. Runtime ECS handles

Bevy `Entity` values are local process handles only.

They MAY be stored in short-lived derived caches for performance if all of the following hold:

- the cache is invalidated safely when entities despawn;
- authoritative serialization/checksums do not depend on the handle value;
- network/replay data never exposes the handle;
- deterministic decisions use `SimId` or another canonical key when order/tie-breaking matters.

Authoritative components that persist cross-tick references SHOULD store `SimId` rather than raw `Entity` unless a proven wrapper preserves canonical identity separately.

## 5. Provisional component inventory

The following inventory expresses intended ownership, not final Rust type names.

### 5.1 Common identity/classification

```text
SimId
ContentId / UnitTypeId / BuildingTypeId
OwnerPlayer
Team
SpawnTick
```

### 5.2 Transform/movement

```text
SimPosition         fixed-point 2D authoritative position
SimVelocity         fixed-point authoritative velocity
CollisionRadius     gameplay collision/separation radius
MovementProfile     speed/steering/pathing class
NavigationState     canonical local-avoidance continuity for current pursuit as needed
```

`CollisionRadius` is authoritative gameplay geometry, not presentation scale. Unit-unit overlap tests use the sum of both radii; explicitly authored radii also constrain movement/spawn legality against blocked topology and map bounds. The verification client may temporarily omit this component and use the simulation's fallback radius derived from the configured legacy separation diameter, but imported gameplay content SHOULD carry the extracted radius explicitly.

Orientation MAY be authoritative if attack arcs/facing affect gameplay; otherwise facing can remain presentation-derived.

### 5.3 Combat

```text
Health
AttackProfile / attack references
CooldownState
TargetState
Targetability
Armor/Resistance
StatusEffects
AbilityState
ManaState (where applicable)
```

Static attack/unit definitions SHOULD normally live in validated content registries referenced by compact IDs rather than copied into every entity.

### 5.4 Builder

The builder is authoritative but intentionally excluded from ordinary combat semantics.

```text
BuilderMarker
OwnerPlayer
SimPosition / builder movement state
Inventory
NonCombat / target-exclusion classification
```

The builder MUST NOT be represented as an ordinary combat unit merely with very high health. Its non-targetability and non-blocking behavior are structural gameplay rules.

### 5.5 Buildings

```text
BuildingFootprint
ProductionState (if applicable)
ManaState (if applicable)
AbilityState / autonomous caster state (if applicable)
UpgradeState
ConstructionState (if construction time exists)
AttackBuildingState (if applicable)
LegendaryAbilityState (if applicable)
```

Building footprint/topology participation is authoritative. A building may combine production, attack, mana/spellcasting, passive aura, and explicit player-targeted legendary ability behavior.

### 5.6 Corpses

Corpses produced by biological/corpse-producing units are authoritative entities because later gameplay may target or consume them.

Possible components:

```text
CorpseMarker
SourceUnit / SourceUnitType
CorpseDefinitionId
SimPosition
CreatedTick
ExpiryTick (if this corpse decays)
CorpseEligibility / tags (if applicable)
```

A corpse has its own stable `SimId` and is created through the deterministic death-resolution/structural-commit path. It is not an ordinary combat unit and SHOULD NOT carry movement, attack, retaliation, or living-unit target state merely to reuse unit systems. A successful consuming effect removes it through the same canonical structural-mutation mechanism used for other authoritative entities.

The executable verification representation currently stores `Position + Corpse` on the corpse entity, where `Corpse` contains source unit/team, stable corpse-definition identity, creation tick, and optional exclusive expiry tick. Living corpse-producing units carry only a compact corpse-production profile; production buildings may carry the same profile for the units they spawn. These profiles and corpse entities are canonical/checksummed state, while corpse entities are excluded from living-unit targeting, movement/collision, and topology systems.

### 5.7 Projectiles

A projectile should be authoritative only if its future trajectory/timing can affect gameplay.

Possible components:

```text
ProjectileSource
ProjectileDeliveryKind
ProjectileTarget or CapturedImpactPoint
ProjectileMotion / launch + impact timing
ProjectileEffect
BounceState (if applicable)
ExpiryTick
```

Guaranteed-hit ranged projectiles retain a target identity; ballistic/siege projectiles retain a fixed captured impact point instead. A purely visual missile for an already-resolved instant attack belongs only in presentation.

## 6. Static content vs mutable state

Large immutable definitions SHOULD be stored once in content registries.

Example:

```rust
pub struct UnitDefinition {
    pub id: UnitTypeId,
    pub movement: MovementDefinition,
    pub attacks: Vec<AttackDefinition>,
    pub target_rules: TargetRuleSetId,
    pub corpse: Option<CorpseDefinitionId>,
    // ...
}
```

An entity stores `UnitTypeId`; systems resolve the immutable definition through the match content registry.

Content lookup MUST be deterministic and MUST NOT depend on filesystem/runtime discovery order.

## 7. Archetype design

Component composition SHOULD reflect hot-path access patterns.

The design SHOULD avoid a single huge `Unit` component containing every property because that:

- broadens scheduler conflicts;
- worsens cache locality;
- causes unrelated changes to touch the same memory;
- discourages specialized queries.

Conversely, splitting every scalar into a separate component is not automatically beneficial. Components SHOULD group values with similar lifetime/access patterns.

Performance decisions require profiling.

## 8. Authoritative references

Long-lived references between entities use stable IDs.

Examples:

```rust
pub struct TargetState {
    pub current: Option<SimId>,
    pub acquired_tick: Option<Tick>,
}
```

Systems MAY resolve a `SimId` to `Entity` once per phase/query through a derived lookup table.

A missing referenced `SimId` MUST have deterministic semantics, e.g. target is invalid and reacquisition is requested.

## 9. Derived indexes

Expected derived indexes include:

- `SimId -> Entity` lookup;
- uniform spatial grid;
- building occupancy grid;
- navigation fields/integration maps;
- content lookup tables;
- optional per-team/per-class entity indexes.

Derived indexes MUST be classified explicitly as reconstructable or canonical.

A reconstructable index MUST be rebuildable from canonical state without changing simulation results.

## 10. Structural-change queues

Hot parallel phases SHOULD NOT perform arbitrary direct world restructuring.

Gameplay systems produce canonical structural operations such as:

```rust
pub enum StructuralOp {
    SpawnUnit(SpawnUnitOp),
    SpawnProjectile(SpawnProjectileOp),
    Despawn { entity: SimId, reason: DespawnReason },
    AddStatus(AddStatusOp),
    RemoveStatus(RemoveStatusOp),
    Build(BuildOp),
    DestroyBuilding { building: SimId, reason: DestroyReason },
}
```

Before applying operations whose interaction order matters, the resolver MUST sort or otherwise canonicalize them.

## 11. Component mutation ownership

A single authoritative datum SHOULD have one clearly defined mutation pathway per phase.

Examples:

- `Health` is modified by combat/effect resolution, not by arbitrary attack systems;
- `SimPosition` is modified by movement resolution, not by target acquisition;
- `NavigationState` is modified by movement resolution and may remember the retained target plus deterministic bypass-side/clear-progress state when local congestion affects later movement;
- `TargetState` is modified by the targeting phase;
- topology occupancy is modified by building structural resolution.

This reduces scheduler conflicts and makes deterministic reasoning tractable.

## 12. Snapshot representation

Snapshots SHOULD serialize logical canonical state rather than attempting to serialize Bevy's internal ECS storage directly.

Snapshot loading SHOULD:

1. validate simulation/content/protocol compatibility;
2. create authoritative entities with their stored `SimId`s;
3. restore canonical resources/counters;
4. rebuild process-local `SimId -> Entity` mappings;
5. rebuild reconstructable derived indexes/caches;
6. validate invariants before advancing the next tick.

ECS insertion/archetype order after restore MUST NOT affect future simulation results.

## 13. Presentation extraction

The client presentation world MAY use the same Bevy `App`, a sub-app, or another extraction scheme, but authoritative components MUST remain conceptually distinct from render components.

Presentation may map:

```text
SimId -> render entity
```

and update render transforms from authoritative snapshots/interpolation.

A render entity MAY outlive an authoritative entity briefly for death animation, provided it cannot affect gameplay.

## 14. Server build

The headless server SHOULD instantiate only the authoritative ECS, simulation schedules, content, networking, persistence/replay, and observability required for server operation.

It MUST NOT require a GPU, display server, audio system, or client asset renderer.

## 15. Validation invariants

Development/test validation SHOULD be able to assert:

- every authoritative entity has exactly one `SimId`;
- no two live entities share a `SimId`;
- every owner/team/content reference is valid;
- all stored entity references use valid `SimId`s or documented tombstone semantics;
- building footprints agree with occupancy state;
- authoritative components contain no NaN/float values if floats are forbidden by the determinism layer;
- derived indexes resolve back to the same canonical entities they index.
