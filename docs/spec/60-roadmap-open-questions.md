# Implementation Roadmap and Open Questions

Status: **planning document**

## 1. Purpose

This document turns the architectural specifications into an implementation sequence that validates the riskiest assumptions early.

The project should resist the temptation to begin with art/UI/content breadth. Determinism, simulation scaling, navigation, targeting, and continuity are the expensive architectural risks; they should be proven while the world can still be rendered as debug circles/boxes.

## 2. Milestone 0 — Workspace and deterministic primitives

Create the Rust workspace and crate boundaries.

Suggested initial crates:

```text
crates/sim
crates/content
crates/protocol
crates/server
crates/client
```

Initial `sim` primitives:

- `Tick`;
- `SimId` + deterministic allocator;
- fixed-point scalar/vector types or selected dependency wrapper;
- deterministic keyed RNG;
- canonical checksum writer;
- worker-count-configurable simulation harness.

Exit criteria:

- same primitive/RNG fixtures pass across debug/release;
- canonical state/checksum format has tests;
- headless `Simulation::step()` skeleton exists;
- CI can run simulation with different worker counts.

## 3. Milestone 1 — Minimal ECS battle

Implement enough ECS to represent:

- two teams;
- units;
- position/movement;
- health;
- one melee attack, with the attack API shaped so the four specified delivery modes can be added without changing targeting identity;
- stable IDs;
- simple target state.

Use a fixed tick schedule with explicit targeting/combat/movement/cleanup phases.

No real networking or graphics required.

Add a minimal logical snapshot round trip in this milestone rather than postponing restoration until networking work.

Exit criteria:

- deterministic duel/battle fixtures;
- 1/2/4/N worker runs yield identical hashes;
- no authoritative float state;
- target decisions use stable tie-breaks;
- snapshot -> restore -> continue matches uninterrupted execution for the minimal battle;
- snapshot reconstruction does not depend on ECS insertion order.

## 4. Milestone 2 — Spatial grid and individual targeting at scale

Add the dynamic uniform spatial index.

Implement per-unit candidate queries and deterministic target ranking.

Before freezing those rules, inspect representative Castle Fight behavior for target retention, melee engagement, attack-building priorities, and caged-target selection. Record each observed rule as verified, inferred, or intentionally different.

Benchmark against a brute-force reference implementation for correctness.

Exit criteria:

- candidate ordering permutations do not change results;
- optimized targeting matches reference selector;
- synthetic 10,000-unit targeting/combat scenario is practical enough to profile interactively;
- phase timings identify actual bottlenecks.

## 5. Milestone 3 — Buildings, topology, flow navigation, caging

Add:

- build grid/footprints;
- objective/castle;
- production buildings;
- deterministic bounded expanding-spiral spawn placement;
- topology field;
- flow/integration navigation;
- reachable attack-position pursuit/filtering;
- local steering;
- attack buildings;
- non-combat builder entity confined to the team's owned third/build region.

Explicitly implement caging tests before adding anti-stuck behavior.

Add a minimal debug visualizer during this milestone (circles/boxes, footprints, selected target links, spatial cells, and flow arrows). It is a development tool only and MUST remain outside authoritative state.

Exit criteria:

- legal cage can be built;
- units remain trapped by ordinary navigation topology;
- trapped units remain individually targetable by attackers that can actually hit them;
- melee attacker rejects an unreachable caged unit and can acquire a reachable cage building instead;
- reachable enemy combat units outrank passive cage buildings;
- spawn search chooses the first valid point in a deterministic bounded spiral and drops an attempt when exhausted;
- destroying cage wall updates pathing and releases units;
- navigation output remains identical across worker counts;
- topology changes do not trigger per-unit global A* searches;
- debug visualization makes footprints, target choices, cages, and navigation directions inspectable while preserving identical headless checksums.

## 6. Milestone 4 — Headless gameplay prototype

Add enough content/game rules for a complete headless match:

- resources;
- building costs;
- production cadence;
- castle health/victory;
- all four attack delivery modes: melee, guaranteed-hit ranged, ballistic/siege, and bounce;
- builder inventory with at least one automatic/passive item and one active area-target item;
- mana-bearing automatic spellcasting building;
- at least one player-targeted legendary building ability;
- data-driven definitions;
- deterministic match seed/config.

A command-line simulation should be able to run an entire match and emit a replay/checksum trace.

Exit criteria:

- full match completes without client/rendering;
- replaying the same command stream reproduces final checksum;
- content bundle hash/version enforced.

## 7. Milestone 5 — Snapshot/replay and continuity hardening

Extend the snapshot mechanism already proven in Milestone 1 into the full canonical state format and finalized-input history required by networking.

Exit criteria:

- full-game snapshot/reload continuation equals uninterrupted simulation;
- replay from match start reproduces checkpoints;
- seek snapshot + fast-forward equals full replay;
- snapshot rebuild does not depend on ECS insertion order;
- snapshot input-stream boundaries and duplicate/gap handling are tested;
- catch-up throughput is measured on defined reference hardware/workloads.

This milestone de-risks reconnect before transport complexity exists.

## 8. Milestone 6 — Authoritative server + deterministic clients

Add protocol and network transport.

Start with correctness over exotic transport optimization.

Implement:

- match handshake/version/content checks;
- player command submission;
- server admission + deterministic execution validation;
- canonical tick/order assignment;
- scheduled command/server-event broadcast plus canonical stream records (finalized ticks and between-tick controls);
- periodic checksum checkpoints;
- disconnect/reconnect;
- delegated teammate control of a disconnected player's builder;
- team-wide pause/resume and disconnect-timeout match end;
- snapshot resync.

Exit criteria:

- multiple clients remain bit/checksum synchronized with server;
- different worker counts remain synchronized;
- fault-injected packet delay/reorder/duplication does not alter canonical command order;
- reconnecting player catches up without pausing match;
- corrupted client state can be repaired from server snapshot.

## 9. Milestone 7 — Player-facing Bevy client

The debug visualizer already exists by this point. This milestone turns presentation into the actual player-facing client:

- window/camera;
- simple meshes/sprites/colored primitives;
- simulation-to-presentation mapping;
- interpolated movement;
- builder movement and owned-region building placement UI;
- builder inventory / active item targeting UI;
- selection/inspection for non-commandable combat units;
- explicit player-targeted legendary ability UI;
- basic audio/event bridge.

Exit criteria:

- graphics can be disabled without changing checksums;
- render FPS independent of simulation rate;
- reconnect fast-forward does not replay all historical cosmetic events;
- building footprint/caging behavior is visually understandable.

## 10. Milestone 8 — Rendering scale and game feel

Profile large visible battles.

Evaluate:

- GPU instancing;
- compact render extraction;
- animation batching;
- culling/LOD;
- audio event aggregation;
- richer effects.

Do not optimize rendering based on assumptions from simulation profiling; profile GPU/client separately.

## 11. Milestone 9 — Content compatibility pass

Once systems are stable, focus on accurately recreating desired Castle Fight rules/content.

For each behavior, record whether it is:

- verified original behavior;
- inferred behavior;
- intentional divergence.

Important compatibility areas likely include:

- exact acquisition/target-retention rules;
- melee engagement behavior;
- attack-building target priorities;
- production timing;
- building footprints/spacing;
- armor/damage types;
- special abilities;
- spawn congestion;
- caging/attack soaking nuances;
- victory/resource pacing.

## 12. Open question — simulation tick rate

Candidate initial values: 20 Hz or 30 Hz.

Need prototype measurements for:

- visual interpolation quality;
- melee/ranged timing feel;
- projectile granularity;
- server CPU budget;
- command latency/input delay.

Decision becomes compatibility-sensitive once replays/network matches exist.

## 13. Open question — fixed-point representation

Need select between:

- custom integer subunit scheme;
- custom Q-format wrapper;
- mature Rust fixed-point crate after auditing semantics/performance.

Requirements:

- explicit overflow behavior;
- predictable rounding;
- efficient vector math;
- serde/network friendliness;
- cross-platform bit identity.

Benchmark before freezing representation.

## 14. Open question — navigation representation

Prototype likely starts with a regular navigation/build grid + flow/integration field.

Need determine:

- cell size;
- diagonal movement rules;
- building footprint resolution;
- how movement radii interact with grid blockers;
- whether distinct movement classes need distinct fields;
- whether hierarchical/tiled fields are needed for map size;
- incremental vs full topology recomputation threshold.

Caging behavior is non-negotiable during these experiments.

## 15. Open question — local crowd steering

Start simple.

Candidates:

- deterministic separation/repulsion;
- reserved local occupancy;
- velocity-obstacle/RVO-like system implemented with deterministic math;
- lane-direction bias plus local collision solver.

Evaluation criteria:

- handles thousands of units;
- does not produce excessive oscillation;
- preserves cages/blocked geometry;
- deterministic across worker counts;
- acceptable melee packing/game feel.

Avoid importing a floating-point nondeterministic physics engine into authoritative movement merely for convenience.

## 16. Partially resolved — combat timing semantics

The initial playable rules are fixed:

- a unit cannot attack on its spawn tick;
- stun/disable that becomes active before an ordinary attack resolves cancels that attack;
- death cancels every later unresolved action by that entity in the tick;
- already-launched persistent projectiles survive source death;
- ballistic/siege impacts resolve after movement and query post-movement occupants.

The verification implementation now also has provisional executable `RangedGuaranteedHit`, `RangedBallistic`, and `Bounce` semantics. Guaranteed-hit uses integer launch-distance/speed travel time, retained target identity, source-death independence, and deterministic invalidation when the target has already died/been removed before impact. Ballistic captures a fixed pre-movement destination, uses the same integer travel rule, resolves a hostile circular splash query against post-movement occupants, and canonically orders due projectiles and affected targets by stable `SimId`. Bounce keeps one persistent projectile identity across a bounded chain, applies integer damage falloff, records bounded hit history, and uses keyed deterministic candidate ranks for subsequent hostile-unit hops. These are specified in `15-targeting-combat.md` and remain compatibility-tunable rather than unexamined behavior.

Compatibility work still needs to establish:

- attack windup and backswing details;
- exact canonical ordering for otherwise simultaneous strikes beyond the stable-ID fallback;
- whether observed Castle Fight behavior requires revising the provisional guaranteed-hit travel/death rules;
- whether observed Castle Fight behavior requires revising the provisional ballistic circular-zone, hostile-only splash, building-intersection, or travel rules;
- whether observed Castle Fight behavior requires revising the provisional bounce range, repeat/building eligibility, chain cap, damage falloff, travel, or keyed-random selection rules;
- target-retention/range hysteresis details;
- splash/chain ordering;
- automatic spell cast timing details not already fixed by stun/death precedence;
- mana regeneration/cast-cost ordering on the same tick.

These should become small executable fixtures as soon as decided.

## 17. Partially resolved — target acquisition and engagement

The initial rules establish that a candidate requiring pursuit is invalid when the attacker has no reachable attack position; a unit that can hit a caged target from its current/reachable position may still select it. Eligible enemy combat units outrank non-attacking buildings for **fresh acquisition**, so an attacker unable to hit units inside a cage may instead attack cage buildings while still preferring reachable enemy units outside it. Stable `SimId` is accepted as the final exact tie-break.

Current-target behavior is now also defined: combat-unit engagements are sticky. A closer/new enemy and nearby ally-defense alerts do not replace a valid current unit target. Building targets are the exception: an actual attack on a nearby allied unit may make an attacker abandon the building and engage that ally's valid attacker, so units pounding a castle can peel off to fight arriving defenders. Mere defender proximity does not trigger this; the ally-defense rule still requires a resolved attack. A valid direct attacker can pre-empt a non-retaliation-locked target once; the first hostile attack in canonical combat-event order becomes the unit's direct-retaliation target, and later attackers cannot replace it while that target remains valid. Dead, disappeared, invisible, invulnerable, unattackable, unreachable, or sufficiently distant retreating targets are dropped and clear the direct-retaliation lock. The verification implementation starts with a provisional three-tile extra pursuit allowance.

Compatibility work still needs to investigate/decide:

- finer target priority classes, including attack-capable buildings/objectives;
- exact acquisition/pursuit distances per unit/content type;
- whether attack buildings use the same base acquisition rules as units;
- explicit taunt/forced-target mechanics, while preserving the rule that builder-held item damage does not create ordinary retaliation;
- detailed air/ground/building preferences.

The engine provides deterministic reachability/capability filtering, sticky engagement state, self-retaliation and nearby-ally defense state, and total ordering; content/game rules fill in the remaining semantic score.

## 18. Open question — transport

Do not freeze transport before protocol semantics.

Evaluate after command/snapshot prototypes exist:

- QUIC ecosystem maturity/performance;
- simpler reliable transports for first playable;
- NAT/dedicated-server assumptions;
- encryption/auth needs;
- snapshot streaming support;
- browser client relevance (currently not a core requirement).

Simulation/protocol should remain transport-agnostic.

## 19. Open question — server deployment model

Likely first target: native headless Linux dedicated server.

Need later decide:

- one process per match vs multiple matches/process;
- match persistence policy;
- crash recovery;
- orchestration/container model;
- authentication/lobby/matchmaking boundaries;
- resource limits per match;
- server tick overload behavior.

These are operational layers above deterministic simulation.

## 20. Open question — scripting/modding

Do not add a scripting runtime until actual content proves static data + Rust systems insufficient.

If needed, evaluate only deterministic/sandboxable choices and snapshot semantics.

Modding is desirable architecturally but not worth compromising the first deterministic core.

## 21. Open question — original assets/IP

A native implementation must distinguish engine/gameplay compatibility work from copyrighted Warcraft III/custom-map assets and other third-party intellectual property.

Before distributing a standalone game, asset/code/name/licensing provenance must be reviewed. The architecture should permit clean original/replacement assets and data.

This is a distribution/legal project concern rather than a simulation rule, but it should be addressed before public release.

## 22. First implementation slice

The recommended first code slice is deliberately tiny:

```text
fixed-point position
+ SimId
+ 2 teams
+ unit ECS components
+ uniform spatial grid
+ individual target selection
+ integer damage/cooldown
+ fixed tick schedule
+ canonical checksum
+ worker-count determinism test
```

Render nothing.

Once that survives a synthetic multicore battle reproducibly, add navigation/buildings/caging and the structurally non-combat builder. Then add the remaining attack delivery modes and ability/item machinery before freezing the network command schema. That sequence validates the project's hardest architectural premise before substantial client/content work accumulates.
