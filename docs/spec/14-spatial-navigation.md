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

## 12. Grid query determinism

Grid storage order MUST NOT become a targeting tie-break.

Queries MAY return candidates in arbitrary internal order only if the consuming system performs explicit deterministic selection/reduction.

If steering aggregates multiple neighbors, accumulation order must either be canonical or mathematically/order-stable under the chosen integer representation.

## 13. Local movement and steering

Strategic navigation provides a preferred direction. Units then apply local movement rules.

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

## 15. No hidden anti-stuck teleportation

Any anti-stuck behavior capable of crossing blockers can destroy caging and other positional strategies.

Therefore the simulation MUST NOT silently teleport/reposition a unit across building/terrain occupancy to "fix" pathing.

If a recovery mechanism is eventually necessary, it must be an explicit gameplay rule with deterministic constraints and tests proving that it does not break legal cages.

## 16. Spawn placement

Production buildings need deterministic unit spawn placement.

The rules MUST define:

- preferred spawn point(s);
- what happens if immediately occupied by other units;
- whether units may initially overlap and separate;
- whether alternative spawn offsets are searched;
- deterministic candidate order;
- behavior when the production building is itself part of a cage.

Spawn rules MUST allow a player intentionally to produce units into an enclosed cage when building geometry causes that outcome.

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
5. ranged enemy targets caged unit when targeting rules prefer it;
6. unit outside cage does not cross building footprint;
7. flow field identical across worker counts;
8. equal-cost route chooses the documented deterministic direction;
9. high-density crowd does not cause O(N²) candidate explosion under ordinary distributions;
10. spawn inside cage behaves predictably and deterministically;
11. builder standing in a lane neither blocks nor steers combat units;
12. builder cannot complete a cage and is absent from ordinary combat target queries.
