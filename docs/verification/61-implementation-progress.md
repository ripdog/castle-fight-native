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
- `tools/cargo-interactive test -p castle-fight-sim`: 179 passed;
- `tools/cargo-interactive clippy -p castle-fight-sim --all-targets -- -D warnings`: passed;
- `cargo fmt --all -- --check`: passed.

Pending:

- the separate in-progress extractor work in the source checkout is intentionally not folded into 9.27/r1 or labeled as a 9.32 extraction;
- a reviewed 9.32 extraction must receive its own retained extraction tree/revision before tooling or runtime content may consume it;
- steps 3–4 will migrate ordinary runtime `include_str!` consumers away from the legacy 9.27 working alias and onto resolved bundle inputs.

## Next action

Step 3: build the immutable canonical content catalog and compatible behavior resolver for the current 9.27 development subset, then replace the Step 1 temporary immutable-configuration checksum identity with the resolved gameplay bundle identity.
