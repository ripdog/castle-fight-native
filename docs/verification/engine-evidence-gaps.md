# Engine-evidence gap checklist

Implemented unit/building abilities whose behavior depends on Warcraft III engine semantics that the extracted map data does not define. Map object data and script control flow fix the numbers and sequencing; the items below need evidence from `Game.dll` (Ghidra, August 2017 build, image base `0x6f000000`) or an in-game observation.

This is a progress tracker, not a specification. Record findings in the race audit named in each row and keep the engine addresses with them. Tick a box only when the evidence is recorded and the clone matches it (or the divergence is documented as accepted).

Evidence tiers: **Ghidra** = traced in `Game.dll`; **In-game** = observed by the project owner; **Community** = third-party reference only (not sufficient on its own); **Owner** = stated by the project owner without a trace.

## Open items

### Elven

- [ ] **Feedback damage class / resistance.** Currently routed through the shared spell-damage path. Need the native damage kind and which immunity/resistance masks apply. Audit: [elven-melee-autocast.md](elven-melee-autocast.md).
- [ ] **Faerie Fire AI cadence and order interruptions.** Native autocast evaluation cadence and what interrupts it. Audit: [elven-melee-autocast.md](elven-melee-autocast.md).
- [ ] **Parasite carrier sub-tick corner.** Retained Parasite buff lifetime and the `nfbr` death summon when an unrelated same-frame death occurs. Deliberately unimplemented until observed. Audit: [elven-caster-audit.md](elven-caster-audit.md).
- [ ] **Healer AI frame fidelity.** Native order cadence, visibility and simultaneous event ordering for healing target selection. Audit: [elven-caster-audit.md](elven-caster-audit.md).
- [ ] **Elven shrine / item native-executable observations.** Audit: [golden-shrine-of-justice-9.27-r1.md](golden-shrine-of-justice-9.27-r1.md).

### Undead

- [ ] **Raise Dead cast range from corpses.** Whether the original AI cast from a corpse beyond the 600 cast range. Audit: [undead-implementation.md](undead-implementation.md).
- [ ] **Death damage through damage listeners.** Stock Death damage would reach listeners (for example Hardened Skin); the engine applies it directly. Audit: [undead-implementation.md](undead-implementation.md).
- [ ] **Conversion vs other death-time listeners.** Conversion removes the victim before listeners the engine runs after the death loop. Audit: [undead-implementation.md](undead-implementation.md).

### Cross-cutting

- [ ] **Defend / Magic Defense versus Universal spell damage (contradiction).** The Ghidra listener shared by `CAbilityDefend` (vtable `0x6fb38628`) and `CAbilityMagicDefense` (vtable `0x6fb38988`) is `0x6f5f9f90`, phase 6. It reduces damage by the damage record's *attack type* (0/4 spells, 2 pierce) and contains no check of the damage *kind*. Universal damage with attack type 0 therefore appears to be reduced. The clone assumes the opposite for Starfall (`starfall.rs`) and Snowveil (`snowveil.rs`) while Whirlwind and Death and Decay do apply Defend/resistance, so the clone is internally inconsistent. Needs confirmation (Defend level-data field mapping, and whether another gate exists) before changing behavior. Audit: [night-elf-implementation.md](night-elf-implementation.md).

### Mechanical

- [ ] **Parasite autocast target mask.** Native mask names structures, mechanical and invulnerable targets; the engine fires only at a visible hostile ground combat unit. Audit: [mechanical-implementation.md](mechanical-implementation.md).

### Cross-race native semantics currently sourced from community notes

Tier is Community only; each needs a Ghidra trace or an explicit acceptance.

- [ ] Thunder Clap hostile eligibility and undispellable slow (Naga).
- [ ] Charm summoned-unit expiry, upgrades and creep-level rule (Naga).
- [ ] Carapace melee/ranged factor scope (Naga).
- [ ] Cleave circle placement and armor exclusion on secondary hits (Naga, Chaos).
- [ ] Aspo shared stacking and second-buff selection (Naga).
- [ ] Life Regeneration Aura buff and literal regeneration (Naga).
- [ ] Chaos aura recipients / hostile Thorns immunity (Chaos).
- [ ] Orc spell-resistance and poison/slow family semantics.

## Resolved

- [x] **Bounce next-target selection.** **Ghidra**: `CMissileBounce` (vtable `0x6fb0a068`) impact handler `0x6f703d70` calls `0x6f704fa0`, which enumerates units within the bounce radius of the missile position and keeps, via `0x6f705050`, the candidate with the strictly smallest distance (initial best distance is a large constant). The current target and already-hit units are excluded by the validator `0x6f702550`; the bounce counter is decremented before the search. The clone previously used a keyed-random rank; it now selects the nearest unit and breaks exact ties by `SimId` (the engine uses group enumeration order, which is not reproducible). Remaining uncertainty: whether the engine metric is centre distance (the unit-position accessor suggests so) rather than edge distance. Audit: [elven-melee-autocast.md](elven-melee-autocast.md).

- [x] **Universal damage armor handling (Death and Decay, Starfall, Whirlwind).** **Ghidra** (`0x6f6cf530`): armor-point reduction only for kind Normal `0x10`; the attack-type by armor-type multiplier applies to all kinds. Matches the clone. Audit: [undead-implementation.md](undead-implementation.md).

- [x] **City of Decay stacking.** Only the oldest standing building per team contributes. Evidence: tooltip plus **In-game** confirmation by the project owner. Audit: [undead-implementation.md](undead-implementation.md).
- [x] **Death and Decay pulse timing and damage kind.** **Ghidra**: per-body random first delay in the first second, one-second period, Universal damage kind. Audit: [undead-implementation.md](undead-implementation.md).
