# Engine-evidence gap checklist

Implemented unit/building abilities whose behavior depends on Warcraft III engine semantics that the extracted map data does not define. Map object data and script control flow fix the numbers and sequencing; the items below need evidence from `Game.dll` (Ghidra, August 2017 build, image base `0x6f000000`) or an in-game observation.

This is a progress tracker, not a specification. Record findings in the race audit named in each row and keep the engine addresses with them. Tick a box only when the evidence is recorded and the clone matches it (or the divergence is documented as accepted).

Evidence tiers: **Ghidra** = traced in `Game.dll`; **In-game** = observed by the project owner; **Community** = third-party reference only (not sufficient on its own); **Owner** = stated by the project owner without a trace.

## Open items

### Elven

- [ ] **Feedback damage class / resistance.** Currently routed through the shared spell-damage path. Need the native damage kind and which immunity/resistance masks apply. Audit: [elven-melee-autocast.md](elven-melee-autocast.md).
- [ ] **Faerie Fire AI cadence and order interruptions.** Native autocast evaluation cadence and what interrupts it. Audit: [elven-melee-autocast.md](elven-melee-autocast.md).
- [ ] **Bounce (Moon Glaive-style) target selection.** Engine rule is provisional deterministic; need the native group iteration / next-target selection order. Audit: [elven-melee-autocast.md](elven-melee-autocast.md), [README.md](README.md).
- [ ] **Parasite carrier sub-tick corner.** Retained Parasite buff lifetime and the `nfbr` death summon when an unrelated same-frame death occurs. Deliberately unimplemented until observed. Audit: [elven-caster-audit.md](elven-caster-audit.md).
- [ ] **Healer AI frame fidelity.** Native order cadence, visibility and simultaneous event ordering for healing target selection. Audit: [elven-caster-audit.md](elven-caster-audit.md).
- [ ] **Elven shrine / item native-executable observations.** Audit: [golden-shrine-of-justice-9.27-r1.md](golden-shrine-of-justice-9.27-r1.md).

### Undead

- [ ] **Raise Dead cast range from corpses.** Whether the original AI cast from a corpse beyond the 600 cast range. Audit: [undead-implementation.md](undead-implementation.md).
- [ ] **Death and Decay: Universal damage armor handling.** Trace `CUnit::Damage` (`0x6f6cf530`) for damage-kind `0x4000000`. Owner states there is no armor interaction; confirm in Ghidra. Audit: [undead-implementation.md](undead-implementation.md).
- [ ] **Death damage through damage listeners.** Stock Death damage would reach listeners (for example Hardened Skin); the engine applies it directly. Audit: [undead-implementation.md](undead-implementation.md).
- [ ] **Conversion vs other death-time listeners.** Conversion removes the victim before listeners the engine runs after the death loop. Audit: [undead-implementation.md](undead-implementation.md).

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

- [x] **City of Decay stacking.** Only the oldest standing building per team contributes. Evidence: tooltip plus **In-game** confirmation by the project owner. Audit: [undead-implementation.md](undead-implementation.md).
- [x] **Death and Decay pulse timing and damage kind.** **Ghidra**: per-body random first delay in the first second, one-second period, Universal damage kind. Audit: [undead-implementation.md](undead-implementation.md).
