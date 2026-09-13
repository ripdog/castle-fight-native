# Warcraft III asset extractor

`cf-wc3-assets` converts Warcraft III presentation assets from a local installation into files Castle Fight Native can consume. It supports both production-unit art and the doodads/destructables actually placed by Castle Fight. The repository and game build do not contain Warcraft III art; extraction happens from the user's local installation.

The production-unit and placed-doodad catalogs are embedded in the executable at build time from the resolved Castle Fight map data. A released extractor therefore does not need the repository, the original map, or the large resolver TSV files at runtime.

## Usage

```sh
cf-wc3-assets \
  --wc3 /path/to/Warcraft\ III \
  --output ./wc3-assets
```

`WC3_INSTALL` may be used instead of `--wc3`. By default every production unit in the embedded Castle Fight catalog is exported. Repeat `--unit RAWCODE` to export a smaller set while developing, for example:

```sh
cf-wc3-assets --wc3 "$WC3_INSTALL" -o /tmp/cf-assets --unit hfoo --unit hrif
```

To export the map decorations for the native 3D client, use the doodad mode and the client's expected generated-asset directory:

```sh
cargo run -p castle-fight-wc3-assets -- \
  --wc3 "$WC3_INSTALL" \
  --doodads \
  --output assets/wc3/doodads
```

This exports only models referenced by the 1,780 committed placements. Invisible LOS/pathing blocker objects are retained in `manifest.json` for provenance but have no render scene. Repeat `--doodad RAWCODE` to restrict extraction while developing.

Use `--keep-source` to retain the extracted MDX and original texture payloads beside the converted output. `--production` and `--object-fields` are development overrides for unit extraction and must be supplied together; normal shipped use relies on the embedded catalogs.

## Castle Fight client integration

The 3D client automatically looks for a generated unit pack at `assets/wc3/units/manifest.json`. The generated Warcraft files are gitignored, so they stay local to the player's installation. A useful first-pass pack for the current demo is:

```sh
cargo run -p castle-fight-wc3-assets -- \
  --wc3 "$WC3_INSTALL" \
  --output assets/wc3/units \
  --unit hfoo \
  --unit n015
```

Those rawcodes currently replace the placeholder meshes for Footman and Ice Troll Shadow Priest. If the manifest or either model is absent, the client silently retains its normal placeholder visual. The client instantiates the converted scene and scale now; animation selection/playback is not wired yet, so these first imported units render in their static exported pose. Ranger and Gryphon Rider already convert, but their Warcraft skins use more than four bone influences on some vertices; Bevy 0.19 ignores glTF `JOINTS_1/WEIGHTS_1`, so they remain placeholders until the runtime or exporter has an explicit compatible path. Catapult is also intentionally still a placeholder because its Warcraft model uses hidden geoset state that the current converter does not yet reproduce.

## Output

The output root contains `manifest.json`, `models/*.gltf`, matching `models/*.bin` buffers, and converted `textures/*.png` files. Unit entries in the manifest carry their rawcode, model path, model scale, converted glTF path, and whether install-resident base art had to replace a custom map model that is unavailable in a stock Warcraft III installation. Doodad manifests preserve every exact editor placement (position/Z, angle, X/Y/Z scale, variation, visibility/solid/fixed-Z flags) and the resolved glTF scene for that variation.

Geometry is converted from Warcraft's Z-up coordinates to glTF/Bevy Y-up coordinates but remains in Warcraft world units. Consumers should apply the per-unit `scale` from `manifest.json` rather than baking scale into shared model geometry.

The extractor follows texture references from MDX files and handles modern installations where an SD model still names `foo.blp` but CASC contains the corresponding `foo.dds`. BLP, DDS, and TGA inputs are decoded to PNG. Shared model and texture outputs are deduplicated.

## Current conversion scope

The converter currently targets classic/SD art. It exports mesh geometry, normals, UVs, glTF skins, Warcraft bone/helper hierarchy, named animation clips, representative glTF materials, texture alpha modes, two-sided flags, and unlit material hints. Classic matrix groups with up to eight influences are preserved through `JOINTS_0/WEIGHTS_0` and `JOINTS_1/WEIGHTS_1` rather than truncating weights.

Warcraft `DontInterp` and linear transform tracks map directly to glTF step/linear animation. Hermite and Bezier tracks are evaluated with Warcraft's interpolation rules and linearized at the source keys plus interval midpoints, which keeps output compact while retaining curve shape. Global-sequence transforms are baked into each exported clip from global time zero; Warcraft normally keeps that global animation clock running across sequence changes, so models using global bone transforms carry a warning in `manifest.json`. The manifest also preserves sequence timing, movement speed, and non-looping metadata for the runtime animation selector.

Warcraft multi-layer materials are flattened to one representative glTF material. Doodad/destructable skin replaceable textures such as Ashenvale tree skins are resolved through `texID`/`texFile` and baked into the exported glTF material; unit team-color and other dynamic replaceables are not yet reproduced exactly. Node flags that disable inheritance of selected parent transforms also cannot be represented exactly by a normal glTF hierarchy and are reported as model warnings.

Models imported into the Castle Fight map are not present in a vanilla Warcraft III installation. When the map requests such a model, the extractor falls back to the corresponding install-resident base object art and marks `fallback_to_base_art: true` while retaining `requested_model` in the manifest. An optional map-archive asset source can later provide exact imported art without changing the install-only path.

MDX particle/ribbon emitters are not yet translated to glTF. Most Castle Fight doodads have normal mesh geometry; the two bubble-geyser doodads are emitter-only and therefore currently instantiate empty scenes until an emitter presentation path is added.

Only assets the user is authorized to access should be extracted. The extractor itself does not bundle Warcraft III asset files.
