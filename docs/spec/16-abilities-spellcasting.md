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

Spellcasting buildings and combat units MAY have an authoritative mana pool. A native building with mana capacity retains and regenerates its pool even while it has no cast ability; granting an ability or completing an upgrade must not silently discard that pool.

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

Native positive stat autocasts select the nearest eligible unbuffed friendly combat unit with stable identity tie-breaking. Cast range and native autocast acquisition range are independent limits; mechanical, flying and magic-immune recipients remain eligible when the source target mask permits them. Invulnerable recipients require explicit source permission. Commitment revalidates the buff family, so competing casters do not spend resources on a recipient already buffed earlier in the phase. A native buff family's replacement removes the previous complete bundle even when a different ability granted it; intrinsic auras and permanent script bonuses remain separate. Native buff identity, polarity and transfer eligibility are authoritative and must survive checksums and snapshots with the timed armor, outgoing damage and regeneration payload.

Native Spell Steal selects an eligible positive hostile buff or negative friendly buff and resolves an opposite-side recipient within the independent source-owned transfer area around its donor. Stable distance/identity ordering selects donors and recipients; buff family/source ordering selects among multiple eligible payloads. Both sides and the buff are revalidated at release before committing resources. Transfer moves one complete timed payload, preserves its absolute expiry and granting ability for presentation, and assigns a negative reveal to the stealing team. Destination immunity, organic restrictions and existing buff families remain independent from donor eligibility. Auras, permanent script bonuses, hero spell families and unmarked weapon side effects do not become transferable through their visible status alone.

A kill-triggered native self-buff commits at credited lethal impact on an eligible combat unit, rather than at weapon release or deferred corpse removal. The live unit source must still be able to issue its native order; a building kill, external death or dead source cannot activate it. Refresh replaces the same granting ability's timed movement, attack-rate and incoming-damage payloads. The free instant order clears pending unit orders and resumes autonomous attack acquisition while already released missiles remain independent. Its incoming multiplier affects subsequent weapon, spell and periodic damage until the authoritative expiry. Later canonical hits in the same phase observe an earlier kill's buff. Timed payloads and their source identity must survive wire restoration across worker counts.

An Ensnare proxy separates the interruptible parent cast from its committed native missile. Parent trigger/cast range, target-centered bounded search and caster-origin child range are independent. A bounded script counts attempted casts, including native failures, rather than filling its quota with successful effects. The physical net can affect magic-immune secondary recipients even when the parent cannot select them. Its homing missile survives proxy/caster removal, revalidates living hostile and invulnerability at impact, and starts the recipient's ordinary or hero expiry there. The net blocks voluntary movement and separation displacement while permitting attacks; it exposes the unit as a ground combat target without changing its underlying movement/collision class. Native melee range, lowering timing, source identity and absolute lifetime are authoritative. Pending casts revalidate grounding before spending resources; presentation derives net art and lowering from the retained source and state.

A reduced damage listener observes the victim's live health before that event commits its health reduction. Separate events in canonical order observe previous committed reductions, including events from weapons, abilities, projectiles and periodic effects. A source-owned persistent threshold controller distinguishes adding an absent native ability from setting the level of an existing ability: level assignment does not implicitly grant the ability, even when the script still changes vertex color. Only damage events run this controller; healing alone does not grant or downgrade its state. Reached ability level, fixed attack bonus and independently reached color persist through ordinary native buff removal and authoritative recovery. Fixed weapon bonuses participate in directed weapon damage before critical resolution and are captured by released projectile payloads. A corpse retains the source's final script vertex color for presentation after recovery.

Native weapon fire leaves a committed impact-centered zone independent of the attacker's continued existence. Each pulse samples live hostile ground recipients and the independently authored structure mask, excludes native invulnerability and spell immunity, and preserves incoming damage modifiers. Full and reduced phases use separate source intervals and durations; a whole pulse due exactly at zone expiry resolves before removal. Zones and their phase counters survive authoritative recovery. Persistent ground art and recipient burning art read the live zone projection, including visibility and removal.

Native weapon poison begins at a successful directed impact on an eligible surviving organic recipient. Each attacker/native buff family owns an independent periodic damage lifetime when the source enables damage stacking; repeated hits from that attacker in that family refresh the profile and expiry without postponing its scheduled pulse. Distinct native buff families retain their own damage values, expiry, icon and model identity. Movement and attack-rate penalties do not multiply with attacker count: the strongest surviving penalty for each component applies, falling back as individual lifetimes expire. This cross-variant slow rule is an explicit engine interpretation; retained map flags describe stacking categories, while independent native experiments have not established every mixed-variant interaction. Normal and hero durations remain separate. Whole-second pulses include the exact expiry when due, respect native organic/physical target rules and incoming damage modifiers, and preserve a health floor when the source excludes lethal poison. Delivered poison survives attacker removal. Explicit full native buff removal invalidates the old generation, so immediate reapplication cannot revive its pending damage. Source, pulse deadlines, generations and recipient status are authoritative and survive recovery across worker counts. Ordinary acquisition can prefer unpoisoned recipients without overriding retained combat targets or retaliation locks.

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

Building casts and zero-cast-point unit casts commit atomically in one ability-resolution subphase. Automatic intents are canonically ordered by source `SimId`, ability ID, cast sequence, then a stable target-kind/target-identity key. Resolution revalidates source active/stun state, mana, readiness, and applicable target eligibility before starting an ordered unit cast. A unit cast with a positive versioned cast point then commits an authoritative pending cast containing the selected ability, cast sequence, stable target identity/point, and absolute release tick; it starts cast animation recovery but does not yet spend mana, start cooldown, or apply the spell effect. At the cast point, the source must still be alive/able to cast, the selected entity/corpse/building must still exist where applicable, and mana/readiness must still permit the cast. Trigger-only AI predicates such as "recently attacked" and approach/range selection are not re-run after wind-up: they chose the committed target before casting and are not effect-time eligibility rules. A successful release subtracts mana, sets `ready_tick = release_tick + cooldown_ticks`, increments the authoritative cast sequence, then applies its effect. If release validation fails, the pending cast is consumed without silently selecting another target or spending resources. Mana profile/state, every ability's profile and ready/cast-sequence/control state, pending-cast identity/deadline/target, delayed secondary-action identity/deadlines, and timed stun state participate in canonical checksums and logical/wire snapshots. Due secondary actions are resolved in source `SimId`/ability-ID order against live mana and corpse availability; one slot MUST NOT overwrite another slot's pending action or cooldown.

Whether resources are reserved at intent creation or charged at resolution MUST remain explicit for later activation modes; the verification rule above is the initial ordinary automatic-cast behavior.

Successful ordered unit casts start the authoritative animation recovery defined in `15-targeting-combat.md` §15 before their spell effect is released. The versioned unit cast point plus backswing supplies the recovery duration; the effect releases at the cast point, while the caster remains movement-locked through the remaining backswing unless an authored post-effect retreat/order truncates that interval. An ordered spell may preempt an attack animation and replace it with cast recovery. No voluntary movement, collision steering, fresh ordered cast or ordinary attack may interrupt the committed wind-up/backswing. Failed initial target/resource validation starts no recovery; a cast interrupted or invalidated before release produces no effect. Recovery is independent of mana, spell cooldown, removable buffs, and script-specific order recovery; every applicable constraint must expire before the unit resumes the corresponding action. Autonomous Phoenix Fire does not issue a caster order and therefore neither starts nor interrupts animation recovery. Building casts have no unit locomotion recovery and retain their immediate resolution semantics.

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

Native spell carriers honor spell immunity; independent Barrage arrows remain ordinary weapon damage and do not inherit that spell-only restriction. Both families exclude invulnerable targets at launch and revalidate invulnerability at impact, before any damage, buff or cleanse mutation. Autonomous native passive firing checks source-team visibility/reveal at both evaluation and live commitment; another team's reveal is not sufficient. This rule does not replace a script's explicit vision-granting dummy helper or its independent selector. A committed missile does not rerun launch visibility selection during flight. Native unit and structure DOT pulses consult live immunity/invulnerability, still advance their deadlines while blocked, and expire normally without replaying skipped damage when vulnerability returns. Structure effects consume authoritative intrinsic classifications from the building snapshot, independently of produced-child flags; pending construction, definition replacement and snapshot restoration preserve those flags.

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

Intrinsic zero-linger speed auras sample live recipient membership before movement and attack cadence. Their versioned profiles explicitly retain hostility, native buff family, movement-class mask, organic and invulnerable permissions, radius and signed speed deltas. Hostile native auras exclude spell-immune recipients; invisible or ability-suppressed sources do not contribute. Within one native buff family, movement and attack speed independently select the strongest eligible magnitude and never add duplicate contributions. Membership ends without an additional lingering tick; distinct buff families retain their independent modifier identities.

Native Sphere is an authoritative presentation primitive: released attacks/spells rotate attachment slots, each with its own restore deadline, and can launch independent visual-only missiles. These flights never resolve damage or trigger combat effects. Their identities MUST use a separate allocation namespace so visual creation cannot change later gameplay entity IDs or deterministic random results. Attachment deadlines, flight destinations/deadlines and the visual allocation counter participate in snapshots/checksums; the client derives visibility and progress without providing gameplay timing.

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

Passive/on-hit effects use the same rule. The current 9.27 playable-unit verification slice covers Ranger Evasion (`A00U`); Defender Evasion plus automatically maintained Defend (`A00U`, `A03G`); Catapult Burning Oil (`A02J`); Ice Troll Shadow Priest's orb-triggered Entangling Roots (`A049` → `A03W`) and Frost Armor autocast (`A03Z`); plus Gryphon Rider Bash (`A05K`) and orb-triggered Chain Lightning (`A01B` → `A05X`). Proc RNG is keyed by match seed, tick, attacking `SimId`, ability rawcode, and authoritative attack sequence, and directed on-hit procs are resolved only for non-missed/non-evaded attacks. Guaranteed-hit ranged attacks carry already-resolved on-hit payloads until impact. Defender Defend is an impact-time defensive passive: it applies its extracted ranged/spell reductions to live damage resolution and uses a separate keyed projectile-deflection roll whose successful unit-source result launches a bounded authoritative return projectile as specified in `15-targeting-combat.md`. Roots/Wand of Freezing uses deterministic timed disable plus damage-over-time state: afflicted units cannot intentionally move or perform ordinary attacks for the authored duration, and the disable retains its own authoritative roots expiry, separate from stun. Roots permit spellcasting; their mask can independently permit structures. Chain Lightning applies the primary hit when the proc resolves, then resolves each later jump at Warcraft's fixed 0.25-second cadence; because the authoritative simulation runs at 30 Hz, cumulative quarter-second deadlines are rounded upward to ticks, producing alternating 8/7-tick gaps. Each jump chooses the canonical nearest valid, still-unhit target at that jump's resolution time using distance/`SimId` ordering rather than precomputing the full chain. Burning Oil creates deterministic persistent impact zones, and Frost Armor uses fixed-point mana regeneration plus timed armor and melee-retaliation slow state. All pending/projectile/status/zone/chain/reflection state that can change future gameplay participates in canonical checksums.

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

## Native proxy area stun

A scripted proxy area stun separates its ground combat-sapper cast trigger from the native effect. Trigger commitment respects the primary order's range, resource cost, cooldown, vulnerability, spell immunity and nonhero restriction. The native effect originates at the caster and can affect ordinary non-sapper units and heroes inside its separate radius. It excludes allies, air units, invulnerable units and spell-immune units, applies spell damage and the appropriate ordinary/hero stun duration, and emits the child ability's presentation identity at that same origin. Absolute stun/cooldown deadlines and all profile fields are covered by checksums and snapshots.

Barrage source discovery uses the retained source object's ability inventory for both unit and building sources. Each independent in-flight arrow retains its source version and Barrage ability identity; homing speed is resolved from that same versioned ability rather than from a particular tower definition. Barrage arrows retain ordinary weapon damage and do not inherit the primary arrow's orb proc.

An ordinary attack's proc eligibility and its invoked child's target restrictions remain separate. Entangling Roots retains the child's explicit nonhero qualifier and independent hero duration. A child rejected for classification does not cancel or undo the already landed ordinary attack. Those profile fields participate in content identity, checksums and in-flight/snapshot state.

Native Frost attacks retain a separate slow payload alongside chance-based child spell procs. A landed directed attack applies the payload at impact using live target immunity, movement class and hero duration; misses do not apply it. Source durations and both slow rates are versioned projections of retained object fields and map Misc constants. Fractional per-second mana rates accumulate through the authoritative regeneration remainder.

Caster-centered proxy debuffs distinguish their selected order trigger from the native effect mask. Timed armor modifiers may carry signed weapon damage changes; active changes add in fixed point against the ordinary weapon damage, with the final damage clamped to zero. Reapplication of the same modifier replaces its state. Hero and ordinary effect expiries remain independent source-derived deadlines.

Native Frost Nova combines specific-target and area damage on its selected primary and applies area damage to independently eligible victims around that target. Its slow retains source normal/hero deadlines. Proxy trigger eligibility remains distinct from native effect eligibility, including a parent that permits hero triggers.

Entangling Roots retains an independent authoritative roots deadline, blocks ordinary attacks and intentional movement, and permits spellcasting. A source child mask that permits structures can apply weapon disable and periodic damage through authoritative building status. Native friendly auras use source-owned radius, payload, recipient weapon classes and lingering duration. Duplicate sources of the same ability refresh one modifier; an excluded weapon class, hostile unit or structure cannot receive it. A zero-duration aura expires outside its active phase; an authored positive linger survives departure or source removal. Mana auras sample live sources before timer-phase regeneration, use flat source-derived regeneration and can remain active during the source's spell cooldown. A zero-duration mana aura does not require an artificial extra linger tick to contribute.

Script-owned terrain effects retain version-scoped grid ownership independently of native removable buffs. Incoming damage modifiers follow the victim's live position/team at each damage phase, including structures and periodic damage. Manual point spells are authoritative commands; shared owner cooldowns and terrain mutations survive snapshot recovery. Point explosions can require both the victim and its marked tile center within the effect radius. Native universal damage bypasses armor and native spell resistance while still passing through script-authored incoming-damage listeners.

Native Freezing Breath retains a distinct exclusive freeze deadline and originating ability identity alongside the ordinary disable deadline. While frozen, an entity cannot release ordered or autonomous spells, ordinary attacks, or movement. An interrupted attack/cast windup is discarded. Building production and construction preserve remaining work by advancing their absolute deadlines during frozen ticks, with no burst of overdue production on thaw. A frozen building rejects a new upgrade, and cancelling an existing upgrade preserves an incoming freeze. Mana, ordinary cooldown and passive regeneration timers continue. A released attack proxy missile keeps its owner, target/destination, native weapon/effect tuning and art identity independently of the caster's lifetime. Its ordinary weapon damage and its spell-immunity-sensitive freeze use separate eligibility checks; the passive ability's inherited area field does not replace the weapon's splash geometry.

A native spell trigger and its script callback selector are independent. Parent eligibility, mana and cooldown are validated and committed before the callback searches for its own effect target. An empty callback selection or rejected secondary order does not refund the parent. Script-created art is emitted only after the callback reaches its art statement.

Native targeted area debuffs commit their parent resources independently of a script-selected child recipient. The child applies its own range, visibility and native target mask. Its released missile retains the source homing flag and source-owned flight state independently of caster survival; a nonhoming missile retains its initial destination and deadline while the targeted effect resolves the surviving live recipient. Native Acid Bomb applies one family of armor and signed movement/attack-speed changes to the primary recipient and eligible nearby units, with independent primary/secondary pulse damage and normal/hero duration. Damage begins at impact and repeats at the authored interval strictly before expiry. Reapplication replaces that family rather than multiplying separate custom buffs. Explicit full native buff removal clears its authoritative periodic state; magical dispel and transferable buff payloads do not infer that eligibility. Future script animation callbacks retain their absolute start and exclusive end deadlines through snapshots; presentation consumes them without affecting cast or damage timing.

Native script-installed permanent abilities MUST retain identity and complete payload independently of timed buffs. Re-adding an installed rawcode leaves its payload unchanged; distinct rawcodes can coexist. Item damage and speed bonuses and flat armor contribute to their respective live calculations, while maximum-life changes adjust the live maximum once per identity. The current engine contract retains current life on addition and clamps it on removal; this interpretation is distinct from a scripted heal. Granted Critical Strike and Evasion use the same native-family resolution as intrinsic passives; ability-disable projections suppress those passives without deleting their permanent state. Native Demolish applies its target-filtered weapon multiplier at release without marking the attack critical or enabling critical-only companion mechanics. Released projectile damage remains committed after grant removal. Ordinary buff removal and Spell Steal do not remove these abilities; explicit native removal lists revoke exactly their identities, including the associated maximum-life adjustment. Grant inventory order, payloads and identities MUST survive canonical checksums and wire recovery.

Script-selected permanent grants may have a native hostile autocast trigger and a separate friendly recipient picker. The picker retains alive/combat classification, allegiance, invulnerability, battlefield bounds and its literal first-pass/retry predicate. A marker used by that predicate is not implicitly installed by the cast. Option randomness is keyed before recipient selection, including empty selections; resource and cast-sequence commitment remain those of the parent order. Permanent grants do not acquire a lifetime from unused native duration fields or their visible self-aura markers.


### Native trap summons

A native Stasis Trap retains its summoned ward identity, fixed origin, activation deadline, detection radius, detonation delay, effect radius and lifetime as authoritative state. Activation and detonation delays are rounded independently. An armed trap waits for a live hostile recipient allowed by its effect mask; invulnerable and spell-immune recipients cannot trigger or receive its magical stun. Detection commits detonation, while detonation samples live recipients at the trap origin with separate normal and hero stun durations. It does not reuse the parent building spell's hostile trigger mask or the original trigger recipient's position. Expiry removes a trap before detection or detonation. A released trap survives its casting dummy and source building.

Native detonation consumes nearby traps of the same summoned ward type before their pending actions can execute. The engine applies that removal across owners, consistent with the unrestricted native trap description; owner scope and invisible-recipient detection have not been independently tested in a native oracle. Invisible combat recipients follow the ordinary mask rather than a visibility requirement for this autonomous detection/effect. A script-hidden trap is retained as a gameplay action without targetable or visible unit presentation. Recipient stun art and caster animation intervals read authoritative state.

### Temporary native aura carriers

A scripted native aura carrier retains its owner/team, fixed caster-centered point, creation and removal deadlines, source effect identity, recipient mask, radius and payload as authoritative state. It is independent of its source building after release. A source dummy with invulnerability, locust, zero movement/collision and no model has no combat or selectable unit presence. Its script removal timer is independent of the source's delayed Stand animation.

A flat native healing aura samples live friendly unit membership, including the source's independent air/ground, organic and invulnerability permissions. Positive healing permits spell-immune recipients and ignores negative-building shields. It neither repairs excluded structures nor raises dead handles. With zero native buff duration, leaving the aura removes its contribution before the next regeneration interval; carrier removal leaves no extra linger. Contributions with one native buff identity select the strongest live regeneration, movement and attack-speed payload independently, falling back when each contributor expires. Signed speed payloads compare absolute magnitude, with the positive value winning an equal-magnitude tie. Duplicate carriers do not add extra copies of a stat. Each selected stat retains its own exclusive deadline; a shared nontransferable native-buff anchor retains family membership and is cleared together with its paired modifiers when membership changes. Distinct native buff identities remain independent. These continuously supplied aura buffs cannot be transferred by Spell Steal and reapply while their carrier remains eligible.

The fixed-step integration contract credits one regeneration interval on each tick after creation through the carrier's removal deadline, including that closing interval. Timer-phase sampling temporarily covers the closing interval; effect-phase membership and presentation use the exclusive removal deadline. Fractional HP combines with the existing authoritative regeneration remainder, clamps at maximum life and clears its remainder at full health. Sampling at simulation ticks and strongest-payload precedence across custom variants are engine interpretations; native field data establishes the rate and duration, without independently proving native sampling cadence. Healing-aura recipient art requires live regeneration and missing health; its icon, model, attachment metadata and inspection values consume source evidence and authoritative state.

A native speed aura samples live friendly unit membership with its independent air/ground, organic and invulnerability permissions. Movement and attack-speed bonuses apply immediately at creation and stop at the exclusive removal deadline, without the healing integration's closing-interval extension. They use the existing signed speed primitives and do not modify health. Interpreting absent vulnerable/invulnerable target tokens as no invulnerability predicate, and selecting each strongest stat across custom native-aura variants, are explicit engine interpretations without a native oracle experiment.

An unmodeled aura dummy can still carry authored ability target art. A read-only carrier view supplies its stable identity, source/team, point, ability and creation/removal deadlines to presentation. Source art follows that live view independently of source-building survival or replayed cast events; recipient art follows the actual live buff payload. Neither view creates a selectable unit or supplies gameplay timing.

Native damage-charge controllers observe reduced damage events before health commitment, preserving fractional signed mana regeneration. Arming removes the controller marker and enables a granted native self-destruct order; it neither absorbs damage nor implies immediate detonation. Pending contact casts are authoritative and interruptible. Native physical ability damage can retain armor-point mitigation while bypassing attack-class multipliers; caster death and ordinary corpse destruction do not themselves create an additional damaging explosion.

Native script-ordered Mirror Image retains a source damage-event sequence, cast-point and split deadlines, cooldown and native image profile. Only registered combat-sapper damage events enter this dispatcher; a zero-damage hit may enter while evasion does not. Native order gates remain separate from the script's proc roll. Images retain owner, source, lifetime and separate incoming/outgoing weapon-damage factors in authoritative state. Recast/removal unsummons without creating corpses. Split invulnerability and order suspension are derived from authoritative state.

Native summoner proxies can hand off production to a different combat body. Production command metadata remains keyed to the trained object; the spawned body consumes its own versioned definition. Intrinsic Lava Spawn counters observe landed eligible attacks, including zero-damage hits and excluding misses, then retain an authoritative split deadline and generation limit. Splits heal the surviving parent and may resolve a script-selected child replacement using native default life. Child ownership, production provenance and delayed strategic-order refresh persist through recovery. Hidden zero-cast-point summoners are collapsed into that atomic handoff; their noncombat retirement does not create a combat corpse.

Native orb cone effects are independent traveling waves with a fixed heading, linearly expanding radius, one-hit history and a shared raw damage budget. Trigger-target eligibility does not constrain other cone recipients. Recipient eligibility is sampled at passage and excludes allies, structures and native immunity unless the versioned profile permits them. Spatial hit order uses forward distance then stable identity. Launched waves survive source removal and retain native missile/impact presentation separately from gameplay. Cone geometry, hit ordering  are explicit deterministic interpretations pending a Warcraft runtime oracle.

Scripted mover arrays retain native timer phase and source array ordering, including swap-removal and same-callback processing of appended movers. Their state may preserve only the most recent victim rather than a lifetime hit set. Delayed volleys retain captured damage and heading, sample the living caster position at each callback and stop new spawns after source death; released movers remain independent. Owner building scaling consumes completed lifecycle buckets and preserves the precursor during an upgrade. Native order and secondary ability eligibility are distinct: an immune collision can consume a scripted missile while rejecting its secondary spell. A requested ability level above the resolved object's level count cannot create additional levels. Integer millidegree sampling/CORDIC motion, stable-identity group selection and fixed-step sub-tick expiry are explicit deterministic interpretations.

Native damage responses inspect pre-commit life and the attacker's intrinsic classification independently of the event attack type or selected weapon delivery. A fatal incoming hit can therefore release an independent dummy missile. Script RNG precedes the secondary native order's target/resource checks; fresh dummy casts do not share a cooldown across events. Simultaneous native fork branches preserve one primary recipient, independently filter secondary recipients in the source-oriented cone and retain unchanged per-target damage rather than adopting a bounce clock or falloff.

Native source damage callbacks use the actual caster of each damage packet and its reduced event damage before the target callback and parent life commitment. A dummy caster remains distinct from the unit that requested its spell, including recursion, retaliation and kill ownership. Released damage retains its native caster identity through flight and periodic pulses. If the source dies while its original handle survives, the final installed source grants and native weapon classes remain authoritative; corpse expiry, consumption or replacement closes that old handle. This retained state participates in canonical checksums, snapshots and round reset. Installing a self aura refreshes its eligible membership during the grant's commit step, before later intents in that tick.

Native point weather separates the parent's order target, script-selected point and native effect recipients. A released dummy channel owns its fixed point, native child profile, original pulse deadlines and expiry independently of the logical building. Each pulse samples live target classes, defenses and positions. Per-wave aggregate caps distribute proportionally before recipient defenses; overlapping Blizzard caster-buff families select the strongest eligible channel using a phase snapshot, including completing channels. Native dummy damage does not inherit the logical building's damage callbacks. Delayed impacts and cosmetic wave progress are deterministic, canonical and restorable; presentation never determines damage timing.

Weather recipient burns use their own buff family, independently of the channel wave family. Positive wave damage can apply a native burn with an immediate impact pulse, followed by one-second deadlines. Refreshes extend the live duration and retain the committed pulse clock; the strongest live payload wins within one recipient buff family. Normal and hero durations are independent; burn pulses stop before the expiry tick. Zero wave damage or zero duration prevents a burn. Dummy-owned burns survive logical source removal, honor live immunity without deferred damage, and can be cleansed.

A construction-granted building damage callback keeps its authoritative source identity, origin, native child profile and deterministic event sequence independently of active weapon state. A released weapon can invoke that callback after its tower dies. The callback rolls before validating the independent child order; each successful order uses a fresh dummy with its own native resources, and dummy damage cannot recursively invoke the logical building callback. The controller is released after its source and outstanding source weapons are gone.


A scripted Roots order separates the parent native target rules and resource commitment from each owner-owned dummy child. The child originates at the selected recipient, has its own native range, immunity and hero-duration rules, and applies an authoritative movement lock and periodic damage that survive the original caster. Parent commitment succeeds even when the child order fails. Bounded multi-target callbacks count selected recipients before checking child legality, with source-authored buff priorities, exclusions and temporary vision; these are versioned profiles rather than AI or presentation guesses.
