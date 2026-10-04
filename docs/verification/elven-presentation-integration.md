# Elven race-owned presentation integration — 9.27 r1

This branch supplies the presentation and authoritative race-selection boundary, **not** independent promotion. The release gate remains off until combined roster/mechanic closure. Roster, builder command-card ordering, hotkeys, models, portraits, attachment points, missile/buff/cast art and lightning identities are generated from retained evidence, not a copied UI table.

## Ownership and transport

Lobby participants carry their canonical builder race through the existing protocol setup. Server validation and match setup reject unsupported source-owned races; participant builder catalogs restrict command admission to that builder's retained direct roots and valid upgrades. Race is included in configuration identity. The mixed-race fixture uses authoritative simulation snapshot JSON/wire round-trip rather than inventing serialization for the local `CastleFightMatchConfig` object.

Model lookup is catalog-aware for the full Elven set instead of Human-only enum matches. Model source identity and map version remain in authoritative unit/building content even before activation, independently of render assets. Native projectile art lookup accepts child ability identity as well as ordinary weapon source identity; the coordinator must wire the new caster projectile view field after merging.

## Native visuals

`wc3-assets/build.rs` generates all Elven ordinary model and native-effect bindings from the source inventory, including proxy child closure. The retained `native-lightning-visuals.json` projection binds Healing Wave primary/secondary and Chain Lightning beam identities to native LightningData definitions. The reproducible extraction tool reads those definitions from the local Warcraft SD CASC archive, records source hashes, and does not commit proprietary installation assets.

The native lightning renderer uses the retained texture, wrap sampling, width, segment length, noise scale, texture scale and color. Healing and damaging lightning are not all flattened to CLPB. Native cast vs impact target/buff attachments and caster recovery visuals use effect bindings. The timed model pool provides native asset lifecycle behavior; no visual timer changes authoritative spell state. Shrine/Hex/carrier-specific integration remains owned by their mechanic branches.

Asset exporters include the full Elven transitive native dependency closure, supported imported line-weapon art, morph forms, and source script effect attachments. `audit-race-presentation.py --builder X00P --output <report>` checks source-root closure against a local extraction pack. It checks asset delivery, not rendered appearance, and must not be presented as a screenshot inspection.

## Validation

Standalone client tests: **212 passed, 3 ignored**; protocol **9 passed**. Workspace all-target Clippy with `-D warnings`: passed (`/tmp/presentation-completion-clippy.log`). Presentation projection tests: **5 passed**. Workspace test run reached the existing TCP timing failure `tcp_duplicate_command_is_acknowledged_once_and_finalized_once` after client/protocol success; combined integration must rerun serially and report the result honestly. No native game/screenshot comparison is claimed by this branch.
