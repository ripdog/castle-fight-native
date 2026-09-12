# Individual Targeting and Combat Resolution

Status: **normative architecture, provisional gameplay details**

## 1. Purpose

This document defines autonomous per-entity target selection and deterministic combat resolution.

The central requirement is that every unit or attack-capable building makes an individual target decision. Shared spatial/navigation structures may accelerate those decisions, but MUST NOT replace them with squad/group targeting.

## 2. Target ownership

Each attack-capable authoritative entity has its own target state.

Conceptual form:

```rust
pub struct TargetState {
    pub current: Option<SimId>,
    pub acquired_tick: Option<Tick>,
}
```

Two adjacent identical units may select different targets if their deterministic candidate sets/scores differ.

No architectural assumption may require a formation, squad, lane segment, or worker partition to share one target.

## 3. Target lifecycle

Targeting is **engagement-sticky**, not a continuous closest-target search.

Each targeting phase conceptually performs:

1. resolve the existing `SimId` target;
2. drop it if it is dead, disappeared, invisible/untargetable, no longer attackable by any applicable attack, unreachable from any valid attack position, or beyond the pursuit leash;
3. if the current target is still valid, normally retain it even if a closer/better enemy has appeared;
4. exception: if the current target is **not** attacking this unit and another valid enemy actually attacked this unit during the preceding combat resolution, switch to that attacker;
5. if there is no retained target, prefer a recent valid attacker, otherwise query nearby candidates and acquire a fresh target;
6. store the selected target or `None`.

An enemy merely selecting, approaching, or standing near a unit does not trigger retaliation. The retaliation rule is caused by an actual resolved attack. In the deterministic phased implementation, an attack received on tick `N` can affect target selection on tick `N+1`.

A newly visible closer unit MUST NOT cause gratuitous retargeting while the existing engagement remains valid.

## 4. Candidate discovery

Target candidate discovery SHOULD use the dynamic spatial index rather than scanning all enemies.

A query may be based on acquisition range and may inspect one or more spatial cells.

Internal candidate enumeration order MUST NOT affect selection.

## 5. Target eligibility

Eligibility is separate from navigation reachability.

Depending on attack/content rules, eligibility may consider:

- hostile team/alliance relationship;
- target is alive/active;
- target class: ground, air, building, summoned, mechanical, etc.;
- acquisition range;
- line-of-sight if the game uses it;
- invulnerability/untargetable status;
- attack-specific target filters;
- special taunt/priority effects.

Navigation reachability and targetability remain separate concepts, but an attacker MUST discard a candidate it cannot actually reach an attack position for with any attack that can affect that candidate.

Examples:

- a long-range attacker already able to hit a caged unit does not need a ground route to that unit;
- a melee attacker outside a cage cannot select a unit inside when no reachable melee attack position exists;
- a unit whose only applicable attack cannot affect the candidate (for example because of target class) ignores it;
- if a reachable attack position exists around ordinary obstacles, pursuit/navigation may route toward that position.

Thus caged units remain valid targets for attackers that can genuinely hit them, while attackers unable to hit them ignore them rather than pushing forever against the cage.

The player builder is explicitly outside ordinary combat targetability. It MUST NOT enter standard combat candidate sets even when physically nearby, hostile by team ownership, or the carrier/origin of an offensive item effect.

## 6. Target ranking

The exact Castle Fight-compatible ranking needs empirical/gameplay specification work. The engine nevertheless requires a total deterministic ordering.

Ranking applies primarily when acquiring a **new** target; it does not continuously replace a valid current target.

For fresh acquisition, the standard rules prefer attackable enemy combat units over non-attacking buildings. Within an otherwise equivalent class, deterministic distance and then `SimId` may be used to resolve candidates that become visible/eligible together.

This is important around cages: if caged units are unreachable to a melee attacker, reachable enemy units outside the cage are preferred; if no attackable combat-unit target is available, the cage buildings themselves may become valid fallback targets. A unit already engaged with a valid cage building does not abandon it merely because another passive candidate is closer.

Forced-target/taunt rules, attack-capable building priorities, and other special threat classes may add explicit exceptions later.

Distance SHOULD be compared using deterministic squared fixed/integer distance where possible.

If two candidates are otherwise identical, the project explicitly accepts stable `SimId` as the final tie-break. No fairness-randomization layer is required.

## 7. Target retention

A valid engagement is retained until a defined break condition occurs or the retaliation exception applies.

A current target is dropped when, as applicable:

- it dies/despawns or otherwise disappears;
- it becomes invisible, invulnerable, untargetable, or otherwise invalid for every applicable attack;
- its target class is no longer attackable;
- no reachable attack position remains for any applicable attack;
- it exceeds the pursuit/chase leash;
- an explicit forced-target rule overrides it.

A short pursuit leash is part of standard behavior: units chase a retreating target for only a modest distance before giving up. The verification implementation initially uses a **3-tile extra pursuit allowance** beyond ordinary attack range, while never making retention shorter than the unit's normal acquisition range. The exact content-compatible value may be tuned later.

Retaliation has one important guard: if current target `B` is itself attacking `A`, then `A` stays engaged with `B` even if another unit `C` also attacks `A`. `C` only pre-empts `B` when `B` is not fighting back and `C` is a valid target that `A` is capable of attacking.

Builder-held item damage and other explicitly non-retaliatory effect sources MUST NOT populate this combat-attacker retaliation state.

## 8. Combat intent

A targeting decision alone does not mutate the victim.

Attack-capable entities in the combat-intent phase determine whether they can act based on:

- current target;
- attack range;
- cooldown;
- windup/backswing rules if modeled;
- status effects;
- attack profile;
- facing if gameplay-relevant.

They produce an explicit intent/effect request.

Conceptual form:

```rust
pub struct AttackIntent {
    pub source: SimId,
    pub target: SimId,
    pub attack: AttackId,
    pub sequence: u16,
}
```

## 9. Attack delivery modes

The authoritative content model MUST distinguish the gameplay semantics of at least four ordinary attack delivery modes. These are not merely presentation styles.

### 9.1 Melee

A melee attack resolves directly against its selected target when the attack's deterministic strike conditions are satisfied.

It has no gameplay projectile. Animation may show weapon travel, but presentation does not delay or redirect the authoritative hit unless the rules explicitly model a windup/strike tick.

### 9.2 Ranged guaranteed-hit

A ranged attack launches a missile toward a specific target and is guaranteed to hit that target once the attack has successfully launched, subject only to explicitly documented invalidation rules such as target removal/death if Castle Fight compatibility requires them.

The projectile's travel time is derived deterministically from attack/target distance and projectile speed/time rules. Its rendered position is interpolated over that travel interval.

This delivery retains the target entity identity and may visually follow/interpolate toward the target as it moves. It MUST NOT accidentally become missable merely because the target changed position after launch.

Because impact time can affect damage ordering, the authoritative simulation MUST retain enough projectile/impact state to apply the hit on the correct tick even if the visual projectile itself is presentation-derived.

### 9.3 Ranged ballistic projectile

This mode is typically used by siege/artillery attacks.

At launch, the attack captures a target position/impact zone. The projectile then follows a deterministic ballistic/arc presentation toward that fixed destination and does **not** follow the original target.

At impact, after movement for that simulation tick has resolved, the simulation refreshes the relevant spatial index and queries entities in the target/impact zone. The effect applies to the entities actually present at those post-movement positions. Therefore:

- the originally selected unit can move away and be missed;
- other units can move into the impact zone and be hit;
- area/splash rules are evaluated at impact time;
- the projectile destination must be authoritative fixed-point state.

The vertical arc may be presentation-only if only impact tick and 2D destination affect gameplay. If arc height itself can interact with gameplay, it becomes authoritative state.

### 9.4 Bounce

A bounce attack is guaranteed to hit its initial selected target, then performs one or more subsequent jumps to other eligible units.

Each bounce defines:

- maximum bounce count;
- candidate range from the previous impact/source;
- eligibility filters;
- whether already-hit entities may be selected again;
- damage/effect scaling per bounce;
- travel delay between bounces if any;
- deterministic-random next-target selection policy.

Random bounce selection MUST use keyed deterministic RNG and MUST be independent of candidate enumeration or worker order.

### 9.5 Authoritative vs presentation projectile state

A projectile/delivery needs authoritative state whenever its future impact time, target identity, destination, area query, bounce sequence, interception, or other behavior can change gameplay.

A purely visual missile for an already-resolved effect MAY remain presentation-only.

The engine SHOULD encode delivery semantics explicitly rather than infer them from missile art or animation assets.

## 10. Combat resolution ordering

Combat interactions need explicit semantics.

Two broad models are acceptable:

### 10.1 Simultaneous aggregate

Same-tick compatible effects are grouped by target and applied as a deterministic aggregate. This is efficient but only correct when ordering does not change secondary effects.

### 10.2 Canonically ordered events

Effects are sorted by a stable key and resolved sequentially.

Possible key:

```text
phase/subphase
impact tick
priority
source SimId
target SimId
intent sequence
```

The final model may mix both approaches by effect class.

Thread completion order MUST NEVER define combat order.

## 11. Damage and secondary effects

Damage resolution may involve:

- base damage;
- armor/resistance;
- damage type;
- critical/random modifiers;
- shields;
- lifesteal;
- reflected damage;
- on-hit effects;
- on-damage triggers;
- death triggers.

Each operation that can interact with another same-tick effect needs an explicit deterministic ordering/subphase.

The simulation SHOULD avoid a generic unconstrained callback graph where effect ordering emerges from registration order.

## 12. Damage source does not imply retaliation

Receiving damage or a hostile effect MUST NOT automatically create a player-like attack order against the source.

Ordinary units retain/reacquire targets only through their autonomous target rules or an explicit taunt/forced-target mechanic. This is required for builder-held offensive items: they may damage a unit while the non-combat builder remains invalid as a target and does not alter that unit's behavior merely by being the source/carrier.

## 13. Random combat effects

Randomness uses keyed deterministic random values as specified in `11-determinism.md`.

Examples:

```text
critical hit:
  key = (match seed, tick, attacker SimId, CriticalHit, attack sequence)

proc chance:
  key = (match seed, tick, source SimId, ProcId, local sequence)
```

Parallel execution order MUST NOT consume or shift another entity's random sequence.

## 14. Death and disable precedence

The initial rules are explicit:

1. **death wins over every later action** — once health reaches the death threshold, that entity may not resolve any later attack, cast, or movement in the tick;
2. **stun/disable wins over an ordinary attack** — if the disabling status becomes active before that attack resolves, the attack is canceled even if its intent was prepared earlier;
3. structural despawn still occurs at the normal commit point so references/effects can resolve deterministically;
4. already-launched persistent projectiles remain independent authoritative state and are not canceled merely because their source subsequently dies.

Same-phase interactions are processed in canonical event order. This means same-tick mutual actions are not implicitly simultaneous: a source killed earlier in canonical resolution cannot complete a later unresolved action.

## 15. Movement/combat interaction

Movement/combat interaction follows these initial rules:

- a unit spawned on the current tick cannot attack until a later tick;
- ordinary attack intent/resolution occurs before that tick's movement;
- an entity killed before movement does not move;
- a stunned/disabled entity does not resolve an ordinary attack after the disable becomes active;
- due ballistic/siege impacts resolve after movement and use post-movement positions;
- melee and other range-limited attackers pursue a reachable attack position for their selected target rather than steering blindly at the target through blockers;
- if no attack position is reachable, the target is invalid and is dropped/reacquired.

Windup/backswing, range hysteresis, and exact guaranteed-hit target-removal semantics remain compatibility details, but they must fit these precedence rules.

## 16. Attack buildings

Attack buildings participate in the same deterministic targeting/combat principles.

They:

- have independent target state;
- query spatial candidates according to their range;
- filter/score candidates individually;
- emit combat/projectile intents;
- do not need a navigation route to their target.

This is essential to cage-based attack soaking.

## 17. Area effects

Area/splash attacks SHOULD use the spatial index to obtain impacted candidates.

Impact candidate enumeration order MUST NOT change outcomes.

If the effect applies identical independent damage to all candidates, parallel per-target calculation may be possible. If targets interact through shared caps/chains/jumps, the effect requires explicit canonical sequencing.

## 18. Chain/jump and bounce attacks

Chain/jump/bounce attacks require deterministic next-target selection for every jump.

Each jump MUST define:

- origin point/entity;
- eligible candidate set;
- maximum jump range;
- already-hit policy;
- scoring or deterministic-random selection policy;
- damage modification per jump;
- travel delay if any;
- deterministic RNG purpose/key when randomness is intended.

For the ordinary `Bounce` delivery mode, the first target is guaranteed to be hit; subsequent targets are selected using the attack's bounce policy.

Spatial traversal order cannot act as the tie-break or alter the random candidate index.

## 19. Performance requirements

Targeting cost should scale approximately with local candidate density rather than total unit count under ordinary conditions.

Metrics SHOULD include:

- target queries/tick;
- mean and percentile candidate counts;
- retained-target percentage;
- full reacquisitions/tick;
- combat intents/tick;
- effects resolved/tick.

Pathological high-density piles must be benchmarked separately because any local broad phase can degrade when thousands of units occupy the same cells.

## 20. Required tests

The targeting/combat test suite MUST eventually include:

1. two equivalent nearby attackers can independently choose different targets when geometry/rules differ;
2. candidate insertion/grid traversal order does not alter target choice;
3. exact tie resolves by documented stable ID rule;
4. caged unit remains targetable by a ranged attacker that can actually hit it;
5. melee attacker rejects a caged unit with no reachable attack position and can instead acquire a reachable cage building;
6. reachable enemy combat unit outside the cage outranks a closer non-attacking cage building;
7. a valid current target is retained even when a closer non-attacking enemy appears;
8. when current target `B` is not attacking `A`, a valid unit `C` that actually attacks `A` causes `A` to switch to `C` on the next targeting phase;
9. when `B` is attacking `A`, another attacker `C` does not break that mutual engagement;
10. dead/invisible/untargetable/unreachable/out-of-pursuit target triggers deterministic reacquisition;
11. same battle yields identical checksum at different worker counts;
12. same-tick multi-attacker damage follows documented canonical/death precedence;
13. a stun applied before attack resolution cancels the affected source's pending attack;
14. a unit cannot attack on its spawn tick;
15. projectile impact timing is identical across runs;
16. deterministic random proc/crit values do not change with worker count;
17. attack building performs independent target selection;
18. melee attack has no authoritative projectile trajectory;
19. guaranteed-hit ranged attack still hits after target movement according to its documented lifetime rules;
20. ballistic ranged projectile captures a fixed destination, queries post-movement occupants, can miss the original moving target, and can hit another eligible unit in the impact zone;
21. bounce attack always hits the initial target and chooses identical subsequent random targets across worker counts;
22. pathological candidate density remains bounded enough for configured performance goals or triggers a known optimization path.
