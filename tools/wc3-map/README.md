# Warcraft III map extraction tooling

This directory contains the reproducible extraction pipeline for the original Castle Fight Warcraft III map. The goal is to preserve as much source evidence as possible in searchable, diffable text before translating it into Castle Fight Native content definitions.

## Run it

From the repository root, first cache the matching Warcraft III base object data from a local install, then regenerate the map extraction:

```sh
WC3_INSTALL=/mnt/gamessd_linux/Games/Warcraft3 tools/wc3-map/extract-base-data.sh
tools/wc3-map/extract.sh
```

`WC3_INSTALL` defaults to `/mnt/gamessd_linux/Games/Warcraft3`. The default map input is `docs/original_map/5329_Castle_Fight_DE_beta9.27_w3p.w3x` and committed output is regenerated under `docs/original_map/extracted/`.

The pipeline uses three ignored working locations:

- `.local-tools/` for source checkouts, builds, and locally built executables;
- `.map-work/` for extracted map members and intermediate translations;
- `.wc3-base/` for the small subset of locally installed Blizzard data needed to resolve inherited object fields and pathing masks.

None of these directories is required in git. `.local-tools/` and `.map-work/` are reproducible from the checked-in scripts/map. `.wc3-base/` must be repopulated from a Warcraft III install before inherited data can be regenerated.

## Source-built third-party tools

Only open-source tooling is fetched, pinned, license-checked, and built locally.

### StormLib

- project: `ladislav-zezula/StormLib`
- revision: `44ebfbfc109d76e2a85bbd5d8b0c949df7e65c6f`
- license: MIT
- bootstrap: `bootstrap-stormlib.sh`

StormLib is compiled from source with CMake/Ninja and bundled dependencies. `mpq_extract.cpp` is our small archive wrapper around StormLib.

### CascLib

- project: `ladislav-zezula/CascLib`
- revision: `2a280f5a231966dc5d1b534978dd9f9f04a374cd`
- license: MIT
- bootstrap: `bootstrap-casclib.sh`

CascLib is compiled from source with CMake/Ninja. `casc_extract.cpp` is our small read-only CASC wrapper. It is used only to copy the required base-game data out of the user's local Warcraft III installation into `.wc3-base/`; no CascLib binaries or Blizzard source data are committed.

### WC3MapTranslator

- project: `ChiefOfGxBxL/WC3MapTranslator`
- revision: `7d477ebb5cea445bee7915fe72cd93ee399252b7`
- license: MIT
- bootstrap: `bootstrap-translator.sh`

Its TypeScript source is installed from the pinned lockfile and compiled locally with `tsc`. It is used for the binary terrain, doodad-placement, and object-editor formats it understands.

WC3MapTranslator currently expects `war3map.w3i` format 33, while this map uses format 31. `decode_map.py` therefore parses this map's W3I directly. Its WTS path also misinterprets this map's UTF-8 strings, so `decode_map.py` parses `war3map.wts` itself and repairs equivalent UTF-8-as-Latin-1 text from translated object tables.

## Base-game data selection

This map records Warcraft III version `2.0.4.23745` and W3I `game_data_set_version = 0`. The matching installed build stores that custom-map balance set under:

```text
war3.w3mod:_balance\custom_v0.w3mod
```

`extract-base-data.sh` therefore caches the `custom_v0` unit/ability/item SLKs and profile files, plus the standard editor metadata, en-US editor/game strings, doodad/destructable data, and all 207 standard pathing TGAs. It intentionally does not copy the whole `war3.w3mod:units` asset tree: only the data tables needed for inheritance are retained, leaving the ignored cache at roughly 11 MiB instead of pulling hundreds of MiB of presentation assets.

`resolve-base-data.py` merges those sources according to the editor metadata (`UnitMetaData.slk`, `AbilityMetaData.slk`, etc.) and emits a manifest with the exact local source-file hashes used for the committed resolution.

## Protected archive behavior

The source map is W3P-protected. It has a normal Warcraft III `HM3W` wrapper with the MPQ archive beginning at byte `0x200`, but its MPQ file table has been deliberately poisoned with tens of thousands of anonymous pseudo-files.

The normal extraction path therefore opens only canonical Warcraft III map-member names by MPQ hash. This recovers the useful map data without trusting the sabotaged file listing.

`mpq_extract` has an explicit `--scan-unknown` mode for forensic work. Do not use it casually: on this 15.9 MB map the poisoned table expanded into roughly 2.4 GB of overlapping/garbage output during investigation. The scan is intentionally not part of `extract.sh`.

## Generated plaintext

`docs/original_map/extracted/` contains:

- `summary.json` — concise source/archive/content inventory;
- `archive-extract.tsv`, `archive-members.json` — recovered canonical members and hashes;
- `map-info.json` — W3I map metadata, players, forces, bounds, environment, mode data;
- `terrain.json` — W3E terrain dimensions, tile palette, heights, textures, cliff data;
- `pathing.json`, `pathing-grid.hex.txt`, `walkability.txt`, `buildability.txt` — WPM navigation/buildability data;
- `shadow.json`, `shadow-grid.hex.txt` — SHD shadow data;
- `doodads.json` — placed doodads/destructables from `war3map.doo`;
- `minimap-icons.json` — MMP minimap markers;
- `strings.json`, `war3map.wts` — UTF-8 trigger strings/tooltips;
- `obj-*.json` — decoded object-editor override tables;
- `catalog/object-fields.tsv` — every map object modification in file order, including duplicate/conflict metadata;
- `catalog/objects.tsv` — compact map-override object index;
- `catalog/units.tsv`, `catalog/buildings.tsv`, `catalog/abilities.tsv` — map-override convenience views only;
- `resolved/object-fields.tsv` — full base + map effective editor fields, with base value, every map candidate, last-write value, recovered value, metadata label/source, and protection-conflict provenance;
- `resolved/base-source-fields.tsv` — inherited SLK source columns for every map object, including computed/non-editor fields such as `realHP`, min/max damage, and source DPS;
- `resolved/units.tsv` — normalized **static object-data** unit/building stats including HP/mana/regen, armor, movement, collision, acquisition, abilities, both attacks, damage ranges, cooldowns, AoE and derived DPS; W3P poisons many spawned-unit combat fields even without duplicate modifications, so use the effective runtime catalog below for covered production units;
- `resolved/effective-unit-stats.tsv` — 162 building→spawned-unit runtime/effective catalog rows from generated Lua, joined to static unit values for HP, armor, DPS, attack range, and movement speed with per-field comparison labels;
- `resolved/buildings.tsv` — resolved building stats plus exact pathing texture dimensions and per-cell walk/fly/build masks;
- `resolved/abilities.tsv` — one row per statically resolved ability level with object-data costs/cooldowns/range/area/targets and labeled Data fields; for W3P-protected cooldown/mana values, cross-check the runtime table below;
- `resolved/protected-ability-fields.tsv` — the generated Lua protection table joined back to resolved static ability data, preserving runtime cooldown/mana values beside the static values and an explicit `static-match`/`static-differs` comparison;
- `resolved/protected-ability-jass-add-restores.tsv` — the smaller hard-coded restore path used when abilities are added through the JASS-compatible wrapper, cross-checked both against the canonical protected table and static object data;
- `resolved/placed-doodads.tsv` — all 1,780 placed doodads/destructables joined to base/custom names and exact pathing masks;
- `resolved/pathing-textures.json` — decoded standard TGA pathing masks; each pixel is one 32-world-unit pathing cell;
- `resolved/protection-conflicts.tsv` — the small set of repeated W3P modifications whose candidate values disagree;
- `resolved/base-data-manifest.json`, `resolved/summary.json` — exact local base-data hashes/build identity and resolution coverage;
- `script/war3map.lua` — verbatim protected runtime Lua;
- `script/functions.tsv`, `script/function-spans.tsv`, `script/readable-function-names.txt` — Lua-aware named-function indexes with true source byte offsets/spans;
- `script/calls.tsv`, `script/call-graph.tsv` — lexical call counts plus caller→callee edges, excluding function declarations and tokens hidden inside strings/comments;
- `script/function-aliases.tsv` — exact simple named-function assignments used heavily by Wurst's generated prototype/virtual-dispatch tables; `slot-name-prefix` marks the structural case where the slot name itself prefixes the assigned implementation name, while `assigned-other-target` preserves inherited/alternate assignments without interpreting them;
- `script/function-value-arguments.tsv` — named functions passed as values to another call (for this protected script, currently the generated `xpcall` initialization layer), indexed separately rather than pretending the value reference is an ordinary direct call;
- `script/rawcode-summary.tsv`, `script/rawcode-reference-sites.tsv`, `script/function-rawcodes.tsv` — direct decimal rawcode references cross-linked back to map object IDs/categories/names and the exact function/call context where they occur;
- `script/runtime-mutators.tsv` — calls that can mutate unit/ability object state at runtime (`BlzSetUnit*`, `BlzSetAbility*`, unit ability add/remove/level, movement/state setters), annotated with map rawcodes directly referenced by the same function;
- `script/protected-ability-fields.tsv` — 452 structurally parsed cooldown/mana assignments from the protected `xD` initializer, covering 259 ability-level rows across 254 abilities; overlapping JASS-add restores are required to agree exactly;
- `script/protected-ability-jass-add-restores.tsv` — 17 explicit guarded restores from `applyProtectedAbilityFieldsForJassAdd`, including the one JASS-only assignment whose ordinary static value is already correct;
- `script/effective-unit-stats.tsv` — 162 structurally parsed rows from generated initializer `xO`, keyed by production building and spawned unit, with effective HP/armor/DPS/range/speed/spawns-per-cycle/air-target capability;
- `script/rawcode-mutator-traces.tsv` — deterministic shortest exact-named-call paths from a function that directly references a map rawcode to a function containing a runtime mutator, including hop count and the full lexical call path;
- `script/rawcode-mutator-summary.tsv` — compact per-rawcode coverage of those static mutation paths, including direct-vs-indirect evidence and reachable mutator kinds/sites;
- `war3mapMisc.txt`, `war3mapSkin.txt` — already-plaintext map configuration.

The ASCII pathing maps use `#` for blocked and `.` for allowed. WPM bit meanings currently decoded are `0x02` no-walk, `0x04` no-fly, `0x08` no-build, `0x20` blight, `0x40` no-water, with unknown/unused bits retained in the hex grid and histogram.

## Important interpretation limits

The inherited object-data gap is now resolved against the exact installed `2.0.4.23745` custom-V0 data set. `resolved/object-fields.tsv` is the complete editor-field view for map objects, while `resolved/base-source-fields.tsv` deliberately also retains source-table columns that do not map one-to-one to editable fields.

W3P protection creates one remaining object-data ambiguity: 29 unit definitions contain repeated field modifications, and six objects have 14 fields whose repeated values actually disagree. Most conflicting numeric fields have a plausible authored value followed by repeated `0`/`1` sentinels (for example Mortar HP `280,1,1`). The resolver records every candidate and normal last-write result, then uses a narrowly labeled recovery rule for convenience tables: a first numeric magnitude greater than one followed only by zero/one sentinels is recovered from the first value. Conflicting strings are not guessed and retain last-write semantics. `resolved/protection-conflicts.tsv` makes every such choice auditable.

Building and static-doodad footprints no longer rely on filename hints. The resolver decodes the actual Blizzard pathing TGAs. TGA red/green/blue channels become unwalkable/unflyable/unbuildable bits respectively, and each TGA pixel is a 32-world-unit pathing cell. Irregular footprints are kept cell-for-cell as hexadecimal bit rows.

The W3P runtime script remains obfuscated/minified. Its source is retained verbatim and thousands of Wurst-generated function names are still searchable, but encrypted string constants and higher-level control flow have not yet been semantically deobfuscated. The script indexer is deliberately lexical: it skips strings/comments, understands named/method/anonymous function scope, records exact byte offsets, and never executes map code. Direct same-function rawcode↔setter links are useful evidence. `rawcode-mutator-traces.tsv` additionally follows only exact calls whose callee resolves to a named function in the same script and keeps one deterministic shortest path; it does **not** infer function-value/callback targets, prove that a branch executes, or prove that the rawcode is passed as an argument to the eventual setter. `function-aliases.tsv` separately recovers exact Wurst dispatch/prototype assignments, but an alias assignment still does not prove which virtual receiver is instantiated or invoked at runtime; the `slot-name-prefix` label is only a structural naming relation, not a semantic confidence score. These indexes are therefore static evidence, not execution claims.

One important protection layer is stronger than generic reachability evidence: the generated `xD` initializer explicitly constructs the table consumed by `applyProtectedAbilityFields`, while `checkProtectedAbilityFieldsOnUnit` and `checkAllProtectedAbilityAdds` compare live Warcraft ability fields against that same table. The extractor structurally parses 452 cooldown/mana assignments from it. On this map every one of those 452 values differs from the statically resolved object-data value, commonly the protection sentinels `99` cooldown and `9999` mana. The runtime values are therefore retained separately in `resolved/protected-ability-fields.tsv`; the resolver does not erase or rewrite the static object evidence.

Spawned-unit combat data has a similar but broader protection layer. Generated initializer `xO` contains one complete effective-stat row for each of 162 production buildings and 162 distinct spawned unit IDs, and `applyEffectiveUnitStat` copies those rows into the building catalog. Static object data disagrees with the effective table for 154/162 HP values, 145/162 armor values, 156/160 comparable DPS values, and 100/161 comparable attack ranges; movement speed is mostly intact (155/162 exact matches). For example Faerie Dragon `e000` is statically HP `1` / armor `0` / range `1`, while the effective table specifies HP `460` / armor `2` / range `350`. These are not duplicate-modification conflicts, so `resolved/effective-unit-stats.tsv` is the preferred source for the covered production-unit combat summary while `resolved/units.tsv` remains the auditable static object-data view.

The absence of a canonical member such as `war3mapUnits.doo`, `war3map.w3q`, or `war3map.imp` means it could not be opened under that standard name in this protected archive. Do not infer from that alone that the original editor project never contained equivalent data; protected-map packaging can remove editor-only sources and hide imported assets.

## Tests

```sh
python -m unittest tools/wc3-map/test_decode.py tools/wc3-map/test_resolve_base.py
python -m py_compile tools/wc3-map/decode_map.py tools/wc3-map/lua_index.py tools/wc3-map/resolve-base-data.py
```

A full end-to-end validation is `tools/wc3-map/extract-base-data.sh` followed by `tools/wc3-map/extract.sh` and inspection of both `summary.json` and `resolved/summary.json`. The current resolved summary requires zero unresolved inheritance anchors, missing referenced pathing textures, or unknown placed doodad rawcodes.
