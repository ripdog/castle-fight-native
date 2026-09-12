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

The builder is not a combat unit. It exists to move for player interaction, construct buildings inside the owning team's build region, and carry/use items. It is non-targetable by ordinary combat, invulnerable to ordinary battle damage, and non-blocking to combat-unit movement.

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

Resources are canonical integer/fixed-point values.

Resource mutations occur through explicit deterministic operations such as:

- periodic income;
- building purchase;
- building sale/refund;
- bounty/reward;
- upgrade purchase;
- game-mode grants.

Resource arithmetic MUST define overflow/clamp behavior and MUST NOT use presentation-side balances as authority.

## 6. Periodic income

If the game uses periodic income, it MUST be scheduled by simulation tick.

Example semantic form:

```text
income event every K ticks
for each player in canonical PlayerId order:
    apply deterministic income formula
```

If income depends on buildings/upgrades, the formula must read canonical state from the defined phase.

The displayed countdown may interpolate against wall clock, but the actual grant is tick-based.

## 7. Building lifecycle

A building instance has an explicit deterministic lifecycle.

Possible states include:

```text
pending command (not yet authoritative)
constructed/active
constructing (if build time exists)
disabled/stunned (if supported)
dying/destroy-pending
removed
```

The first playable version MAY treat accepted placement as immediately constructed if that matches desired gameplay.

The state in which a building begins blocking navigation, producing units, attacking, or granting upgrades MUST be explicitly defined.

## 8. Placement and ownership

Building placement MUST be inside the owning team's canonical build region. In the standard map this corresponds to that team's third of the battlefield.

Accepted placement creates an authoritative building with:

- deterministic `SimId`;
- owner `PlayerId`;
- team;
- building content ID;
- canonical grid position/rotation;
- footprint/occupancy;
- initial health/state;
- production/attack timers as applicable;
- mana/ability state where applicable.

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

## 21. Unit death and rewards

When a unit dies, reward/bounty/resource effects must occur in a deterministic death-resolution stage.

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
