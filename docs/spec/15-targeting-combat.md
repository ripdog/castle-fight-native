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
    pub direct_retaliation_lock: bool,
}
```

Two adjacent identical units may select different targets if their deterministic candidate sets/scores differ.

No architectural assumption may require a formation, squad, lane segment, or worker partition to share one target.

## 3. Target lifecycle

Targeting is **engagement-sticky**, not a continuous closest-target search.

Each targeting phase conceptually performs:

1. resolve the existing `SimId` target;
2. drop it if it is dead, disappeared, invisible/untargetable, no longer attackable by any applicable attack, unreachable from any valid attack position, or beyond the pursuit leash; clearing any direct-retaliation lock with it;
3. if the current target is valid and already carries a direct-retaliation lock, retain it unless an explicit forced-target rule overrides it;
4. otherwise, if a valid enemy actually attacked this unit during the preceding combat resolution, switch to the first such attacker in canonical combat-event order and set the direct-retaliation lock; if that attacker is already the current target, retain it and set the lock;
5. otherwise, if the valid current target is a combat unit, retain it without consulting ally-defense alerts;
6. otherwise, if the valid current target is a building, consider a valid nearby ally-defense attacker; switch to that attacker when one exists, otherwise retain the building;
7. if there is no retained target, prefer a recent valid self-attacker and set the direct-retaliation lock;
8. otherwise, while idle, consider a valid nearby ally-defense attacker; if none exists, query nearby candidates and acquire a fresh target;
9. store the selected target or `None` and its lock state.

An enemy merely selecting, approaching, or standing near a unit does not trigger retaliation or ally defense. Both rules are caused by an actual resolved attack. In the deterministic phased implementation, an attack resolved on tick `N` can affect target selection on tick `N+1`.

The initial ally-defense radius is the defending unit's normal acquisition range around the attacked ally's position. The attacker must itself remain a valid target that the defender can attack or pursue under ordinary reachability/pursuit rules. If several nearby allies were attacked, the initial deterministic ordering is nearest attacked ally, then nearest valid attacker, then stable IDs.

The verification implementation accelerates this exact ordering with derived one-tick indexes: resolved alerts are grouped by attacked victim, attacked victims are queried in nearest-distance layers, and each victim's actual attacker relation has its own spatial partition for nearest-valid-attacker lookup. These indexes are acceleration structures only. They MUST NOT discard a valid alert, change the nearest-ally/nearest-attacker ordering, or make result ordering depend on hash/grid traversal.

A one-tick canonical defense alert survives the victim dying from the triggering attack, so nearby allies may still react to the killer on the following targeting phase.

A newly visible closer unit MUST NOT cause gratuitous retargeting while an existing combat-unit engagement remains valid. Ally defense likewise MUST NOT pre-empt a valid current **unit** target. A current **building** target is the deliberate exception: an actual attack on a nearby allied unit may cause the attacker to abandon the building and engage the valid enemy that attacked that ally. Mere enemy proximity or arrival does not trigger this exception; it still requires a resolved attack/defense alert.

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

The current authoritative ordinary-attack filter is an explicit target mask with independent ground-unit, air-unit, and building bits. Movement class does not imply attack capability: a ground melee unit may be authored to hit air, an air unit may be ground-only, and an attack building uses the same mask. Target-class filtering applies to fresh acquisition, sticky retention, retaliation/ally-defense validation, the final attack-intent gate, ballistic splash victims, and bounce-chain candidates; a secondary projectile effect MUST NOT bypass the originating attack's target mask.

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

A valid combat-unit engagement is retained until a defined break condition occurs, a first direct attacker establishes a direct-retaliation target, or an explicit forced-target rule applies. Ally defense never pre-empts a valid current unit target. Building targets are objective/fallback targets rather than sticky combat engagements and may be pre-empted by ally defense after an actual nearby allied unit is attacked.

A current target is dropped when, as applicable:

- it dies/despawns or otherwise disappears;
- it becomes invisible, invulnerable, untargetable, or otherwise invalid for every applicable attack;
- its target class is no longer attackable;
- no reachable attack position remains for any applicable attack;
- it exceeds the pursuit/chase leash;
- an explicit forced-target rule overrides it.

A short pursuit leash is part of standard behavior: units chase a retreating target for only a modest distance before giving up. The verification implementation initially uses a **3-tile extra pursuit allowance** beyond ordinary attack range, while never making retention shorter than the unit's normal acquisition range. The exact content-compatible value may be tuned later.

Direct retaliation has a one-time lock rule. If unit `A` has no direct-retaliation lock and valid enemy `C` is the first hostile entity to actually attack `A` in canonical combat-event order, `A` switches to `C` on the next targeting phase and marks `C` as its direct-retaliation target. A direct attacker is considered retaliation-valid while it remains hostile/alive and within **3× `A`'s ordinary target acquisition range**, even when it is outside ordinary acquisition range or has no currently reachable attack position. This lets an attacked unit commit to fighting back and attempt best-effort pursuit rather than silently ignoring a nearby attacker because of a cage or other disconnected topology. While `C` remains inside this retaliation leash, later attackers do not replace it. The lock is cleared when `C` dies/disappears, leaves the retaliation leash, or an explicit forced-target rule overrides it.

If the current target is already the first direct attacker, the same target is retained and becomes locked. If several enemies first attack `A` during the same tick, canonical combat resolution order defines which attack is first; worker completion or spatial enumeration order MUST NOT participate. The 3× retaliation leash applies to both unit and attack-building sources, using point distance for unit attackers and point-to-footprint distance for building attackers.

Ally-defense alerts are considered while `A` has no valid current target **or** while its valid current target is a building. Once ally defense or ordinary acquisition chooses a valid enemy combat unit, attacks on other nearby allies cannot make `A` revolve between those attackers. If `A` is attacking a building and a nearby ally is actually attacked, `A` may instead engage the valid attacker; this allows groups pounding a castle or other structure to peel off and fight arriving defenders.

Builder-held item damage and other explicitly non-retaliatory effect sources MUST NOT populate self-retaliation or nearby-ally defense alerts.

### 7.1 Attack-building source behavior

Attack-capable buildings are authoritative combat entities with independent `AttackProfile`, cooldown, target state, and spawn tick. They participate in the same canonical ordinary-attack stream as units; there is no separate scheduler-dependent "tower phase". A building cannot perform an ordinary attack on its spawn tick, death before its canonical intent resolves cancels that intent, and an already-launched persistent projectile survives later source death exactly as for a unit source.

The current verification acquisition rule for an attack building is provisional but explicit:

- navigation reachability is irrelevant because the source does not move;
- range/acquisition distance is measured from the source building's authoritative footprint, using point-to-footprint distance for unit targets and footprint-to-footprint distance for building targets;
- living hostile combat units are preferred over hostile buildings for fresh acquisition;
- within the same class, lower squared distance then lower stable `SimId` wins;
- a valid target is retained while it remains hostile/alive and inside the building's acquisition range;
- attack buildings do not currently run unit-style self-retaliation or nearby-ally-defense retargeting for their own target state.

Candidate discovery MAY use an expanded center-radius broad-phase query, but exact footprint distance determines eligibility. Changing spatial-cell size or bucket iteration MUST NOT change the selected target. A ranged attack building may therefore attack a caged unit with no ground route when the ordinary attack rules otherwise allow the target.

For projectile delivery, the current verification launch position is the building footprint center while attack-range/travel distance uses the same exact footprint-distance rule used by the intent. This is provisional presentation/compatibility behavior and MUST remain explicit if revised.

Damage caused by an attack building is otherwise ordinary combat damage. Its building `SimId` is the damage source, so a struck unit may establish that building as its first direct-retaliation target only when that unit's own attack target mask permits buildings. The unit's 3× acquisition-range retaliation leash applies even if the building lies outside ordinary acquisition range; nearby allies may separately consume the corresponding defense alert when the attacker is valid/reachable under their own rules.

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

A ranged attack launches a missile toward a specific target and, once any applicable attack-accuracy gate has succeeded, is guaranteed to hit that target while that target remains a live authoritative entity through the impact subphase. Here, "guaranteed-hit" means target movement after launch cannot dodge the projectile; it does **not** bypass global attack-accuracy rules such as uphill miss. The current verification rule invalidates the impact if the retained target identity has already died or been removed before the due impact resolves; the missile does not retarget another entity.

The verification travel-time rule is deterministic and integer-only. At launch, use the same authoritative source-to-target distance used by the attack-range calculation, round Euclidean subunit distance upward to the next integer subunit, divide that distance upward by the authored positive `speed_per_tick`, and clamp the result to at least one tick. The resulting due impact tick is `launch_tick + travel_ticks`. This exact rule is provisional compatibility behavior, but any replacement MUST remain explicit and deterministic.

The projectile retains the target `SimId`, source `SimId`, damage, launch position, launch tick, and impact tick as authoritative state. Target movement after launch does not make the projectile miss or change its due tick. Presentation may visually follow/interpolate toward the target's current render position during that interval.

A successfully launched projectile is independent of its source. Source death after launch MUST NOT cancel the projectile. Because impact time can affect damage/death ordering, live guaranteed-hit projectiles participate in canonical state/checksums until they impact or invalidate.

### 9.3 Ranged ballistic projectile

This mode is typically used by siege/artillery attacks.

The verification implementation authors ballistic delivery with positive integer `speed_per_tick` and a non-negative integer circular `impact_radius`. At launch, ordinary pre-movement combat captures the selected target's current authoritative 2D position as a fixed destination. Travel ticks use the same upward-rounded integer distance/speed rule as `RangedGuaranteedHit`, and the projectile stores source identity/team, damage, launch position, fixed destination, impact radius, launch tick, and due impact tick as canonical state. Once launched it is independent of source death.

The projectile then follows presentation toward that fixed destination and does **not** follow the original target. Its vertical arc is presentation-only in this slice because only the 2D destination and due impact tick affect gameplay.

When the projectile becomes due, its impact resolves in a dedicated subphase **after movement** for that simulation tick. The simulation builds a fresh spatial index from living units' post-movement positions and evaluates the authored circular zone. The current provisional splash rule damages every living hostile unit whose center lies within the radius and every living hostile building whose footprint intersects the radius; there is no friendly fire in this verification rule. A landing counts as a projectile impact even when the zone is empty, while each successfully damaged entity is counted as a separate projectile effect.

Due ballistic projectiles resolve in ascending projectile `SimId`; targets inside each impact resolve in ascending target `SimId`. Spatial enumeration order and worker completion order MUST NOT affect effect order. Therefore:

- the originally selected unit can move away and be missed;
- another unit can move into the captured zone and be hit;
- area/splash membership is evaluated from post-movement state at impact time;
- an empty destination still consumes/resolves the projectile without retargeting;
- the projectile destination and impact timing remain authoritative and participate in canonical checksums.

The exact Castle Fight-compatible zone shape, friendly-fire policy, building interaction, splash falloff, and presentation arc remain compatibility-tunable content/rule details, but replacements MUST preserve explicit deterministic impact-time semantics.

### 9.4 Bounce

A bounce attack is guaranteed to hit its initial selected target, then may perform a bounded number of subsequent guaranteed-hit jumps. The current verification delivery authors positive integer `speed_per_tick`, non-negative `bounce_range`, `max_bounces`, integer `damage_percent_per_bounce` in `1..=100`, and `allow_repeat_targets`. `max_bounces` counts **additional** jumps after the initial selected target; the verifier currently caps it at eight so authoritative hit history fits fixed-size projectile state.

Launch creates one authoritative persistent bounce projectile with a monotonic `SimId`. That same projectile entity/identity survives across every hop; it stores source identity/team, current target, current damage, launch position/tick, due impact tick, travel speed, range/rules, remaining/indexed bounce count, and bounded hit history. Source death after launch does not cancel the chain. If the current retained hop target has died/disappeared before its due impact, that hop invalidates and the chain ends without retargeting.

A successful impact applies the current integer damage before selecting a later hop. If another hop is allowed, the current provisional candidate set contains living hostile **units** within `bounce_range` of the just-hit target's authoritative impact position. The current target is always excluded. Earlier hit targets are also excluded when `allow_repeat_targets == false`; buildings are not subsequent-hop candidates in this verification rule, although an ordinary initial attack may have selected a building. Bounce impacts occur in the same pre-movement persistent-projectile subphase as guaranteed-hit impacts, so candidate positions are the authoritative pre-movement positions for that tick.

Next-target randomness is keyed and enumeration-order independent. Every valid candidate receives a deterministic random rank derived from:

```text
match seed
+ current impact tick
+ persistent projectile SimId
+ stable BounceTarget purpose discriminator combined with candidate SimId
+ next bounce index
```

The candidate with minimum `(random rank, SimId)` wins. The current verifier uses a specified SplitMix64-based keyed mixer; the algorithm and purpose discriminator are simulation-version behavior and MUST NOT change silently. Spatial-grid traversal order, hash-table iteration, and worker completion order therefore cannot choose the next hop.

After choosing the next target, damage is scaled with exact integer floor arithmetic:

```text
next_damage = current_damage * damage_percent_per_bounce / 100
```

and travel time uses the same upward-rounded integer distance/speed rule as the other persistent projectile deliveries, minimum one tick. There is no extra implicit hop delay beyond that authored travel time.

Guaranteed-hit and bounce projectiles that become due in the same pre-movement subphase resolve together in ascending projectile `SimId`; ballistic impacts remain a later post-movement subphase by definition.

The exact Castle Fight-compatible bounce range, repeat policy, scaling, building eligibility, maximum chain length, and random-selection distribution remain compatibility-tunable. Any replacement MUST remain explicitly bounded, keyed-deterministic, and independent of candidate enumeration/worker order.

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

## 11. Damage, attack types, and armor

Every ordinary damaging attack has an authoritative Warcraft/Castle Fight attack/damage class and every damageable unit or building has an authoritative defense/armor class plus numeric armor. These values are content, not presentation metadata, and they remain attached to persistent projectiles until impact so ranged, ballistic, splash, and bounce resolution cannot lose the source attack type.

Castle Fight's exact attack-type-versus-defense-type table is extracted from the map's `war3mapMisc.txt` gameplay constants. The native armor order is Warcraft's eight-column order: Small/Light, Medium, Large/Heavy, Fortified, Normal, Hero, Divine, Unarmored. The committed Castle Fight table is:

| Attack type | Light | Medium | Heavy | Fortified | Normal | Hero | Divine | Unarmored |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Normal | 70% | 175% | 100% | 50% | 100% | 60% | 25% | 105% |
| Pierce | 175% | 100% | 70% | 45% | 100% | 60% | 25% | 105% |
| Siege | 70% | 70% | 70% | 160% | 80% | 40% | 20% | 100% |
| Magic | 100% | 70% | 175% | 40% | 100% | 60% | 25% | 105% |
| Hero | 110% | 110% | 110% | 60% | 110% | 60% | 40% | 110% |
| Spells | 100% | 100% | 100% | 100% | 100% | 70% | 25% | 100% |
| Chaos | 100% | 100% | 100% | 100% | 100% | 100% | 100% | 100% |

The first six rows above are direct map overrides. The map does not override Chaos, so Chaos uses Warcraft III's 100%-against-all baseline. `war3mapSkin.txt` contains human-facing tooltip text for the same table, but where tooltip text and gameplay constants disagree the gameplay constants are authoritative; notably the extracted Pierce-versus-Fortified gameplay value is 45%.

Ordinary attack damage then applies numeric armor. Castle Fight does not override Warcraft's armor coefficient, so the native compatibility rule uses the Warcraft III factor `0.06` per armor point. Positive armor therefore multiplies post-type damage by `1 / (1 + 0.06 * armor)`. Negative armor uses Warcraft's `2 - 0.94^(-armor)` amplification rule. The deterministic implementation evaluates these with integer/fixed-point arithmetic and one defined rounding step rather than authoritative floating-point math. Current imported base armor values are integral; exact compatibility for future fractional armor modifiers remains a separate fixed-point-content question.

Automatic ability `Damage` and `AreaDamage` are currently ordinary Warcraft-style spell damage: they use Castle Fight's `DamageBonusSpells` defense-type row but do **not** use the target's numeric armor rating. A future effect that represents triggered physical damage, universal damage, or another Warcraft damage-type category MUST declare that semantic explicitly rather than reusing the spell default accidentally.

Damage resolution may additionally involve critical/random modifiers, shields, spell resistance/immunity, lifesteal, reflected damage, on-hit/on-damage effects, and death triggers. Each operation that can interact with another same-tick effect needs an explicit deterministic ordering/subphase. The simulation SHOULD avoid a generic unconstrained callback graph where effect ordering emerges from registration order.

## 12. Damage source does not imply retaliation

Receiving damage from a **non-attack effect** or a hostile effect MUST NOT automatically create a player-like attack order against the source.

Ordinary units retain/reacquire targets through their autonomous target rules. An ordinary attack is the explicit exception: a resolved ordinary attack records a one-tick retaliation source, and the next targeting phase may establish the source as the unit's direct-retaliation target under the 3× acquisition-range rule in §7. Builder-held offensive items and other explicitly non-retaliatory effects may damage a unit while their non-combat source remains invalid as a target and does not alter that unit's behavior merely by being the source/carrier.

## 13. Random combat effects

Randomness uses keyed deterministic random values as specified in `11-determinism.md`.

Examples:

```text
critical hit:
  key = (match seed, tick, attacker SimId, CriticalHit, attack sequence)

uphill miss:
  key = (match seed, tick, attacker SimId, UphillMiss, attack sequence)

proc chance:
  key = (match seed, tick, source SimId, ProcId, local sequence)
```

Parallel execution order MUST NOT consume or shift another entity's random sequence.

### 13.1 Uphill miss

Castle Fight inherits a terrain-height combat advantage: when a **ground** unit attacks an enemy unit that is on higher authoritative terrain, the attack has a chance to miss. This rule applies to every ordinary ground-unit attack attempt regardless of its delivery mode. Air units are not grounded on the sampled cliff surface and therefore do not use this elevation disadvantage. It does not automatically apply to spells/abilities or attacks made by buildings unless their own rules explicitly opt into the same accuracy mechanic.

"Uphill" MUST be determined from authoritative map/gameplay elevation data at the units' authoritative ground positions, not from rendered mesh height, camera-space coordinates, animation offsets, projectile arcs, or floating-point presentation transforms. In Castle Fight's standard map layout, each team's base is elevated relative to the central lane; this therefore provides defenders in the base with the intended terrain advantage without any special-case "base defense" modifier.

The uphill miss check is a deterministic accuracy gate on the attack attempt. A miss consumes the attack attempt/cooldown but produces no ordinary attack damage or on-hit effect from that attack. Delivery-specific guarantees apply only after this accuracy gate succeeds: for example, `RangedGuaranteedHit` still cannot be dodged by later target movement, but it may fail its uphill accuracy roll before that guarantee applies. If a chained/bounce attack is authored as one initial ordinary attack followed by secondary guaranteed effects, only the portions explicitly classified as attack attempts perform their own uphill checks; compatibility extraction must settle the original-map behavior rather than inferring it from visuals.

The native implementation samples authoritative combat elevation at ordinary attack resolution, before that tick's movement. The committed Warcraft terrain importer reads the W3E-derived `terrain.json` vertex grid and uses `layerHeight` as the discrete combat cliff level; `groundHeight` is retained as source data but smooth decorative height variation does not by itself create an uphill miss. A target unit is uphill exactly when its sampled cliff level is greater than the attacking unit's sampled cliff level. The terrain JSON is stored north-to-south by the extraction translator, so the loader reverses world-Y lookup accordingly and uses Warcraft's 128-world-unit terrain spacing.

The miss probability is an explicit deterministic rule parameter expressed in parts per 10,000 (`0..=10_000`); no compatibility value is hardcoded. A non-zero chance requires authoritative terrain data covering the full simulation navigation bounds. The roll uses keyed deterministic randomness with a stable `UphillMiss` purpose and the unit's canonical attack-attempt sequence. That sequence increments on every ordinary unit attack attempt, including a miss, so worker scheduling, ECS iteration order, or unrelated random effects cannot shift later rolls. The exact Castle Fight/Warcraft miss probability remains compatibility data to recover; changing it requires only content/rule configuration, not combat-code changes.

## 14. Death and disable precedence

The initial rules are explicit:

1. **death wins over every later action** — once health reaches the death threshold, that entity may not resolve any later attack, cast, or movement in the tick;
2. **stun/disable wins over ordinary autonomous actions** — if the disabling status becomes active in the pre-combat ability phase, the affected entity performs no fresh targeting/retaliation switch, ordinary attack, or intentional movement while the stun is active; a still-valid pre-stun target may remain retained;
3. structural despawn still occurs at the normal commit point so references/effects can resolve deterministically;
4. already-launched persistent projectiles remain independent authoritative state and are not canceled merely because their source subsequently dies.

Same-phase interactions are processed in canonical event order. This means same-tick mutual actions are not implicitly simultaneous: a source killed earlier in canonical resolution cannot complete a later unresolved action.

## 15. Movement/combat interaction

Movement/combat interaction follows these initial rules:

- a unit spawned on the current tick cannot attack until a later tick;
- ordinary attack intent/resolution occurs before that tick's movement;
- an entity killed before movement does not move;
- a stunned/disabled entity does not perform fresh target acquisition, resolve an ordinary attack, or intentionally move while the disable is active; timed stun uses the exclusive absolute-expiry rule defined in `16-abilities-spellcasting.md`;
- due ballistic/siege impacts resolve after movement and use post-movement positions;
- melee and other range-limited attackers pursue a reachable point in the selected target's legal attack envelope rather than steering blindly at one target-center/perimeter point through blockers;
- maximum range is a legality boundary, not a preferred standoff distance: an attacker already at any closer legal range remains there and may attack without backing away; a future positive minimum-range rule would define the envelope's inner boundary;
- local congestion may move a pursuer tangentially around occupied parts of the envelope so multiple attackers can naturally surround a unit/building without assigned formation slots;
- if no attack position is reachable, the target is invalid and is dropped/reacquired.

Windup/backswing and range hysteresis remain compatibility details. Guaranteed-hit target-removal behavior and the initial integer travel-time rule are defined provisionally in §9.2 and may be revised only as an explicit simulation/gameplay rule change.

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
9. attacking a passive castle/building does not prevent `A` switching to a valid unit that attacks `A`;
10. an idle unit or a unit fighting a target that is not fighting it back switches to defend a nearby ally from a valid attacker on the next targeting phase;
11. a mutual engagement is not broken merely because a nearby ally is attacked;
12. ally-defense alerts still exist for one targeting phase when the triggering attack killed the ally;
13. when several nearby allies are attacked simultaneously, the documented deterministic alert ordering produces the same target across runs/worker counts;
14. when `B` is attacking `A`, another attacker `C` does not break that mutual engagement;
15. dead/invisible/untargetable/unreachable/out-of-pursuit target triggers deterministic reacquisition;
16. same battle yields identical checksum at different worker counts;
17. same-tick multi-attacker damage follows documented canonical/death precedence;
18. a stun applied before attack resolution cancels the affected source's pending attack;
19. a unit cannot attack on its spawn tick;
20. projectile impact timing is identical across runs;
21. deterministic random proc/crit values do not change with worker count;
22. uphill eligibility is derived from authoritative terrain elevation rather than presentation height;
23. equivalent uphill attack sequences produce identical hit/miss results across runs and worker counts;
24. downhill/level-ground attacks are not subjected to the uphill miss gate;
25. attack building performs independent target selection;
26. melee attack has no authoritative projectile trajectory;
27. guaranteed-hit ranged attack still hits after target movement according to its documented lifetime rules once any applicable accuracy gate succeeds;
28. ballistic ranged projectile captures a fixed destination, queries post-movement occupants, can miss the original moving target, and can hit another eligible unit in the impact zone;
29. bounce attack chooses identical subsequent random targets across worker counts, with any uphill interaction following its explicit authored/compatibility semantics;
30. pathological candidate density remains bounded enough for configured performance goals or triggers a known optimization path.
