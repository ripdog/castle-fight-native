# Match Gameplay Model

Status: **provisional gameplay specification**

## 1. Purpose

This document defines the match-level rules that sit above combat/navigation and below faction-specific content: players and teams, resources, building lifecycle, autonomous production, ownership, castle/objective state, and victory.

Exact Castle Fight balance values are intentionally not specified here. They belong in content data. This document defines semantics that must remain deterministic regardless of values.

## 2. Match participants

A match contains one or more players assigned to teams/sides.

Canonical player state includes at least:

- stable `PlayerId`;
- team/side;
- connection/control status where gameplay-relevant;
- resources;
- faction/race selection;
- upgrade state;
- game-mode-specific state.

Network connection state itself is operational, but any gameplay consequence of absence must be represented explicitly in canonical state/rules.

### 2.1 Player builder

Each active player has exactly one directly controlled builder under the standard rules.

The builder is not a combat unit. It exists to move for player interaction, summon buildings inside the owning team's build region, repair friendly buildings, and carry/use items. It is non-targetable by ordinary combat, invulnerable to ordinary battle damage, and non-blocking to combat-unit movement. It remains at terrain/ground height, can move freely through units and buildings inside the owning build region, and cannot leave that region.

Ordinary combat units are never player-commandable. Their movement, target acquisition, attacks, and autonomous abilities remain simulation-driven.

See `42-builder-items.md` for the detailed builder/item contract.

## 3. Disconnect semantics

A single player disconnecting does not otherwise alter that player's game state. Their buildings, combat units, builder movement already in progress, passive/automatic items, resources, production, and spellcasting continue normally on the authoritative server.

While that player is disconnected, **any still-connected teammate may control the disconnected player's builder** and issue the same builder/build/item commands that builder could normally receive. Delegated control does not transfer ownership: the builder, inventory, buildings, and player-slot resources remain owned by the disconnected player. Concurrent teammate commands are resolved by the ordinary canonical command order.

When the original player reconnects, normal control returns to that player and teammate delegation ends.

Connection/disconnection changes have gameplay consequences and therefore MUST enter the replayable canonical server-event stream rather than existing only as transport state.

If every player on one team is disconnected, the entire match pauses at a completed tick boundary and a configured real-time reconnect timeout begins. Simulation ticks do not advance while paused. If any player on that team reconnects before timeout, the server records a canonical resume event and play continues from the same simulation boundary. If the timeout expires, the server records a canonical timeout/end event and the match ends under the game mode's abandonment/forfeit result.

Replay playback applies the recorded pause/resume/end events immediately at their stream positions; it need not reproduce the real-world waiting time.

## 4. Teams and hostility

Target eligibility depends on canonical team/alliance relationships rather than client-side presentation labels.

The common two-side game is a special case of a more general relationship table if future modes need free-for-all or temporary alliances.

Any alliance change during a match is an authoritative command/rule event and must have deterministic timing.

## 5. Player resources

Resources are canonical integer/fixed-point values. For Castle Fight 9.27, a normal player starts with **250 gold**, **125 lumber**, and a legendary-building allowance of **1** (`0 / 1` used/cap at match start). The legendary allowance maps to Warcraft III's food-used/food-cap state in the original map. Cheese increases that cap by one in the original game; Cheese purchasing is intentionally deferred until the Main Castle/shop mechanics are implemented.

Resource mutations occur through explicit deterministic operations such as:

- periodic income;
- building purchase;
- the lumber award from completing a zero-lumber-cost building;
- building sale/refund if the selected Castle Fight mode supports one;
- bounty/reward;
- upgrade purchase;
- game-mode grants.

A normal zero-lumber-cost building awards lumber equal to its gold cost when it finishes. A Siege building instead awards **75%** of its gold cost. Buildings that themselves cost lumber do not grant this construction lumber. These rules are extracted from the original map's resource help/runtime logic rather than inferred from object costs.

Resource arithmetic MUST define overflow/clamp behavior and MUST NOT use presentation-side balances as authority. Failed or rejected building placement MUST NOT spend resources or grant lumber. An accepted builder order commits its gold/lumber cost before the worker reaches the site. Once construction starts, the footprint is authoritative but the construction-lumber award and building income contribution remain inactive. Cancelling before completion uses the map's construction refund rate and grants no completion reward. Completion atomically grants construction lumber, activates income, and activates the building's functional gameplay components.

## 6. Periodic income

Castle Fight 9.27 pays income every **10 seconds**, which is **300 simulation ticks at 30 Hz**. Every active player starts with **5 gold of base income per payout**: the original runtime initializes the player's raw income accumulator to `0.5`, then multiplies the accumulator by `10` before applying tax. The first payout occurs after the first complete 10-second interval. The displayed progress/countdown is presentation of that simulation-tick phase; pausing the simulation therefore also pauses income progress.

Each live finished building adds a map-defined contribution on top of that base income. The common extracted factors are 0.20 for normal production, 0.18 for Siege, 0.09 for utility, and 0.04 for towers. The map multiplies the summed contribution by ten at payout time, so these correspond to 2%, 1.8%, 0.9%, and 0.4% of gold cost per payout. Upgrade-line buildings include the contribution inherited from their precursor chain rather than discarding prior income.

Normal income then uses the original progressive tax: **25-gold brackets**, starting at 100%, then 90%, 80%, and so on for up to eight full brackets; income above those brackets remains at the final 20% rate. The deterministic native simulation performs the equivalent calculation in fixed point and truncates only the final payout, matching the original map's `real_toInt` behavior. Treasure Box and team-domination income multipliers belong on top of the same raw-income pipeline when those mechanics are implemented.

Income MUST read canonical live building state from the payout phase. Buildings destroyed earlier on the payout tick do not contribute. Player build commands are resolved before that tick's income event, so income granted on a tick cannot retroactively fund a purchase already rejected that same tick; conversely, a building successfully completed before the payout phase contributes immediately.

## 7. Building lifecycle

A building instance has an explicit deterministic lifecycle.

Possible states include:

```text
pending builder command (cost committed, no site yet)
constructing (authoritative occupied footprint, functionality inactive)
constructed/active
disabled/stunned (if supported)
dying/destroy-pending
removed
```

Castle Fight construction duration is authoritative versioned content. The native 9.27 profile reads the resolved object `Build Time`; the currently exposed production buildings are 2 seconds / 60 simulation ticks, while Watch Tower and Poof Tower are 20 seconds / 600 ticks in the extracted object data.

A constructing building begins blocking navigation and building placement as soon as its site is created. It MUST NOT produce units, attack, cast automatic spells, contribute income, or grant its completion lumber before the completion tick. Production and combat timers begin from completion rather than construction start. The owning player may cancel before completion; 9.27's `ConstructionRefundRate=1.0` fully refunds the committed gold/lumber cost, and cancellation does not grant construction lumber.

## 8. Placement and ownership

Building placement MUST be inside the owning team's canonical build region. In the standard map this corresponds to that team's third of the battlefield.

Construction start creates an authoritative building site with:

- deterministic `SimId`;
- owner `PlayerId`;
- team;
- building content ID;
- canonical grid position/rotation;
- footprint/occupancy;
- construction start/completion ticks;
- initial health/state.

Production/attack timers and mana/ability state are attached only when construction finishes.

Placement cost is charged according to one explicit atomic rule. Recommended: server validation verifies resources and occupancy against the command's canonical execution state, then construction and cost deduction commit together.

There MUST NOT be a state where another command can spend the same resource balance because validation/commit ordering was ambiguous.

## 9. Building destruction and sale

Sale and combat destruction are distinct reasons even if both remove the building.

Rules must define:

- refund amount/timing;
- whether a building remains blocking until the structural commit phase;
- what happens to an in-progress production timer;
- what happens to authoritative projectiles already fired;
- when navigation topology is invalidated;
- whether death/sale effects trigger.

Removing a cage wall may reconnect previously trapped navigation cells. The navigation field update must occur at a deterministic phase boundary.

## 10. Production buildings

Each production building runs an independent deterministic production state machine.

Conceptually:

```rust
pub struct ProductionState {
    pub next_spawn_tick: Tick,
    pub sequence: u64,
}
```

Static produced-unit rules live in content definitions.

Production MUST NOT depend on render time or whether the owner is connected.

## 11. Spawn timing

A production building's spawn cadence is defined in ticks.

When the spawn tick arrives, the building emits a deterministic spawn operation.

If multiple buildings spawn on the same tick, their semantic outcome MUST not depend on scheduler completion order. Stable building `SimId` is the default tie-break where placement interactions require order.

If independent spawns can be resolved simultaneously without interaction, no unnecessary global sequential ordering is required.

## 12. Spawn location

Production definitions specify a canonical preferred spawn point/offset relative to the building footprint.

On each production attempt, the simulation searches for empty placement using a deterministic expanding spiral beginning at that point. The first candidate at which the produced unit's footprint fits valid traversable space without overlapping a blocking building or combat unit is used.

The spiral has a deterministic finite search limit. If no valid position is found before that limit, the unit is not created. The failed attempt does not create a production backlog; the building proceeds to its next ordinary production cycle.

The spawn search has no special knowledge of caging or connected navigation components. It neither tries to keep a spawn inside a cage nor tries to escape one. A valid position encountered by the bounded spiral is accepted regardless of which connected component it belongs to.

## 13. Production selection randomness

If a building can produce one of several units randomly, selection uses the authoritative keyed RNG.

A suitable key includes:

```text
match seed
building SimId
production sequence
RandomPurpose::ProductionSelection
```

Using production sequence rather than worker/tick call order ensures that unrelated parallel work cannot change the produced unit.

## 14. Autonomous unit objective

A spawned unit's strategic default objective is determined by team/map/game mode, typically the opposing castle/objective.

The unit does not receive an individually player-authored movement order.

Its behavior is composed from:

- strategic navigation toward objective when reachable;
- individual target retention/acquisition;
- movement toward attack position/current target where appropriate;
- combat;
- deterministic no-route behavior when caged/disconnected.

For the standard Castle Fight lane profile, targetless units do **not** steer toward the castle centerline. Units spawned north or south of the lane first take a roughly 45-degree forward-and-inward route toward the nearest legal lane edge, rather than targeting a fixed x-coordinate entrance at the inner base wall. In clear space this gets them onto the path after moving forward only about as far as they were vertically displaced from it, matching the intended early defensive convergence near their own castle. Once the unit's collision footprint fits inside the lane corridor, its strategic preference is horizontal progress toward the enemy side at its **current authoritative `y`**. It does not continue toward the lane center. This behavior is stateless: no spawn-lane/home-line value is stored on the unit. A unit already lane-aligned behind its own castle therefore routes around the castle and then marches horizontally from whichever side of the castle it emerged on. Combat, crowd flow, or later topology detours may likewise change the unit's `y`; while it remains inside the lane corridor, targetless movement simply resumes horizontally from that new line. A valid combat target overrides this rule with ordinary attack-envelope pursuit.

## 15. Objective/castle entities

Castles/objectives are authoritative entities or canonical match structures with:

- stable identity;
- team/ownership;
- position/footprint;
- health/defense state where applicable;
- targetability rules;
- victory relevance.

If the castle participates in ordinary combat targeting, its target categories/rules are defined in content like other buildings.

## 16. Victory

Victory evaluation occurs at an explicit phase after relevant combat/death resolution.

The rules MUST define outcomes when multiple victory conditions become true on the same tick, e.g. both castles reach zero health.

Possible deterministic policies include:

- draw;
- game-mode-specific priority;
- canonical event ordering.

The outcome MUST NOT depend on which worker reported destruction first.

Once match outcome is final, the authoritative gameplay simulation is terminal: production, movement, combat, abilities, and other gameplay ticks MUST NOT continue advancing. The server MAY retain the final frozen state for results, replay, spectators, or post-match UI, but further gameplay commands are rejected or ignored according to protocol state.

## 17. Player elimination

If a mode distinguishes player elimination from team victory, it requires explicit canonical rules.

Possible triggers:

- castle/objective loss;
- no remaining production potential;
- surrender;
- scripted condition.

Network disconnection alone MUST NOT be treated as elimination unless the game mode deliberately specifies a timeout/forfeit rule.

## 18. Surrender

Surrender, if supported, is an authoritative player command.

Team surrender semantics must be explicit for multiplayer teams: individual surrender, voting, immediate team loss, or transfer of assets are separate possible rules.

## 19. Upgrades

Upgrade purchase is an authoritative command validated against:

- owner/player;
- prerequisites;
- resource cost;
- current upgrade level/state;
- match phase.

Upgrade effects begin at the command's canonical execution tick/phase.

Whether existing units are modified retroactively or only future spawns inherit an upgrade is content/rule-specific and must be explicit.

## 20. Attack and spellcasting buildings

Buildings may combine topology/ownership with combat or spellcasting behavior.

Attack buildings:

- occupy navigation cells;
- may help form cages;
- independently acquire targets;
- do not navigate;
- fire attacks/projectiles using the deterministic delivery modes in `15-targeting-combat.md`;
- remain capable of targeting caged units if ordinary target rules select them and the attack can genuinely reach/hit them.

Spellcasting buildings may instead or additionally:

- have a mana pool and deterministic regeneration;
- automatically select eligible friendly/enemy targets;
- cast nearby buffs/debuffs/damage spells;
- provide passive auras;
- spend mana and use cooldowns without player orders.

Placement can therefore be strategically significant for both traffic/caging and ability coverage: for example, a friendly buff caster may be placed in the path of allied units so they pass through its effective range.

Their presence demonstrates why topology reachability, targetability, and ability coverage are separate concerns.

### 20.1 Legendary buildings

Legendary buildings use the same core ability/combat systems but may expose unusually powerful effects.

The engine MUST be able to represent at least:

- an infinite-range attack/wave aimed at a deterministic-random enemy unit;
- a cast that stuns all eligible enemy combat units for a fixed duration;
- artillery manually aimed by the player at an enemy unit, where the server captures that unit's authoritative position at launch and the projectile continues to that fixed position instead of following the target.

Only content explicitly marked with a player-targeted ability exposes such a command. This does not weaken the rule that ordinary combat units are non-commandable.

## 21. Unit death, corpses, and rewards

When a unit dies, reward/bounty/resource effects must occur in a deterministic death-resolution stage.

Biological/corpse-producing units MUST leave an authoritative corpse at their death position. The corpse is gameplay state, not merely a presentation artifact: abilities may query, target, consume, or transform it, including mechanics such as corpse explosion and raise dead.

Corpse creation occurs as part of deterministic death resolution after the victim has become unable to perform further actions. A corpse is not a living combat unit: it does not move, attack, retaliate, acquire targets, or participate in ordinary unit separation/path blocking unless a particular content rule explicitly gives a corpse additional behavior. Its authoritative state MUST be sufficient to determine at least its stable identity, position, source unit/type or corpse definition, creation tick, and any content-defined eligibility/expiry state.

A successful corpse-consuming effect MUST consume/remove the selected corpse atomically with its effect. If multiple same-tick effects compete for the same corpse, canonical effect ordering decides which one succeeds; later effects must revalidate that the corpse still exists and remains eligible. Corpse selection among multiple eligible corpses follows the same deterministic candidate/tie-breaking requirements as other ability targeting.

Whether a particular unit leaves a corpse, what corpse definition it produces, and whether/how long that corpse decays are content data. Biological units are corpse-producing by default unless imported/original-map semantics explicitly specify otherwise. A corpse remains authoritative until consumed, expired by its deterministic lifetime rule, or removed by another explicit gameplay rule.

Corpse lifetime, when authored, uses an exclusive absolute expiry. A corpse created while resolving tick `T` with positive lifetime `D` stores `expires_tick = T + D`; it exists after the tick-`T` structural commit and remains eligible while `current_tick < expires_tick`. It is removed at the deterministic timer/expiry boundary before ordinary gameplay evaluation on tick `expires_tick`. An absent lifetime means the corpse persists until consumed or explicitly removed. A zero-tick lifetime is invalid content.

The current executable verification slice carries corpse production through an explicit `CorpseProfile` attached to a directly spawned unit or to a production building's spawned-unit profile. This explicit authoring hook is transitional infrastructure for the content importer; it does not change the compatibility rule that imported biological/corpse-producing unit definitions should receive their recovered corpse semantics automatically.

Rules must define credit when:

- several attackers damage the victim on its death tick;
- reflected/environmental damage kills it;
- no meaningful attacker exists;
- summoned units have modified/no bounty.

If kill credit is irrelevant to Castle Fight economy, avoid introducing it unnecessarily; team/player reward rules can use simpler deterministic semantics.

## 22. Resource and production ordering

The phase relationship between income, purchases, production, and refunds must be stable.

For example, if income and a building command occur on the same tick, whether newly granted income may fund that command is a gameplay rule.

The schedule should document one canonical order rather than relying on system registration order.

## 23. Match seed

Each match has a server-selected canonical seed included in the initial state/replay header.

The seed affects only explicit deterministic-random mechanics.

Map/faction content that contains no randomness should not behave differently merely because the seed differs.

## 24. Initial state

Match construction must deterministically establish:

- map;
- team/player slots;
- factions;
- starting resources;
- castles/objectives;
- preplaced buildings/units;
- initial navigation topology;
- simulation tick;
- match seed;
- deterministic allocators.

Equivalent match configuration must result in the same initial canonical checksum.

## 25. Game modes

The engine should permit mode-specific definitions for:

- teams;
- starting resources;
- income;
- victory;
- allowed content;
- build regions;
- round/wave rules.

Core engine code should not assume every future mode is exactly 1v1, but abstraction should remain modest until a concrete second mode exists.

## 26. Required gameplay tests

The eventual gameplay suite should include:

1. accepted placement atomically charges resources and creates exactly one building;
2. insufficient resources rejects placement with no partial mutation;
3. production fires on exact documented ticks independent of worker count;
4. one player's disconnect does not pause that player's production/units and allows connected teammates to control that player's builder;
5. if an entire team disconnects, the match pauses at a tick boundary, resumes if a teammate returns before timeout, and ends on canonical timeout if nobody returns;
6. two same-tick production buildings yield identical state across worker counts;
7. bounded spiral spawning selects the same first valid location and drops the spawn attempt when the search is exhausted;
8. selling/destroying cage wall invalidates topology at the documented phase;
9. same-tick income/purchase follows documented ordering;
10. upgrade starts affecting entities at the documented tick;
11. simultaneous objective destruction resolves according to explicit draw/priority rule;
12. snapshot/reload preserves production sequences and next spawn ticks;
13. replay reproduces resource balances, ownership, delegated builder control, production, pause/resume, and victory exactly;
14. ordinary combat units have no player-order path;
15. builder is the only directly movable unit and does not block/participate in combat;
16. spellcasting building mana/cooldowns/autocast continue deterministically during a single-player disconnect;
17. player-targeted legendary artillery captures the selected unit's position and does not track it after launch.
