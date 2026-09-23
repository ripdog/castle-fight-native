# Implementation Plan — Versioned Content and Multiplayer

Status: **ordered implementation handoff for Sol; not an implementation-completion report**

Prepared: **2026-09-16**, from a static review of the current checkout and the requirement to let users choose historical Castle Fight map versions at match creation.

## Objective

Make the existing simulation support reproducible historical rulesets, a sustainable content pipeline, and authoritative multiplayer with reconnect. Preserve its deterministic navigation/combat architecture while replacing the prototype interfaces that would make those features difficult to extend.

The first multiplayer milestone uses the existing five combat units, five production buildings, and two towers in an explicitly restricted development configuration. A complete historical ruleset becomes selectable for ordinary play only when all behavior reachable in that configuration is supported. Archived extraction alone does not imply playable support.

## Instructions for Sol

- Work through the numbered steps in order. Each step may require several small commits; the suggested commit boundaries below are not requests for one large commit per step.
- Commit completed work automatically in narrowly scoped chunks. Distinguish implemented work from verified work in handoff notes; do not report an unexecuted gate as passed.
- Update the affected normative specs whenever behavior changes. Preserve current phase order during mechanical refactors. Explicitly version intentional simulation-semantic changes.

## Existing work to preserve

- Rendering-independent `bevy_ecs` simulation with integer positions and stable `SimId`s.
- Keyed randomness, stable resolution ordering, parallel calculation, and ordered mutation.
- Individual targeting, retaliation, all four attack delivery classes, authoritative projectiles, ground/air movement, caging, collision, and production placement.
- Builder movement/build/repair commands, construction lifecycle, and economy.
- Native-effect implementation IDs, version-range bindings, separate 9.27 tuning, and extraction provenance/coverage tooling.
- Existing focused regressions, phase metrics, and historical benchmark fixtures. Historical measurements are not a performance guarantee for the changed build.

## Architecture boundaries

Keep deterministic domain types, command execution, logical state, and simulation phases in the shared simulation layer. Keep immutable content definitions and loading behind a clear shared interface; a separate content crate is optional until the dependency direction is clean. Do not introduce a `sim -> content -> sim` cycle merely to match the proposed workspace diagram.

Add a protocol crate for versioned wire envelopes/encoding and a headless server crate for session/transport/scheduling. Both server and client use the same deterministic command and state semantics. The simulation must not depend on sockets, client UI, render assets, or wall-clock scheduling. Internal generic fixture APIs can remain separate from the player command surface.

Use explicit typed effect implementations and reusable primitives. Extend the existing registry; do not add a general-purpose scripting VM, per-version copies of the entire engine, or an ECS/navigation rewrite as prerequisites.

## Execution order

| Step | Deliverable | Required foundations |
| --- | --- | --- |
| 1 | Complete authoritative-state inventory and checksum coverage | Current checkout |
| 2 | Retained extraction history and release manifests | Current checkout and source provenance |
| 3 | Canonical content catalog and compatible behavior resolver | 2 |
| 4 | Shared version-selected match initialization | 3 |
| 5 | Player ownership and match lifecycle | 4 |
| 6 | Canonical commands and local execution path | 3–5 |
| 7 | Logical snapshots, replay, and deterministic restoration | 1, 3–6 |
| 8 | Split simulation responsibilities behind explicit phases | 7 |
| 9 | Minimal authoritative server and deterministic clients | 4–8 |
| 10 | Reconnect, delegation, and desync recovery | 5–7, 9 |
| 11 | Reusable attack, ability, and status extensions | 3, 6–8 |
| 12 | Repeatable roster onboarding and content batches | 3–7 and the relevant step 11 mechanics |

The numbered order is the recommended execution priority; the final column identifies technical dependencies. In particular, step 11 follows multiplayer in the recommended sequence to finish the small end-to-end match first, but ability infrastructure does not technically depend on reconnect. Snapshot/replay support precedes new complex stateful mechanics. Transport can be developed using the existing combat slice; it does not need every ability to be implemented first. Verification and performance review apply throughout, as described at the end of this plan.

## 1. Inventory authoritative state and close checksum gaps

**Starting points:** `crates/sim/src/simulation.rs` (`Simulation`, `canonical_checksum`, `CanonicalEntity`), `components.rs`, specs `11` and `21`.

Tasks:

- Inventory every future-affecting field: allocator state, tick, seed, configuration/combat rules, resources, entities, pending construction/build orders, retaliation alerts, navigation continuity, projectiles, persistent effects, timers, mana remainders, and random/cast/attack sequences.
- Classify each field as mutable authoritative state, immutable match/content input, safely rebuildable derived state, or presentation/diagnostics. In particular, distinguish stored navigation continuity from disposable path caches.
- Fix the known checksum omissions: `next_id` and authoritative configuration/seed/rules must affect state/compatibility identity. Until the content bundle exists, use explicit canonical configuration encoding; replace that with the appropriate immutable identity in step 3.
- Audit component presence and identities as well as numeric values. Extract a canonical writer/state visitor if helpful; do not maintain unrelated hand-written lists for future snapshot and checksum coverage.
- Record the checksum/simulation compatibility change and add a regression case where identical live entities but different allocator histories must not have the same authoritative identity.

**Acceptance:** changed allocator state, seed, or rules is detected; changing worker count or diagnostic timings does not affect canonical identity. The state inventory names everything required by step 7.

**Commit boundaries:** state inventory documentation; checksum fix with focused regression cases and compatibility/spec changes.

## 2. Preserve map extraction history

**Starting points:** `docs/original_map`, `tools/wc3-map`, `crates/sim/data/castle-fight`, `version.rs`, spec `40` section 4.1.

Tasks:

- Establish a version/revision-addressed layout and manifest for extracted data and validated runtime content. Record map archive digest, declared CF release, Warcraft base-data version, extractor revision/schema, and content revision.
- Preserve the existing 9.27 source and extraction before introducing newer outputs. Treat in-progress extraction changes explicitly; do not label a mixture of versions as a historical snapshot.
- Make extractor/resolver destinations explicit and prevent a newer extraction from overwriting a retained snapshot. Update consumers of unversioned `include_str!` paths as they migrate in steps 3–4.
- Register the available 9.32 archive only after checking its actual release identity and source inputs. Do not infer ability compatibility or playable support from its presence.
- Support two release snapshots coexisting. Corrected extraction for the same map label receives a new revision/hash; retain addressability of older artifacts needed by saved identities.

**Acceptance:** selecting a release resolves its own source/derived data; adding or correcting another snapshot cannot silently change it. Any unavailable extraction tooling/input is recorded as pending, with no fabricated data or release coverage.

**Commit boundaries:** manifests/layout and 9.27 migration; version-aware tooling/paths; separately reviewed additional extraction artifacts when available and authorized.

## 3. Build the canonical content catalog and behavior resolver

**Starting points:** `content.rs`, `native_effects.rs`, native-effect JSON files, `native_effect_coverage.py`, spec `40`.

Tasks:

- Introduce stable explicit unit/building/ability IDs and immutable validated definitions. Keep source rawcodes/provenance; avoid deriving IDs from enum position, file order, or map iteration order.
- Load or generate numeric definitions once per content bundle. Migrate the current roster, builders, production links, costs, footprints, command-card metadata, and effect tuning. Remove repeated runtime TSV scans from ordinary lookups.
- Make unit creation consume a complete resolved definition. Eliminate the need for callers to remember separate spellcasting/properties spawn variants; direct and production spawning must preserve the same authored mechanics.
- Extend the existing binding registry into a match-content resolver. Starting from the configured mode/catalog, traverse required abilities and indirect dependencies, including proc child effects, upgrades, summons, and items. Use a visited set for valid reference cycles; reject unsupported cycles with a useful diagnostic.
- Select exactly one verified implementation binding per required source kind/key for the requested release. Validate implementation availability, tuning schema/ranges, and all required dependencies. Support reuse across releases and separate handlers for changed semantics without widening validity speculatively.
- Produce stable errors for missing/overlapping/incompatible bindings. Explicit no-runtime marker bindings are valid; silent effect omission and nearest-version fallbacks are not.
- Freeze and canonically hash resolved gameplay definitions, source-to-implementation bindings, tuning, and applicable rules. Keep cosmetic asset changes outside gameplay identity.
- Distinguish archived, supported development-subset, and supported full-configuration availability.

**Acceptance:** catalog/registry input order cannot change the bundle. Two versions can use different tuning with a shared handler, or different handlers when semantics change. Missing indirect effects reject configuration. Old releases retain their selected behavior. Focused synthetic resolver fixtures must be clearly labeled and must not advertise invented compatibility for real releases.

**Commit boundaries:** immutable catalog/current-slice migration; resolver and dependency diagnostics; canonical bundle identity.

## 4. Move match initialization out of the client and expose version selection

**Starting points:** `crates/client/src/demo.rs`, `main.rs`, `build_ui.rs`, `unit_models.rs`, shared content/configuration modules, specs `30`, `31`, `40`, `41`.

Tasks:

- Introduce shared deterministic match construction from an explicit selected release, content revision/bundle, mode, seed, and participant configuration.
- Move authoritative map bounds, lane/build regions, castle stats/placement, terrain/pathing loading, and other gameplay defaults from `demo.rs` behind the selected version's shared definitions. Keep camera/render setup client-owned.
- Resolve all gameplay defaults from the chosen bundle once. Consolidate the simulation tick-rate definition; never independently substitute a default map version in a production/unit/effect lookup.
- Add match setup selection for available CF versions with clear availability diagnostics. Keep an explicit development-subset mode for the current playable slice rather than claiming a complete 9.27/9.32 ruleset.
- Drive build menus and asset selection from the resolved allowed catalog. Remove the separate seven-entry menu and current-unit model whitelist; preserve deliberate asset fallbacks and avoid eagerly loading unrelated roster art.

**Acceptance:** a headless caller and the client construct the same authoritative initial state from the same config. Selecting another available bundle changes all relevant gameplay data together. Unsupported selection is explained before construction. Existing development placement and visuals remain usable.

**Commit boundaries:** shared map/match bootstrap; catalog-driven menus/assets; version-selection UI.

## 5. Separate players from teams and implement match lifecycle

**Starting points:** `Team`, spawn/ownership properties, `player_resources`, builder lookup/spawn, construction/refunds, `bridge.rs`, resource/selection UI, spec `41` and `42`.

Tasks:

- Add stable `PlayerId` and explicit ownership for builders, buildings, produced units, and gameplay provenance where required. Keep `Team` for allegiance, team objectives, and shared build regions.
- Replace team-indexed player resources/income with canonical player records. Preserve ownership through production, construction cancellation/refunds, and effects; model shared castles separately.
- Enforce one builder per active player, not per team. Distinguish who owns an entity from who currently has permission to control it.
- Add explicit lifecycle state and authoritative objective identities. Evaluate victory at a documented phase, define simultaneous-objective destruction behavior in version/mode rules, and freeze gameplay once the outcome is terminal.
- Represent gameplay-relevant connection/delegation and pause state in the shared model. Actual network-driven transitions arrive through canonical controls in steps 6 and 10.
- Update presentation and UI to distinguish the local player, allies, opponents, and permitted actors.

**Acceptance:** a 2v2 fixture has four builders and four independent economies; allied purchases/refunds/production retain the right owner. Unauthorized allied control fails unless explicitly delegated. Terminal matches stop production, movement, combat, and gameplay tick advancement.

**Commit boundaries:** player identity/ownership propagation; per-player economy/UI; lifecycle/objective rules.

## 6. Route every player action through canonical commands

**Starting points:** public simulation mutators, `builder_controls.rs`, `build_ui.rs`, `main.rs`, specs `20`, `31`, `41`, `42`.

Tasks:

- Define typed domain commands and structured outcomes for every existing player action: move/follow/stop/blink, build purchase/cancel, repair/autocast, and permitted building targeting. Preserve ordinary combat-unit autonomy.
- Commands reference IDs and deterministic coordinates. Resolve building definitions/costs on the authoritative side; never accept caller-authored stats or arbitrary component edits as a player command.
- Add envelopes with player identity, client sequence, execution tick, and canonical within-tick order. Separate admission checks from shared execution-time checks, including ownership and changing resources/occupancy.
- Define finalized tick inputs, including empty ticks, and stream positions for gameplay-relevant between-tick controls. Specify duplicate/conflicting-sequence handling and gap detection. Pausing must not prevent canonical resume/end controls from being applied.
- Introduce a local match driver that assigns/finalizes commands through the same execution path the server will use. Replace direct UI mutations with submission and acknowledgement, preserving previews as non-authoritative.
- Limit raw mutation APIs to construction, internal execution, and fixtures where appropriate. Give playback/debug controls an explicit role separate from normal match commands.

**Acceptance:** the same finalized inputs yield the same outcomes/state on independent instances. Build contention and resource failures are atomic. UI frame timing determines only when a request is submitted; once assigned, its execution is tick-defined. Duplication cannot double-charge or double-execute.

**Commit boundaries:** command domain/executor; canonical stream/local driver; migration of all input handlers and outcomes.

## 7. Implement logical snapshots, restore, and replay

**Starting points:** step 1 inventory, `Simulation`, canonical entity encoding, command stream, specs `11`, `12`, `21`.

Tasks:

- Create an explicit logical authoritative snapshot schema independent of Bevy entity handles and presentation snapshots. Share field coverage/encoding discipline with checksums.
- Capture the exact compatible content/simulation identities, allocator, completed-tick boundary (including initial state), canonical stream position, player/lifecycle state, and every persistent field in the inventory.
- Account explicitly for admitted future commands and the scheduler/history boundary. A restore must neither lose an already scheduled command nor replay it twice; follow spec `21` for snapshot/history ownership.
- Restore stable IDs and references, validate invariants, and rebuild only genuinely derived spatial/navigation/index state. Reconstruction must not consume gameplay IDs/randomness or depend on ECS insertion order.
- Add replay headers, finalized-input/control history, checkpoint hashes, and seek snapshots. Validate input/schema sizes and incompatible identities before applying data.
- Make cosmetic events resettable/suppressible during restoration and fast-forward; publish a fresh presentation state at the live boundary.

**Acceptance:** uninterrupted and restored/replayed continuations agree across worker counts and altered ECS insertion order. Cover in-flight projectiles, bounce/chain history, Burning Oil, status expiry, fractional mana/repair state, construction, pending paid build orders, future commands, paused state, and terminal matches. Include a clear initial-state boundary case.

**Commit boundaries:** state schema/export/restore; replay history/boundaries; continuation/seek regression cases and diagnostics.

## 8. Split simulation responsibilities without changing phase semantics

**Starting points:** approximately 10,000-line `simulation.rs`, its approximately 1,100-line `step()`, and the large test module in `lib.rs`.

Tasks:

- Keep a small simulation facade and explicit tick coordinator. Extract construction/builders/economy, abilities/statuses, targeting/combat/projectiles, navigation/movement, and canonical state into cohesive modules incrementally.
- Preserve the exact documented order, including construction occupancy refresh, ability/death precedence, projectile impact phases, collision commitment, income, and outcome evaluation.
- Use narrow phase inputs and typed outputs/commit buffers. Avoid turning the old monolith into modules that all receive unrestricted mutable access to the entire simulation.
- Consolidate complete resolved spawn definitions and explicit entity construction so a new mechanic cannot be omitted from one creation path.
- Move tests beside the owning subsystem where useful. Add a rebuildable `SimId -> Entity` index only where it simplifies repeated resolution and preserves ordering; do not combine speculative performance changes with mechanical moves.

**Acceptance:** for fixed initial states and finalized inputs, the refactor preserves canonical outcomes. Reuse restoration/replay fixtures to compare behavior. Any intended rule change is separated into its own versioned commit.

**Commit boundaries:** one subsystem extraction at a time, with no unrelated formatting or content expansion.

## 9. Add the minimal authoritative server

**Starting points:** new `crates/protocol` and `crates/server`, shared match driver/state, client connection setup, spec `20`.

Tasks:

- Choose a simple reliable ordered transport suitable for the initial native prototype. Document the choice; keep matchmaking, public hosting/discovery, and advanced prediction outside this step.
- Implement protocol framing/schema versions, bounded decoding, compatibility handshake, player/session assignment, command submission/acknowledgement, finalized-input broadcast, and checkpoint messages.
- Run headless authoritative matches using the same bootstrap, command executor, and simulation as clients. Bind each connection to an assigned player rather than trusting a player ID in the payload.
- Define input scheduling and pacing explicitly. Clients must not interpret missing finalized input as an empty tick or invent an alternate canonical order. A late client catches up or waits according to the driver policy.
- Broadcast deterministic execution outcomes and reject incompatible map/content/implementation/simulation identities before joining.

**Acceptance:** a headless server and two independent clients remain synchronized on the existing slice; also exercise a four-player/2v2 scenario. Delay/duplication/disconnect injection cannot change already finalized order or permit unauthorized commands. The server has no rendering dependency.

**Commit boundaries:** protocol/handshake; server match/session loop; network client integration; integration scenarios.

## 10. Complete reconnect, delegated control, and resynchronization

**Starting points:** server sessions/history, snapshot/replay APIs, match controls, specs `20`, `21`, `41`, `42`.

Tasks:

- Translate connection changes into canonical controls. Delegate a disconnected player's allowed builder actions to connected teammates without transferring ownership/resources; revoke delegation on reconnect.
- Implement team-wide disconnect pause/resume/timeout. The server observes real time and records the resulting controls at completed boundaries; replay applies those records without reproducing wall-clock waiting.
- Transfer an authoritative snapshot with a pinned history/live-subscription boundary. Queue subsequent live records, detect duplicates/gaps, retain required history until handoff finishes, and bound memory/transfer work.
- Catch up from history and verify checkpoints. When catch-up cannot close the gap, choose a fresher snapshot rather than growing an unbounded backlog.
- Recover deliberately corrupted client state by replacing it and replaying authoritative history. Reset interpolation/event deduplication and suppress historical cosmetic effects during catch-up.

**Acceptance:** reconnect during active combat/construction preserves future state; single-player absence continues gameplay when a teammate remains; team-wide absence pauses and resumes/ends correctly. History/live handoff has no gaps or duplicate effects. Desync recovery restores equality without merging divergent gameplay state.

**Commit boundaries:** lifecycle/delegation controls; snapshot transfer/history retention; client catch-up/resync.

## 11. Extend attacks, abilities, and status handling for roster breadth

**Starting points:** attack/spellcasting/status components, native effect modules/registry, extracted attack and mechanic inventories, specs `15`, `16`, `40`, `42`.

Implement these as separate substeps, each updating state restoration, checksums, and content compatibility:

1. **Multiple attacks:** stable slots, authored activation/target masks, switching or concurrent behavior as supported by evidence, and per-slot state/RNG identity. Preserve the single-attack case. Represent extracted splash damage tiers rather than one flat outer radius.
2. **Multiple abilities:** stable slots with per-ability cooldown/sequence state, shared mana, explicit priority and contention rules. Support automatic and explicitly permitted manual activation through the same validated effect vocabulary.
3. **Status storage and stacking:** define source/instance identity, refresh/stack/dispel/expiry semantics, and deterministic modifier order. Remove arbitrary reachable runtime capacity panics: either prove bounds for a configuration at load time or use deterministic storage that supports valid combinations. Do not silently drop effects on overflow.
4. **Reusable triggers/effects:** extend damage/healing/status application, on-hit/on-kill/on-death, summons, corpse selection/consumption, and auras only through representative extracted mechanics. Preserve caster/owner/ability provenance and explicit resolution order.
5. **Representative command-driven abilities/items:** implement one appropriate active item and one manual building ability to verify targeting, permissions, costs, and persistence through the established command/protocol path.

Choose named examples from the selected version's extraction before coding each extension. Record verified behavior, inference, and intentional divergence separately. Retain old handlers when semantics differ across map versions; new implementations must enter the resolver and coverage ledger.

**Acceptance:** competing abilities, multi-attack behavior, overlapping statuses, source death, summons, and effect chains remain deterministic and restore correctly. Their presentation cannot affect damage/timing. At least one data-only tuning difference and one explicit behavior-selection difference exercise the version resolver without fabricating historical claims.

**Commit boundaries:** one abstraction extension plus its representative mechanic per logical series; no full-roster import in this step.

## 12. Establish roster onboarding and add content in batches

**Starting points:** versioned extraction/catalog generation, effect coverage, catalog-driven client, representative mechanics from step 11.

Tasks:

- Generate a per-version onboarding report joining units/buildings to stats, attacks, required behavior/dependencies, native bindings, upgrade/production relations, and visual asset availability. Report extraction coverage separately from executable behavior coverage.
- Make adding an ordinary unit/building data work through the catalog, without editing core simulation dispatch, menu lists, or model allowlists. Unique mechanics extend a reusable primitive or add an explicitly versioned handler.
- First add a small batch using already supported mechanics. Then add batches grouped by shared mechanics, with upgrade chains and all reachable effects validated together.
- Keep incomplete units/configurations explicitly unsupported or in named development subsets. Promote a complete historical configuration only when every reachable requirement resolves safely.
- Preserve historical snapshots and prior implementations while importing additional releases. Adding a release must not replace older lookups or rewrite old replay identities.

**Acceptance:** each batch has complete required behavior coverage, valid production/upgrades, correct version-scoped stats, usable presentation/fallbacks, and focused scenarios. Existing historical bundles retain their identities and behavior unless a new revision is deliberately introduced.

**Commit boundaries:** onboarding/report generation; small reviewed content batches; new behavior implementations separately from bulk numeric data.

## Verification and performance gates

These are evidence requirements for implementation sign-off once execution is permitted. They do not override the current no-tests/no-builds instruction.

- **Every behavior change:** focused regression cases, relevant formatting/lint/build checks, explicit compatibility/spec updates, and worker-count determinism scenarios.
- **Before transport sign-off:** command ordering/outcomes, ownership, initial-state reconstruction, snapshot continuation, replay boundaries, terminal and paused states.
- **Before multiplayer readiness:** independent server/client agreement, fault-injected delivery, reconnect/live-history boundaries, delegated control, compatibility rejection, and desync recovery.
- **Before bulk-content readiness:** catalog-only ordinary additions, dependency-complete behavior resolution, representative multi-attack/multi-ability/status interactions, and persistence of all added state.
- **Performance:** use existing 700-unit compatibility and larger traffic/projectile/status fixtures. Record hardware/build/configuration plus tick distributions, phase costs, memory, snapshot size/encode/restore time, catch-up throughput, and client frame cost separately. Unit definitions alone are not the scaling metric; live units, projectiles, effects, and content mix are.
- Optimize only measured bottlenecks. Candidates to inspect include repeated extraction/full-state allocation, per-tick checksum cost, dense effect candidate searches, presentation extraction, and animation/render costs. Do not replace ECS or navigation based on file size or historical worst-case probes alone.
- Add/update automated verification entry points and a worker/platform matrix where infrastructure permits. Refresh `docs/verification/README.md` so implemented features, pending checks, and measured results are clearly distinguished.

## Readiness decisions

- **Small additions with existing mechanics:** reasonable after steps 3–6 establish catalog, version selection, ownership, and command construction. Keep the main sequence moving toward restoration; do not add new persistent mechanics before step 7.
- **Minimal multiplayer prototype:** implemented after step 9; broader multiplayer readiness requires step 10 and the corresponding executed evidence.
- **Bulk roster expansion:** begin after the relevant step 11 primitives and step 12 onboarding process are ready. Full historical-version support remains a per-configuration coverage decision.

## Resume and completion notes

At the end of each implementation chunk, record the completed step/substep, commits, changed compatibility identities, static/executed verification evidence, pending checks, and next concrete action. Do not mark a step verified solely because code or fixtures exist.

The next action for Sol is **step 1: reconcile the checkout, inventory authoritative state, and fix checksum coverage in a narrow commit**. This planning handoff itself implements none of the runtime features above.
