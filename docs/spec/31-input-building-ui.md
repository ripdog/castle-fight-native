# Input, Building Placement, and Player UI

Status: **normative command boundary, provisional UX details**

## 1. Purpose

This document defines how player actions move from local input/UI into deterministic authoritative commands, with special attention to building placement because buildings modify navigation topology and enable caging.

## 2. Input is not authority

Mouse/keyboard/controller input is client-local presentation state.

A click becomes authoritative only after the client constructs a `PlayerCommand`, the server validates it, and the server assigns it a canonical simulation tick/order.

Local UI may predict expected results, but prediction does not mutate canonical server state.

Until the network server from step 9 exists, the development client uses the shared `MatchDriver` locally: UI handlers submit `PlayerCommand`s, the driver performs admission, assigns client sequence/tick/within-tick order, explicitly finalizes even empty ticks, executes commands through the shared executor, and returns structured outcomes. This is the same simulation-facing path future server-finalized input will use. F8 cheats, local pause/speed, stress population, and synthetic fixtures are explicitly development/bootstrap paths rather than ordinary player commands.

## 3. Core player command surface

Normal play has exactly one directly controlled unit per player: the builder.

The first playable game is expected to need commands such as:

- move the player's builder;
- activate the builder's Build command and select a building type for placement;
- place production/attack/spell/utility/legendary building within the owning team's build region;
- sell/destroy owned building where rules permit;
- purchase upgrade;
- use an active builder-held item, including a point/area target where required;
- activate a player-targeted building ability where that building explicitly exposes one;
- choose faction/race/random options before match;
- game-mode-specific actions.

Ordinary combat units MUST NOT expose move, attack, stop, focus-fire, ability, or other player-authored orders. Selecting a combat unit is inspection only. Attack-capable **buildings/towers** are the deliberate exception: they may expose the Warcraft-style Attack action so the player can manually choose an otherwise-valid hostile target. The tower remains stationary and ordinary range/target-category rules still apply.

A generic RTS `OrderUnit` command is intentionally outside the core protocol.

### 3.1 Warcraft-style action panel and targeting modality

The client command card is a generic 4×3 Warcraft-style **action panel**, not a permanently open build menu. It occupies the right side of the bottom console. A selected builder exposes its applicable commands there (currently Move, Repair, Blink, and Build); a selected production building exposes production-building actions; and an attack-capable selected tower exposes Attack. Actions common to every member of a multiple selection remain available and dispatch once per applicable selected entity. This includes cancelling multiple unfinished constructions/upgrades, cancelling queued production across selected production buildings, and upgrading multiple same-type production buildings once their queues are empty. Build opens the building submenu and its versioned 9.27 command hotkey is `B`. The bottom-right command-card cell is Cancel/Back where a modal/submenu needs it. The set of buildable entries, their rawcodes/definitions, upgrade targets, command-card data, and presentation lookup version all come from the match's already-resolved content bundle; the client MUST NOT maintain a separate hardcoded playable-building roster or independently fall back to a default map version. Visible actions use their versioned Warcraft UI icons through the presentation catalog: object-backed actions resolve object rawcode plus authored icon role, while stock engine commands use semantic command bindings. Text remains as an overlay/fallback rather than becoming the asset identity.

The client also exposes a compact builder-shortcut strip anchored below the top resource bar. It contains one button for every builder the local player can currently control according to the authoritative ownership/takeover rules, not a fixed number of slots. Each button resolves the builder unit's versioned Warcraft game-interface icon from its rawcode and uses that builder owner's Warcraft player colour as its border. Activating a shortcut selects that builder and recentres the client-local camera on its current presented position. Shortcut UI MUST consume pointer targeting so clicking a shortcut cannot simultaneously issue a battlefield target/order.

Production-building actions use the same panel rather than a separate production UI. The train command uses the produced unit's versioned command data, and Cancel removes one queued entry. **Upgrade** is available only after the production queue is empty: upgrade edges come from the selected map version's extracted production-building precursor/successor graph, and a target that has an implemented precursor is not simultaneously offered as direct builder placement. The upgrade action occupies the target building's authored Warcraft command-card position, with deterministic fallback only in mixed verification catalogs where unrelated races collide.

Point/entity/build-placement commands use one shared targeting modality. Choosing Move, Repair, Blink, Attack, or one building enters a targeting mode and replaces the command card with only **Cancel**. `Esc` performs that same cancellation. The client uses the selected Castle Fight version's Warcraft cursor presentation for this modality: ordinary interaction uses the stock Normal cursor, battlefield targeting uses the stock Target crosshair, build placement uses the stock InvalidTarget cursor when the locally known footprint/resource checks reject the point, and Blink uses InvalidTarget when the destination is outside its authored range. Hovering client UI while a targeting command is armed restores the Normal cursor. This cursor state is presentation-only and MUST NOT become authoritative placement/target validation. Future point-target actions, including Rescue Strike when exposed by the native command surface, use the same shared Target cursor rather than adding action-specific cursor paths. Build placement is nested: cancelling a selected building returns to the build submenu, while pressing `Esc`/Cancel from the build submenu returns to the ordinary action panel. Successful placement likewise returns to the build submenu so another building may be chosen, and it MUST keep the builder selected rather than treating the placement click as a new battlefield selection. Production-building Upgrade is an immediate authoritative command rather than a targeting mode: once accepted, the same building `SimId` enters upgrade construction and its panel exposes construction Cancel. Cancelling that upgrade restores the precursor building in place and keeps it selected; cancelling a newly placed unfinished building removes that site and therefore clears the selection. A battlefield right-click is Warcraft's context-sensitive **Smart** order and first returns the action panel to its ordinary state regardless of the currently open submenu/targeting mode. For the Castle Fight actors currently under manual control, Smart dispatches to Repair when a builder right-clicks a damaged friendly building or mechanical unit, to Follow when a builder right-clicks another unit/building that does not qualify for Repair, to Move when a builder right-clicks open ground, and to Attack when a controllable tower right-clicks an enemy unit/building. A tower Smart-click on ground or a friendly target only cancels the current modal state because towers cannot move. WC3 Smart-order branches whose underlying gameplay does not exist in Castle Fight (for example resource gathering) are not synthesized. Right-clicking an autocast-capable command button follows Warcraft command-card semantics instead of issuing a world Smart order; currently right-clicking the builder's Repair button toggles Repair autocast.

Command-card coordinates and hotkeys come from Warcraft/map content rather than native layout taste and are part of the selected Castle Fight version's content definition. Stock command data used by 9.27 supplies Move `(0,0)`, Attack `(3,0)`, Build `(0,2)` with hotkey `B`, and Cancel `(3,2)`. Castle Fight 9.27 object data places Repair at `(1,1)` and its live builder Blink at `(1,2)`. Building submenu entries use their versioned extracted unit button positions (`ubpx`/`ubpy`) and effective unit hotkeys (`uhot`, including inherited base-object values). While the build submenu is open those building hotkeys temporarily take precedence over ordinary client/window keyboard shortcuts; closing or leaving the submenu removes that binding. Each visible build button renders its hotkey in the top-right corner. Synthetic mixed-race verification menus may contain authored-slot or hotkey collisions that cannot occur in one normal race catalog; slot collisions are resolved into otherwise-empty slots deterministically, while hotkey collisions are tolerated and the last eligible building in the resolved direct-building order claims the key. Native/client callsites consume the version-aware content API rather than embedding those coordinates or hotkeys directly.

Hovering a building in the Build submenu displays that building's exact versioned Warcraft basic and extended object-data tooltips rather than a native paraphrase. Warcraft text markup is interpreted at presentation time: classic `|cAARRGGBB` color starts, `|r` color reset, `|n` line breaks, and `||` literal-pipe escaping are supported (letter tags case-insensitively), while malformed or unknown pipe sequences remain visible literally instead of silently dropping text. The source tooltip strings remain extracted content so a future map version can supply different copy without changing client callsites.

## 4. Builder control and building placement

The builder's movement is authoritative because its position may matter to builder-local item effects and any placement interaction/range rules. The builder itself remains non-combat and does not participate in combat-unit collision, path blocking, targeting, threat, or damage resolution.

Building placement and builder movement are restricted to the owning team's authoritative build area. In the standard map this is the team's third of the battlefield; map content defines the exact canonical region.

For Castle Fight 9.27, the central stone road inside each base is placement-blocked across seven 128-world-unit building rows (`y = -448..448`). The full road band remains blocked from the inner base edge to the castle's front edge, and the stone strips beside the castle remain blocked. Behind the castle, only the two stone strips (`y = -448..-192` and `192..448`) are blocked; the three middle mossy rows (`y = -192..192`) are buildable, subject to ordinary occupancy and authored doodad pathing. These road exclusions block building placement without changing unit navigation or the strategic movement lane. The optional map grid clips to the same static placement blockers so it shows the rear mossy cells.

Recommended client placement flow:

1. player activates Build and selects a buildable type from the build submenu;
2. client enters the shared placement/targeting mode;
3. cursor is projected into authoritative map coordinates and, when the default-enabled grid-snap
   checkbox is active, the footprint origin is snapped to the owning side's build grid;
4. local placement preview evaluates current known occupancy/build rules;
5. UI shows legal/illegal preview;
6. player confirms;
7. client sends `PlaceBuilding` command with canonical position/rotation representation;
8. server validates against canonical state;
9. server rejects admission or schedules the command for a future/canonical tick; execution may still deterministically fail if state changes before that tick;
10. all simulations apply accepted placement identically.

## 5. Canonical placement coordinates

The network command MUST NOT send arbitrary render-space floating point transforms.

Preferred options:

- integer build-grid coordinates + discrete rotation;
- deterministic fixed-point coordinates validated against snapping rules.

If buildings are grid-aligned, grid coordinates are strongly preferred because occupancy/caging behavior becomes simpler and less error-prone.

## 6. Placement preview

The client SHOULD show building footprint and relevant constraints before confirmation. While one building type is armed for placement, the client always shows the snapped footprint grid that confirmation would use. The grid evaluates each covered navigation cell through simulation-owned placement diagnostics: individually legal cells are green and cells blocked by navigation/build-region limits, authored placement blockers, existing buildings, or live ground-unit occupancy are red. These cell colors are explanatory presentation data only; the whole-footprint placement validation remains authoritative and MUST be re-evaluated on confirmation. When the currently known whole-footprint placement check succeeds, the client additionally shows that building's resolved Warcraft model in its completed/Stand form, rendered with the same ordinary depth, lighting, culling, filter-mode, and terrain-occlusion behavior as the completed building but tinted green. When the whole-footprint placement check fails, no building model preview is rendered so the blocked cells remain unobscured. Because Warcraft building models are rigid while the presentation terrain can vary across one footprint, the valid preview and completed building presentation use the highest terrain sample covered by the footprint as their common support elevation; this prevents low model geosets from being clipped by a higher footprint edge and avoids a vertical jump after confirmation.

Build-menu entries whose current known gold/lumber cost cannot be paid SHOULD be visibly disabled and MUST NOT enter placement mode. Locally queued placements MAY reserve their displayed cost for UI affordability until authoritative processing so rapid clicks do not misleadingly appear affordable.

Preview may display:

- occupied cells;
- buildable/unbuildable terrain;
- overlap;
- resource cost;
- production exit/spawn indication;
- attack range;
- optional navigation/flow consequences for advanced/debug UI.

The preview MUST NOT claim authoritative acceptance until the server accepts the command.

## 7. Caging and placement legality

The UI MUST NOT mark a placement illegal merely because it disconnects a traversable area or traps friendly units, unless a later explicit gameplay rule intentionally changes that behavior.

A final walling building that completes a legal cage must remain placeable if all ordinary footprint/resource rules pass.

Authored doodad/destructable pathing may contribute **placement-only** no-build cells even when the same object is intentionally walkable or flyable. Those cells participate in ordinary footprint validation without being promoted into unit-navigation blockers. Conversely, purely visual overhang outside the authored no-build footprint does not make a nearby snapped building placement illegal.

The client and server share the same deterministic placement validation code where practical, but server validation remains authoritative.

An accepted pending builder construction order reserves its canonical footprint before construction begins. Later placement commands, including commands finalized in the same tick, test those reservations in canonical command order. A losing contention command is rejected before spending resources; replacing the same builder's own pending order may ignore that builder's existing reservation while still respecting every other builder's reservation. Cancelling or otherwise dropping the pending order releases the reservation. This prevents two players from both being charged for the same future site and makes build contention an immediate deterministic command outcome rather than a later race between builder arrival times.

## 8. Prediction and reconciliation

For low latency, the client MAY show a speculative building immediately after sending placement.

Speculative presentation MUST be visibly/reliably reconcilable:

- acceptance: convert/bind to authoritative `SimId` once command is applied;
- rejection: remove speculative object and display reason;
- altered schedule: keep visual feedback without assuming production/resources begin before authoritative tick.

An early implementation MAY simply wait for acceptance to reduce complexity.

## 9. Command identity

Each client command SHOULD carry a monotonic client sequence/nonce so reconnect/retry cannot accidentally apply the same click twice.

The UI can associate pending state with this ID.

Conceptually:

```text
local cmd #441: place barracks at (17, 23)
    pending...
server: accepted -> tick 10124, order 2
```

## 10. Rejection reasons

The server SHOULD return structured rejection reasons suitable for user feedback, such as:

- insufficient resources;
- occupied;
- unbuildable terrain;
- outside allowed region;
- invalid building type;
- unavailable/locked content;
- match state disallows action;
- stale ownership reference;
- rate limited.

A rejection is expected protocol behavior and must not be treated as a desync.

## 11. Race between preview and authoritative state

The client preview may be legal when clicked but invalid by the time the server validates it because another command consumed resources or occupied the location.

This is normal.

The server's canonical tick/order resolves conflicts. UI should explain the rejection rather than trying to force local outcome.

## 12. Building footprint visualization

Because occupancy is strategically significant, the client SHOULD make footprint boundaries comprehensible.

Potential affordances:

- a player-toggleable map grid aligned to the production-building footprint scale; its origin and clipping are derived from the selected release's canonical build-region/lane geometry, so it does not extend into the lane band, centre no-man's-land, side dead-space, or other terrain outside those authored regions; the left overlay preserves the authored top/left phase beneath the wall doodads, while the right overlay is independently anchored to the top/right boundary and extends one additional building square toward the centre without leaving a gap at the outside edge; placement origins occupy the squares immediately inside those boundary lines; for Castle Fight 9.27 one grid square is one production-building footprint and every fourth line is shown as a double line;
- an independent **Snap** checkbox beside the grid-visibility button; snapping is enabled by default, applies to every building-placement preview and confirmed command, and uses the same side-specific lattice as the overlay even when the overlay itself is hidden;
- cell grid while placing;
- exact occupied-cell overlay;
- nearby building footprint outlines;
- blocked spawn exit warning if applicable without forbidding legal caging;
- attack/acquisition range overlay.

The UI should not conceal collision/pathing geometry behind cosmetic meshes.

## 13. Production status UI

Production buildings SHOULD expose enough state for the player to understand autonomous behavior:

- next spawn progress;
- produced unit type;
- disabled/stunned state if applicable;
- upgrades affecting spawn;
- ownership;
- potentially whether spawn area is heavily congested.

A building inside/creating a cage remains ordinary; the UI should not label it erroneous solely for disconnected navigation.

## 14. Unit inspection

Standard combat units are autonomous and MAY be selectable for inspection.

The local selection set holds at most 24 stable `SimId`s. Ground clicks leave it unchanged. Ordinary clicks may inspect any player's unit, builder, or building, including opponents; selection never grants command authority. Shift-click toggles membership; Ctrl-click or double-click selects visible entities of the same type and owner. A drag rectangle prefers locally owned builders, then locally owned production buildings, then locally owned military units. If none are enclosed, it selects other players' builders, buildings, or military units in that priority order for inspection. Flying units are tested at their displayed altitude, not their ground position. Shift-drag adds the eligible box result. Grave/tilde cycles idle local builders and focuses the camera on the chosen builder. Ctrl+0–9 stores a local selection group, 0–9 recalls it, and a rapid second press focuses the camera on its first surviving entity. Tab and Shift-Tab cycle the focused type within a mixed selection. Dead or vanished entities are pruned from selection and groups. All of this state is client-local and MUST NOT affect authoritative simulation, checksums, snapshots, or network commands.

The bottom console reserves a left minimap region, shows selected entity information and up to 24 extracted Warcraft unit/building icons in the centre, and places the 4×3 action panel on the right. The minimap itself is deferred. The centre panel keeps six inventory slots on its far right for future builder items; they are presently display-only. Production progress and its two-entry queue sit to the right of the selected building's information, before the inventory slots. Selection tiles show health where the authoritative presentation supplies it, and clicking a tile focuses that member; Shift-clicking removes it. The normal world view shows selection markers without diagnostic target lines or pending-footprint outlines. Those details and the performance panel remain available through the F1 development overlay.

An owned production building exposes its map-defined training command and the versioned cancel command. The two-entry queue is visible alongside its training progress and time remaining; Escape cancels one queued entry at a time, and upgrade commands become available only when the queue is empty. Any player may inspect another player's production building's health, training time, queue, and progress. The ordinary selection details show owner, health, attack and armor types, and active buffs/debuffs where represented by the simulation. Hovering a selected unit's or building's attack/armor type exposes the relative damage percentages from the active map's damage rules. An inspection-only selection, or a multiple selection containing any uncontrollable member, exposes no command actions. The offline development "Control all players" toggle allows command actions for selected player-owned builders and buildings by submitting under their owner's authority; network play does not gain that debug override.

Inspection can show:

- type/name;
- owner/team;
- health;
- attacks/armor/status;
- current target;
- movement/pathing state for debug/advanced UI;
- buffs/debuffs.

Selection MUST NOT imply or enable direct control. The builder is the sole normal exception and has its own movement/build/item interaction UI.

Development/verification clients MAY expose a **local simulation pause** for inspection. This pause stops local authoritative simulation ticks at a completed tick boundary while leaving rendering, camera control, selection, inspection, and other presentation-only UI responsive. It MUST NOT mutate gameplay state merely by being toggled, and it is distinct from canonical network/match pause controls. While locally paused, inspection SHOULD show the latest committed authoritative state rather than an interpolated in-between presentation state. Useful debug fields include the unit's current target/order, most recent attacker and attack tick, and whether direct-retaliation or ally-defense lock is active.

Offline development clients MAY also expose quicksave/quickload at completed authoritative boundaries. The default development bindings are `Ctrl+S` to write the current logical simulation snapshot and `Ctrl+O` to restore it; these modified shortcuts take precedence over command-card and camera letter bindings. Quickload rebuilds the local command driver from the restored boundary rather than pretending pre-load command history still applies. Development debug UI MAY populate completed instances of every production-building and tower definition available in the selected versioned content bundle, arranging one top-to-bottom vertical line behind each team's castle without charging player resources.

## 15. Camera/input mappings

Controls should be configurable and may include:

- edge/keyboard pan;
- middle/right-drag pan;
- wheel zoom;
- hotkeys for build categories/buildings;
- cancel placement;
- rotate building;
- select castle/production groups;
- debug overlays in development builds.

Input bindings are client configuration and are not part of deterministic state.

## 16. Latency display and command feedback

Because gameplay commands are discrete and relatively low frequency, clear acknowledgement may be more valuable than aggressive prediction.

Development UI SHOULD optionally display:

- current local/server tick estimate;
- command pending/accepted/rejected;
- measured RTT;
- scheduled command tick;
- resync/catch-up state.

Production UI can simplify this substantially.

## 17. Reconnect UI

During disconnect/reconnect, the client SHOULD clearly distinguish:

- transport lost;
- reconnecting;
- downloading snapshot;
- fast-forwarding/catching up;
- checksum verification;
- live.

The player should not be shown misleading stale placement affordances while their canonical local state is significantly behind.

## 18. Spectator/replay input mode

Replay/spectator mode shares camera/inspection UI but does not emit active player commands.

Replay controls may include pause/speed/seek, none of which alter replayed authoritative outcomes.

## 19. Accessibility and configuration

UI scale, color choices, keybindings, camera sensitivity, audio level, and similar preferences are local configuration and MUST remain outside authoritative match state.

Gameplay content must not rely solely on color where avoidable.

## 20. Required UI/command tests

Integration tests should eventually verify:

1. local legal preview + server acceptance produces exactly one building;
2. duplicate command retry is idempotently recognized;
3. stale legal preview can be server-rejected without desync;
4. completing a cage is accepted when ordinary placement rules permit it;
5. rejected speculative building leaves no authoritative residue;
6. building coordinate serialization round-trips exactly;
7. replay/spectator input cannot emit gameplay commands;
8. reconnect disables/reconciles stale pending placement state safely;
9. no ordinary combat unit exposes an authoritative move/attack/order action;
10. builder movement is accepted for the connected owner, or for a connected teammate while that owner is canonically disconnected;
11. builder movement outside the owning team's area is rejected;
12. placement outside the owned team build region is rejected;
13. active item point/area targets serialize to canonical authoritative coordinates;
14. manually targeted building abilities validate control permission, cooldown/resources, and target semantics before scheduling.
