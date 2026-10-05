# Abilities, Mana, and Spellcasting

Status: **normative architecture, provisional gameplay details**

## 1. Purpose

This document defines deterministic abilities used by buildings, autonomous combat-unit spellcasters, items, and other authoritative sources that can cast spells.

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

Spellcasting buildings and combat units MAY have an authoritative mana pool.

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

An automatic spellcasting building or combat unit periodically evaluates whether an ability can be cast.

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

An automatic source supports up to `MAX_AUTOMATIC_ABILITIES` distinct ability IDs (currently eight), sharing one mana pool. Each ability retains its own ready tick, cast sequence, autocast/manual flags, and applicable delayed secondary-action state. Duplicate IDs and capacity overflow MUST be rejected atomically rather than merged, silently dropped, or assigned unit-specific exceptions. Ability registration order MUST NOT affect results; same-source casts compete for mana in the canonical intent order and revalidate after each preceding commitment. The existing building control commands continue to address the primary ability; additional slots do not implicitly extend player commands. Content translation MUST combine native and scripted automatic definitions instead of silently choosing one. All translated definitions must agree on the source's mana profile. An explicit primary preserves control identity; without an explicit primary, the lowest ability ID is selected deterministically. A conflicting mana profile is a content error, not a choice of which source values to keep.

Combat-only hostile autocasts select the nearest eligible unmarked enemy, with stable identity tie-breaking, using authoritative combat/target state. The commitment phase MUST recheck live target eligibility, immunity, existing status and mana before spending resources. A timed armor/reveal effect is keyed by modifier identity, retains ordinary and hero duration parameters separately, and expires both armor and reveal state together; repeated eligibility MUST NOT stack or refresh an existing mark implicitly. Visibility consumers may use the authoritative revealing team, but presentation cannot control the effect lifetime.

The scheduler contract remains: integer mana regeneration happens in the timer phase before automatic eligibility; mana is clamped to the authored maximum; production happens after that timer phase, so a caster spawned during tick `T` begins with its authored starting mana and first regenerates during tick `T + 1`; ability evaluation then reads an immutable post-production/pre-targeting snapshot. Building and unit cast intents share one canonical resolution order. Committed ability effects are visible to later ability intents, combat target acquisition, and ordinary attacks in the same tick. A unit killed or stunned by an earlier canonical automatic ability therefore cannot cast a later pending intent, acquire a target, move, or perform an ordinary attack as applicable. An ordered spellcasting source is ineligible while native stun or independent script/order recovery is active; mana regeneration and absolute cooldown readiness continue. Native passive firing that does not issue an interruptible order ignores ordinary stun/order recovery at both evaluation and live commitment, but MUST still honor ability disable, death, resources, cooldown and target/buff eligibility. Primary and additional slots obey the same interruption rules.

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

The executable verification target policies are currently `RandomEnemyUnit`, `RandomEnemyUnitGlobal`, and `AllEnemyUnits`. `RandomEnemyUnit` selects within an authored finite range measured from a unit caster's authoritative point or a building caster's authoritative footprint. `RandomEnemyUnitGlobal` selects one live enemy combat unit anywhere on the map and uses authored range `0` as an explicit global sentinel. `AllEnemyUnits` likewise uses range `0` and addresses the complete live hostile combat-unit set. Every random eligible candidate receives an order-independent keyed random rank and minimum `(rank, SimId)` wins, so spatial-grid enumeration and worker completion order cannot affect the selected target. Global selection is therefore deterministic without pretending that global range is a very large finite radius. `AllEnemyUnits` resolves its affected unit set in stable `SimId` order.

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

The current implementation makes cast commitment atomic in one ability-resolution subphase. Automatic intents are canonically ordered by source `SimId`, ability ID, cast sequence, then a stable target-kind/target-identity key. Resolution revalidates source active/stun state, mana, readiness, and applicable target liveness/team/range rules, and rejects the cast without spending mana or starting cooldown if those conditions no longer hold. A successful cast subtracts mana, sets `ready_tick = cast_tick + cooldown_ticks`, increments the authoritative cast sequence, then applies its effect. Mana profile/state, every ability's profile and ready/cast-sequence/control state, delayed secondary-action identity/deadlines, and timed stun state participate in canonical checksums and logical/wire snapshots. Due secondary actions are resolved in source `SimId`/ability-ID order against live mana and corpse availability; one slot MUST NOT overwrite another slot's pending action or cooldown.

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
- resource/mana modification where content requires it;
- corpse query/selection;
- consume corpse;
- corpse-driven area damage (for example corpse explosion);
- corpse-driven unit spawning/transformation (for example raise dead).

Effects reuse the deterministic resolution rules in `15-targeting-combat.md`. The executable `AreaDamage` primitive requires a selected enemy unit, captures that unit's authoritative position at cast resolution as the area center, and damages every live enemy combat unit whose authoritative point lies within the authored radius, including the selected unit itself. The affected set is traversed in stable `SimId` order; the primitive does not implicitly damage buildings and does not generate ordinary attack retaliation/ally-defense events. `AreaDamage` is therefore not valid with `AllEnemyUnits`, which has no single center. Corpse-targeted abilities operate on authoritative corpse entities rather than presentation objects. They MUST revalidate corpse existence/eligibility at resolution, use canonical ordering/tie-breaking when selecting among multiple corpses, and atomically consume a corpse when the effect definition says it is spent so one corpse cannot satisfy multiple competing casts nondeterministically.

### 8.1 Native target qualifiers

Script selection and native effect eligibility are distinct. A script's combat-sapper or marker restriction MUST be checked at evaluation and live commitment without imposing it on unrelated autonomous native firing. Physical target classes do not replace relation, hero/organic or vulnerability qualifiers; consumers MUST implement retained qualifiers or reject unsupported evidence explicitly rather than discarding tokens.

Native spell carriers honor spell immunity; independent Barrage arrows remain ordinary weapon damage and do not inherit that spell-only restriction. Both families exclude invulnerable targets at launch and revalidate invulnerability at impact, before any damage, buff or cleanse mutation. Autonomous native passive firing checks source-team visibility/reveal at both evaluation and live commitment; another team's reveal is not sufficient. This rule does not replace a script's explicit vision-granting dummy helper or its independent selector. A committed missile does not rerun launch visibility selection during flight. Native unit DOT pulses consult live immunity/invulnerability, still advance their deadlines while blocked, and expire normally without replaying skipped damage when vulnerability returns.

### 8.2 Native projections and removal

A temporary morph or ability-disable projection MUST NOT overwrite the logical unit's persistent movement/armor/passive baseline or its cold resurrection definition. Removing explicitly listed permanent grants operates on the live baseline, not on a snapshot whose passives may currently be suppressed. After native morph removal, the current baseline is projected again; unrelated grants remain intact.

Post-cast order recovery has its own exclusive absolute deadline in authoritative status, separate from removable native stun. It is preserved in hashes/snapshots and suspends normal targeting, movement and ordered casting; an explicitly scripted retreat can run during that recovery. Starting recovery must invalidate later same-tick ordered intents without suppressing autonomous native passive firing.

Native buff removal and independent script control are separate operations. Dispel MUST NOT erase delayed callbacks, order recovery, or unrelated permanent control state merely because they affect the same target. Ability-granted state is removed only when the version-scoped source recipe explicitly identifies that grant. Native positive-damage/live-source removal predicates are revalidated at impact; failure MUST NOT dispel the existing native/control states, even when the committed projectile still deals damage or applies its own native buff. Presentation restore events do not substitute for authoritative removal or schedule gameplay callbacks.

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

A building may automatically cast a damage spell against a random eligible enemy unit. Its policy may be finite-range or explicitly global. A common verification pattern is a mana-charging building whose ability cost equals maximum mana, so it casts immediately upon reaching full mana, chooses one deterministic-random enemy globally, and applies `AreaDamage` around that unit.

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

A global stun ability obtains the canonical set of live eligible enemy combat units at cast resolution and applies a timed stun to each in stable `SimId` order. The current executable slice targets combat units only; passive geometry/builders are not included merely because they belong to the opposing team.

Stun duration is expressed in ticks with an exclusive absolute expiry. A stun committed during tick `T` for positive duration `D` stores `stunned_until_tick = T + D` and the entity is stunned while `current_tick < stunned_until_tick`. Thus duration `1` suppresses actions during the cast tick and expires before tick `T + 1`; duration `2` suppresses ticks `T` and `T + 1`. Reapplication uses the later expiry (`max(old, new)`), so a shorter stun cannot truncate a longer existing stun.

While stunned, a combat unit performs no fresh target acquisition, retaliation/ally-defense target switch, ordinary attack, or intentional movement. A still-valid pre-stun target is retained rather than cleared. Attack/spell buildings with status state likewise do not retarget, attack, or cast while stunned. Cooldowns and mana regeneration continue according to their normal absolute/timer rules. Receiving an ordinary attack while stunned does not create a delayed retaliation beyond the normal one-tick retaliation memory; an attack on the final stunned tick may therefore be consumed on the next active targeting phase, while older attacks are not queued indefinitely.

Application MUST remain deterministic regardless of entity query order or worker count. Parallel effect execution is permitted only after the affected set and conflict semantics are fixed.

### 14.1 Timed movement-speed modifier verification primitive

The verification implementation also contains one deliberately narrow timed stat-modifier primitive so the engine can exercise canonical modifier storage, refresh, stacking, expiry, and effective-stat derivation without mutating authored unit definitions. `ModifyMovementSpeedPercent` carries a stable `ModifierId`, a signed integer percentage delta, and a positive duration in ticks. The base `MovementProfile` remains immutable authoritative content; each movement step derives an effective speed from the currently active modifiers.

For this provisional primitive, modifiers are stored in stable `ModifierId` order in a fixed-capacity authoritative status array. Reapplying the same `ModifierId` with the same authored percentage refreshes to the later expiry and does not stack another copy. Different IDs stack additively. Effective percent is `clamp(100 + sum(percent_delta), 0, 1000)`, and effective integer speed is `base_speed * percent / 100` with deterministic integer truncation. Expiry uses the same exclusive absolute-tick convention as stun: an entry with `expires_tick == T` is removed during the timer phase for tick `T` before movement is evaluated.

This rule is an executable architecture primitive, **not** a claim that all original Warcraft III buffs/auras use this stacking model. Imported content must still define whether a particular effect refreshes, replaces, stacks by source, stacks by buff identity, is aura-derived, or uses Warcraft-specific movement/attack-speed clamps. The modifier representation and import mapping may be revised when extraction establishes those exact semantics.

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

Imported abilities additionally separate native behavior from map-version tuning. A behavior binding identifies a stable native implementation and the inclusive Castle Fight map-version range for which that implementation's semantics are valid. A selected map version then supplies extracted numeric tuning to that implementation. Balance-only changes therefore do not require duplicated Rust behavior, while semantic rewrites require a new implementation ID and a new, non-overlapping validity range. A missing binding for the selected version is an implementation gap and MUST NOT fall back to an adjacent map version.

Passive/on-hit effects use the same rule. The current 9.27 playable-unit verification slice covers Ranger Evasion (`A00U`); Defender Evasion plus automatically maintained Defend (`A00U`, `A03G`); Catapult Burning Oil (`A02J`); Ice Troll Shadow Priest's orb-triggered Entangling Roots (`A049` → `A03W`) and Frost Armor autocast (`A03Z`); plus Gryphon Rider Bash (`A05K`) and orb-triggered Chain Lightning (`A01B` → `A05X`). Proc RNG is keyed by match seed, tick, attacking `SimId`, ability rawcode, and authoritative attack sequence, and procs are resolved only for non-missed/non-evaded attacks. Guaranteed-hit ranged attacks carry already-resolved on-hit payloads until impact. Defender Defend is an impact-time defensive passive: it applies its extracted ranged/spell reductions to live damage resolution and uses a separate keyed projectile-deflection roll whose successful unit-source result launches a bounded authoritative return projectile as specified in `15-targeting-combat.md`. Roots/Wand of Freezing uses deterministic timed disable plus damage-over-time state: afflicted units cannot intentionally move or perform ordinary attacks for the authored duration, and the disable participates in the same authoritative stun expiry used by the client status presentation. Chain Lightning applies the primary hit when the proc resolves, then resolves each later jump at Warcraft's fixed 0.25-second cadence; because the authoritative simulation runs at 30 Hz, cumulative quarter-second deadlines are rounded upward to ticks, producing alternating 8/7-tick gaps. Each jump chooses the canonical nearest valid, still-unhit target at that jump's resolution time using distance/`SimId` ordering rather than precomputing the full chain. Burning Oil creates deterministic persistent impact zones, and Frost Armor uses fixed-point mana regeneration plus timed armor and melee-retaliation slow state. All pending/projectile/status/zone/chain/reflection state that can change future gameplay participates in canonical checksums.

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
14. recursive effect definitions are rejected or deterministically trip the expansion guard rather than hanging;
15. timed movement modifiers expire on the exact tick, same-ID reapplication refreshes without duplicate stacking, distinct IDs combine deterministically, and worker count does not alter the resulting state;
16. a versioned native effect loads numeric tuning from the selected map version, rejects uncovered versions, and switches implementation IDs rather than silently reusing behavior when semantic ranges differ;
17. passive attack procs and evasion are deterministic across worker counts, do not trigger through a missed/evaded attack, and ranged on-hit status is applied at projectile impact rather than launch;
18. Roots/Wand of Freezing periodic damage includes the final whole-second pulse of its configured duration and its movement/attack disable expires on the exact authored tick;
19. Frost Armor friendly autocast targets an eligible ally attacked on the preceding tick, spends exact fixed-point-regenerated mana, refreshes no duplicate active armor instance, and applies its melee retaliation slow deterministically;
20. persistent Burning Oil zones and Chain Lightning jump selection produce identical authoritative state across worker counts.
