# Elven implementation checklist

Status: **enabled for retained 9.27 r1; fidelity verification continues**.

This file tracks promotion work, not a second encoding of map tuning. Resolve every
row against the selected retained release using the shared
[unit/building checklist](unit-implementation-checklist.md).

## Integrated completion

Elven is selectable through ordinary local/lobby/server setup for retained 9.27
r1. The mixed-race regression uses public setup, checks source-owned command
admission, and restores canonical state with a different worker count. Runtime
content is `cf-native-dev-slice-r13`; authoritative compatibility remains content
bundle schema 6, checksum schema 21 and snapshot schema 16. Release publication
pins the exact integrated source commit and current generated tuning/binding
hashes. This work changes selection and presentation, not authoritative state
shape.

Network lobby selection uses protocol revision 7: each authenticated player can
select its own supported builder before the host starts. Both peers rebuild the
accepted tick-zero roster before processing canonical ticks. The lobby displays
both teams in equal-width cards, and builder models refresh after a race change.
The installed schema-6 effects pack passes the Elven presentation audit with no
findings; local real-pack loading and an isolated rendered smoke check passed.

Final integrated validation: **724 workspace tests passed** (four optional local
asset tests ignored by default), strict workspace all-target Clippy passed,
formatting and diff checks passed, and both retained release manifests verify.
The three local client pack-loading tests were also run explicitly and passed.
Client and standalone server binaries were rebuilt. Isolated 1v1 and 3v3 lobby
previews verified team alignment, race labels and the guest waiting state.

Native buff attachment/loading closure is recorded in
[the presentation audit](elven-presentation-integration.md). Remaining native
Warcraft oracle caveats concern frame/AI ordering, the sub-tick Parasite corner,
and exact visual comparison. Fog-of-war consumers remain an engine-wide future
system. The global multi-art buff association caveat does not affect Elven's
single-art status models. These limits are not claims that those observations
have been completed.

## Source references

For the retained 9.27 release, use `docs/original_map/releases.json` for revision
identity and `docs/original_map/extracted/` for its working evidence alias:

- Roster and production: `script/race-buildings.tsv` (builder `X00P`),
  `resolved/production-buildings.tsv`.
- Unit data: `resolved/units.tsv`, `protected-unit-stats.tsv`,
  `production-unit-attacks.tsv`, `production-unit-corpses.tsv`.
- Abilities: `resolved/production-unit-abilities.tsv`, `abilities.tsv`,
  `object-fields.tsv`, `protected-ability-fields.tsv`.
- Scripted casts: `resolved/unit-spells.tsv`, `unit-spell-semantics.tsv`,
  `unit-spell-mechanics.tsv`, `building-spell-mechanics.tsv`.
- Cross-system mechanics: `resolved/runtime-system-mechanics.tsv`.

## Promotion status

| Source identity | Status / remaining work |
| --- | --- |
| `n022`, `n023`, `n006`; `h08X`, `h08Y`, `h00T` | Native verification fixtures; not a complete race |
| `n00Y` / `h03F` | r12 fixture: complete passive inventory, independent Feedback/critical handling; native damage-class probe remains |
| `h00U` / `h00V` | r12 fixture: source-derived bounce weapon and native Faerie Fire; native AI/bounce-order probes and visibility/dispel integration remain |
| `n01Y` / `h06Y` | Scripted Solar Strike and proxy effect |
| `e005` / `h070` | Native line weapon; do not substitute ballistic splash |
| `h07B` / `h09X` | Healing Wave, orb child effect and scripted recovery |
| `h00W` / `h00X` | Healing Wave, orb/Phoenix Fire and complete marker inventory |
| `h00Z` | Hex, shields and reengagement sequence |
| `h005` | Persistent cleanse carrier and removal lifecycle |
| `h014` | Native multishot semantics; reconcile object/script/tooltip discrepancy |
| `h059` | Delayed one-time revival, classifications and death-generation checks |

See the [melee/autocast](elven-melee-autocast.md), [caster](elven-caster-audit.md),
[City](city-of-magic-9.27-r1.md), and [tower](elven-towers.md) audits for source
traces, reusable verification and explicit remaining gates.

Integrated recovery/cleansing checks now separate removable native stun from
scripted order recovery and preserve callbacks/passive baselines. Primary and
additional Phoenix Fire ignore ordinary order interruption but respect Hex at
evaluation and live commitment. Subsequent Shrine/native-activation source-version and standalone timer wire
validation passed all 384 simulation tests, strict simulation Clippy and client all-target checks.
The following Hex/native-tower class and strict-mask integration passed all 388
simulation tests, strict Clippy and client checks. Native Phoenix unit visibility and live unit-DOT continuation then passed all
390 simulation tests, strict Clippy and client all-target checks. Source-backed
structure classifications and native structure DOT immunity subsequently passed
all 396 simulation tests, strict Clippy and client all-target checks, including
live/cold hash/wire, activation, replacement/cancellation, blocked and final
pulse continuation. This does not close imported visual closure,
final release/source publication, workspace validation, or native-oracle caveats.

Elven selection is enabled at the user's request. Race-aware match setup consumes
the authored build roots; reachable behavior and presentation verification continue.
Use reusable mechanic tests. Add an entity-specific regression only when a concrete
integration issue cannot be demonstrated with a synthetic mechanic fixture.
