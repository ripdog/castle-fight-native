# Undead implementation

Retained Castle Fight 9.27/r1 extraction tree: `8ea806dca331ff254995e94e6f0baf225a14bf10`. Builder `X018` ("Undead Builder", `uabi` `Aeth,A06E,AInv,A005,A001,Ahrp,Avul`, `urac` undead) and the generated `catalog-supplement` build-card coordinates define implementation order. Complete each root and its entire upgrade chain before moving right, then the next row. Only completed entities become buildable. The order is Rotting Hay Cart → Tainted Farm → Defiled Granary; Temple of Bone → Temple of Necromancy → High Citadel of Necromancy; Crypt → Graveyard; Haunted Mine; Slaughter House; Dragon Cemetery; Death Pit; Skull Pile → Skull Shrine; Turret of Souls; City of Decay; the shared Treasure Box (already promoted). (`Aeth`, `AInv`, `Ahrp`, `Avul` and the build-menu abilities are the shared builder body already used by every promoted race.)

## Rotting Hay Cart / Zombie

- Identity/source: runtime r207; `h01R` (`hbla`) produces `n00B` (`nzom`) in the top-left slot and upgrades to the Tainted Farm. Protected life, armor, movement and the Chaos melee weapon (ground and structures; the second weapon is disabled and unavailable) come from the catalogs. Corpse data (`can-raise-decays`, with the retained decay timers) is carried by the corpse catalog; Raise Dead is a later Necromancer ability and consumes it unchanged.
- Complete inventory/control flow: the sole ability is `A01L` (`ANpi`, "Permanent Immolation"), registered object data with no script consumer and no hidden proxy. It is the shared native permanent Immolation primitive (`warcraft-permanent-immolation-v1`, also used by the Infernal): radius, damage per interval, interval and target mask come from the retained row. Unlike the Infernal's variant its mask is organic-only, so mechanical bodies are never burned (the recipe projects the `organic` qualifier).
- State: none beyond the shared Immolation pulse phase.
- Undead becomes selectable with this completed root; later roots remain unavailable until their own mechanics are verified.

## Tainted Farm / Suffocated Zombie

- Identity/source: runtime r208; `h04Z` (`hbla`) upgrades from the Rotting Hay Cart (and leads to the Defiled Granary) and produces `n01K` (`nzom`). Protected life, armor, movement and the Chaos melee weapon come from the catalogs; the second weapon is disabled and unavailable.
- Complete inventory/control flow: `A05Q` (`ACvs`, "Envenomed Weapons") and `A05P` (`ANpi`, "Permanent Immolation"), both registered object data with no script consumer, proxy or hidden marker. `A05Q` is the shared native poison primitive (`warcraft-poison-v1`): per-second damage, movement factor (the retained `DataB` slow), attack-speed factor (zero), duration, the shorter hero duration, per-source stacking and the organic-only non-lethal pulses all come from the retained row. `A05P` is the same organic-only native Immolation as the Zombie's with its own retained radius and damage.
- State: ordinary poison and Immolation state only.
