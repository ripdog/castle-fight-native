# Original Castle Fight map evidence

This directory keeps the original Warcraft III map used as compatibility evidence and a reproducible plaintext extraction of its gameplay/map data.

Source map:

`5329_Castle_Fight_DE_beta9.27_w3p.w3x`

SHA-256: `9e3519bbc2a0fb6145460dd8b5730e9a35b2fd5923d63404eaf31e3618208a88`

The map identifies itself as **Castle Fight DE Beta 9.27**, authored by **Frotty**. Its map-info format is 31 and records Warcraft III version `2.0.4.23745`.

Do not edit files under `extracted/` by hand. Regenerate them with:

```sh
tools/wc3-map/extract.sh
```

See `tools/wc3-map/README.md` for tool provenance, build instructions, archive-protection details, format caveats, and the meaning of each generated file.

## What is already recoverable

The first extraction recovers substantial compatibility data:

- total terrain: **132 × 64** Warcraft terrain tiles;
- playable area: **100 × 56** tiles;
- pathing/buildability grid: **528 × 256** cells;
- six configured player slots split across two forces, with the three western starts near `x=-4352` and three eastern starts near `x=4352`;
- **1,780** placed doodads/destructables;
- **585** custom unit/building object definitions plus 28 modified standard units;
- **709** custom abilities plus 14 modified standard abilities;
- **96** custom buffs, **16** custom items, **26** custom destructables, and four custom doodad definitions;
- map-specific gameplay constants from `war3mapMisc.txt`, including damage-type/armor multipliers;
- human-readable names and tooltips describing production rates, health, attacks, armor, spell effects, cooldowns, mana use, special targeting behavior, and legendary mechanics;
- the complete **11.8 MB** W3P-protected runtime Lua plus static indexes of **5,708** function definitions and **7,688** distinct call tokens.

Examples visible directly in the extracted catalogs include Footman, Mortar, Sniper, Faerie Dragon, their production buildings, and legendary/utility buildings such as Shrine of Destruction, Eraser, Snowveil Fountain, and Well of Pain. These tooltips already expose many mechanics that will eventually become compatibility fixtures rather than guessed native behavior.

## Evidence quality

Treat this directory as reverse-engineering evidence, not yet as final native content.

The terrain/pathing/doodad placement files are direct decodes of authoritative map data. Object-editor JSON/TSV records are direct map overrides, but Warcraft object definitions inherit unspecified fields from the base game; missing fields therefore cannot yet be interpreted as zero/default. `catalog/buildings.tsv` likewise records a pathing-texture filename size hint, not a finalized native footprint.

The runtime Lua is the strongest source for custom mechanics that are implemented in script rather than object fields. It is W3P-obfuscated, so current extraction indexes readable Wurst-generated names and preserves the original code without executing it. Semantic deobfuscation and cross-linking script functions to rawcodes/abilities is a separate next phase.

When compatibility conclusions are promoted into Castle Fight Native, record whether each value/behavior is directly verified from these files, inferred by cross-referencing several sources, observed in Warcraft III, or an intentional divergence. That matches the provenance policy in `docs/spec/40-content-data.md`.
