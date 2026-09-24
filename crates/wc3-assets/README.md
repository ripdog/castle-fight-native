# Warcraft III asset extractor

`cf-wc3-assets` converts Warcraft III presentation assets from a local installation into files Castle Fight Native can consume. It supports every resolved non-building Castle Fight unit object, the complete resolved building catalog, the doodads/destructables actually placed by Castle Fight, map-referenced projectile/spell/buff visual models, and UI textures such as command-card, ability, status, resource, and cursor art. The repository and game build do not contain Warcraft III art; extraction happens from the user's local installation.

The unit, building, placed-doodad, visual-effect, and UI presentation catalogs are embedded in the executable at build time from the resolved Castle Fight map data. A released extractor therefore does not need the repository, the original map, or the large resolver TSV files at runtime. The normal unit inventory is deliberately broader than the currently implemented gameplay roster so summons, helpers, alternate bodies, and future roster additions are converted before they are enabled in native gameplay.

## Usage

```sh
cf-wc3-assets \
  --wc3 /path/to/Warcraft\ III \
  --output ./wc3-assets
```

`WC3_INSTALL` may be used instead of `--wc3`. For a normal Castle Fight installation, generate the complete presentation pack in one pass:

```sh
cf-wc3-assets \
  --wc3 "$WC3_INSTALL" \
  --map /path/to/Castle_Fight.w3x \
  --castle-fight \
  --output assets/wc3
```

This writes the full embedded unit, building, doodad, effect, and UI catalogs to their normal subdirectories and fails the overall run when any sub-pack has unresolved assets. Narrow modes remain useful while developing. With no mode, every resolved non-building unit object in the embedded Castle Fight catalog is exported. Repeat `--unit RAWCODE` to export a smaller set, for example:

```sh
cf-wc3-assets --wc3 "$WC3_INSTALL" -o /tmp/cf-assets --unit hfoo --unit hrif
```

To export building art, use `--buildings` for the complete resolved Castle Fight building catalog or repeat `--building RAWCODE` for a smaller pack. The current native slice is:

```sh
cargo run -p castle-fight-wc3-assets -- \
  --wc3 "$WC3_INSTALL" \
  --output assets/wc3/buildings \
  --building hcas \
  --building h000 \
  --building h039 \
  --building h03D \
  --building h02I \
  --building h03K \
  --building h015 \
  --building h006 \
  --building h07P \
  --building h003 \
  --building h004 \
  --building h05D \
  --building h0A1
```

These are the Main Castle, the Human Q/W/E production buildings and upgrades, retained verification buildings, Watch Tower, and Poof Tower. Building art comes from the resolved Warcraft unit-object `umdl`/`usca` fields, not the inherited profile `ifil`/`isca` fields. The extractor combines each object's `uani` required-animation properties with the sequences actually present in its shared model and records the selected `Birth`, ambient `Stand`, and `Death` sequence names in the building manifest. This handles shared models such as Town Hall and Human Tower without per-building renderer mappings, including upgrade-qualified lifecycle clips. Particle emitters also carry the exact model sequences in which they are active so construction/destruction effects can follow the selected clip. Future native building content only needs a matching rawcode in the generated building manifest. Poof Tower references `war3mapImported\\PandarenTower.mdl`; supply the matching map with `--map` to export that exact model.

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

To export UI icons, use `--ui`:

```sh
cargo run -p castle-fight-wc3-assets -- \
  --wc3 "$WC3_INSTALL" \
  --map /path/to/Castle_Fight.w3x \
  --ui \
  --output assets/wc3/ui
```

UI mode is driven by every resolved object-data field whose Warcraft metadata declares `value_type=icon`, rather than a hand-maintained Castle Fight icon list. That includes normal/research/turn-off ability icons, buff/status icons (used for both positive buffs and debuffs), unit and item interface icons, and caster-upgrade art. The catalog also includes Warcraft's stock gold, lumber, supply, and upkeep resource-bar icons; stock command-card art currently needed by the native client (Move, Attack, Human Build, and Cancel); and the complete Human, Orc, Undead, and Night Elf cursor sprite-sheet textures. Shared source textures are converted only once, while each rawcode/role or semantic command/cursor binding remains explicit in `manifest.json`. Map-imported TGA/DDS/BLP/PNG UI art is read from `--map` first; unresolved references are recorded as failures without aborting the rest of the UI pack.

Use `--keep-source` to retain the extracted MDX and original texture payloads beside the converted output. `--production` and `--object-fields` are development overrides for unit extraction and must be supplied together; normal shipped use relies on the embedded catalogs.

## Castle Fight client integration

The 3D client automatically looks for generated unit and building packs at `assets/wc3/units/manifest.json` and `assets/wc3/buildings/manifest.json`. The generated Warcraft files are gitignored, so they stay local to the player's installation. A useful first-pass pack for the current demo is:

```sh
cargo run -p castle-fight-wc3-assets -- \
  --wc3 "$WC3_INSTALL" \
  --output assets/wc3/units \
  --unit hfoo \
  --unit h03A \
  --unit hrif \
  --unit hmtm \
  --unit h0A2 \
  --unit h05C \
  --unit e003 \
  --unit o001 \
  --unit n015 \
  --unit h016 \
  --unit X00C
```

Those rawcodes cover the Human Q/W/E lines, retained combat verification models, and the Human Builder as a quick development subset. The default unfiltered unit export is the full resolved non-building Castle Fight inventory. Explicit `no_model.mdl` objects remain intentionally invisible instead of falling back to their base unit's art. If a generated unit or building model is absent, the client retains its normal placeholder visual. Building-pack schema 5 records authored model scale, extractor-resolved Birth/Stand/Death lifecycle sequences, and sequence-scoped particle emitters, while omitting Warcraft's engine-rendered Team Glow and Background geosets, which are billboard/decal geometry rather than ordinary building meshes. Entries that had to substitute inherited base art are deliberately skipped by the client rather than displaying a convincing-but-wrong structure. Newly observed buildings play Birth once before Stand; buildings restored from an initial/rejoin snapshot begin at Stand; authoritative removal may leave a cosmetic-only Death animation without retaining gameplay occupancy. Unit-pack schema 5 retains per-object Warcraft vertex tint and passive ability target art bound to named model attachment points. Marksman uses its extracted RGB tint although it shares the Rifleman mesh with Sniper and Heavy Gunner; Heavy Gunner keeps its Slow Aura target art on the animated weapon point. The Defender activation flash follows the shield-hand point. The pack also selects stand, walk, attack, spell-cast, death, flesh-decay, and bone-decay clips as appropriate. Movement, attacks, and casts are driven by authoritative simulation snapshots; corpse animation phase is synchronized to the authoritative corpse lifetime. Re-run the extractor after exporter updates. The client deliberately rejects older packs so stale exports cannot silently retain outdated animation/geometry contracts.

## Output

The output root contains `manifest.json`, `models/*.gltf`, matching `models/*.bin` buffers, and converted `textures/*.png` files. Unit and building entries in their manifests carry rawcode, model path, model scale, converted glTF path, and whether install-resident base art had to replace a custom map model that is unavailable in a stock Warcraft III installation. Doodad manifests preserve every exact editor placement (position/Z, angle, X/Y/Z scale, variation, visibility/solid/fixed-Z flags) and the resolved glTF scene for that variation. Effect manifest schema 4 binds unit/ability/buff rawcodes and art roles to converted scenes and retains Warcraft particle/ribbon emitter definitions beside each model because glTF has no native equivalent for those emitters. It also records persistent status visuals keyed by the authoritative ability modifier and status category, allowing buff target art such as Frozen, Frost Armor, and Frost slow to remain visible for exactly the simulation-owned modifier lifetime. UI manifest schema 2 binds object/resource identifiers plus semantic command/cursor identities to their original Warcraft texture path and converted PNG path, with a deduplicated texture list plus explicit failures. Cursor bindings point at Warcraft's complete fixed-layout sprite sheets; the client selects the stock Normal, Target, and InvalidTarget cells and preserves the authored click hotspot. Converted transient effects retain their named glTF animation clips; the client prefers an authored `Birth` sequence (then `Stand`), while persistent status visuals prefer looping `Stand`, so geoset-animated effects such as `DefendCaster.mdx` play instead of appearing as frozen geometry.

Geometry is converted from Warcraft's Z-up coordinates to glTF/Bevy Y-up coordinates but remains in Warcraft world units. Consumers should apply the per-object `scale` from `manifest.json` rather than baking scale into shared model geometry.

The extractor follows texture references from MDX files and handles modern installations where an SD model still names `foo.blp` or a v1800 model names source `foo.tif` art but CASC contains the corresponding `foo.dds`. Warcraft III 3.x presentation payloads are resolved from the base namespace first, then the `_de.w3mod` and `_hd.w3mod` layers used by current models. BLP, DDS, and TGA inputs are decoded to PNG. Shared model and texture outputs are deduplicated. Models are staged one at a time through Whiteout's file-stream MDX parser rather than its in-memory span parser: current Warcraft III 3.0-era assets can trigger pathological multi-gigabyte allocations in the latter. CASC's decoded-container cache is flushed between reads and source/PNG/JSON buffers are streamed where practical so extraction stays bounded instead of accumulating asset data across the pack.

## Current conversion scope

The converter currently targets classic/SD art. It exports mesh geometry, normals, UVs, glTF skins, Warcraft bone/helper hierarchy, named animation clips, representative glTF materials, texture alpha modes, two-sided flags, and unlit material hints. Bevy 0.19 only consumes the first four glTF skin influences, so classic Warcraft matrix groups with more than four equal-weight bones are deterministically truncated to the first four and renormalized. The manifest records a warning for affected geosets instead of emitting `JOINTS_1/WEIGHTS_1` that the runtime would silently ignore.

Warcraft `DontInterp` and linear transform tracks map directly to glTF step/linear animation. Hermite and Bezier tracks are evaluated with Warcraft's interpolation rules and linearized at the source keys plus interval midpoints, which keeps output compact while retaining curve shape. Geosets are exported as separate skinned glTF nodes so Warcraft geoset alpha animation can drive visibility; binary visible/hidden states are preserved exactly, while partial alpha fades are currently approximated as binary visibility because core glTF has no animated material-alpha channel. Global-sequence transforms are baked into each exported clip from global time zero; Warcraft normally keeps that global animation clock running across sequence changes, so models using global transforms carry a warning in `manifest.json`. The manifest also preserves sequence timing, movement speed, and non-looping metadata for the runtime animation selector.

Warcraft multi-layer materials are flattened to one representative glTF material, with explicit metadata retained for runtime reconstruction where one layer is not enough. The exporter understands both legacy layer texture IDs and v1100+ sub-texture slots. WC3 `FilterMode None` layers whose decoded texture actually contains transparent pixels are exported as glTF alpha-mask materials; `Transparent` remains alpha-tested, while the native client restores additive, additive-alpha, blend, and modulate behavior from the retained WC3 filter mode. Animated representative-layer alpha and diffuse texture selection are evaluated at runtime against the model's WC3 sequence/global clock; animated diffuse frames are resolved from the converter's texture inventory rather than model-specific renderer rules. Doodad/destructable skin replaceable textures such as Ashenvale tree skins are resolved through `texID`/`texFile` and baked into the exported glTF material. Unit replaceable IDs 1 and 2 are handled specially: team-color underlays are reconstructed from the owning WC3 player slot and team-glow layers select the corresponding stock `TeamGlowNN` texture. Packs containing replaceable team glow export all 24 modern Warcraft III player-colour glow textures so presentation is not limited to the old red/blue placeholder pairing. Other dynamic replaceables are not yet reproduced exactly. Node flags that disable inheritance of selected parent transforms also cannot be represented exactly by a normal glTF hierarchy and are reported as model warnings.

Models imported into Castle Fight are not present in a vanilla Warcraft III installation. Unit/building/doodad extraction can fall back to corresponding install-resident base object art where available, while map-backed extraction can read exact imported models and textures when `--map` supplies the matching `.w3x`/MPQ archive. Missing custom references remain explicit failures in the generated manifest rather than being silently substituted with unrelated art.

For doodads, the native client only autoplays an emitted animation whose name is exactly `Stand` (case-insensitive), at half presentation speed. It deliberately ignores `Stand Hit`, numbered stand variants, destruction/death clips, and declared WC3 sequences that produced no glTF transform channels. This keeps ambient fish/birds/etc. moving without accidentally animating static walls or trees through hit/death states. Doodad-pack schema 2 also requires the current per-geoset visibility export and carries particle/ribbon metadata into the client; older schema-1 packs are rejected so stale exports cannot leave normally hidden death/explosion geosets visible.

MDX ParticleEmitter2, legacy model-particle, ribbon, light, and child-attachment definitions are retained in `manifest.json`; their source nodes are also kept in the converted hierarchy even when a model has no bones or mesh geometry. The native client renders ParticleEmitter2 data for both transient effects and placed doodads with textured billboards using the source pack's textures, emission rate, speed/cone, gravity, lifetime, colors, segment scaling, and WC3 emitter filter mode (Blend, Additive, Modulate/Modulate2x approximation, or AlphaKey), with a bounded one-shot fallback for `Squirt` emitters whose source emission rate is animation-driven. Ribbon emitters are rendered as textured two-sided strips sampled from the moving source, retaining source material/filter mode, height-above/below, lifetime fade, emission rate, color/alpha, and gravity; trails survive briefly after their source projectile despawns so the tail can decay naturally. Model-embedded omni lights are reconstructed as node-local point lights with animated color, intensity, attenuation-end radius, and visibility sampled from the same WC3 sequence/global clock used by material tracks; model scale also scales the light radius. Child attachment models are resolved through the generated model inventory, parented to their authored animated attachment node, gated by the parent's WC3 visibility track, and bring their own animation plus PE2/ribbon metadata with them. The light and child-model passes remain approximate because Bevy's point-light attenuation differs from Warcraft's attenuation-start/end and ambient-light semantics, and a hidden child model is respawned from the beginning when its attachment becomes visible again. Legacy model-particle metadata remains preserved for later native rendering, and animated emitter tracks plus texture-atlas frame selection are still approximated. Projectile glTF geometry remains visible alongside the native particle/ribbon pass, while Chain Lightning is rendered procedurally from the authoritative resolved jump path because Warcraft represents it as a lightning primitive rather than an MDX projectile model.

Only assets the user is authorized to access should be extracted. The extractor itself does not bundle Warcraft III asset files.
