# Spatial Indexing, Navigation, and Caging

Status: **normative behavior, provisional algorithms**

## 1. Purpose

This document defines the spatial model needed for high unit counts while preserving important emergent behavior, especially intentional caging.

The implementation should minimize per-unit global pathfinding. Shared navigation data and local steering are preferred.

## 2. World model

The authoritative battlefield is 2D even if rendered in 3D.

Authoritative positions use deterministic fixed-point/integer coordinates.

The map contains:

- static terrain/navigation constraints;
- player-buildable footprints;
- objective/castle footprints;
- optional terrain regions/cost classes;
- dynamic combat units and projectiles;
- one non-combat builder per player, whose position is authoritative but which does not block or steer combat units.

## 3. Two spatial structures with different purposes

The design SHOULD separate:

1. **topology/occupancy navigation data** — relatively slow-changing terrain/building blockage;
2. **dynamic proximity index** — rapidly rebuilt/indexed unit positions for targeting and local steering.

Conflating these causes unnecessary path recomputation.

## 4. Building occupancy

Buildings have deterministic authoritative footprints.

A legal building placement reserves/blocks the cells or geometry defined by that footprint.

Placement legality MAY include:

- map bounds;
- buildable terrain;
- overlap rules;
- player region constraints;
- resource/cooldown requirements;
- explicit game-mode exclusions.

Placement legality MUST NOT automatically require preserving a traversable path between each spawn location and the enemy objective.

## 5. Caging is required behavior

A player MUST be able to create an enclosed area using legal buildings, including an area containing friendly spawned units.

The engine MUST NOT automatically:

- reject the final building solely because it seals a region;
- relocate trapped units outside the enclosure;
- allow trapped units to phase through friendly buildings;
- despawn trapped units merely because no objective path exists;
- make trapped units untargetable merely because they are unreachable by ground navigation.

A trapped unit remains an ordinary combat entity.

This behavior is strategically meaningful because trapped units may attract valid long-range attacks away from units progressing down the lane.

## 6. Navigation reachability is separate from targetability

Navigation answers questions such as:

> Which traversable direction leads toward the enemy objective?

Combat targeting answers questions such as:

> Which enemy entities are valid targets under this unit's attack rules?

These MUST remain independent.

A ranged attack building may target a unit inside an enclosed cage if:

- it is within acquisition/attack rules;
- the target category is valid;
- line-of-sight rules, if any, permit it;
- deterministic target ranking selects it.

Whether the attacker could walk to the target is irrelevant unless a particular attack rule explicitly says otherwise.

## 7. Shared strategic navigation

### 7.1 Preferred approach

The initial prototype SHOULD evaluate integration/flow fields or an equivalent shared destination field computed from the enemy objective through currently traversable topology.

Each traversable navigation cell can expose a preferred downhill direction/cost toward the relevant objective.

This avoids performing an independent full A* search for every unit.

### 7.2 Disconnected regions

Cells in a caged/disconnected region may have no finite route to the objective.

That is a normal state, not an error.

A unit in such a region MUST use deterministic "no route" behavior. Provisional behavior:

- continue ordinary local combat/targeting;
- do not invent a route through blocked topology;
- if no combat action applies, remain or perform limited local steering without escaping the connected component.

The exact idle behavior is a gameplay detail to refine during compatibility work.

## 8. Topology invalidation

Navigation topology changes when blockers change, such as:

- building constructed;
- building sold/destroyed;
- destructible terrain changes if supported;
- gate opens/closes if supported.

Moving units SHOULD NOT be inserted as hard blockers into the global strategic path field. They are handled by local steering/collision.

This allows navigation recomputation frequency to scale with structural events rather than unit count.

## 9. Incremental vs full field recomputation

The first correct implementation MAY recompute an entire affected field whenever topology changes.

Later optimization MAY use:

- tiled/sector fields;
- dirty-region propagation;
- hierarchical flow fields;
- multiple cached objective fields;
- parallel deterministic field construction.

Optimization MUST preserve the same defined navigation semantics/tie-break rules.

## 10. Deterministic flow/path tie-breaking

Equal-cost routes MUST resolve deterministically.

The navigation algorithm MUST define:

- neighbor enumeration order;
- exact integer/fixed-point movement costs;
- diagonal rules if diagonals exist;
- corner-cutting rules;
- tie-break rules for equal integration cost;
- treatment of footprint boundaries.

Parallel field construction MUST not expose task completion order to final field values/directions.

## 11. Dynamic spatial index

The simulation SHOULD use a uniform grid or similarly simple deterministic broad-phase structure for dynamic unit queries.

A uniform grid is preferred initially because the game world is expected to have:

- many similarly sized agents;
- frequent radius queries;
- a largely bounded battlefield;
- predictable neighborhood locality.

The grid supports:

- target candidate lookup;
- local steering/separation;
- nearby aura/effect lookup;
- local collision candidates;
- optional projectile broad-phase queries.

For **melee/ground-reachability target acquisition**, the verification implementation partitions target buckets by `(team, navigation connected-component)`. A unit therefore queries only hostile units in its own reachable component instead of enumerating nearby enemies in disconnected cages and rejecting them one by one. This is an acceleration structure only; every unit still makes its own target decision.

Other query classes (for example long-range attacks that may hit across disconnected ground components, auras, or projectiles) MUST use an index/query partition appropriate to their own semantics rather than incorrectly inheriting the melee reachability partition.

## 12. Grid query determinism

Grid storage order MUST NOT become a targeting tie-break.

Queries MAY return candidates in arbitrary internal order only if the consuming system performs explicit deterministic selection/reduction.

If steering aggregates multiple neighbors, accumulation order must either be canonical or mathematically/order-stable under the chosen integer representation.

## 13. Local movement, pursuit, and steering

Strategic navigation provides a preferred direction toward the objective. When a unit has an individually selected target that is not yet in attack position, movement instead pursues a reachable attack position for that target.

Target pursuit MUST respect topology. If no valid attack position is reachable for any applicable attack, that target is invalid for this attacker and targeting must select something else.

The verification implementation uses a cheap deterministic greedy step while it makes progress toward the selected attack position. If a blocker creates a local minimum (for example a straight wall directly between attacker and target), it falls back to deterministic A* and takes the first step of the resulting route. This avoids paying full pathfinding cost on ordinary open-lane movement while still routing around real building obstacles instead of oscillating against them.

This algorithm is provisional and must be profiled under realistic obstacle density. Cached target fields, bounded/local path search, or another deterministic strategy may replace it if A* fallback frequency becomes expensive.

Units then apply local movement rules.

Conceptually:

```text
flow-field preferred direction
        +
local separation / anti-overlap
        +
combat stop/engagement constraints
        +
unit-specific movement rules
        ↓
desired movement intent
```

The implementation SHOULD avoid a heavyweight general rigid-body solver unless measurements/gameplay prove it necessary.

Units may be modeled as circles/capsules or another simple footprint for local interaction.

## 14. Congestion and blocking

Friendly/enemy **combat units** are dynamic obstacles for local movement but SHOULD NOT permanently rewrite strategic reachability.

Builders are excluded from combat-unit local separation/collision and MUST NOT act as dynamic obstacles for battle movement. A combat unit's movement result must be unchanged merely because a builder occupies or crosses the same area.

The game needs deterministic rules for:

- temporary congestion;
- units approaching one another from opposite directions;
- melee engagement stopping distances;
- overlapping spawn positions;
- units packed against cages/buildings;
- units being surrounded.

The first implementation SHOULD prefer simple, testable steering over physically realistic pushing.

A combat unit that reaches attack range stops its strategic forward movement to fight. Units queued behind that engagement do **not** treat the engaged unit as permanent topology: when lateral traversable space exists, local steering SHOULD sidestep around the stationary fight and continue toward a reachable attack position. A deterministic `SimId`-derived side preference may break perfectly symmetric congestion. If geometry genuinely leaves no room to pass, ordinary congestion is allowed and no teleport/push-through exception is created.

The verification implementation uses a deterministic two-stage crowd pass. Units first compute movement intents from one immutable phase snapshot. A second data-parallel pass queries a small local spatial grid around those intended positions and applies a bounded separation offset. Integer accumulation and stable `SimId`-derived directions resolve exact overlaps and symmetric sidestep choices, so worker scheduling and neighbor enumeration order cannot alter results. The final separated position is accepted only if it remains traversable and in the unit's original connected navigation component.

The initial separation distance/strength are verification parameters rather than frozen gameplay constants; they should be tuned from measured congestion/game-feel fixtures.

## 15. No hidden anti-stuck teleportation

Any anti-stuck behavior capable of crossing blockers can destroy caging and other positional strategies.

Therefore the simulation MUST NOT silently teleport/reposition a unit across building/terrain occupancy to "fix" pathing.

If a recovery mechanism is eventually necessary, it must be an explicit gameplay rule with deterministic constraints and tests proving that it does not break legal cages.

## 16. Spawn placement

Production buildings use a simple deterministic space search rather than special-case cage logic.

For each production attempt:

1. start at the building's authored preferred spawn point;
2. enumerate candidate positions in a fixed expanding spiral/order;
3. choose the first position where the unit's authoritative footprint fits on valid traversable space without overlapping a blocking building or another combat unit;
4. stop after a configured deterministic candidate/radius limit;
5. if no candidate succeeds, the unit is not spawned and that production attempt is lost; no backlog is created by the core rules.

The exact spiral step/orientation and search limit are simulation/content parameters and MUST be deterministic.

The search deliberately does **not** reason about cages, connected components, or routes to the enemy objective. Caging remains emergent geometry rather than a spawn-system feature. Consequently, if the bounded spiral's first valid empty position happens to be outside an enclosure, the spawn may occur there; if no valid position is found within the search bound, it simply fails.

Builders are ignored as blockers during this search.

## 17. Builder spatial treatment

The builder's position remains authoritative for builder movement, building interaction rules, and range-limited item effects, but the builder MUST NOT:

- occupy navigation topology cells;
- appear in ordinary combat target candidate queries;
- participate in combat-unit local separation;
- block a narrow passage;
- complete or reinforce a cage.

If item systems require nearby-unit queries centered on the builder, they use the builder's position as a query origin without inserting the builder as a combat obstacle/target.

## 18. Attack buildings and spatial queries

Attack buildings use the same dynamic spatial infrastructure as units where practical.

They MUST still perform individual target selection according to their own target rules.

A tower's candidate query may have much larger radius than a melee unit's; broad-phase data structures must support this without reverting to all-entity scans wherever possible.

## 19. Maps with multiple lanes/objectives

The architecture MUST not hard-code a single straight lane.

Maps MAY contain:

- multiple lanes;
- merged/split routes;
- bridges/chokepoints;
- multiple objectives;
- player-specific destination fields;
- terrain movement classes.

Shared navigation fields can be keyed by destination/team/movement class as required.

## 20. Verification scenarios

Spatial/navigation tests MUST eventually include:

1. open lane: units advance to enemy objective;
2. building detour: field routes around a blocker;
3. complete cage: unit has no objective route and remains enclosed;
4. cage destruction: field updates and trapped units can leave;
5. ranged enemy targets caged unit when targeting rules prefer it and the attack can genuinely hit it;
6. unit outside cage does not cross building footprint;
7. flow field identical across worker counts;
8. equal-cost route chooses the documented deterministic direction;
9. reachable attack-position filtering correctly rejects unreachable melee targets without rejecting valid long-range targets;
10. disconnected high-density cages do not force melee targeting to enumerate every caged unit; component-partitioned target queries remain bounded by the attacker's reachable component;
11. target pursuit routes around a simple wall instead of oscillating at a greedy local minimum;
12. bounded expanding-spiral spawn search always selects the same first valid position;
13. a fully congested search region causes the production attempt to fail with no backlog;
14. spawn search does not special-case cage connectivity and may select a valid position across an enclosure boundary if reached by the bounded spiral;
15. builder standing in a lane neither blocks nor steers combat units;
16. builder cannot complete a cage and is absent from ordinary combat target queries;
17. exact-overlap crowd separation is deterministic across worker counts;
18. separation cannot move a unit through blocked topology or into another disconnected component;
19. in a perfectly aligned three-unit melee column meeting a mirrored enemy column, the front engagement stops while rear units deterministically sidestep through available lateral space and eventually reach an attack position;
19. dense opposing crowds remain benchmarked separately from ordinary lane movement.
