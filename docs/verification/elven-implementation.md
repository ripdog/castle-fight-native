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
| `n00Y` / `h03F` | Feedback and complete passive inventory |
| `h00U` / `h00V` | Native Faerie Fire autocast |
| `n01Y` / `h06Y` | Scripted Solar Strike and proxy effect |
| `e005` / `h070` | Native line weapon; do not substitute ballistic splash |
| `h07B` / `h09X` | Healing Wave, orb child effect and scripted recovery |
| `h00W` / `h00X` | Healing Wave, orb/Phoenix Fire and complete marker inventory |
| `h00Z` | Hex, shields and reengagement sequence |
| `h005` | Persistent cleanse carrier and removal lifecycle |
| `h014` | Native multishot semantics; reconcile object/script/tooltip discrepancy |
| `h059` | Delayed one-time revival, classifications and death-generation checks |

Before lobby exposure, finish reachable behavior coverage, consume authored build
roots in race-aware match setup, and verify presentation bindings/attachments.
Use reusable mechanic tests. Add an entity-specific regression only when a concrete
integration issue cannot be demonstrated with a synthetic mechanic fixture.
