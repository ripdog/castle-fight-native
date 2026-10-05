# Elven race-owned presentation integration — 9.27 r1

Elven selection is enabled for retained 9.27 r1 at the user's request. Remaining
fidelity caveats below are verification work, not selection gates. Roster, builder
command-card ordering, hotkeys, models, portraits, attachment points,
missile/buff/cast art and lightning identities are generated from retained
evidence, not a copied UI table.

## Integrated attachment and local loading closure

The client now requires effects schema 6 and consumes retained attachment/count
metadata. Single-art status models bind to their authored animated nodes;
unattached art follows the imported model root. Render entries retain the exact
model root, including roots created in the same frame, so morphs and building
reconstruction cannot bind a status to an obsolete scene. Attached effects keep
local transforms during interpolation. Status scenes detach before actor teardown
and pool reuse removes old parent/binding state. Synthetic tests cover delayed
node availability, animated-node motion, attachment-slot deduplication, removal,
unready/nonpoolable destruction and reuse on a different actor.

The schema-6 staging pack is installed at `assets/wc3/effects`; the previous local
pack is preserved at `assets/wc3/effects-schema5-before-elven`. The installed
Elven audit reports 26 source-owned entities, 61 model/dependency bindings and
zero findings (`target/elven-installed-pack-audit.json`). Actual client loader
checks passed for units, buildings and effects. That loading check exposed the
signed native drain-lightning texture scale; validation now preserves finite
signed scales instead of rejecting the entire effects pack. No installation art
is committed.

The Elven status models have unambiguous single-art attachment metadata. Global
multi-art buffs remain at the model root: exported rows are sorted by path and do
not preserve native art-list order, and native model-to-point association has not
been established. This limitation is explicit rather than a guessed positional
mapping. Loading and synthetic hierarchy tests do not establish Warcraft rendered
conformance; native comparison caveats remain below.

## Ownership and transport

Lobby participants carry their canonical builder race through the existing protocol setup. Server validation and match setup reject unsupported source-owned races; participant builder catalogs restrict command admission to that builder's retained direct roots and valid upgrades. Race is included in configuration identity. The mixed-race fixture uses authoritative simulation snapshot JSON/wire round-trip rather than inventing serialization for the local `CastleFightMatchConfig` object.

Model lookup is catalog-aware for the full Elven set instead of Human-only enum matches. Model source identity and map version remain in authoritative unit/building content even before activation, independently of render assets. Native projectile art lookup accepts child ability identity as well as ordinary weapon source identity; the integrated projectile view and frontend selection preserve that distinction.

## Native visuals

`wc3-assets/build.rs` generates all Elven ordinary model and native-effect bindings from the source inventory, including proxy child closure. The retained `native-lightning-visuals.json` projection binds Healing Wave primary/secondary and Chain Lightning beam identities to native LightningData definitions. The reproducible extraction tool reads those definitions from the local Warcraft SD CASC archive, records source hashes, and does not commit proprietary installation assets.

The native lightning renderer uses the retained texture, wrap sampling, width, segment length, noise scale, texture scale and color. Healing and damaging lightning are not all flattened to CLPB. Native cast vs impact target/buff attachments and caster recovery visuals use effect bindings. The timed model pool provides native asset lifecycle behavior; no visual timer changes authoritative spell state. Shrine/Hex/carrier-specific integration remains owned by their mechanic branches.

Asset exporters include the full Elven transitive native dependency closure, supported imported line-weapon art, morph forms, and source script effect attachments. `audit-race-presentation.py --builder X00P --output <report>` checks source-root closure against a local extraction pack. It checks asset delivery, not rendered appearance, and must not be presented as a screenshot inspection.

## Integrated delivery audit and remaining gates

The local older schema-5 effect pack initially passed `audit-race-presentation.py`
with **zero findings despite no native lightning section**. That was an audit
false negative, not delivery closure. The checker now follows typed native
ability references (including orb children), requires selected native lightning
bindings/definitions and delivered textures against the source-linked projection,
and requires script-child ownership aliases instead of inspecting only rows
already present. The selected object-evidence digest must match the lightning
projection; the report retains its projection digest. Synthetic regressions cover
missing/stale/duplicate beams, missing textures, child/parent role identity,
wrong-owner aliases and cyclic native reference closure. All **nine presentation
projection/audit tests passed**. The older local Elven pack now reports **nine
findings**, correctly rejecting missing Healing/Chain lightning and source art
ownership. See disk-backed `old-pack-race-audit{,-strict}.json` reports.

Current effects regeneration against the local SD CASC installation and matching
map archive completed into disk-backed staging: 240 unique models, two unrelated
unresolved references (`.mdx` and `sandshield.mdx`). The initial strict checker
mistakenly expected nested effect definitions and target art inside the beam
binding; the exporter flattens definitions and delivers target art through
ordinary visual rows. Tests now mirror that actual schema and require those
ordinary target-art rows separately. All nine tests passed again. The staged
Elven race audit checks 26 source-owned entities, 60 model/dependency bindings,
all four caster beam bindings, and reports zero findings
(`current-pack-race-audit.json`). This is delivery evidence only, not a
loading/attachment test, rendered inspection or Warcraft comparison. A stricter native-inventory ownership audit then exposed four missing orb-child
missile/beam-target aliases despite successful ordinary delivery. The exporter
now follows typed ability references transitively per visible inventory user;
there is no Elven orb/rawcode table. A catalog-wide Rust test checks native
referenced art ownership, and the Python audit independently requires these
aliases. All ten Python tests, 72 asset tests (one ignored), strict asset
all-target Clippy, regeneration and the stricter Elven staging audit passed.
The four ownership findings became zero
(`native-child-ownership-{tests,clippy,export}.log`,
`current-pack-native-ownership-{before,after}.json`).
Native structure-buff lifetime/attachment visuals and loading still need
integrated review. Script City/Shield/Overheat art is now present in the
regenerated pack, but appearance/lifecycle is not established by delivery.

## Native buff source closure and required delivery

The versioned `native-buff-visuals-r1.json` projection covers all 174 referenced
buffs. `build_native_buff_visuals.py` reads the retained release's object fields,
base-data manifest and native-build summary. The cached native skin must match
its retained byte count and digest before any projection is produced; this uses
the original resolved-data build, not a newer install merely sharing filenames.
All six projection tests and complete regeneration checks passed. Native stock
Faerie Fire target art and its authored head attachment are retained; Phoenix's
resolved burn art is retained; the carrier's explicit empty target art remains
empty. No independent stock-buff path table is needed.

The delivery audit now requires persistent status bindings and their buff
identity/attachment metadata against that projection, rather than accepting an
unrelated ordinary target-art row or a model already on disk. All 13 presentation
audit tests passed, including missing/duplicate/unresolved status art, attachment
drift and forbidden fallback from an explicit empty carrier buff. The previously
zero-finding staged pack correctly reports **two native-status findings** for
Faerie Fire and Phoenix (`current-pack-native-buff-before.json`). Schema-6 effects
regeneration completed with 241 unique models and 33 persistent status rows.
The required Elven audit now checks 26 source-owned entities, 61 model/dependency
bindings and both persistent status bindings with **zero findings**
(`native-buff-{export,audit-after}.log`, `current-pack-native-buff-after.json`).
The two unrelated global unresolved paths remain `.mdx` and `sandshield.mdx`;
this is not a globally failure-free pack. Staging is still separate from the
repository pack loaded by the client. Requiring delivered
attachment metadata does not prove that the client applies it to the correct
animated node; source attachment placement and rendered inspection remain open.

The asset generator now consumes that version-scoped projection instead of an
authored stock-buff fallback. Status recipes include Faerie Fire armor and native
fire/carrier DOT families; empty carrier art produces no status model. The
exported effects manifest is schema **6**, retaining buff identity and authored
attachment/count metadata. The client recognizes the DOT category and uses live
status identity/expiry to create/remove looping unit and structure-root effects,
including zero-damage native buffs. It does not require the launch source to
remain alive. The reusable status iterator is allocation-free; death/removal,
wrong identity/category, expiry and read-only state are tested. Authored node
placement is still pending, so these status roots are not evidence of correct
Faerie Fire head attachment. All **73 asset tests** (one ignored), **219 client
tests** (three ignored), strict asset/client all-target Clippy, formatting and
diff checks passed (`native-buff-{assets-tests,client-tests,clippy}.log`).

## Integrated structure-status readback

The presentation bridge copies the authoritative `StatusState` for every
building, including passive structures without attack or spellcasting state.
The existing damage-over-time badge reader is shared by units and structures;
it uses the retained modifier identity, damage and expiry rather than a copied
Phoenix tuning table. Badge inspection is read-only and hides expired entries.

Synthetic regressions cover passive-structure burn status, wire restoration,
one/four-worker continuation, expiry and unchanged simulation checksums during
capture. All 396 simulation tests, 218 client tests (three ignored), and strict
simulation/client all-target Clippy passed, including the passive-structure
badge lifetime/nonmutation regression. This transport/UI work does **not** establish persistent model spawning,
source-authored attachment placement, current-pack loading or rendered fidelity.
No authoritative state or schema changed: compatibility remains bundle 6,
checksum 21, snapshot 16. Elven is still unpromoted.

The expanded debug catalog also outgrew the historical single-column planner.
Both debug-menu failures reproduced without status changes. The separate bounded
column planner preserves source footprints, ownership, definition order and
behind-castle placement, with a dense fallback and pre-mutation rejection when
space is genuinely insufficient. All six focused layout tests and 216 client
tests passed (three ignored), as did strict client Clippy after separate immutable
render-input grouping. This developer fixture does not bypass normal race
selection or imply Elven promotion.

## Historical branch validation

Standalone client tests: **212 passed, 3 ignored**; protocol **9 passed**. Workspace all-target Clippy with `-D warnings`: passed (`/tmp/presentation-completion-clippy.log`). Presentation projection tests: **5 passed**. Workspace test run reached the existing TCP timing failure `tcp_duplicate_command_is_acknowledged_once_and_finalized_once` after client/protocol success; combined integration must rerun serially and report the result honestly. No native game/screenshot comparison is claimed by this branch.
