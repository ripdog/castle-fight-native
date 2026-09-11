# Builder and Item Gameplay

Status: **normative gameplay boundary, provisional item catalog semantics**

## 1. Purpose

This document defines the player's sole directly controlled unit, the builder, and the authoritative item inventory/effect system attached to it.

The builder is intentionally outside ordinary combat. Items may influence combat, but the builder itself must not become an attacker, target, blocker, tank, or aggro-management tool.

## 2. Exactly one commandable unit

Each active player has exactly one builder under normal rules.

Ordinary combat units MUST NOT accept player-authored orders. The builder is the only unit with direct movement commands.

The builder may be selected and moved by its owner. Ownership validation occurs server-side.

## 3. Builder purposes

The builder has exactly two core gameplay responsibilities:

1. construct buildings in the player's owned build region;
2. hold and use items.

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

The builder may still need collision/pathing rules for its own movement relative to static terrain/buildings. Those rules are independent of whether combat units collide with the builder.

## 5. Authoritative builder state

Canonical builder state includes at least:

```text
SimId
OwnerPlayer
Team
SimPosition
movement state/destination
inventory
item cooldown/charge state
```

The builder's position is authoritative because local-range items and any proximity-based building interaction may depend on it.

The builder need not exist in the same dynamic broad-phase buckets used for combat-unit collision/targeting. A dedicated/query-filtered index may be used where item range checks require its position.

## 6. Owned build region

A player's building placement is restricted to that player's canonical owned build region.

In the standard Castle Fight-style map this is the player's third of the battlefield.

Map content MUST define the region authoritatively; the client visualizes it but does not define it.

The build region restriction is independent of navigation-connectivity checks. Legal placements may still complete cages.

## 7. Builder movement commands

A builder move command conceptually contains a canonical destination:

```rust
MoveBuilder {
    destination: SimPoint,
}
```

The server validates ownership and destination legality, assigns the accepted command a canonical tick/order, and all simulations apply the same movement intent.

Builder movement MUST NOT push, stop, separate, or reroute combat units.

The exact builder movement/navigation model is open: it may navigate around static buildings/terrain or use another deterministic rule, provided this does not affect combat-unit pathing.

## 8. Building construction relationship

Building placement commands are player commands associated with the builder/player.

The standard rules require placement inside the player's owned region. Whether the builder must physically approach the site, has a construction range, or construction is effectively remote/instant is a compatibility/game-feel rule to verify separately.

No implementation should assume builder proximity unless content/rules explicitly require it.

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
    target: AbilityTarget::Area {
        center: SimPoint,
        radius: SimDistance,
    },
}
```

The server validates:

- item belongs to the player's builder;
- item is ready and has sufficient charges/resources;
- target coordinate is valid under the item's rules;
- match state permits activation.

At the canonical execution tick, eligible units in the authoritative area receive the deterministic effect.

The target coordinate MUST be authoritative fixed-point/grid space, never a raw client render-space float.

## 14. Item projectiles and combat interaction

Item-generated projectiles reuse the delivery modes and projectile state machines in `15-targeting-combat.md`.

An item may therefore create guaranteed-hit homing/interpolated ranged delivery, ballistic target-zone delivery, or another explicitly defined ability projectile.

The source semantics remain separate from targetability. A projectile may have originated from an invulnerable builder-held item without making that builder a valid target.

## 15. Item effects and target behavior

Taking damage is not itself a generic command to attack the damage source.

Standard combat units continue their autonomous target lifecycle. An item only changes target behavior when its explicit effect says so, such as a future taunt/forced-target status.

This rule is essential for offensive builder items: enemy units may be damaged by them without trying to chase or attack the builder.

## 16. Disconnect behavior

On disconnect, the builder remains in canonical state.

Autonomous/passive item effects SHOULD continue because the match continues on the authoritative server.

Player-activated items naturally cannot receive new commands while the player is absent.

The builder SHOULD stop or complete its current movement according to one explicit disconnect policy; disconnection MUST NOT cause nondeterministic movement.

## 17. Death/elimination behavior

Because the builder is normally invulnerable/non-combat, ordinary battle death does not remove it.

If player elimination removes/disables a builder or inventory, the transition must be an explicit match rule with deterministic timing. Passive auras/automatic items must stop or transfer exactly as specified by that rule.

## 18. Presentation

The client should make the builder visually distinct from combat units and expose:

- movement control;
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
PlaceBuilding
SellBuilding
PurchaseUpgrade
UseItem
ActivateBuildingAbility
```

A command that attempts to direct an ordinary combat unit MUST be structurally impossible where practical, otherwise explicitly rejected by validation.

## 20. Snapshot and replay requirements

Snapshots MUST preserve all future-relevant builder/item state, including:

- builder position/movement state;
- inventory contents and stable item instance IDs;
- charges;
- cooldown/ready ticks;
- automatic-item cast/attack sequences;
- any persistent aura state not fully derivable from inventory.

Replay uses ordinary accepted builder/item commands and deterministic autonomous item behavior.

## 21. Required tests

The builder/item suite MUST eventually verify:

1. ordinary combat units reject/no-op no player order because no such valid command exists;
2. only the owner can move a builder;
3. builder never appears in ordinary enemy target candidate sets;
4. builder does not affect combat-unit pathing or local separation;
5. builder cannot be used to complete a cage;
6. building placement outside the player's owned region is rejected;
7. automatic offensive item damages eligible nearby enemy without changing that enemy's target merely toward the builder;
8. map-wide aura affects eligible friendlies regardless of builder position;
9. active area item uses the canonical selected area and affects exactly the eligible units there;
10. item cooldown/charge state survives snapshot/rejoin;
11. automatic/passive item effects continue deterministically during owner disconnect;
12. replay and different worker counts produce identical item outcomes.
