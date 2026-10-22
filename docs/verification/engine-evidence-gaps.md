# Engine-evidence gap checklist

Implemented unit/building abilities whose behavior depends on Warcraft III engine semantics that the extracted map data does not define. Map object data and script control flow fix the numbers and sequencing; the items below need evidence from `Game.dll` (Ghidra, August 2017 build, image base `0x6f000000`) or an in-game observation.

This is a progress tracker, not a specification. Record findings in the race audit named in each row and keep the engine addresses with them. Tick a box only when the evidence is recorded and the clone matches it (or the divergence is documented as accepted).

Evidence tiers: **Ghidra** = traced in `Game.dll`; **In-game** = observed by the project owner; **Community** = third-party reference only (not sufficient on its own); **Owner** = stated by the project owner without a trace.

## Open items

### Elven

- [ ] **Faerie Fire AI cadence and order interruptions.** Native autocast evaluation cadence and what interrupts it. Audit: [elven-melee-autocast.md](elven-melee-autocast.md).
- [ ] **Parasite carrier sub-tick corner.** Retained Parasite buff lifetime and the `nfbr` death summon when an unrelated same-frame death occurs. Deliberately unimplemented until observed. Audit: [elven-caster-audit.md](elven-caster-audit.md).
- [ ] **Healer AI frame fidelity.** Native order cadence, visibility and simultaneous event ordering for healing target selection. Audit: [elven-caster-audit.md](elven-caster-audit.md).
- [ ] **Elven shrine / item native-executable observations.** Audit: [golden-shrine-of-justice-9.27-r1.md](golden-shrine-of-justice-9.27-r1.md).

### Undead

- [ ] **Raise Dead cast range from corpses.** Whether the original AI cast from a corpse beyond the 600 cast range. Audit: [undead-implementation.md](undead-implementation.md).
- [ ] **Death damage through damage listeners.** Stock Death damage would reach listeners (for example Hardened Skin); the engine applies it directly. Audit: [undead-implementation.md](undead-implementation.md).
- [ ] **Conversion vs other death-time listeners.** Conversion removes the victim before listeners the engine runs after the death loop. Audit: [undead-implementation.md](undead-implementation.md).

### Mechanical

- [ ] **Parasite autocast target mask.** Native mask names structures, mechanical and invulnerable targets; the engine fires only at a visible hostile ground combat unit. Audit: [mechanical-implementation.md](mechanical-implementation.md).

### Cross-race native semantics currently sourced from community notes

Tier is Community only; each needs a Ghidra trace or an explicit acceptance.

- [ ] Thunder Clap hostile eligibility and undispellable slow (Naga).
- [ ] Charm summoned-unit expiry, upgrades and creep-level rule (Naga).
- [ ] Aspo shared stacking and second-buff selection (Naga).
- [ ] Life Regeneration Aura buff and literal regeneration (Naga).
- [ ] Orc poison/slow family stacking semantics (flags `+0x190`, `+0x1dc`, `+0x1e0` are constants shared by every poison variant; their meaning is untraced).

## Resolved

- [x] **Defend attack-type selection and spell resistance on weapon damage (Orc, Elven, Human).** **Ghidra** (`0x6f5f9f90`, `0x6f5750a0`): Defend reduces Pierce attack-type damage by its pierce factor and Spells and Magic attack-type damage by its spell factor, and leaves Normal, Siege, Chaos and Hero untouched; `AIsr` spell resistance scales attack type Spells only, so an ordinary Magic or Chaos weapon is not reduced by it (the clone already documented this). The clone applied Defend's ranged factor to every ranged projectile; `defend_factor_for_attack_type` now selects the factor by damage type and a test covers Pierce, Magic, Siege, Normal and Chaos. Melee Pierce attack types are not yet routed through Defend. Audit: [elven-passive-fidelity.md](elven-passive-fidelity.md).

- [x] **Thorns / Spiked Shell return basis and armor (Chaos, Naga).** **Ghidra**: `CBuffAuraThorns` (`0x6f4ec790`) and `CAbilityThornyShield` (`0x6f5b2820`) are phase-1 damage listeners, and the damage routine runs phase 1 before the armor-point reduction, Defend and every later phase. In percentage mode they return the ability fraction of the record amount, clamped to at least 1, as Plant (`0x100000`) and Defensive (`0x200000`) kind respectively with the default attack type 0. The returned fraction is therefore based on the unreduced hit, and the returned packet takes no numeric armor points (only the Spells row of the defense-type table). The clone previously used the post-armor hit and applied armor points to the Shell return; `spiked_shell.rs` and the damage path now use the unreduced basis, no armor points, and the one-point minimum. Audit: [chaos-implementation.md](chaos-implementation.md), [naga-implementation.md](naga-implementation.md).

- [x] **Feedback damage class (`Afbt`).** **Ghidra** (`CAbilityFeedback`, vtable `0x6fb333d0`; pre-hit hook `0x6f5c3cb0`, post-hit hook `0x6f5c3b50`): the post-hit hook reads the ability's burn amount and mana multiplier, clamps the burn to the target's current mana and calls the shared magic-damage helper `0x6f5809b0`, which builds a damage record of kind Magic (`0x4000`) with the default attack type 0 (Spells). Magic-kind damage is blocked by spell immunity (mask `0x033ee700`) and passes ethereal bodies; Defend, Magic Defense and spell-damage-reduction listeners and the Spells defense-type row all apply. This is the shared spell-damage path the clone already uses. Audit: [elven-melee-autocast.md](elven-melee-autocast.md).

- [x] **Spiked/Thorny Shell (`ANth`) received and returned damage scope (Naga Dragon Turtle).** **Ghidra** (`CAbilityThornyShield`/`CAbilityThorns`, listener `0x6f5b2820`, phase 1): the listener acts only when the incoming record's flag bit 0 (the JASS `ranged` argument) is clear and its kind is Normal (`0x10`), the attacker is present and not spell-immune. The returned damage is the ability's fraction of the original record amount, dealt with kind Defensive (`0x200000`) and the default attack type, and is computed before the received-damage factor rescales the record, so the two are independent. The received factor therefore applies to melee Normal-kind hits only, not to all damage as the tooltip says. The clone matches. Audit: [naga-implementation.md](naga-implementation.md).

- [x] **Defend / Magic Defense / spell damage reduction versus Universal damage.** **Ghidra**: the damage routine (`0x6f6cf530`) runs every registered listener regardless of damage kind. Defend and Magic Defense share listener `0x6f5f9f90` (phase 6) and `CAbilityPassiveDefense` (`0x6f5fa140`) is identical: they scale damage by the record's *attack type* (0 Spells and 4 Magic use the spell factor, 2 Pierce the pierce factor). `CAbilityItemSpellReduction` (`0x6f5750a0`, the base of the map's `AIsr` spell-resistance abilities `A0AH`, `A0AN`, `A0FN`, `A0H0`) scales attack type 0 only. `CAbilityMagicImmunity` (phase 5) appears to zero attack type 4 (Magic) damage, so it does not block attack type 0 Universal damage. Every native Universal damage site (`CAbilityDeathAndDecay` pulse `0x6f6239c0`, and the sites at `0x6f55a3e0`, `0x6f57ede0`, `0x6f5d91b0`, `0x6f60cb20`) builds its record with the default attack type 0, so Defend and spell reduction apply to it. The clone had assumed otherwise for Starfall; `starfall.rs` now routes unit damage through `spell_damage_after_defend` and its test proves a partial `AIsr` factor reduces it. Open: Snowveil's explosion and other script-passed `UnitDamageTarget` calls use whatever attack type the map script passes, which has not been traced, so `snowveil.rs` is unchanged. Audit: [night-elf-implementation.md](night-elf-implementation.md).

- [x] **Cleaving Attack placement, damage kind and armor (Naga, Chaos).** **Ghidra** (`CAbilityCleavingAttack`, vtable `0x6faea080`, post-hit hook `0x6f5016d0`): the area centre is the attacker's position plus one area radius along the attacker's facing; the secondary damage is the ability's percentage of the hit record's amount (read before any target-specific reduction) applied through the shared area-damage helper `0x6f7032d0` with damage kind `0x20` (Enhanced, JASS damage type 5) and the original attack's attack type, so numeric armor points do not apply (they only apply to kind `0x10`) while the attack-type by armor-type multiplier does. The primary target is skipped by the callback `0x6f4f6e00`. The clone matches (`combat.rs` uses `DamageArmorRule::IgnorePoints`); it offsets toward the target rather than along the facing angle, which coincides for a melee swing at the target. Audit: [naga-implementation.md](naga-implementation.md).

- [x] **Bounce next-target selection.** **Ghidra**: `CMissileBounce` (vtable `0x6fb0a068`) impact handler `0x6f703d70` calls `0x6f704fa0`, which enumerates units within the bounce radius of the missile position and keeps, via `0x6f705050`, the candidate with the strictly smallest distance (initial best distance is a large constant). The current target and already-hit units are excluded by the validator `0x6f702550`; the bounce counter is decremented before the search. The clone previously used a keyed-random rank; it now selects the nearest unit and breaks exact ties by `SimId` (the engine uses group enumeration order, which is not reproducible). Remaining uncertainty: whether the engine metric is centre distance (the unit-position accessor suggests so) rather than edge distance. Audit: [elven-melee-autocast.md](elven-melee-autocast.md).

- [x] **Universal damage armor handling (Death and Decay, Starfall, Whirlwind).** **Ghidra** (`0x6f6cf530`): armor-point reduction only for kind Normal `0x10`; the attack-type by armor-type multiplier applies to all kinds. Matches the clone. Audit: [undead-implementation.md](undead-implementation.md).

- [x] **City of Decay stacking.** Only the oldest standing building per team contributes. Evidence: tooltip plus **In-game** confirmation by the project owner. Audit: [undead-implementation.md](undead-implementation.md).
- [x] **Death and Decay pulse timing and damage kind.** **Ghidra**: per-body random first delay in the first second, one-second period, Universal damage kind. Audit: [undead-implementation.md](undead-implementation.md).

## Engine reference (shared findings)

- Damage record (`CUnit` vtable `0x6fb68914`, slot `+0x120` = `0x6f6cf530`): `[0]` source, `[1]` weapon type, `[3]` flags (`0x100` attack, `0x40` suppress retaliation bookkeeping, `0x10` zero-amount), `[4]` amount, `[5]` damage kind bit, `[8]` attack type.
- Damage kind bit is `1 << JASS damage type`: Normal `0x10`, Enhanced `0x20`, Divine `0x2000`, Magic `0x4000`, Spirit Link `0x1000000`, Universal `0x4000000`.
- Attack type indices equal the JASS values: 0 Spells (named `ATTACK_TYPE_NORMAL` in JASS), 1 Normal, 2 Pierce, 3 Siege, 4 Magic, 5 Chaos, 6 Hero; the multiplier tables are `DamageBonus<Name>` at `0x6fd38708 + (attackType * 8 + armorType) * 4`.
- Damage listeners are objects with a phase accessor at slot `+0xf8` and an apply at slot `+0xfc`; the damage routine runs phase 1 first, then phases 2 to 8 in order. Defend and Magic Defense share listener `0x6f5f9f90` (phase 6).
- RTTI recovery: `python3 tools/wc3-engine/rtti.py GAME_DLL vtable CLASS` prints a class's vtable slots; `slot OFFSET PREFIX` lists classes overriding a slot and `const OFFSET PREFIX` decodes constant-returning slots (for example listener phases at `0xf8`). Set `WC3_RTTI_CACHE` to a file path to cache the ~40 s class scan. Functions that Ghidra has not defined can be created at the slot address before decompiling.
