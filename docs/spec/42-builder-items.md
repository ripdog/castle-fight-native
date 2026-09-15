# Builder and Item Gameplay

Status: **normative gameplay boundary, provisional item catalog semantics**

## 1. Purpose

This document defines the player's sole directly controlled unit, the builder, and the authoritative item inventory/effect system attached to it.

The builder is intentionally outside ordinary combat. Items may influence combat, but the builder itself must not become an attacker, target, blocker, tank, or aggro-management tool.

## 2. Exactly one commandable unit

Each active player has exactly one builder under normal rules. Native match bootstrap now creates one authoritative builder per configured `PlayerId`; ownership remains with that player even when a disconnected owner temporarily delegates builder control to a connected teammate.

Ordinary combat units MUST NOT accept player-authored orders. The builder is the only unit with direct movement commands.

The builder may be selected and moved by its owner. Ownership validation occurs server-side.

## 3. Builder purposes

The builder has exactly three core gameplay responsibilities:

1. summon/construct buildings in the owning team's build region;
2. repair friendly buildings;
3. hold and use items.

Additional convenience UI or cosmetic behavior MUST NOT accidentally make the builder a combat participant.

## 4. Non-combat invariants

The builder MUST:

- have no ordinary attack;
- be excluded from enemy combat target candidate sets;
- be unaffected by ordinary unit damage unless a future explicit rule says otherwise;
- not block strategic navigation topology;
- not participate in combat-unit local separation/collision;
- not provide a body-blocking surface;
- not be usable as a cage wall;
- not draw aggro/retaliation because an item it carries damages an enemy;
- not count as a combat unit for global enemy/friendly effects unless the effect explicitly targets builders.

The standard rules SHOULD model the builder as invulnerable to ordinary battle damage.

The builder is a ground-height non-combat unit, not a flying unit. Its rendered height follows the terrain under it. Terrain pathing, static blockers, buildings, and units do not obstruct or redirect builder movement; its owning build-region boundary is the only movement boundary. This obstacle immunity is a builder-specific movement rule and MUST NOT be represented by treating the builder as an air combat unit.

## 5. Authoritative builder state

Canonical builder state includes at least:

```text
SimId
OwnerPlayer
Team
SimPosition
movement state/destination
repair target/progress
appearance/source builder identity
locomotion metadata
build catalog/menu
inventory
item cooldown/charge state
```

The builder's position is authoritative because local-range items and any proximity-based building interaction may depend on it.

The builder need not exist in the same dynamic broad-phase buckets used for combat-unit collision/targeting. A dedicated/query-filtered index may be used where item range checks require its position.

### 5.1 Race appearance and build catalog

Native gameplay uses one configurable builder entity/type rather than a different simulation type for every WC3 builder rawcode. Configuration separates **appearance/locomotion identity** from the **authoritative build catalog**.

In standard non-draft modes, choosing a race configures the builder with that race's extracted builder rawcode/name, locomotion metadata, movement profile, and ordered building catalog. The 9.27 extraction contains 15 race catalogs; Critter is campaign-only, leaving 14 standard race builders. Normal race builders move at 550 world units/second. Corrupted Builder is the extracted `hover` movement-type exception; this is locomotion/presentation metadata and does not make it an air unit. Campaign-only Critter Builder uses the extracted 190 movement speed.

In draft modes, the same builder entity remains in place while its build catalog is replaced by the buildings granted by the draft. Drafting therefore does not require synthesizing a new unit class or overloading a WC3 race-builder rawcode. Appearance may remain fixed or be configured separately by the mode, but build legality always reads the current authoritative catalog.

The WC3 builder rawcodes remain content provenance and presentation identities. They are not native simulation classes.

## 6. Owned build region

A player's building placement is restricted to that player's/team's canonical owned build region.

In the standard Castle Fight-style map this is the team's third of the battlefield. The builder itself is confined to that same team area and cannot be ordered or move outside it.

Map content MUST define the region authoritatively; the client visualizes it but does not define it.

The build region restriction is independent of navigation-connectivity checks. Legal placements may still complete cages.

## 7. Builder movement commands

A builder move command conceptually contains a canonical destination:

```rust
MoveBuilder {
    destination: SimPoint,
}
```

The server validates control rights/admission and team-area bounds, assigns the movement command a canonical tick/order, and all simulations execute or reject it identically from canonical state at that tick.

Builder movement MUST NOT leave the owning team's area and MUST NOT push, stop, separate, or reroute combat units. It travels directly toward its authoritative destination at its configured movement rate, remains at ground/terrain height for presentation, and ignores terrain pathing, static blockers, buildings, and units.

### 7.1 Blink

The round-start runtime replaces the stale object-data Blink (`A001`) with the scripted builder Blink `A0-1`. Native gameplay MUST follow the runtime behavior, not the removed object-data ability.

For 9.27, Blink is a zero-mana, zero-cooldown point command with **10,000 world units** cast range. On a successful cast, the requested point is clamped independently on X and Y to the owning castle/base rectangle with a **64 world-unit inset**, the builder is teleported immediately to that resolved point, and its current order is stopped. Native Blink therefore cancels any active move, follow, repair, or pending build order while preserving the Repair autocast toggle state. Because the builder is non-colliding and ignores terrain/blockers, Blink does not perform ordinary pathability or occupancy checks inside the owning base rectangle.

### 7.2 Smart follow

Warcraft Smart right-click on a unit/building that does not resolve to a higher-priority contextual action issues a target-following movement order. Native builders therefore retain the target `SimId` rather than reducing Smart to a one-time point move. The builder tracks the live target deterministically, stopping at a building footprint edge or the target unit's collision envelope and resuming movement if the target moves away. Follow uses the same builder-only obstacle immunity and owning-build-region boundary as ordinary Move; if the target disappears or following it would require leaving the owning build region, the follow order ends. Repair takes precedence over Follow for damaged friendly repairable targets.

## 8. Building construction relationship

Building placement commands are player commands associated with the builder/player.

The standard rules require placement inside the owning team's build region. A build click creates an authoritative builder construction order rather than constructing remotely. Placement legality is checked when the order is accepted and again when construction begins; it still checks the map's placement restrictions, static obstacles, existing buildings, and ordinary ground-unit occupancy.

Castle Fight builders use the stock Warcraft worker construction-contact rule. The stock `AHbu` Build ability has no editable `Rng` field; native compatibility uses Warcraft's **50 world-unit** worker construction contact range. If the requested footprint is farther away, the builder walks directly toward it using the same non-colliding, blocker-ignoring movement semantics as ordinary builder movement. Construction begins only after the builder reaches that range. At that point an authoritative construction site is created immediately and its footprint blocks placement/navigation, but the building's production, attacks, automatic spells, income contribution, and completion lumber reward remain inactive until the construction timer finishes.

Construction duration is versioned content taken from the resolved Warcraft `Build Time` (`ubld` / `bldtm`) field rather than a presentation constant. In 9.27 the five production buildings exposed by the current native slice each have a **2-second / 60-tick** construction window. The extracted Watch Tower (`h006`) and Poof Tower (`h07P`) instead have **20-second / 600-tick** build times; native compatibility preserves those values unless later script extraction proves a runtime override.

Gold/lumber cost is committed when the build order is accepted. Replacing/cancelling an order before construction begins, or cancelling the authoritative construction site before its completion tick, refunds the committed construction cost according to the map's extracted `ConstructionRefundRate`; 9.27 sets this to **1.0**, so the refund is complete. A cancelled site grants no construction lumber and contributes no income. If the footprint becomes illegal before the builder reaches it, the pending order is cancelled and the unstarted construction cost is refunded. Completion removes the construction state, grants any extracted construction-lumber reward, activates income, and starts production/attack/spell timers from that completion tick.

### 8.1 Repair

Repair is a targeted builder order against a living friendly **building or mechanical unit**. The mechanical eligibility comes from the extracted WC3 unit classification rather than race/name-specific native lists. An active repair order replaces the builder's movement destination; if the target is outside repair range, the builder moves directly toward it while continuing to ignore pathing, units, buildings, and static blockers. The order ends when the target reaches full health, becomes invalid, or another builder command replaces it.

For the 9.27 compatibility profile, the builder moves at **550 world units/second**, Repair has **50 world units** cast range, and the extracted Warcraft Repair time ratio is **1.5×**. Repair duration comes from the target object's extracted **Repair Time** field (`urtm` / base-table `reptm`), not from construction/build time. The native sim multiplies that target-specific value by Repair's 1.5× ratio and distributes the target's full HP across the resulting deterministic tick count. Important 9.27 examples are: Main Castle `urtm=700` → **1050 seconds** full repair-equivalent time for one builder; Barracks `urtm=70` → **105 seconds**; Watch Tower `urtm=110` → **165 seconds**; Catapult `urtm=36` → **54 seconds**. Integer remainder accumulation keeps partial repair deterministic. Repair cost/resource charging is deferred until player resources are authoritative; the extracted 0.35 Repair Cost Ratio must be applied when that system lands rather than silently discarded.

Repair autocast is authoritative and toggleable. For all 14 standard 9.27 race builders it is **enabled by default**: their extracted WC3 `Default Active Ability` is Repair (`Ahrp`, or a race-specific Repair derivative such as `A071`/`A08G`). The campaign-only Critter Builder still has Repair but leaves `Default Active Ability` blank, so its native default is autocast off. Repair exposes the canonical `repairon` / `repairoff` toggle orders and auto-repair tooltips. When autocast is enabled and the builder has no explicit move, follow, repair, or build order, it may acquire a damaged friendly repairable target within the builder's extracted **500 world-unit acquisition range** and move to repair it. Disabling autocast prevents new automatic acquisitions but does not cancel an explicit or already-active repair order.

Until exact Warcraft built-in Repair autocast target-priority behavior is separately recovered, native automatic acquisition is normalized deterministically: choose the nearest eligible damaged target, breaking equal-distance ties by `SimId`. Explicit player orders always take precedence over autocast acquisition.

## 9. Inventory

The builder owns an authoritative item inventory.

A provisional representation is:

```rust
pub struct Inventory {
    pub slots: Vec<ItemInstance>,
}

pub struct ItemInstance {
    pub item: ItemId,
    pub instance_id: ItemInstanceId,
    pub charges: Option<u16>,
    pub ready_tick: Tick,
}
```

Stable instance identity is useful when two copies of the same item can exist with separate charges/cooldowns.

Inventory capacity, acquisition, dropping, trading, purchasing, and replacement rules are content/gameplay concerns not yet fixed.

## 10. Item activation classes

Items may be:

```text
Passive
Automatic
PlayerActivatedUntargeted
PlayerActivatedEntity
PlayerActivatedPoint
PlayerActivatedArea
```

Items use the deterministic ability/effect machinery specified in `16-abilities-spellcasting.md`.

## 11. Automatic offensive items

An item may automatically fire a projectile or other attack at nearby enemy combat units.

Its range query is centered on the builder's authoritative position unless content defines otherwise.

The item chooses targets according to its explicit deterministic target policy. The builder itself still has no attack profile and never becomes a valid retaliation target.

Damage attribution may retain builder/player/item identity for statistics, but ordinary units MUST NOT change behavior solely because the damage source traces back to the builder.

## 12. Passive map-wide aura items

An item may grant a map-wide aura benefiting eligible friendly combat units.

The aura remains active while the item is in the qualifying inventory/equipped state according to content rules.

The implementation MAY represent this as a global team modifier rather than materializing one aura entity/status per unit every tick, provided observable authoritative results are identical.

The aura MUST be snapshot/replay safe and deterministic when items are gained, lost, disabled, or duplicated.

## 13. Active area-buff items

An item may allow the player to choose an arbitrary area of the battlefield and buff friendly units within that area.

Conceptual player command:

```rust
UseItem {
    item_instance: ItemInstanceId,
    target_center: SimPoint,
}
```

The authoritative effect radius is derived from the validated `ItemId`/item definition, not supplied by the client. If a future item deliberately supports a player-adjustable radius, the command must carry only a bounded authored parameter and the server must validate it against the content-defined range.

The server validates:

- item belongs to the player's builder;
- item is ready and has sufficient charges/resources;
- target coordinate is valid under the item's rules;
- match state permits activation.

At the canonical execution tick, eligible units in the authoritative content-defined area receive the deterministic effect.

The target coordinate MUST be authoritative fixed-point/grid space, never a raw client render-space float.

## 14. Item projectiles and combat interaction

Item-generated projectiles reuse the delivery modes and projectile state machines in `15-targeting-combat.md`.

An item may therefore create guaranteed-hit homing/interpolated ranged delivery, ballistic target-zone delivery, or another explicitly defined ability projectile.

The source semantics remain separate from targetability. A projectile may have originated from an invulnerable builder-held item without making that builder a valid target.

## 15. Item effects and target behavior

Taking damage is not itself a generic command to attack the damage source.

Standard combat units continue their autonomous target lifecycle. An item only changes target behavior when its explicit effect says so, such as a future taunt/forced-target status.

This rule is essential for offensive builder items: enemy units may be damaged by them without trying to chase or attack the builder.

## 16. Disconnect and delegated control

On owner disconnect, the builder remains in canonical state and continues any already-issued movement. Passive/automatic item effects continue normally.

Any still-connected teammate gains temporary authority to issue builder movement, building, and item commands for the disconnected player's builder. The builder, inventory, buildings, and player-slot resources do not change owner; teammate control is delegation only. Multiple teammates may submit commands, with conflicts resolved by the server's ordinary canonical command order.

When the owner reconnects, delegated teammate authority ends and normal owner control resumes.

These permission changes are driven by canonical server-authored disconnect/reconnect events so replay and deterministic execution do not depend on invisible transport state.

If all players on the team are disconnected, match-level rules pause the game and begin the reconnect timeout described in `41-match-gameplay.md`.

## 17. Death/elimination behavior

Because the builder is normally invulnerable/non-combat, ordinary battle death does not remove it.

If player elimination removes/disables a builder or inventory, the transition must be an explicit match rule with deterministic timing. Passive auras/automatic items must stop or transfer exactly as specified by that rule.

## 18. Presentation

The client should make the builder visually distinct from combat units and expose:

- movement control;
- explicit Repair targeting and Repair autocast state/toggle;
- Blink point targeting;
- building palette/placement;
- inventory slots;
- item cooldown/charges;
- active item targeting modes;
- owned build-region boundaries where useful.

Combat-unit selection UI must remain inspection-only so there is no ambiguity about which unit is controllable.

## 19. Networking

The protocol SHOULD define specific commands rather than a generic unit-order envelope:

```text
MoveBuilder
FollowBuilder
BlinkBuilder
RepairBuilder
SetBuilderRepairAutocast
PlaceBuilding
SellBuilding
PurchaseUpgrade
UseItem
ActivateBuildingAbility
```

A command that attempts to direct an ordinary combat unit MUST be structurally impossible where practical, otherwise explicitly rejected by validation.

## 20. Snapshot and replay requirements

Snapshots MUST preserve all future-relevant builder/item state, including:

- builder position/movement/follow target state;
- inventory contents and stable item instance IDs;
- charges;
- cooldown/ready ticks;
- automatic-item cast/attack sequences;
- any persistent aura state not fully derivable from inventory.

Replay uses the canonical stream containing builder/item commands, disconnect/reconnect delegation records, and deterministic autonomous item behavior.

## 21. Required tests

The builder/item suite MUST eventually verify:

1. ordinary combat units reject/no-op no player order because no such valid command exists;
2. connected owner can control their builder, and while that owner is disconnected any connected teammate can control it instead;
3. builder destination/movement cannot leave the team's authored third/area;
4. builder never appears in ordinary enemy target candidate sets;
5. builder does not affect combat-unit pathing or local separation;
6. builder cannot be used to complete a cage;
7. building placement outside the owned team region is rejected;
8. automatic offensive item damages eligible nearby enemy without changing that enemy's target merely toward the builder;
9. map-wide aura affects eligible friendlies regardless of builder position;
10. active area item uses the canonical selected area and affects exactly the eligible units there;
11. item cooldown/charge state survives snapshot/rejoin;
12. automatic/passive item effects and already-issued builder movement continue deterministically during owner disconnect;
13. owner reconnect revokes delegated teammate control at the canonical reconnect boundary;
14. replay and different worker counts produce identical item outcomes.
