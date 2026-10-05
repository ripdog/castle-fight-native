# Elven implementation checklist

Status: **incomplete; not a playable race**.

This file tracks promotion work, not a second encoding of map tuning. Resolve every
row against the selected retained release using the shared
[unit/building checklist](unit-implementation-checklist.md).

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
simulation tests, strict Clippy and client checks. This does not close native
Phoenix visibility, unit/structure DOT immunity and structure classification
transport, imported visual closure,
final release/source publication, workspace validation, or native-oracle caveats.

Before lobby exposure, finish reachable behavior coverage, consume authored build
roots in race-aware match setup, and verify presentation bindings/attachments.
Use reusable mechanic tests. Add an entity-specific regression only when a concrete
integration issue cannot be demonstrated with a synthetic mechanic fixture.
