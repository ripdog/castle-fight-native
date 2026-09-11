# Gameplay Content and Data Definitions

Status: **normative data principles, provisional schema**

## 1. Purpose

This document defines how units, buildings, attacks, factions, maps, upgrades, and other gameplay content are represented without hard-coding every rule into simulation systems.

Content is authoritative input to deterministic simulation and therefore must be versioned, validated, and hashed canonically.

## 2. Content vs runtime state

Content definitions describe immutable rules/templates.

Runtime ECS state stores mutable instance state and compact references to content definitions.

Example:

```text
UnitDefinition "footman"
  movement speed
  max health
  attack definitions
  collision radius
  target rules

Unit entity #18422
  UnitTypeId = footman
  current health = 317
  position = (...)
  target = SimId(...)
  cooldown = ...
```

Static definitions SHOULD NOT be duplicated into every entity unless profiling justifies a hot cached subset.

## 3. Stable content identifiers

Gameplay content MUST use stable explicit IDs.

Human-readable names/paths may be used for source files, but runtime/network/replay representations should use validated stable identifiers or canonical hashes.

IDs MUST NOT depend on filesystem enumeration order or hash-map insertion order.

Examples:

```rust
pub struct UnitTypeId(pub u32);
pub struct BuildingTypeId(pub u32);
pub struct AttackId(pub u32);
pub struct UpgradeId(pub u32);
pub struct FactionId(pub u32);
pub struct MapId(pub u32);
```

Exact widths/types are provisional.

## 4. Canonical content bundle

Before a match starts, source content is loaded and validated into a canonical immutable bundle.

The simulation consumes only this validated representation.

The bundle has a canonical hash used for:

- multiplayer compatibility;
- replay compatibility;
- snapshot validation;
- server/client mismatch detection;
- diagnostics.

Cosmetic-only assets SHOULD be separable from gameplay content so harmless visual differences do not necessarily alter gameplay compatibility.

## 5. Source format

Source format is open.

Candidates include:

- RON;
- TOML;
- JSON/JSON5;
- custom editor-generated format.

Selection criteria:

- readable diffs;
- good Rust tooling/serde support;
- strict schema validation;
- convenient inheritance/composition if required;
- explicit numeric representation;
- modding potential.

Runtime correctness must not depend on parser map ordering.

## 6. Numeric content values

Authors may want ergonomic decimal values, but authoritative runtime representation uses deterministic integer/fixed-point quantities.

Content loading MUST convert/quantize values deterministically and reject out-of-range/ambiguous data.

For example:

```text
source: speed = 2.75 tiles/sec
validated runtime: exact fixed-point/tick movement value
```

The conversion algorithm is part of content/simulation semantics.

## 7. Unit definitions

A unit definition may include:

```text
identity/name/localization key
presentation asset references
max health
movement profile
collision/separation radius
unit classifications/tags
attack list
armor/resistance profile
target rule set
production-related metadata
bounty/reward if applicable
abilities/passives
status immunities
```

The simulation should consume compact validated IDs/structures rather than dynamically interpret arbitrary scripts in hot loops.

## 8. Attack definitions

Every ordinary attack definition MUST declare an authoritative delivery kind rather than deriving behavior from projectile art.

Initial required kinds:

```text
Melee
RangedGuaranteedHit
RangedBallistic
Bounce
```

An attack definition may include:

```text
damage / damage distribution rule
damage type
cooldown
windup / backswing if relevant
attack range
acquisition/retention modifiers
target filters
delivery kind
projectile speed / travel-time rule where applicable
ballistic impact-zone radius/effect where applicable
bounce count/range/repeat policy/damage scaling where applicable
splash/area behavior
random proc definitions
on-hit/on-impact effects
```

`RangedGuaranteedHit` retains its intended target and cannot become missable merely because the target moves after launch. `RangedBallistic` captures a fixed impact position and may miss the original target or hit other eligible units present in the impact zone. `Bounce` guarantees its first hit and selects later bounce targets according to explicit deterministic-random rules.

All values that influence gameplay are authoritative content.

Targeting policy should reference explicit rule sets rather than deriving behavior from presentation asset type.

## 9. Building definitions

Buildings are not limited to production. One building may combine several deterministic roles.

A building definition may include:

```text
footprint
placement constraints
cost
health/armor
production definition
attack definitions if attack building
mana pool / mana regeneration if spellcasting
ability definitions/references
activation mode for abilities (automatic/passive/player-targeted)
legendary classification/limits if applicable
upgrade relationships
sell behavior
spawn location/offset policy
build/construction time if present
```

The footprint is authoritative and MUST NOT be derived from the visual mesh bounds.

This is critical for deterministic placement/navigation/caging.

Spellcasting and legendary building behavior is specified in `16-abilities-spellcasting.md`.

## 10. Production definitions

Production content must define deterministic spawn timing and produced-unit selection.

Potential fields:

```text
unit type or deterministic selection table
spawn interval in ticks
initial delay
spawn count
spawn offset/pattern
upgrade modifiers
random-selection purpose/key rules if randomized
```

Production timing uses simulation ticks only.

## 11. Factions/races

Faction definitions may organize:

- available buildings;
- tech/upgrades;
- random pools;
- UI grouping;
- starting state;
- faction-specific rules.

The core engine SHOULD avoid special-casing factions by Rust type where data/effect composition suffices.

Unique mechanics may still require dedicated deterministic systems.

## 12. Upgrades

Upgrades should express deterministic modifications to validated gameplay values.

The engine needs an explicit policy for modification order to avoid data-order-dependent outcomes.

Possible approach:

```text
base definition
+ additive modifiers sorted by stable modifier ID
+ multiplicative fixed-point modifiers in defined stage/order
+ clamps/overrides in defined stage
```

Exact semantics are open and should be kept simple initially.

## 13. Tags and categories

Targeting/effects will likely need stable gameplay categories such as:

- ground;
- air;
- building;
- mechanical;
- summoned;
- hero/boss;
- organic;
- magic immune;
- etc.

These should be explicit stable bitsets/IDs, not string comparisons in hot loops.

The actual set depends on the intended Castle Fight rules/content.

## 14. Effect and ability definitions

Reusable deterministic effects may represent:

- direct damage/healing;
- apply/remove status;
- area damage;
- spawn unit;
- modify resource or mana;
- knockback if supported;
- timed aura/status;
- map-wide aura/modifier;
- stun/disable;
- chained/bounce attack;
- spawn attack/ability projectile.

Ability definitions compose target policy, activation mode, cost/cooldown/charges, and one or more effects. Automatic building abilities and player-targeted legendary abilities use the same validated vocabulary rather than separate scripting paths.

The effect system SHOULD be constrained and explicitly ordered rather than becoming an unconstrained scripting VM prematurely.

## 14.1 Item definitions

Builder-held item content may include:

```text
inventory/equip rules
passive or activation mode
automatic trigger cadence
cooldown / charges
range or global scope
target filters and target policy
ability/effect references
stacking rules
```

Items may provide map-wide friendly auras, automatically fire at nearby enemies, or expose player-targeted area effects. Item behavior MUST NOT make the builder an ordinary targetable/blocking combat unit.

## 15. Scripting

General gameplay scripting is **open** and SHOULD be deferred until concrete content cannot be expressed cleanly through data + deterministic systems.

If scripting is introduced, it MUST satisfy:

- deterministic execution;
- deterministic iteration APIs;
- bounded/resource-controlled server execution;
- no wall clock/filesystem/network/OS randomness access;
- explicit versioning;
- snapshot-safe script state;
- replay-safe semantics.

A native Rust effect/system implementation is preferable for early milestones.

## 16. Map definitions

Authoritative map content includes:

- map dimensions/coordinate system;
- static navigation/buildability grid/geometry;
- objectives/castles;
- player start/build regions;
- spawn-related anchors if needed;
- lane/objective connectivity semantics;
- terrain movement costs/classes;
- game-mode parameters.

Decorative terrain/props MAY be client-only where they do not affect navigation/visibility/gameplay.

## 17. Content validation

Loading must fail before match start for invalid authoritative content.

Validation SHOULD catch:

- duplicate IDs;
- missing references;
- impossible/overflowing numeric values;
- invalid footprint geometry;
- empty target rule sets where prohibited;
- zero/negative production intervals;
- invalid projectile speeds/lifetimes;
- cyclic upgrade dependencies unless intentionally supported;
- unsupported effect combinations;
- map objectives outside bounds;
- inconsistent build/navigation grids.

## 18. Hot reload

Content hot reload is useful during development but MUST NOT modify authoritative gameplay definitions mid-network-match unless the change is represented as a deliberate synchronized simulation operation/version transition.

The simplest rule is:

- development sandbox: hot reload allowed with simulation reset;
- live match: authoritative content bundle immutable.

Presentation assets may hot reload independently if gameplay geometry/state is unaffected.

## 19. Modding

Modding is not required for the first playable version, but data-driven content and canonical hashes should make it possible later.

A server hosting modded authoritative content must advertise the exact bundle identity and ensure joining clients possess compatible gameplay definitions.

Asset distribution/security/licensing is outside this specification for now.

## 20. Original-game compatibility data

If the project aims to reproduce particular Castle Fight behaviors, observed rules should be recorded as explicit content/behavioral fixtures rather than scattered magic constants.

Where original behavior is uncertain, tests/content should label the choice as:

- verified original behavior;
- inferred behavior;
- intentional divergence.

This will prevent accidental changes while reverse-engineering game feel.
