# Original Castle Fight map evidence

This directory keeps the original Warcraft III map used as compatibility evidence and a reproducible plaintext extraction of its gameplay/map data.

Source map:

`5329_Castle_Fight_DE_beta9.27_w3p.w3x`

SHA-256: `9e3519bbc2a0fb6145460dd8b5730e9a35b2fd5923d63404eaf31e3618208a88`

The map identifies itself as **Castle Fight DE Beta 9.27**, authored by **Frotty**. Its map-info format is 31 and records Warcraft III version `2.0.4.23745`.

Do not edit files under `extracted/` by hand. The full inherited-data extraction uses the matching local Warcraft III install and is regenerated with:

```sh
WC3_INSTALL=/mnt/gamessd_linux/Games/Warcraft3 tools/wc3-map/extract-base-data.sh
tools/wc3-map/extract.sh
```

See `tools/wc3-map/README.md` for tool provenance, build instructions, archive-protection details, format caveats, and the meaning of each generated file.

## What is already recoverable

The first extraction recovers substantial compatibility data:

- total terrain: **132 × 64** Warcraft terrain tiles;
- playable area: **100 × 56** tiles;
- pathing/buildability grid: **528 × 256** cells;
- six configured player slots split across two forces, with the three western starts near `x=-4352` and three eastern starts near `x=4352`;
- **1,780** placed doodads/destructables, now joined to inherited names and pathing footprints;
- **585** custom unit/building object definitions plus 28 modified standard units;
- **709** custom abilities plus 14 modified standard abilities;
- **96** custom buffs, **16** custom items, **26** custom destructables, and four custom doodad definitions;
- map-specific gameplay constants from `war3mapMisc.txt`, including damage-type/armor multipliers;
- human-readable names and tooltips describing production rates, health, attacks, armor, spell effects, cooldowns, mana use, special targeting behavior, and legendary mechanics;
- the complete **11.8 MB** W3P-protected runtime Lua plus Lua-aware indexes of **6,879** named function definitions, **5,723** distinct lexical call targets, and **29,478** caller→callee edges;
- **12,611** direct script references covering **1,332** of the map's **1,461** distinct object rawcodes, plus **157** runtime unit/ability mutation call sites; exact named-call propagation links **117** of those mutator sites to **270** rawcodes through **6,728** auditable shortest-path traces;
- a structurally recovered protected-ability runtime table containing **452** cooldown/mana assignments across **259** ability-level rows and **254** abilities, plus **17** explicit JASS-add restores;
- a generated **162-row** building→spawned-unit effective combat catalog containing HP, armor, DPS, attack range, move speed, spawn count, and air-target capability for 162 distinct production units;
- **57,253** exact named-function alias assignments from Wurst's generated dispatch/prototype tables, including **2,801** structurally self-named slot→implementation mappings, plus **308** named function-value arguments from the generated initialization layer;
- a full inheritance merge against Warcraft III `2.0.4.23745` custom data set V0: **148,765** resolved editor-field rows and **184,817** inherited source-table field rows across all 1,487 map object definitions;
- all **207** standard pathing textures decoded to exact 32-world-unit pathing cells, with every unit/building and placed doodad pathing reference resolved;
- normalized static unit/object tables, a separate runtime/effective production-unit combat table, and **325** building-like definitions with exact footprint masks rather than filename size guesses.

Examples visible directly in the extracted catalogs include Footman, Mortar, Sniper, Faerie Dragon, their production buildings, and legendary/utility buildings such as Shrine of Destruction, Eraser, Snowveil Fountain, and Well of Pain. These tooltips already expose many mechanics that will eventually become compatibility fixtures rather than guessed native behavior.

## Evidence quality

Treat this directory as reverse-engineering evidence, not yet as final native content.

The terrain/pathing/doodad placement files are direct decodes of authoritative map data. The original `catalog/` files remain map-override views. The newer `extracted/resolved/` tree fills inherited fields from the exact installed Warcraft III build and W3I-selected `custom_v0` balance set. `resolved/object-fields.tsv` records base value, all map candidates, last-write value, recovered value, editor metadata, and source table/field so a later importer does not have to infer inheritance again. `resolved/base-data-manifest.json` hashes every ignored local base file used to produce that committed merge.

Physical geometry is likewise explicit now. `resolved/buildings.tsv` contains exact pathing-mask dimensions and cells; `resolved/pathing-textures.json` retains the masks themselves; and `resolved/placed-doodads.tsv` joins all 1,780 placed objects to their inherited pathing texture and object name. Ordinary moving-unit physical size remains represented by the unit collision field rather than a building-style pathing mask.

The W3P object files contain 14 conflicting repeated fields across six objects. These are not hidden: `resolved/protection-conflicts.tsv` preserves every value and the resolution reason. Convenience catalogs recover only the observed numeric `value → 0/1 → 0/1` duplicate-sentinel pattern; ambiguous string changes retain ordinary last-write semantics. This is only one protection mechanism: many spawned-unit fields are poisoned by single map values and therefore cannot be repaired from object-data duplication alone.

The runtime Lua remains the strongest source for custom mechanics implemented in script and for values mutated after map load. It is W3P-obfuscated, so the extraction preserves the original code without executing it and lexes the minified source into exact function spans, caller→callee edges, rawcode reference sites, runtime unit/ability mutator sites, and Wurst's generated function-alias/dispatch assignments. `script/rawcode-mutator-traces.tsv` follows exact calls to named functions and records one deterministic shortest path from direct rawcode evidence to reachable mutator sites. `script/function-aliases.tsv` separately records exact slot assignments; the `slot-name-prefix` classification is only a structural clue that makes concrete-looking generated handlers easy to find, while inherited/alternate assignments are retained as `assigned-other-target`. Neither a call trace nor an alias proves argument flow, branch execution, virtual receiver selection, or callback execution. Conversely, absence of a trace is not proof that a runtime relationship does not exist.

The protected ability-field table is stronger evidence than generic reachability. The generated Lua constructs explicit expected cooldown/mana rows, applies them through `applyProtectedAbilityFields`, and contains verification helpers that compare live Warcraft ability fields against those values. `resolved/protected-ability-fields.tsv` joins all **452** assignments back to static resolved object data; all **452** differ from the static values, which are commonly W3P sentinels such as cooldown `99` and mana `9999`. For example Rescue Strike `A005` resolves statically to `99/9999` but the protected runtime table specifies cooldown **60** and mana **0**; Snowfall `A0HO` specifies **1** and **15**. The extractor preserves both sources rather than silently replacing the static object data.

Spawned-unit combat stats have a similarly authoritative generated table. Function `xO` creates one `UnitEffectiveStat` row for each of **162** production buildings and **162** unique spawned unit IDs, and `applyEffectiveUnitStat` copies those values into the building catalog. `resolved/effective-unit-stats.tsv` compares that table against static object data. Static values disagree for **154/162 HP**, **145/162 armor**, **156/160 comparable DPS**, and **100/161 comparable attack-range** values; movement speed is mostly preserved (**155/162** exact matches). Faerie Dragon `e000`, for example, is statically HP `1`, armor `0`, and range `1`, while the effective table specifies HP **460**, armor **2**, and range **350**. For those covered production units, the effective table is the preferred combat-summary source; the static tables remain useful provenance. Higher-level semantic/data-flow deobfuscation remains a separate next phase.

When compatibility conclusions are promoted into Castle Fight Native, record whether each value/behavior is directly verified from these files, inferred by cross-referencing several sources, observed in Warcraft III, or an intentional divergence. That matches the provenance policy in `docs/spec/40-content-data.md`.
