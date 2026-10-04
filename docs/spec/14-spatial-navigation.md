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

Equal-cost routes MUST resolve deterministically, but determinism MUST NOT collapse every unit onto the same arbitrary coordinate side of a symmetric route. For shared objective fields **and individual target-pursuit routes**, the executable verifier uses a stable `SimId`-derived two-sided bias when more than one next step has the same canonical path cost. Roughly half of sequential unit identities prefer the low-coordinate branch and half prefer the high-coordinate branch. The bias is stable for that unit rather than rerolled per tick, applies equally to greedy and A* target pursuit, and is part of exact pursuit-cache identity. This distributes symmetric north/south (or equivalent) capacity without scheduler-dependent randomness while leaving target selection itself unchanged.

The navigation algorithm MUST define:

- neighbor enumeration order;
- exact integer/fixed-point movement costs;
- diagonal rules if diagonals exist;
- corner-cutting rules;
- tie-break rules for equal integration cost, including any stable per-unit distribution rule;
- treatment of footprint boundaries.

The verifier keeps topology connectivity, shared integration fields, and exact A* fallback costs on four cardinal unit-cost edges. Route following may nevertheless take a one-cell diagonal shortcut when that diagonal strictly improves the chosen route. A diagonal is a local movement shortcut, not a new connectivity edge: both orthogonal side cells must be traversable, so units may not cut between touching blocked corners. Radius-aware movement additionally requires the unit's collision circle to fit at both orthogonal side-cell centers as well as the diagonal destination. This preserves caging/reachability semantics while avoiding visible north/east or south/east stair-stepping on otherwise open routes.

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

Strategic navigation provides a preferred direction toward the objective. In the standard Castle Fight lane profile, targetless movement has two stateless phases rather than blindly using the castle centerline or blindly walking east/west from every spawn point. A unit whose collision footprint is **outside the lane's north/south corridor** aims for the nearest legal lane edge with an equal forward and inward displacement in map coordinates: a clear north-base spawn therefore travels southeast/southwest at roughly 45 degrees, and a south-base spawn northeast/northwest, until its full collision footprint reaches the lane. There is deliberately **no fixed x-coordinate lane entrance** to calibrate against; on an unobstructed route the unit reaches the path after moving forward only about as far as its perpendicular distance from the path, rather than walking all the way to the inner base wall first. As soon as the unit's full collision footprint fits inside the lane corridor, it stops calibrating vertically and prefers **pure horizontal progress** toward the enemy objective side at its current authoritative `y`. It does not seek the corridor centerline. There is no remembered spawn lane or preferred lane coordinate: if a unit already starts lane-aligned behind its castle, it attempts horizontal progress immediately, routes around the castle or other blocker as required, and then resumes horizontal marching on whichever north/south line that detour leaves it on. Combat and crowd flow may likewise change `y`; while the resulting position remains inside the lane corridor, subsequent targetless movement uses that new line without recentering. If displacement carries the unit fully outside the corridor, the same stateless ingress rule applies again from its current position.

When the immediate horizontal step is blocked but the strategic objective remains reachable, the unit MUST use the stable shared objective field for the topology detour rather than repeatedly pathfinding to a same-row goal that changes whenever the detour changes rows; the latter can create deterministic two-sided heading oscillation at obstacle layouts. Once direct horizontal progress becomes legal again, targetless movement resumes from the unit's new `y`. If the locally projected lane-edge ingress goal is unreachable because the unit is caged/disconnected, lane-ingress guidance MUST NOT freeze it or invent a route through blockers: the existing deterministic best-effort objective-side behavior applies within the current connected component. A closed cage therefore still causes units to accumulate against the cage wall nearest the enemy castle without crossing the blocker.

Radius-aware movement MUST also handle the difference between a legal navigation-cell route and an off-center continuous unit position. A unit can legally stand near a blocked-cell corner while the straight segment from that exact point toward the next route cell would clip its collision circle against the blocker, even though the next cell center itself has sufficient clearance. Repeating that rejected segment forever is not a valid stationary state. When this occurs, the verifier may first move the unit toward the **center of its current legal navigation cell**, at normal movement speed and subject to the same topology/collision legality checks, before continuing the chosen route. This is a local route-alignment step, not teleportation and not new sticky path state. If it changes the unit's `y`, that resulting position becomes the unit's new stateless horizontal line once direct objective progress is clear again.

When a unit has an individually selected target that is not yet in attack position, movement instead pursues the nearest reachable part of that target's **attack envelope**: the region of authoritative positions from which the unit's current attack is legal.

The attack envelope contains every collision-legal position whose distance to the target geometry is `<= max_range`, excluding an authored minimum-range inner region when the typed delivery has one. A unit already anywhere inside that legal band stops pursuing and may attack from its current position; it MUST NOT back away merely to sit at maximum range. Line delivery carries the minimum range without imposing unrelated constructor fields on other delivery primitives.

For a unit target, the current target geometry is its authoritative point position plus the ordinary combat-unit collision exclusion. For a building target, distance is measured to the authoritative building footprint rather than its center. An approaching unit initially aims at the nearest point on the maximum-range boundary, but that point is not a reserved slot or formation assignment: local congestion may bend the unit around the envelope or carry it to any closer legal attack position.

Target pursuit MUST respect topology. If the directly projected attack-envelope point lies in blocked/disconnected topology, navigation falls back to a reachable approach for the same target geometry; if no valid attack position is reachable for any applicable attack, that target is invalid for this attacker and targeting must select something else.

The verification implementation first proves that repeated deterministic greedy descent from the current navigation cell reaches the selected attack-position cell. When such a monotonic greedy route exists, the unit takes its first step without a global search. If the greedy route reaches a local minimum (for example a straight wall directly between attacker and target), pursuit falls back to deterministic A*. Merely taking one A* step and resuming naive greedy pursuit is insufficient: that can backtrack on the following tick and oscillate at the blocker.

Exact A* fallback results MAY be cached as a deterministic derived navigation cache keyed by the exact `(source cell, target cell)` pair. Reusing such an entry MUST return the same next cell as a fresh canonical search, MUST NOT enter checksums/snapshots as authoritative state, and MUST be invalidated when navigation topology changes. Cache capacity/eviction is therefore performance-only and MUST NOT affect outcomes.

This algorithm remains provisional and must be profiled under realistic obstacle density. Fallback frequency, cache-hit rate, and expanded-node count should be measured separately; cached target fields, bounded/local path search, or another deterministic strategy may replace exact-pair caching if adversarial maps still make fallback work expensive.

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

A combat unit that reaches attack range stops its strategic forward movement to fight. Units queued behind that engagement do **not** treat the engaged unit as permanent topology: when lateral traversable space exists, local steering flows tangentially around congestion and continues toward any free part of the attack envelope. This rule is independent of delivery class and target class. A dense melee group may therefore wrap around a unit or building at close range, while a dense ranged group naturally occupies a larger ring/band because its attack envelope is larger. Enemy units, allied units, blockers, and other simultaneous fights may deform those shapes; no circular formation is prescribed or reserved.

The verification implementation uses deterministic movement intent plus local crowd resolution. Units first compute movement intents from one immutable phase snapshot. A data-parallel steering pass queries a small local spatial grid around those intended positions, applies hard-overlap repulsion, and also anticipates nearby units ahead when relative movement is closing the gap. Anticipatory pressure bends the mover on a stable tangential side while excluding its retained target from that steering repulsion: the target defines the desired attack envelope rather than acting as an obstacle that must be avoided before reaching legal range. Integer accumulation and stable directions keep worker scheduling and neighbor enumeration order from altering results.

Because a memoryless local field can still alternate between equally legal sides as neighbors move by tiny amounts, each combat unit carries a small authoritative avoidance state while actively bypassing congestion. The state identifies the current navigation goal (`Target(SimId)` or the team's `Objective`), the deterministic bypass side, and a bounded count of consecutive clear direct steps. A blocked direct step activates the stable `SimId`-derived side; the resolver keeps flowing on that side until direct progress has remained clear for several consecutive ticks. If the chosen side itself becomes physically unavailable, the canonical hard resolver may use the opposite side and records that new side rather than flipping back on the next tick. Target/goal change, reaching the attack envelope/objective, stun/death, or sufficiently sustained clear progress clears the bypass state. This state is canonical and participates in checksums because it affects later movement.

Soft separation is a steering/game-feel mechanism, **not** the correctness boundary. After steering, a deterministic hard reservation/commit pass MUST reject any set of proposed final positions that would overlap live collision-enabled combat units. The pass reasons about units' proposed **final** positions rather than treating every old position as occupied for the entire tick: a packed convoy may therefore translate coherently into spaces that its neighbors are simultaneously vacating. Stationary units still propose their current positions and remain real obstacles. Conflicting proposals are repaired in canonical `SimId` priority while continually updating the reservation grid; when an emergency local search has symmetric legal positions, its enumeration order uses the same stable per-unit two-sided bias rather than a global negative/positive coordinate preference. The final committed set MUST be globally non-overlapping. Physical collision is global and MUST NOT be partitioned by navigation connected-component: topology answers whether a route exists, not whether another physical body exists. The resolver may choose a deterministic alternate position or leave the unit at its previous legal position; it MUST NOT resolve pressure by compressing units into overlapping space or teleporting them through blockers.

Each combat unit may carry an authoritative circular collision radius. Unit-unit legality uses the sum of the two radii; a small and large unit therefore reserve different center distances rather than sharing one global spacing constant. An explicitly authored radius also applies against map bounds and blocked navigation cells: the unit's complete circle must fit on traversable topology. Radius-aware pursuit uses radius-valid path cells and radius-aware attack-envelope fallback positions. Shared objective navigation may cache one derived distance field per distinct authored radius; those fields preserve the strategic objective while excluding cells through which that radius physically cannot pass.

After a completed movement commit, two ordinary collision-enabled combat units MUST NOT occupy overlapping authoritative collision footprints. Production and building placement SHOULD prevent impossible overpacked states from being created in the first place; when there is no legal movement space, units queue or flow around one another rather than violating the collision invariant. Production placement MUST test the produced unit's own authored radius both against nearby unit radii and against static topology before spawning it.

The verification fallback for units without imported collision geometry remains the configured global separation diameter split evenly between the two units. That fallback exists for placeholder fixtures/presentation development; imported production content SHOULD author its real collision radius. The initial steering strength remains a verification/game-feel parameter rather than a frozen content rule.

## 15. No hidden anti-stuck teleportation

Any anti-stuck behavior capable of crossing blockers can destroy caging and other positional strategies.

Therefore the simulation MUST NOT silently teleport/reposition a unit across building/terrain occupancy to "fix" pathing.

If a recovery mechanism is eventually necessary, it must be an explicit gameplay rule with deterministic constraints and tests proving that it does not break legal cages.

## 16. Spawn placement

Production buildings use a simple deterministic space search rather than special-case cage logic.

For each production attempt:

1. start at the building's authored preferred spawn point;
2. enumerate candidate positions in a fixed expanding spiral/order;
3. choose the first position where the unit's authoritative footprint fits on valid traversable space without overlapping a blocking building or another combat unit; this requires an actual collision-radius/footprint query against nearby units, not merely checking whether the candidate navigation cell is empty;
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

The current authoritative movement-class model distinguishes `Ground` and `Air`. Ground units use ordinary blocked-cell topology, connected components, radius-aware clearance, and building footprints. Air units ignore ground blockers/building footprints, but they do **not** ignore explicitly authored no-fly/static air pathing or the authoritative map bounds. Air pathing therefore uses a separate deterministic topology containing only those air blockers; direct flight is used while legal, and a flyer routes through that topology when a direct segment would enter no-fly space. Ground and air units occupy separate collision layers: ground units do not physically block air units and air units do not block ground units or ground building placement; units within the same movement class still use deterministic pairwise collision/reservation.

Shared navigation fields can be keyed by destination/team/movement class as required if additional terrain movement classes are introduced later.

## 20. Verification scenarios

Spatial/navigation tests MUST eventually include:

1. open lane: units advance to enemy objective;
2. building detour: field routes around a blocker;
3. complete cage: unit has no objective route, remains enclosed, and continues best-effort movement until blocked against the objective-side cage wall;
4. cage destruction: field updates and trapped units can leave;
5. ranged enemy targets caged unit when targeting rules prefer it and the attack can genuinely hit it;
6. unit outside cage does not cross building footprint;
7. flow field identical across worker counts;
8. equal-cost objective routes use the documented stable per-unit bias so symmetric branches are both exercised without worker-count dependence;
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
19. sustained convergence on one destination never commits overlapping live unit collision footprints;
20. in a perfectly aligned three-unit melee column meeting a mirrored enemy column, the front engagement stops while rear units deterministically sidestep through available lateral space and eventually reach an attack position;
21. when no legal lateral/forward position exists, trailing units remain queued rather than compressing through the frontline;
22. production rejects a candidate spawn point that is in an empty navigation cell but lies within another unit's collision radius in a neighboring cell;
23. collision remains global across disconnected navigation components, including units approaching opposite sides/corners of blocking topology;
24. long-running production/convergence with hundreds of units does not panic and never commits overlapping collision footprints;
25. dense opposing crowds remain benchmarked separately from ordinary lane movement;
26. a ranged unit arriving behind an occupied allied firing clump commits to deterministic local flow, reaches a firing position without repeated side-to-side jitter, and produces the same result across worker counts;
27. a melee unit arriving behind an occupied engagement flows around the engaged units without repeated side switching and reaches legal attack range;
28. many melee units converging on one unit target distribute around the target's legal attack envelope rather than remaining in a single tail;
29. many melee units converging on a large building reach multiple faces of its legal attack envelope without assigned perimeter slots;
30. a ranged unit already inside maximum range remains at its current legal range rather than backing away toward the outer boundary;
31. mixed unit radii enforce pairwise clearance using the sum of the two radii and remain deterministic across worker counts;
32. an explicitly authored unit radius cannot clip blocked topology or map bounds merely because its center cell is traversable;
33. a radius-aware pursuer can route around inflated blocker clearance to an alternate legal attack position for both unit and building targets;
34. radius-aware objective pursuit can temporarily detour away from the objective and still converge through a physically wide-enough route without oscillating;
35. an objective-following unit blocked by stationary allies preserves a deterministic bypass side and flows around the clump when lateral space exists;
36. an unobstructed targetless lane unit preserves its exact current `y` while advancing toward the enemy side rather than drifting toward the objective centerline;
37. after combat or a forced topology detour moves a unit vertically, targetless movement resumes horizontally from the unit's new `y` without restoring any remembered spawn/home line;
38. a unit in a disconnected cage makes one-way best-effort progress along its current row toward the objective-side wall, then remains blocked there rather than pacing or crossing the cage;
39. a radius-authored objective mover approaching a blocking building corner from an off-center legal position cannot remain indefinitely corner-locked merely because the next route cell is legal only from a better-aligned point inside the current cell; it locally aligns within the current cell and proceeds without combat perturbation or teleportation;
40. an air unit crosses ground blockers/building footprints while remaining inside map bounds;
41. ground and air units may overlap in 2D authoritative position because they occupy separate collision layers, while two air units still cannot overlap each other;
42. an air unit ignores disconnected ground topology when pursuing an otherwise valid target;
43. an air unit encountering authored no-fly topology deterministically routes through the nearest legal opening instead of crossing the blocked area or stalling forever at its edge;
44. a tightly packed convoy can advance into positions its neighbors are simultaneously vacating, while the final committed positions remain globally non-overlapping;
45. a targetless unit detouring around reachable static topology does not repeatedly reverse heading because its temporary same-row detour destination changed as it crossed rows; it follows the stable shared objective field until direct horizontal progress is legal again.
