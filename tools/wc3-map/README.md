# Warcraft III map extraction tooling

This directory contains the reproducible extraction pipeline for the original Castle Fight Warcraft III map. The goal is to preserve as much source evidence as possible in searchable, diffable text before translating it into Castle Fight Native content definitions.

## Run it

From the repository root:

```sh
tools/wc3-map/extract.sh
```

The default input is `docs/original_map/5329_Castle_Fight_DE_beta9.27_w3p.w3x` and the committed output is regenerated under `docs/original_map/extracted/`.

The pipeline uses two ignored working locations:

- `.local-tools/` for source checkouts, builds, and locally built executables;
- `.map-work/` for extracted binary members and intermediate translations.

Neither directory is required in git. Deleting both and rerunning `extract.sh` rebuilds the tooling and extraction from the checked-in source map.

## Source-built third-party tools

Only open-source tooling is fetched, pinned, license-checked, and built locally.

### StormLib

- project: `ladislav-zezula/StormLib`
- revision: `44ebfbfc109d76e2a85bbd5d8b0c949df7e65c6f`
- license: MIT
- bootstrap: `bootstrap-stormlib.sh`

StormLib is compiled from source with CMake/Ninja and bundled dependencies. `mpq_extract.cpp` is our small archive wrapper around StormLib.

### WC3MapTranslator

- project: `ChiefOfGxBxL/WC3MapTranslator`
- revision: `7d477ebb5cea445bee7915fe72cd93ee399252b7`
- license: MIT
- bootstrap: `bootstrap-translator.sh`

Its TypeScript source is installed from the pinned lockfile and compiled locally with `tsc`. It is used for the binary terrain, doodad-placement, and object-editor formats it understands.

WC3MapTranslator currently expects `war3map.w3i` format 33, while this map uses format 31. `decode_map.py` therefore parses this map's W3I directly. Its WTS path also misinterprets this map's UTF-8 strings, so `decode_map.py` parses `war3map.wts` itself and repairs equivalent UTF-8-as-Latin-1 text from translated object tables.

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
- `catalog/object-fields.tsv` — every decoded object modification in one flat table;
- `catalog/objects.tsv` — compact object index;
- `catalog/units.tsv` — useful unit/building combat fields for quick inspection;
- `catalog/buildings.tsv` — building-oriented fields plus a conservative pathing-texture size hint;
- `catalog/abilities.tsv` — ability fields including level-aware raw field data;
- `script/war3map.lua` — verbatim protected runtime Lua;
- `script/functions.tsv`, `script/calls.tsv`, `script/readable-function-names.txt` — static indexes into the protected Lua without executing it;
- `war3mapMisc.txt`, `war3mapSkin.txt` — already-plaintext map configuration.

The ASCII pathing maps use `#` for blocked and `.` for allowed. WPM bit meanings currently decoded are `0x02` no-walk, `0x04` no-fly, `0x08` no-build, `0x20` blight, `0x40` no-water, with unknown/unused bits retained in the hex grid and histogram.

## Important interpretation limits

Warcraft III object files are override/delta tables, not self-contained effective object definitions. A custom object based on `hfoo`, `hbla`, and so on inherits fields from Blizzard's base game data that may not occur in `war3map.w3u`. The extraction deliberately preserves `rawcode`, `base_rawcode`, and every override rather than inventing missing inherited values. Fully resolving effective stats will require matching base-game object metadata in a later import phase.

`catalog/buildings.tsv` derives `pathing_size_hint` only from filenames such as `PathTextures\\4x4SimpleSolid.tga`. It is evidence about the authored pathing texture, not yet a normalized Castle Fight Native footprint.

The W3P runtime script remains obfuscated/minified. Its source is retained verbatim and thousands of Wurst-generated function names are still searchable, but encrypted string constants and control flow have not yet been semantically deobfuscated. No map code is executed by this pipeline.

The absence of a canonical member such as `war3mapUnits.doo`, `war3map.w3q`, or `war3map.imp` means it could not be opened under that standard name in this protected archive. Do not infer from that alone that the original editor project never contained equivalent data; protected-map packaging can remove editor-only sources and hide imported assets.

## Tests

```sh
python -m unittest tools/wc3-map/test_decode.py
python -m py_compile tools/wc3-map/decode_map.py
```

A full end-to-end validation is `tools/wc3-map/extract.sh` followed by inspection of `summary.json`; the custom W3I parser records consumed/remaining byte counts so truncated or version-mismatched parsing is visible.
