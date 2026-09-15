# Implementation Plan 61 — Progress Ledger

This ledger records completed implementation-plan steps, compatibility identity changes, executed verification, and the next concrete action. It supplements `docs/spec/61-implementation-plan.md`; a step is not considered verified merely because its code exists.

## Step 1 — Authoritative-state inventory and checksum coverage

Status: **implemented and verified**

Merged commits:

- `3fbc749` — `docs: inventory authoritative simulation state`
- `aa7be0c` — `fix(sim): cover authoritative checksum state`

Implemented:

- added `docs/spec/22-authoritative-state-inventory.md` as the field-level authoritative/immutable/derived/presentation inventory used by checksum and future snapshot work;
- checksum schema advanced to revision 2 and now includes deterministic allocator state, immutable simulation configuration/combat-rule identity (including match seed), live content rawcodes, and audited optional-component presence;
- malformed `SimId`-bearing authoritative entity shapes can no longer disappear silently from checksum projection;
- worker count, derived navigation caches, presentation events, and diagnostic timings remain explicitly outside canonical identity.

Compatibility identity changes:

- canonical checksum schema is now `CANONICAL_CHECKSUM_SCHEMA_VERSION = 2`; prototype-schema checksums are intentionally incompatible.

Executed verification:

- focused checksum regression suite: 5 passed;
- full `castle-fight-sim` suite after Step 1: 178 passed;
- `tools/cargo-interactive clippy -p castle-fight-sim --all-targets -- -D warnings`: passed.

Pending:

- step 3 replaces the temporary canonical encoding of immutable simulation/content inputs with the resolved content-bundle identity;
- step 7 must use the authoritative-state inventory as its restore-coverage checklist.

## Step 2 — Retained extraction history and release manifests

Status: **implemented and verified**

Step commits:

- `341b782` — `content: register retained Castle Fight releases`
- `7e973ea` — `tools: make map extraction revision-aware`

Implemented:

- added `docs/original_map/releases.json` schema 1 with exact `(map_version, revision)` identities;
- registered 9.27/r1 as the retained development-subset baseline, including source archive digest, declared Warcraft build, base-data manifest digest, exact retained extraction Git tree, extractor revision, runtime-content source revision, tuning digest, and binding-registry digest;
- retained the verified 9.32/r1 source archive at `docs/original_map/releases/9.32/r1/source.w3x` with SHA-256 `4181d8aecc3bfe15f66fa071d079ea62deda1f663f5e19f5dc34edd7401b738c` after checking its internal W3I identity as Castle Fight DE Beta 9.32 / Frotty / format 31 / Warcraft 2.0.4.23745;
- 9.32/r1 is deliberately `archived`, `extraction=pending`, `runtime=unsupported`; source retention does not advertise playable compatibility;
- retained large 9.27 extraction data by immutable Git tree identity rather than duplicating the generated tree on disk; its existing `docs/original_map/extracted` path remains a working alias;
- added release-manifest validation/resolution tooling with exact-revision semantics and source/content identity verification;
- added `extract-release.sh`; retained revisions cannot be selected as extraction targets, and `extract.sh` refuses non-empty output destinations by default;
- `native_effect_coverage.py` now resolves its extraction from the release registry and refuses to use 9.27 extraction for 9.32;
- added `MapVersion::CASTLE_FIGHT_9_32` as a stable version identity without claiming content support.

Compatibility/content identity changes:

- release identity is now the exact `(map_version, revision)` pair plus recorded source/extraction/content identities;
- 9.27/r1 runtime content remains explicitly `supported-development-subset`;
- 9.32/r1 is archive-only and has no runtime content revision.

Executed verification:

- `python -m unittest tools/wc3-map/test_release_manifest.py tools/wc3-map/test_native_effect_coverage.py`: 7 passed;
- `python tools/wc3-map/release_manifest.py verify`: 9.27/r1 and 9.32/r1 both verified;
- revision guard rejects extraction into retained 9.27/r1 and resolves pending 9.32/r1 to its own destination;
- `native_effect_coverage.py --map-version 9.27` resolves the retained 9.27 extraction; `--map-version 9.32` rejects because no retained 9.32 extraction exists;
- `bash -n tools/wc3-map/extract.sh tools/wc3-map/extract-release.sh`: passed;
- after rebasing onto the Defender/production-upgrade work, `tools/cargo-interactive test -p castle-fight-sim`: 187 passed;
- `tools/cargo-interactive clippy -p castle-fight-sim --all-targets -- -D warnings`: passed after a separate narrow cleanup of two Defender-era lint warnings (`e91c5fa`);
- `cargo fmt --all -- --check`: passed.

Pending:

- the separate in-progress extractor work in the source checkout is intentionally not folded into 9.27/r1 or labeled as a 9.32 extraction;
- a reviewed 9.32 extraction must receive its own retained extraction tree/revision before tooling or runtime content may consume it;
- step 4 will move shared match construction and remaining ordinary gameplay defaults behind the selected resolved bundle.

## Step 3 — Canonical content catalog and compatible behavior resolver

Status: **implemented and verified**

Step commits:

- `f1b5e64` — `content: resolve native effect dependencies`
- `9392643` — `content: build immutable Castle Fight catalog`
- `f950297` — `sim: consume resolved content definitions`
- `7320bb1` — `content: pin 9.27 r2 compatibility identity`

Implemented:

- added stable explicit unit, building, builder, and ability IDs that do not depend on enum position, rawcode ordering, file order, or map iteration order;
- added an immutable version-scoped `CastleFightContentBundle` with explicit `archived`, `supported-development-subset`, and `supported-full` availability states; 9.27 remains the supported development subset while 9.32 remains archived-only;
- migrated current unit/building/tower/builder numeric definitions onto indexed retained 9.27 extraction inputs, loaded once rather than rescanning TSV data on ordinary lookups;
- separated broad extraction ingestion from executable-content promotion: trustworthy rows outside the current native slice may remain indexed even when fields or mechanics are unsupported, while promoted definitions require every native primitive field they consume;
- added a generated 9.27/r1 runtime-catalog supplement for object fields such as repair times and command-card positions; its generator reads the immutable retained extraction Git tree and tests cross-check the generated snapshot rather than requiring hand-copied per-unit numbers;
- pinned the runtime catalog to the retained 9.27/r1 extraction tree and a deterministic aggregate source-evidence hash so a changed working extraction alias cannot silently redefine the named content revision;
- extended native-effect resolution to typed source identities, exact version-valid binding selection, tuning compatibility checks, indirect dependency traversal, stable missing/ambiguous/incompatible/cycle errors, and explicit no-runtime bindings;
- derive required behavior roots from extracted abilities on every currently playable unit, production building, and tower; the current resolved bundle contains 12 stable behavior bindings including indirect Chain Lightning/Entangling Roots dependencies and the tower `A09A` range-display helper as an explicit no-runtime behavior;
- made per-unit native mechanics depend on the unit's extracted ability inventory rather than tuning-file row order; shared rawcodes reuse the same verified tuning and implementation;
- introduced `ResolvedUnitDefinition` and `Simulation::spawn_resolved_unit`; direct spawning and production now consume the same template, gameplay properties, passive effects, and spellcasting definition;
- canonical gameplay identity now hashes the resolved definitions, rules, stable IDs, behavior mappings, and implementation/tuning-sensitive gameplay data in stable order, excluding cosmetic assets;
- simulation configuration identity now includes an optional gameplay-bundle identity; checksum schema is `CANONICAL_CHECKSUM_SCHEMA_VERSION = 4`, and otherwise-identical simulations with different gameplay bundle identities do not share an authoritative checksum;
- registered runtime content revision `cf-native-dev-slice-r2` in `releases.json`, pinned to the immutable catalog source commit and current tuning/binding digests.

Compatibility/content identity changes:

- 9.27/r1 runtime content revision: `cf-native-dev-slice-r2`;
- content-bundle schema: `1`;
- canonical 9.27/r2 gameplay hash: `0x4ff93f04c1803f1e`;
- canonical checksum schema: `4`;
- tuning SHA-256: `0bf1e9716b9ba69daee16c6fb3269ffde1c9fbd8234039105123c8fcac19cff0`;
- native binding-registry SHA-256: `c2ed1cdaea00319062b58934f26e00f328f76c070fad135771e1e77450435b37`.

Executed verification:

- catalog-focused tests after broad-ingestion fixes: 20 passed;
- native-effect resolver/order/dependency tests: 10 passed;
- full `tools/cargo-interactive test -p castle-fight-sim` after rebasing onto then-current master: 205 passed;
- `tools/cargo-interactive clippy -p castle-fight-sim --all-targets -- -D warnings`: passed;
- `tools/cargo-interactive check -p castle-fight-client -p castle-fight-debug-viewer -p castle-fight-sim-bench`: passed;
- `python -m unittest tools/wc3-map/test_release_manifest.py tools/wc3-map/test_native_effect_coverage.py tools/wc3-map/test_build_runtime_catalog.py`: 9 passed;
- `python tools/wc3-map/release_manifest.py verify`: both 9.27/r1 and 9.32/r1 verified after the r2 metadata update;
- fixed gameplay-hash compatibility fixture: passed;
- `cargo fmt --all` and `git diff --check`: passed.

Pending:

- the catalog deliberately does not promote every extracted 9.27 object; new units/buildings become executable only after assigning stable IDs and proving complete reachable native behavior coverage;
- protected/static object rows that disagree with recovered runtime behavior remain extraction-audit work rather than being silently promoted as gameplay truth;
- step 4 must make ordinary match construction select and carry this bundle instead of independently selecting default-version content at callsites.

## Step 4 — Shared version-selected match initialization

Status: **implemented and verified**

Step commits:

- `63e8042` — `sim: add version-selected match bootstrap`
- `97a08ee` — `client: consume selected match content`
- `3bc0ede` — `docs: define version-selected match setup`
- `6ad8738` — `client: keep selected content in icon action panel`
- `0bc6d44` — `client: load UI icons for selected map version`
- `1f42a3f` — `client: bind cursor validation to selected content`

Supporting cleanup:

- `334b0a7` — `style(client): satisfy current clippy`
- `ed782f4` — `style(client): simplify animation system parameters`
- `b9ab3c8` — `style(sim): apply current rustfmt to catalog helper`
- `fdfcb58` — `style(client): satisfy inspector clippy`

Implemented:

- added shared `CastleFightMatchConfig`, release descriptors, resolved-match state, and deterministic `create_castle_fight_match`; authoritative match setup no longer lives in the Bevy demo client;
- match selection is pinned to the exact `(map_version, release_revision, content_revision, gameplay bundle identity)` and rejects unregistered, archived-only, stale-content, or mismatched selections before constructing gameplay state;
- moved 9.27 authoritative map bounds, build regions, targetless lane, castle placement/stats, terrain elevation, doodad/pathing blockers, economy, damage rules, participant builders, and match seed behind shared match construction;
- pinned the retained 9.27/r1 terrain and placed-doodad evidence with a versioned map-source manifest and checkout-line-ending-stable hashes;
- runtime release descriptors are tested against `docs/original_map/releases.json` so retained-history metadata and the typed playable selector cannot silently diverge;
- client startup accepts exact `--map-version` / `--map-revision` selection and can list registered releases with their availability; the window title exposes the selected release/content revision rather than an implicit default;
- the client consumes the resolved match terrain source and simulation config from shared setup while keeping camera/render-only bounds in presentation code;
- build menus, production upgrades, tooltips, stress-unit spawning, model selection, building/unit rawcodes, command-card layout, and WC3 UI icon loading now consume the selected immutable content bundle instead of maintaining the former hardcoded seven-building/current-unit/default-version paths;
- generated unit/building model loaders receive only rawcodes reachable from the selected bundle, preserving lazy/fallback behavior without eagerly loading unrelated retained content;
- headless and Bevy-client bootstrap paths share the same authoritative initializer; the client adds only presentation/stress-fixture state afterward.

Availability behavior:

- Castle Fight 9.27/r1 remains explicitly `supported-development-subset`, not a claim of complete historical 9.27 support;
- Castle Fight 9.32/r1 remains visible as `archived` and is rejected as non-playable rather than silently substituting 9.27 content;
- unknown release revisions are rejected exactly; no nearest/latest revision fallback is used.

Executed verification after rebasing onto the then-current master:

- `tools/cargo-interactive test -p castle-fight-sim`: **214 passed**;
- `tools/cargo-interactive test -p castle-fight-client`: **105 passed** after rebasing over Warcraft cursor presentation;
- match-setup focused suite: 6 passed, including worker-independent initial checksums, retained-registry parity, stale bundle rejection, map-source pinning, and topology preservation;
- selected UI-icon suite: 3 passed;
- `tools/cargo-interactive clippy -p castle-fight-sim -p castle-fight-client --all-targets -- -D warnings`: passed;
- `tools/cargo-interactive check -p castle-fight-debug-viewer -p castle-fight-sim-bench`: passed;
- `python tools/wc3-map/release_manifest.py verify`: 9.27/r1 and 9.32/r1 both verified;
- `cargo fmt --all -- --check` and `git diff --check`: passed;
- client regression `client_and_headless_bootstrap_share_the_same_authoritative_initial_state`: passed.

Pending:

- Step 5 must separate stable player identity/ownership from team allegiance, move economy from team-indexed slots to player records, and add canonical match lifecycle/outcome state before command/network work builds on top of it.

## Step 5 — Player ownership and match lifecycle

Status: **implemented and verified**

Step commits:

- `e4c03dc` — `sim: separate players from teams`
- `fc456c5` — `client: respect player ownership and colors`
- `9db246a` — `assets: resolve team glow by player slot`
- `885a0a4` — `docs: define player ownership and lifecycle`

Implemented:

- added stable `PlayerId` ownership independently from `Team`; units, builders, production buildings, produced units, construction/refund paths, upgrades, corpses, and presentation samples retain the owning player while team remains the allegiance/objective/build-region axis;
- mapped Castle Fight 9.27 participants to the authored Warcraft III slots: Western `0/1/2`, Eastern `6/7/8`, with fixed builder starts at Y `+128/0/-128`; supported development rosters are the balanced authored 1v1, 2v2, and 3v3 prefixes;
- replaced team-indexed economy state with canonical per-player resource/income records and enforced one builder per active player; allied purchases, refunds, income, and production remain charged/credited to the actual owner;
- separated ownership from control permission: connected owners control their own builders/buildings, a disconnected owner's builder may be delegated to a connected teammate, ownership/resources never transfer, and allied buildings remain non-delegated;
- registered each side's Main Castle as an explicit authoritative team objective while retaining map ownership by the first authored slot (Western player 0, Eastern player 6);
- added canonical running, team-disconnect pause, and terminal match lifecycle state; objective loss is evaluated after structural death resolution, simultaneous castle destruction is a draw, terminal state rejects later connection-state mutation and freezes gameplay/tick advancement;
- advanced the canonical checksum schema to revision 5, covering player records/resources/connections, entity ownership, objective identities, lifecycle/outcome state, and corpse owner provenance;
- migrated client action permissions/resource display to the selected local `PlayerId`; inspection can still view allies/opponents while normal command surfaces appear only for controllable actors;
- player colour is now keyed by Warcraft III owner slot rather than `Team` throughout imported model tint, fallback materials, health bars, corpses/remnants, debug presentation, and resource UI;
- removed the remaining two-colour Team Glow assumption: runtime selects `TeamGlowNN` from owner slot and the WC3 asset exporter emits all 24 modern Team Glow textures for packs containing ReplaceableId 2.

Compatibility/state changes:

- canonical checksum schema: `CANONICAL_CHECKSUM_SCHEMA_VERSION = 5`;
- default development 1v1 is authored slot `0` versus slot `6`, not a dense native player `0` versus `1`;
- player colour identity is presentation keyed by `PlayerId`; `Team` no longer doubles as player colour identity.

Executed verification:

- `tools/cargo-interactive test -p castle-fight-client`: **105 passed**;
- `tools/cargo-interactive test -p castle-fight-sim`: **220 passed**;
- `tools/cargo-interactive test -p castle-fight-wc3-assets`: **42 passed**;
- Step 5 match-setup suite: **12 passed**, including 2v2 independent economies/ownership, produced-unit ownership, authored slot validation, disconnected-builder delegation, pause/resume, terminal victory/draw, and final-state freezing;
- `tools/cargo-interactive clippy -p castle-fight-sim -p castle-fight-client -p castle-fight-wc3-assets --all-targets -- -D warnings`: passed;
- `tools/cargo-interactive check -p castle-fight-debug-viewer -p castle-fight-sim-bench`: passed;
- live WC3 CASC integration probe using Catapult (`o001`) exported `TeamGlow00` through `TeamGlow23` with exactly 24 glow textures, including `TeamGlow06` used by the default Eastern player slot;
- `cargo fmt --all` and `git diff --check`: passed.

Pending:

- actual connection events are still external to the simulation; Step 6/10 will represent their gameplay effects through the canonical control/command stream rather than direct runtime calls;
- generated local WC3 unit/building packs must be regenerated after this Step 5 code is merged so existing two-colour packs gain the complete Team Glow set.

## Next action

Step 6: define canonical typed player commands and finalized tick inputs, introduce the local match driver, and migrate all ordinary client input away from direct simulation mutation.
