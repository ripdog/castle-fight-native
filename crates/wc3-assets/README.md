# Warcraft III asset extractor

`cf-wc3-assets` converts Warcraft III presentation assets from a local installation into files Castle Fight Native can consume. It supports production-unit art, the doodads/destructables actually placed by Castle Fight, and map-referenced projectile/spell/buff visual models. The repository and game build do not contain Warcraft III art; extraction happens from the user's local installation.

The production-unit, placed-doodad, and visual-effect catalogs are embedded in the executable at build time from the resolved Castle Fight map data. A released extractor therefore does not need the repository, the original map, or the large resolver TSV files at runtime.

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

To export the projectile, spell, buff, and status-effect models referenced by the resolved map data, use effect mode:

```sh
cargo run -p castle-fight-wc3-assets -- \
  --wc3 "$WC3_INSTALL" \
  --map /path/to/Castle_Fight.w3x \
  --effects \
  --output assets/wc3/effects
```

`--map` is optional, but supplying the matching Castle Fight map lets the extractor read imported MDX and texture files directly from the MPQ before falling back to install-resident CASC art. Effect extraction is best-effort across the whole catalog: unresolved references are recorded in `manifest.json` rather than aborting the pack. The manifest also identifies abilities based on Warcraft's Chain Lightning primitives and records the stock stun target art used by the client.

Use `--keep-source` to retain the extracted MDX and original texture payloads beside the converted output. `--production` and `--object-fields` are development overrides for unit extraction and must be supplied together; normal shipped use relies on the embedded catalogs.

## Castle Fight client integration

The 3D client automatically looks for a generated unit pack at `assets/wc3/units/manifest.json`. The generated Warcraft files are gitignored, so they stay local to the player's installation. A useful first-pass pack for the current demo is:

```sh
cargo run -p castle-fight-wc3-assets -- \
  --wc3 "$WC3_INSTALL" \
  --output assets/wc3/units \
  --unit hfoo \
  --unit e003 \
  --unit o001 \
  --unit n015 \
  --unit h016
```

Those rawcodes cover the complete current native unit slice: Footman, Ranger, Catapult, Ice Troll Shadow Priest, and Gryphon Rider. If a generated model is absent, the client silently retains its normal placeholder visual. The client selects stand, walk, attack, spell-cast, death, flesh-decay, and bone-decay clips as appropriate. Movement, attacks, and casts are driven by authoritative simulation snapshots; corpse animation phase is synchronized to the authoritative corpse lifetime. Re-run the extractor after exporter updates: unit-pack schema 4 includes per-geoset visibility animation, Bevy-compatible four-influence skins, and exported WC3 overhead attachment points used by status effects. The client deliberately rejects older packs so stale exports cannot keep rendering decay geometry, unsupported `JOINTS_1/WEIGHTS_1` data, or incorrect attachment metadata.

## Output

The output root contains `manifest.json`, `models/*.gltf`, matching `models/*.bin` buffers, and converted `textures/*.png` files. Unit entries in the manifest carry their rawcode, model path, model scale, converted glTF path, and whether install-resident base art had to replace a custom map model that is unavailable in a stock Warcraft III installation. Doodad manifests preserve every exact editor placement (position/Z, angle, X/Y/Z scale, variation, visibility/solid/fixed-Z flags) and the resolved glTF scene for that variation. Effect manifests bind unit/ability/buff rawcodes and art roles to converted scenes and retain Warcraft particle/ribbon emitter definitions beside each model because glTF has no native equivalent for those emitters.

Geometry is converted from Warcraft's Z-up coordinates to glTF/Bevy Y-up coordinates but remains in Warcraft world units. Consumers should apply the per-unit `scale` from `manifest.json` rather than baking scale into shared model geometry.

The extractor follows texture references from MDX files and handles modern installations where an SD model still names `foo.blp` but CASC contains the corresponding `foo.dds`. BLP, DDS, and TGA inputs are decoded to PNG. Shared model and texture outputs are deduplicated.

## Current conversion scope

The converter currently targets classic/SD art. It exports mesh geometry, normals, UVs, glTF skins, Warcraft bone/helper hierarchy, named animation clips, representative glTF materials, texture alpha modes, two-sided flags, and unlit material hints. Bevy 0.19 only consumes the first four glTF skin influences, so classic Warcraft matrix groups with more than four equal-weight bones are deterministically truncated to the first four and renormalized. The manifest records a warning for affected geosets instead of emitting `JOINTS_1/WEIGHTS_1` that the runtime would silently ignore.

Warcraft `DontInterp` and linear transform tracks map directly to glTF step/linear animation. Hermite and Bezier tracks are evaluated with Warcraft's interpolation rules and linearized at the source keys plus interval midpoints, which keeps output compact while retaining curve shape. Geosets are exported as separate skinned glTF nodes so Warcraft geoset alpha animation can drive visibility; binary visible/hidden states are preserved exactly, while partial alpha fades are currently approximated as binary visibility because core glTF has no animated material-alpha channel. Global-sequence transforms are baked into each exported clip from global time zero; Warcraft normally keeps that global animation clock running across sequence changes, so models using global transforms carry a warning in `manifest.json`. The manifest also preserves sequence timing, movement speed, and non-looping metadata for the runtime animation selector.

Warcraft multi-layer materials are flattened to one representative glTF material, with explicit metadata retained for runtime reconstruction where one layer is not enough. The exporter understands both legacy layer texture IDs and v1100+ sub-texture slots. WC3 `FilterMode None` layers whose decoded texture actually contains transparent pixels are exported as glTF alpha-mask materials; `Transparent` remains alpha-tested, while the native client restores additive, additive-alpha, blend, and modulate behavior from the retained WC3 filter mode. Doodad/destructable skin replaceable textures such as Ashenvale tree skins are resolved through `texID`/`texFile` and baked into the exported glTF material. Unit replaceable IDs 1 and 2 are handled specially: team-color underlays are reconstructed per side and team-glow layers use the corresponding stock red/blue glow texture. Other dynamic replaceables are not yet reproduced exactly. Node flags that disable inheritance of selected parent transforms also cannot be represented exactly by a normal glTF hierarchy and are reported as model warnings.

Models imported into Castle Fight are not present in a vanilla Warcraft III installation. Unit/doodad extraction can fall back to corresponding install-resident base object art where available, while effect extraction can read exact imported models and textures when `--map` supplies the matching `.w3x`/MPQ archive. Missing custom references remain explicit failures in the generated manifest rather than being silently substituted with unrelated art.

For doodads, the native client only autoplays an emitted animation whose name is exactly `Stand` (case-insensitive), at half presentation speed. It deliberately ignores `Stand Hit`, numbered stand variants, destruction/death clips, and declared WC3 sequences that produced no glTF transform channels. This keeps ambient fish/birds/etc. moving without accidentally animating static walls or trees through hit/death states.

MDX ParticleEmitter2, legacy model-particle, and ribbon definitions are retained in `manifest.json`; emitter nodes are also kept in the converted hierarchy even when a model has no bones or mesh geometry. The native client renders ParticleEmitter2 data with textured billboards using the source emission rate, speed/cone, gravity, lifetime, colors, segment scaling, and WC3 emitter filter mode (Blend, Additive, Modulate/Modulate2x approximation, or AlphaKey), with a bounded one-shot fallback for `Squirt` emitters whose source emission rate is animation-driven. Ribbon emitters are rendered as textured two-sided strips sampled from the moving source, retaining source material/filter mode, height-above/below, lifetime fade, emission rate, color/alpha, and gravity; trails survive briefly after their source projectile despawns so the tail can decay naturally. Legacy model-particle metadata remains preserved for later native rendering, and animated emitter tracks plus texture-atlas frame selection are still approximated. Projectile glTF geometry remains visible alongside the native particle/ribbon pass, while Chain Lightning is rendered procedurally from the authoritative resolved jump path because Warcraft represents it as a lightning primitive rather than an MDX projectile model.

Only assets the user is authorized to access should be extracted. The extractor itself does not bundle Warcraft III asset files.
