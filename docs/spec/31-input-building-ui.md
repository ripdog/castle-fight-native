# Input, Building Placement, and Player UI

Status: **normative command boundary, provisional UX details**

## 1. Purpose

This document defines how player actions move from local input/UI into deterministic authoritative commands, with special attention to building placement because buildings modify navigation topology and enable caging.

## 2. Input is not authority

Mouse/keyboard/controller input is client-local presentation state.

A click becomes authoritative only after the client constructs a `PlayerCommand`, the server validates it, and the server assigns it a canonical simulation tick/order.

Local UI may predict expected results, but prediction does not mutate canonical server state.

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

The client command card is a generic 4×3 Warcraft-style **action panel**, not a permanently open build menu. A selected builder exposes its applicable commands there (currently Move, Repair, Blink, and Build); a selected production building exposes production-building actions; and an attack-capable selected tower exposes Attack. Build opens the building submenu and its versioned 9.27 command hotkey is `B`. The bottom-right command-card cell is Cancel/Back where a modal/submenu needs it. The set of buildable entries, their rawcodes/definitions, upgrade targets, command-card data, and presentation lookup version all come from the match's already-resolved content bundle; the client MUST NOT maintain a separate hardcoded playable-building roster or independently fall back to a default map version. Visible actions use their versioned Warcraft UI icons through the presentation catalog: object-backed actions resolve object rawcode plus authored icon role, while stock engine commands use semantic command bindings. Text remains as an overlay/fallback rather than becoming the asset identity.

Production-building actions use the same panel rather than a separate production UI. The first executable action is **Upgrade**: upgrade edges come from the selected map version's extracted production-building precursor/successor graph, and a target that has an implemented precursor is not simultaneously offered as direct builder placement. The upgrade action occupies the target building's authored Warcraft command-card position, with deterministic fallback only in mixed verification catalogs where unrelated races collide. The action-panel representation is deliberately extensible so future queue controls, production toggles, rally-like commands, or other production-building actions can be added to this same selected-building surface without creating a parallel control path.

Point/entity/build-placement commands use one shared targeting modality. Choosing Move, Repair, Blink, Attack, or one building enters a targeting mode and replaces the command card with only **Cancel**. `Esc` performs that same cancellation. The client uses the selected Castle Fight version's Warcraft cursor presentation for this modality: ordinary interaction uses the stock Normal cursor, battlefield targeting uses the stock Target crosshair, build placement uses the stock InvalidTarget cursor when the locally known footprint/resource checks reject the point, and Blink uses InvalidTarget when the destination is outside its authored range. Hovering client UI while a targeting command is armed restores the Normal cursor. This cursor state is presentation-only and MUST NOT become authoritative placement/target validation. Future point-target actions, including Rescue Strike when exposed by the native command surface, use the same shared Target cursor rather than adding action-specific cursor paths. Build placement is nested: cancelling a selected building returns to the build submenu, while pressing `Esc`/Cancel from the build submenu returns to the ordinary action panel. Successful placement likewise returns to the build submenu so another building may be chosen, and it MUST keep the builder selected rather than treating the placement click as a new battlefield selection. Production-building Upgrade is an immediate authoritative command rather than a targeting mode: once accepted, the same building `SimId` enters upgrade construction and its panel exposes construction Cancel. Cancelling that upgrade restores the precursor building in place and keeps it selected; cancelling a newly placed unfinished building removes that site and therefore clears the selection. A battlefield right-click is Warcraft's context-sensitive **Smart** order and first returns the action panel to its ordinary state regardless of the currently open submenu/targeting mode. For the Castle Fight actors currently under manual control, Smart dispatches to Repair when a builder right-clicks a damaged friendly building or mechanical unit, to Follow when a builder right-clicks another unit/building that does not qualify for Repair, to Move when a builder right-clicks open ground, and to Attack when a controllable tower right-clicks an enemy unit/building. A tower Smart-click on ground or a friendly target only cancels the current modal state because towers cannot move. WC3 Smart-order branches whose underlying gameplay does not exist in Castle Fight (for example resource gathering) are not synthesized. Right-clicking an autocast-capable command button follows Warcraft command-card semantics instead of issuing a world Smart order; currently right-clicking the builder's Repair button toggles Repair autocast.

Command-card coordinates and hotkeys come from Warcraft/map content rather than native layout taste and are part of the selected Castle Fight version's content definition. Stock command data used by 9.27 supplies Move `(0,0)`, Attack `(3,0)`, Build `(0,2)` with hotkey `B`, and Cancel `(3,2)`. Castle Fight 9.27 object data places Repair at `(1,1)` and its live builder Blink at `(1,2)`. Building submenu entries use their versioned extracted unit button positions (`ubpx`/`ubpy`). Native/client callsites consume the version-aware content API rather than embedding those coordinates or hotkeys directly. Synthetic mixed-race verification menus may contain authored-slot collisions that cannot occur in one normal race catalog; those collisions are resolved into otherwise-empty slots deterministically while preserving every non-conflicting authored position.

Hovering a building in the Build submenu displays that building's exact versioned Warcraft basic and extended object-data tooltips rather than a native paraphrase. Warcraft text markup is interpreted at presentation time: classic `|cAARRGGBB` color starts, `|r` color reset, `|n` line breaks, and `||` literal-pipe escaping are supported (letter tags case-insensitively), while malformed or unknown pipe sequences remain visible literally instead of silently dropping text. The source tooltip strings remain extracted content so a future map version can supply different copy without changing client callsites.

## 4. Builder control and building placement

The builder's movement is authoritative because its position may matter to builder-local item effects and any placement interaction/range rules. The builder itself remains non-combat and does not participate in combat-unit collision, path blocking, targeting, threat, or damage resolution.

Building placement and builder movement are restricted to the owning team's authoritative build area. In the standard map this is the team's third of the battlefield; map content defines the exact canonical region.

Recommended client placement flow:

1. player activates Build and selects a buildable type from the build submenu;
2. client enters the shared placement/targeting mode;
3. cursor is projected into authoritative map coordinates;
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

The client SHOULD show building footprint and relevant constraints before confirmation. Build-menu entries whose current known gold/lumber cost cannot be paid SHOULD be visibly disabled and MUST NOT enter placement mode. Locally queued placements MAY reserve their displayed cost for UI affordability until authoritative processing so rapid clicks do not misleadingly appear affordable.

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
