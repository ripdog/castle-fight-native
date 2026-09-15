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

## Next action

Step 4: move deterministic match initialization out of the client, construct matches from an explicit selected release/content bundle, and drive gameplay defaults/catalog-facing UI from that one resolved selection.
