# Abilities, Mana, and Spellcasting

Status: **normative architecture, provisional gameplay details**

## 1. Purpose

This document defines deterministic abilities used by buildings, items, and any future autonomous combat entity that can cast spells.

The ability system is broader than ordinary attacks. Buildings may consume mana to cast buffs, damage spells, crowd-control effects, map-wide effects, or special projectile attacks. Some abilities are fully autonomous; specific legendary abilities may require a player-selected target.

The ability system MUST preserve the core rule that ordinary combat units are not player-commandable.

## 2. Ability ownership and activation modes

Every ability instance belongs to an authoritative source and has one explicit activation mode.

Initial modes:

```text
Automatic
PlayerTargetPoint
PlayerTargetArea
PlayerTargetEntity
PassiveAura
PassiveRule
```

`PlayerTarget*` MUST only be exposed for content that deliberately allows manual activation, such as a legendary building or active builder-held item. It MUST NOT provide a generic way to order ordinary combat units.

## 3. Mana

Spellcasting buildings MAY have an authoritative mana pool.

Conceptual mutable state:

```rust
pub struct ManaState {
    pub current: FixedMana,
    pub maximum: FixedMana,
    pub regen_remainder: FixedMana,
}
```

Static values such as maximum mana, starting mana, regeneration rate, and ability costs belong in validated content definitions where possible.

Mana changes MUST use deterministic integer/fixed-point arithmetic and simulation ticks.

## 4. Ability state

Each stateful ability MAY track:

```text
cooldown ready tick
cast sequence
charges
channel/cast state if applicable
last target if rules need it
ability-specific deterministic state
```

Mutable state MUST be snapshot/replay safe.

A spell cast MUST NOT become dependent on wall-clock time, animation completion, or presentation frame rate.

## 5. Automatic spellcasting

An automatic spellcasting building periodically evaluates whether an ability can be cast.

The decision may consider:

- source is active/not stunned/disabled;
- sufficient mana;
- cooldown/charges;
- existence of an eligible target or affected population;
- ability-specific minimum-value rules if explicitly defined;
- target range;
- team relationship;
- status/target-category filters.

If legal, the caster chooses the target according to the ability's deterministic target policy and emits a cast intent.

The current verification slice makes one automatic ability per spellcasting building executable. This is deliberately narrower than the eventual content model, but it fixes the scheduler architecture: integer mana regeneration happens in the timer phase before automatic eligibility; mana is clamped to the authored maximum; ability evaluation then reads an immutable post-production/pre-targeting snapshot; committed ability effects are visible to combat target acquisition and ordinary attacks later in the same tick. A unit killed by an automatic ability therefore cannot acquire a target, move, or perform an ordinary attack later that tick.

Automatic casting MUST continue through an individual player's disconnect while the match is still running. If an entire team disconnects and the match enters the canonical reconnect pause, simulation casting pauses with the rest of the simulation.

## 6. Deterministic automatic target selection

Ability target selection MUST NOT depend on ECS query order, spatial-grid traversal order, hash-map order, or worker completion order.

Target policies may include:

```text
nearest eligible
farthest eligible
lowest/highest health
random eligible
random eligible in range
all eligible in area
self-centered area
map-wide team set
```

All non-random ties require a canonical final tie-break such as `SimId`.

The executable verification target policy is currently `RandomEnemyUnit` within an authored finite range measured from the caster building's authoritative footprint. Every eligible candidate receives an order-independent keyed random rank and minimum `(rank, SimId)` wins, so spatial-grid enumeration and worker completion order cannot affect the selected target.

Random target selection uses keyed deterministic RNG. A suitable key is:

```text
match seed
caster SimId
ability stable ID
cast sequence
RandomPurpose::AbilityTarget
```

The candidate set MUST first be canonicalized or selected with an order-independent deterministic algorithm so internal enumeration order cannot influence the random result.

## 7. Cast intents and resolution

Ability evaluation produces an intent rather than directly mutating arbitrary target state.

Conceptual form:

```rust
pub struct CastIntent {
    pub source: EffectSource,
    pub ability: AbilityId,
    pub target: AbilityTarget,
    pub sequence: u64,
}

pub enum AbilityTarget {
    None,
    Entity(SimId),
    Point(SimPoint),
    Area { center: SimPoint, radius: SimDistance },
}
```

At deterministic resolution the simulation validates any rules that must still hold, spends mana/charge, begins cooldown, and emits canonical effects/projectiles.

The current implementation makes cast commitment atomic in one ability-resolution subphase. Automatic intents are canonically ordered by source `SimId`, ability ID, cast sequence, then target `SimId`. Resolution revalidates source state, mana, readiness, target liveness/team/range, and rejects the cast without spending mana or starting cooldown if those conditions no longer hold. A successful cast subtracts mana, sets `ready_tick = cast_tick + cooldown_ticks`, increments the authoritative cast sequence, then applies its effect. Mana profile/state and ability ready/cast-sequence state participate in canonical checksums.

Whether resources are reserved at intent creation or charged at resolution MUST remain explicit for later activation modes; the verification rule above is the initial ordinary automatic-cast behavior.

## 8. Common ability effects

The constrained effect vocabulary should support at least:

- damage;
- healing;
- apply/remove timed status;
- stat buff/debuff;
- stun/disable;
- area effect;
- map-wide team effect;
- spawn authoritative projectile;
- spawn wave/beam-like deterministic attack;
- aura activation;
- resource/mana modification where content requires it.

Effects reuse the deterministic resolution rules in `15-targeting-combat.md`.

## 9. Buffing spell buildings

A spell building may automatically cast a buff on nearby friendly combat units.

Such a building benefits strategically from placement near the route used by friendly units.

The building:

1. remains static navigation geometry;
2. maintains mana/cooldown state;
3. discovers eligible friendly units using the spatial index;
4. selects or affects targets according to its ability definition;
5. spends mana and applies a deterministic timed/status effect.

The builder does not need to remain nearby after placement unless a particular ability explicitly says otherwise.

## 10. Random enemy damage spell buildings

A building may automatically cast a damage spell against a random eligible enemy unit.

The random target MUST be chosen from the canonical eligible set using keyed RNG. Parallel candidate discovery MUST NOT change the selected unit.

Damage source attribution MUST be explicit but MUST NOT implicitly force the victim to retarget. Target changes occur only through the ordinary targeting rules or an explicit taunt/forced-target effect.

## 11. Passive and map-wide auras

An aura is an authoritative gameplay rule even when no projectile or visible cast occurs.

Aura scope may be:

- radius around a source;
- all friendly units on the map;
- another explicit region/filter.

A map-wide builder-item aura does not require the builder to be inserted into combat targeting or collision structures. The source inventory/item is canonical state; eligible friendly combat entities receive the aura's effect according to deterministic aura semantics.

Implementations SHOULD avoid physically adding/removing identical status components every tick if a cheaper deterministic derived-modifier model can produce identical results.

Aura stacking, refresh, source removal, and multiple-copy behavior MUST be explicitly defined by content/effect rules.

## 12. Legendary buildings

Legendary buildings use the same deterministic ability machinery but may have unusual scale or activation semantics.

Examples that MUST be representable include:

- automatically firing an infinite-range powerful wave attack at a deterministic-random enemy unit;
- automatically or periodically stunning every eligible enemy combat unit for a fixed duration such as two seconds;
- firing artillery at a player-selected enemy unit, with the projectile aimed at the target's position at cast/launch time rather than following the target.

`Legendary` is a gameplay/content classification, not a separate nondeterministic subsystem.

## 13. Player-targeted legendary abilities

A manually activated legendary ability uses an explicit `PlayerCommand` carrying the source building and required target.

For entity-targeted artillery:

```text
player selects enemy unit SimId
        ↓
server validates building ownership + ability readiness + target eligibility
        ↓
at canonical execution tick, resolve target's authoritative position
        ↓
commit mana/cooldown/charge
        ↓
spawn projectile whose destination is that captured SimPoint
```

After launch, the projectile MUST NOT follow the target unless the ability definition explicitly describes a homing attack. If the original target moves, the projectile continues toward the captured position and may miss it.

This is distinct from the guaranteed-hit ranged attack mode in `15-targeting-combat.md`.

## 14. Global stun example

A global stun ability conceptually obtains the canonical set of eligible enemy combat units at cast resolution and applies a timed stun to each.

The duration is expressed in ticks.

Application MUST be deterministic regardless of entity query order. If the status is independent per target, execution may be parallelized after the eligible set/effect semantics are fixed.

The builder and other explicitly non-combat entities are not included merely because they belong to the opposing team.

## 15. Ability source and threat semantics

Damage/effects SHOULD carry an explicit source description rather than assuming every source is a targetable attacker.

Conceptually:

```rust
pub enum EffectSource {
    Entity(SimId),
    BuilderItem {
        owner: PlayerId,
        builder: SimId,
        item_instance: ItemInstanceId,
        item_type: ItemId,
    },
    BuildingAbility { building: SimId, ability: AbilityId },
    Environment(EffectId),
}
```

Receiving damage or a hostile effect MUST NOT by itself create a generic retaliation order. An ordinary unit changes target only through the normal autonomous target lifecycle or an explicit forced-target/taunt mechanic.

The executable building-ability damage effect follows this rule: it changes health but does **not** populate ordinary `last_attacker`/self-retaliation state or nearby-ally defense alerts. This is intentionally different from an attack-capable building's ordinary attack, which is an ordinary combat threat source and does create those reactions.

This ensures builder-held projectile items and non-retaliatory building spells can damage enemies without causing units to attempt to target a source merely because an effect was attributed to it.

## 16. Scheduling

The current executable ability phase relationship is:

```text
apply finalized tick inputs
        ↓
automatic ability eligibility/target evaluation
        ↓
ability cast intents
        ↓
commit costs/cooldowns/casts
        ↓
spawn/apply effects and projectiles
        ↓
ordinary combat/projectile resolution according to phase rules
```

For the verification slice, mana regeneration is part of the timer update immediately before this phase, and automatic damage/death is committed before ordinary target acquisition. Future compatibility findings may revise finer cast/windup semantics, but changing this visibility/precedence ordering is a simulation-version change and requires timing regressions plus an update to `13-time-and-scheduling.md`.

## 17. Content requirements

Ability definitions SHOULD include only the fields relevant to their activation/effects, including as applicable:

```text
stable AbilityId
activation mode
mana cost
cooldown
charges
range
candidate/target filters
target policy
area radius
effect list
projectile/delivery profile
status duration
random-purpose identity
stacking/refresh policy
```

All authoritative numeric quantities use deterministic types.

Distinct item copies MUST retain `ItemInstanceId` through source attribution, deterministic ordering, cooldown/charge state, and RNG keys wherever identity matters. Two copies of the same `ItemId` MUST NOT accidentally share a random stream or effect-order identity.

## 18. Effect expansion and termination

Effects that can recursively generate more effects (reflection, on-hit, on-damage, death triggers, chained procs, etc.) MUST have explicit termination semantics.

Content validation SHOULD reject unbounded static effect cycles. Runtime resolution MUST additionally enforce a deterministic per-root expansion budget/depth guard as a safety invariant. Exceeding that guard is a simulation/content error that is surfaced deterministically; the engine MUST NOT hang or depend on wall-clock watchdog timing.

The guard value and failure behavior are part of the simulation version.

## 19. Performance

Automatic spellcasting MUST not imply an all-world scan per building per tick.

Use the dynamic spatial index for finite-radius abilities. Global abilities may use canonical team/class indexes or other derived sets.

Expensive ability evaluation MAY be staggered if and only if the stagger schedule is itself deterministic and does not change the intended cast semantics.

## 20. Required tests

The ability test suite MUST eventually include:

1. mana regeneration and costs are identical across worker counts;
2. automatic caster chooses the same target when candidate enumeration order is permuted;
3. random target choice is stable across worker counts and replay;
4. nearby friendly buff building applies only to eligible units;
5. map-wide aura applies without making the builder targetable or blocking;
6. hostile item/building effect does not implicitly retarget its victim to the builder;
7. global stun applies to all and only eligible enemy combat units for the exact tick duration;
8. player-targeted artillery captures the target's position at command execution/launch and does not follow later movement;
9. an individual player's disconnect does not pause autonomous building casting while a teammate remains connected;
10. snapshot/reload preserves mana, cooldown, charge, and cast-sequence state;
11. rejected manual cast spends no mana/charge and creates no projectile/effect;
12. replay of the same finalized input stream reproduces all ability outcomes exactly;
13. two identical item types with different `ItemInstanceId`s do not share RNG/cooldown/effect identity;
14. recursive effect definitions are rejected or deterministically trip the expansion guard rather than hanging.
