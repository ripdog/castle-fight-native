//! The castle item system: shop stock, inventory commands, walk-then-act item orders and the
//! effects of carried and used items. Item definitions come from the version-scoped catalog; this
//! module only interprets them.
use super::*;
use crate::items::{
    BlastEffect, ItemCatalog, ItemDefinition, ItemEffect, ItemId, OrbChainLightningEffect,
    PointScrollRestorationEffect, PointScrollSpeedEffect, ShopEntry, item_catalog_for_version,
    seconds_to_ticks, speed_per_tick, world_to_subunits,
};
use crate::{
    AcquireOutcome, GroundItem, INVENTORY_SLOTS, ITEM_INTERACTION_RANGE_WORLD_UNITS, Inventory,
    InventoryError, ItemOrder, ItemUseTarget, PendingItemImpact, PendingItemOrder, TeamItemBuff,
};

const RANDOM_PURPOSE_BLAST_STAFF: u64 = 0x424c_4153_5453_0001;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemCommandError {
    NotAuthorized,
    BuilderNotFound,
    ItemSystemUnavailable,
    UnknownShopEntry,
    ShopClosed,
    OutOfStock,
    ShopButtonCooling,
    InsufficientGold { available: u32, required: u32 },
    InsufficientLumber { available: u32, required: u32 },
    InventoryFull,
    InvalidSlot,
    EmptySlot,
    NotUsable,
    OnCooldown,
    InvalidTarget,
    NotDroppable,
    GroundItemNotFound,
    RecipientNotFound,
}

impl From<InventoryError> for ItemCommandError {
    fn from(error: InventoryError) -> Self {
        match error {
            InventoryError::UnknownItem | InventoryError::EmptySlot => Self::EmptySlot,
            InventoryError::InvalidSlot => Self::InvalidSlot,
            InventoryError::Full => Self::InventoryFull,
        }
    }
}

/// Where a pending item order stands relative to its target.
enum Approach {
    Ready,
    Walk(SimPoint),
    Cancel,
}

#[derive(Clone, Copy)]
struct BuilderHandle {
    entity: Entity,
    id: SimId,
    owner: PlayerId,
    team: Team,
    position: SimPoint,
}

/// A carried item's aura, flattened for the per-tick recipient pass.
#[derive(Clone, Copy)]
struct AuraSource<'a> {
    team: Team,
    position: SimPoint,
    effect: &'a ItemEffect,
}

fn interaction_range_sq() -> u64 {
    square_i32(world_to_subunits(ITEM_INTERACTION_RANGE_WORLD_UNITS))
}

fn status_has_armor_modifier_room(status: &StatusState, modifier: &TimedArmorModifier) -> bool {
    let active = &status.armor_modifiers[..usize::from(status.armor_modifier_count)];
    usize::from(status.armor_modifier_count) < crate::components::MAX_TIMED_ARMOR_MODIFIERS
        || active.iter().any(|existing| {
            existing.id == modifier.id
                || modifier
                    .native_buff
                    .zip(existing.native_buff)
                    .is_some_and(|(incoming, current)| incoming.rawcode == current.rawcode)
        })
}

fn status_has_movement_modifier_room(status: &StatusState, id: ModifierId) -> bool {
    usize::from(status.movement_modifier_count) < crate::components::MAX_TIMED_MOVEMENT_MODIFIERS
        || status.movement_modifiers[..usize::from(status.movement_modifier_count)]
            .iter()
            .any(|existing| existing.id == id)
}

fn status_has_attack_speed_modifier_room(status: &StatusState, id: ModifierId) -> bool {
    usize::from(status.attack_speed_modifier_count)
        < crate::components::MAX_TIMED_ATTACK_SPEED_MODIFIERS
        || status.attack_speed_modifiers[..usize::from(status.attack_speed_modifier_count)]
            .iter()
            .any(|existing| existing.id == id)
}

/// Item auras never crash a unit that already carries a full modifier table; the aura is simply
/// not shown to that recipient until a slot frees.
fn apply_item_armor_modifier(status: &mut StatusState, modifier: TimedArmorModifier) {
    if status_has_armor_modifier_room(status, &modifier) {
        apply_timed_armor_modifier(status, modifier);
    }
}

fn item_native_buff(rawcode: u32) -> Option<crate::NativeBuffIdentity> {
    Some(crate::NativeBuffIdentity {
        rawcode,
        positive: true,
        stealable: false,
        organic_only: false,
    })
}

/// A shop purchase that passed every check, ready to be charged and delivered.
struct Purchase {
    catalog: &'static ItemCatalog,
    handle: BuilderHandle,
    index: usize,
    team_index: usize,
    entry: &'static ShopEntry,
    definition: &'static ItemDefinition,
}

impl Simulation {
    fn item_catalog(&self) -> Option<&'static ItemCatalog> {
        self.config.item_system.and_then(item_catalog_for_version)
    }

    /// Per-team castle shop contents in shop-position order, for presentation.
    #[must_use]
    pub fn item_shop(&self, team: Team) -> &[crate::ShopSlotState] {
        self.items
            .shops
            .get(usize::from(team.0))
            .map_or(&[], Vec::as_slice)
    }

    /// Whole seconds since the current round started, for countdowns on shop buttons.
    #[must_use]
    pub fn round_elapsed_seconds(&self) -> u32 {
        let ticks = self.next_tick.saturating_sub(self.round_start_tick);
        u32::try_from(ticks / CASTLE_FIGHT_SIMULATION_HZ as u64).unwrap_or(u32::MAX)
    }

    #[must_use]
    pub fn ground_items(&self) -> Vec<(SimId, GroundItem)> {
        let mut items: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| Some((*entity.get::<SimId>()?, *entity.get::<GroundItem>()?)))
            .collect();
        items.sort_unstable_by_key(|(id, _)| *id);
        items
    }

    #[must_use]
    pub fn team_item_buffs(&self) -> &[TeamItemBuff] {
        &self.items.team_buffs
    }

    fn builder_handle(
        &self,
        controller: PlayerId,
        builder: SimId,
    ) -> Result<BuilderHandle, ItemCommandError> {
        if !self.can_player_control_builder(controller, builder) {
            return Err(ItemCommandError::NotAuthorized);
        }
        self.builder_handle_unchecked(builder)
    }

    fn builder_handle_unchecked(&self, builder: SimId) -> Result<BuilderHandle, ItemCommandError> {
        self.world
            .iter_entities()
            .find_map(|entity| {
                (entity.get::<SimId>().copied() == Some(builder)
                    && entity.get::<Builder>().is_some())
                .then(|| {
                    Some(BuilderHandle {
                        entity: entity.id(),
                        id: builder,
                        owner: entity.get::<Owner>()?.0,
                        team: *entity.get::<Team>()?,
                        position: entity.get::<Position>()?.0,
                    })
                })
                .flatten()
            })
            .ok_or(ItemCommandError::BuilderNotFound)
    }

    fn inventory_of(&self, entity: Entity) -> &Inventory {
        self.world
            .get::<Inventory>(entity)
            .expect("builders carry an inventory")
    }

    fn inventory_of_mut(&mut self, entity: Entity) -> bevy_ecs::world::Mut<'_, Inventory> {
        self.world
            .get_mut::<Inventory>(entity)
            .expect("builders carry an inventory")
    }

    /// Whole round minutes elapsed, which scale the Orb of Lightning's level.
    fn round_minutes(&self, tick: u64) -> u64 {
        tick.saturating_sub(self.round_start_tick) / (60 * CASTLE_FIGHT_SIMULATION_HZ as u64)
    }

    // ---------------------------------------------------------------- commands

    /// Whether the shop button at `position` would sell to `builder` right now, with the first
    /// reason it would not. The purchase command runs the same checks, so presentation can grey
    /// out buttons without restating the rules.
    pub fn can_buy_item(
        &self,
        controller: PlayerId,
        builder: SimId,
        position: u8,
    ) -> Result<(), ItemCommandError> {
        self.check_purchase(controller, builder, position)
            .map(|_| ())
    }

    fn check_purchase(
        &self,
        controller: PlayerId,
        builder: SimId,
        position: u8,
    ) -> Result<Purchase, ItemCommandError> {
        let catalog = self
            .item_catalog()
            .ok_or(ItemCommandError::ItemSystemUnavailable)?;
        let handle = self.builder_handle(controller, builder)?;
        let (index, entry) = catalog
            .shop
            .entries
            .iter()
            .enumerate()
            .find(|(_, entry)| entry.position == position)
            .ok_or(ItemCommandError::UnknownShopEntry)?;
        let definition = catalog
            .item(ItemId(entry.item))
            .ok_or(ItemCommandError::UnknownShopEntry)?;
        let team_index = usize::from(handle.team.0);
        let slot = *self.items.shops[team_index]
            .get(index)
            .ok_or(ItemCommandError::UnknownShopEntry)?;
        if !slot.opened {
            return Err(ItemCommandError::ShopClosed);
        }
        if slot.stock == 0 {
            return Err(ItemCommandError::OutOfStock);
        }
        if self.next_tick < slot.button_ready_tick {
            return Err(ItemCommandError::ShopButtonCooling);
        }
        let resources = self
            .player_resources_for(handle.owner)
            .expect("builder owner must have player resources");
        if resources.gold < definition.gold_cost {
            return Err(ItemCommandError::InsufficientGold {
                available: resources.gold,
                required: definition.gold_cost,
            });
        }
        if resources.lumber < definition.lumber_cost {
            return Err(ItemCommandError::InsufficientLumber {
                available: resources.lumber,
                required: definition.lumber_cost,
            });
        }
        if !definition.powerup
            && !self.inventory_of(handle.entity).can_hold(
                catalog,
                definition.id(),
                definition.initial_charges,
            )
        {
            return Err(ItemCommandError::InventoryFull);
        }
        Ok(Purchase {
            catalog,
            handle,
            index,
            team_index,
            entry,
            definition,
        })
    }

    pub(crate) fn buy_item_as(
        &mut self,
        controller: PlayerId,
        builder: SimId,
        position: u8,
    ) -> Result<(), ItemCommandError> {
        let Purchase {
            catalog,
            handle,
            index,
            team_index,
            entry,
            definition,
        } = self.check_purchase(controller, builder, position)?;
        let tick = self.next_tick;

        let resources = &mut self
            .player_state_mut(handle.owner)
            .expect("builder owner must exist")
            .resources;
        resources.gold -= definition.gold_cost;
        resources.lumber -= definition.lumber_cost;

        let stock = &mut self.items.shops[team_index][index];
        stock.stock -= 1;
        stock.button_ready_tick = tick + seconds_to_ticks(entry.runtime_button_cooldown_seconds);
        if stock.restock_tick.is_none() && stock.stock < entry.stock.maximum {
            stock.restock_tick =
                Some(tick + seconds_to_ticks(f64::from(entry.stock.replenish_interval_seconds)));
        }

        if definition.powerup {
            self.consume_powerup(&handle, definition);
        } else {
            self.give_item(
                catalog,
                handle.entity,
                definition.id(),
                definition.initial_charges,
            )
            .expect("room was checked before charging");
        }
        Ok(())
    }

    /// Powerups never occupy a slot: the pickup handler consumes them on the spot.
    fn consume_powerup(&mut self, handle: &BuilderHandle, definition: &ItemDefinition) {
        let tick = self.next_tick;
        for effect in &definition.effects {
            match effect {
                ItemEffect::TeamDamageCarrier(carrier) => {
                    let expires_tick = tick + seconds_to_ticks(carrier.carrier_lifetime_seconds);
                    match self
                        .items
                        .team_buffs
                        .iter_mut()
                        .find(|buff| buff.team == handle.team && buff.item == definition.id())
                    {
                        // The buff is one aura family: another carrier extends it, never stacks.
                        Some(buff) => buff.expires_tick = buff.expires_tick.max(expires_tick),
                        None => self.items.team_buffs.push(TeamItemBuff {
                            team: handle.team,
                            item: definition.id(),
                            expires_tick,
                        }),
                    }
                }
                ItemEffect::LegendaryPoint(point) => {
                    let resources = &mut self
                        .player_state_mut(handle.owner)
                        .expect("builder owner must exist")
                        .resources;
                    resources.legendary_points_cap = resources
                        .legendary_points_cap
                        .saturating_add(point.legendary_points);
                }
                _ => {}
            }
        }
    }

    /// Places an item straight into a builder's inventory, bypassing the shop. For debug tools and
    /// presentation fixtures only; it is not a player command and is never replicated.
    pub fn debug_grant_item(&mut self, builder: SimId, item: ItemId, charges: u8) -> bool {
        let Some(catalog) = self.item_catalog() else {
            return false;
        };
        let Ok(handle) = self.builder_handle_unchecked(builder) else {
            return false;
        };
        self.give_item(catalog, handle.entity, item, charges)
            .is_ok()
    }

    /// Lays an item on the ground. Debug tools and fixtures only, like [`Self::debug_grant_item`].
    pub fn debug_spawn_ground_item(&mut self, item: ItemId, charges: u8, position: SimPoint) {
        self.spawn_ground_item(item, charges, position);
    }

    fn give_item(
        &mut self,
        catalog: &ItemCatalog,
        entity: Entity,
        item: ItemId,
        charges: u8,
    ) -> Result<AcquireOutcome, InventoryError> {
        let tick = self.next_tick;
        self.inventory_of_mut(entity)
            .acquire(catalog, item, charges, tick)
    }

    pub(crate) fn move_item_as(
        &mut self,
        controller: PlayerId,
        builder: SimId,
        from: u8,
        to: u8,
    ) -> Result<(), ItemCommandError> {
        let catalog = self
            .item_catalog()
            .ok_or(ItemCommandError::ItemSystemUnavailable)?;
        let handle = self.builder_handle(controller, builder)?;
        self.inventory_of_mut(handle.entity).move_slot(
            catalog,
            usize::from(from),
            usize::from(to),
        )?;
        Ok(())
    }

    pub(crate) fn use_item_as(
        &mut self,
        controller: PlayerId,
        builder: SimId,
        slot: u8,
        target: ItemUseTarget,
    ) -> Result<(), ItemCommandError> {
        let catalog = self
            .item_catalog()
            .ok_or(ItemCommandError::ItemSystemUnavailable)?;
        let handle = self.builder_handle(controller, builder)?;
        let stack = self
            .inventory_of(handle.entity)
            .slot(usize::from(slot))
            .copied()
            .ok_or(ItemCommandError::EmptySlot)?;
        let definition = catalog
            .item(stack.item)
            .ok_or(ItemCommandError::EmptySlot)?;
        let effect = definition.use_effect().ok_or(ItemCommandError::NotUsable)?;
        if self
            .inventory_of(handle.entity)
            .on_cooldown(definition.cooldown_group, self.next_tick)
        {
            return Err(ItemCommandError::OnCooldown);
        }
        match (effect, target) {
            (
                ItemEffect::PointScrollRestoration(_) | ItemEffect::PointScrollSpeed(_),
                ItemUseTarget::Point(_),
            ) => {}
            (ItemEffect::OrbChainLightning(_), ItemUseTarget::Unit(id)) => {
                self.orb_target(handle.team, id)
                    .ok_or(ItemCommandError::InvalidTarget)?;
            }
            _ => return Err(ItemCommandError::InvalidTarget),
        }
        self.commit_item_order(catalog, handle, ItemOrder::Use { slot, target });
        Ok(())
    }

    pub(crate) fn drop_item_as(
        &mut self,
        controller: PlayerId,
        builder: SimId,
        slot: u8,
        position: SimPoint,
    ) -> Result<(), ItemCommandError> {
        let catalog = self
            .item_catalog()
            .ok_or(ItemCommandError::ItemSystemUnavailable)?;
        let handle = self.builder_handle(controller, builder)?;
        let stack = self
            .inventory_of(handle.entity)
            .slot(usize::from(slot))
            .copied()
            .ok_or(ItemCommandError::EmptySlot)?;
        if !catalog.item(stack.item).is_some_and(|item| item.droppable) {
            return Err(ItemCommandError::NotDroppable);
        }
        self.commit_item_order(catalog, handle, ItemOrder::Drop { slot, position });
        Ok(())
    }

    pub(crate) fn give_item_as(
        &mut self,
        controller: PlayerId,
        builder: SimId,
        slot: u8,
        recipient: SimId,
    ) -> Result<(), ItemCommandError> {
        let catalog = self
            .item_catalog()
            .ok_or(ItemCommandError::ItemSystemUnavailable)?;
        let handle = self.builder_handle(controller, builder)?;
        let stack = self
            .inventory_of(handle.entity)
            .slot(usize::from(slot))
            .copied()
            .ok_or(ItemCommandError::EmptySlot)?;
        if !catalog.item(stack.item).is_some_and(|item| item.droppable) {
            return Err(ItemCommandError::NotDroppable);
        }
        let target = self
            .builder_handle_unchecked(recipient)
            .map_err(|_| ItemCommandError::RecipientNotFound)?;
        if target.id == handle.id || target.team != handle.team {
            return Err(ItemCommandError::RecipientNotFound);
        }
        self.commit_item_order(catalog, handle, ItemOrder::Give { slot, recipient });
        Ok(())
    }

    pub(crate) fn pickup_item_as(
        &mut self,
        controller: PlayerId,
        builder: SimId,
        item: SimId,
    ) -> Result<(), ItemCommandError> {
        let catalog = self
            .item_catalog()
            .ok_or(ItemCommandError::ItemSystemUnavailable)?;
        let handle = self.builder_handle(controller, builder)?;
        self.ground_item_entity(item)
            .ok_or(ItemCommandError::GroundItemNotFound)?;
        self.commit_item_order(catalog, handle, ItemOrder::Pickup { item });
        Ok(())
    }

    fn ground_item_entity(&self, id: SimId) -> Option<(Entity, GroundItem)> {
        self.world.iter_entities().find_map(|entity| {
            (entity.get::<SimId>().copied() == Some(id))
                .then(|| Some((entity.id(), *entity.get::<GroundItem>()?)))
                .flatten()
        })
    }

    fn orb_target(&self, team: Team, target: SimId) -> Option<SimPoint> {
        self.world.iter_entities().find_map(|entity| {
            if entity.get::<SimId>().copied() != Some(target)
                || entity.get::<BuildingFootprint>().is_some()
            {
                return None;
            }
            let position = entity.get::<Position>()?.0;
            let alive = entity.get::<Health>()?.current > 0;
            let classifications = entity.get::<UnitClassifications>();
            (alive
                && *entity.get::<Team>()? != team
                && !classifications.is_some_and(|flags| flags.invulnerable || flags.locust))
            .then_some(position)
        })
    }

    /// Acts at once when the order is already in reach (leaving the builder's other orders
    /// untouched); otherwise the builder walks to the target and acts on arrival.
    fn commit_item_order(
        &mut self,
        catalog: &ItemCatalog,
        handle: BuilderHandle,
        order: ItemOrder,
    ) {
        match self.item_approach(catalog, &handle, order) {
            Approach::Ready => self.execute_item_order(catalog, &handle, order),
            Approach::Cancel => {}
            Approach::Walk(destination) => {
                self.world
                    .get_mut::<BuilderState>(handle.entity)
                    .expect("builder missing command state")
                    .item_order = Some(PendingItemOrder {
                    order,
                    walking: false,
                });
                self.begin_item_walk(handle.entity, destination);
            }
        }
    }

    // ------------------------------------------------------------ item orders

    fn item_approach(
        &self,
        catalog: &ItemCatalog,
        handle: &BuilderHandle,
        order: ItemOrder,
    ) -> Approach {
        let near = |point: SimPoint, range_sq: u64| {
            if handle.position.distance_sq(point) <= range_sq {
                Approach::Ready
            } else {
                Approach::Walk(point)
            }
        };
        match order {
            ItemOrder::Pickup { item } => match self.ground_item_entity(item) {
                Some((_, ground)) => near(ground.position, interaction_range_sq()),
                None => Approach::Cancel,
            },
            ItemOrder::Drop { slot, position } => {
                if self
                    .inventory_of(handle.entity)
                    .slot(usize::from(slot))
                    .is_none()
                {
                    return Approach::Cancel;
                }
                near(position, interaction_range_sq())
            }
            ItemOrder::Give { slot, recipient } => {
                if self
                    .inventory_of(handle.entity)
                    .slot(usize::from(slot))
                    .is_none()
                {
                    return Approach::Cancel;
                }
                match self.builder_handle_unchecked(recipient) {
                    Ok(target) if target.team == handle.team => {
                        near(target.position, interaction_range_sq())
                    }
                    _ => Approach::Cancel,
                }
            }
            ItemOrder::Use { slot, target } => {
                let Some(stack) = self.inventory_of(handle.entity).slot(usize::from(slot)) else {
                    return Approach::Cancel;
                };
                let Some(effect) = catalog
                    .item(stack.item)
                    .and_then(ItemDefinition::use_effect)
                else {
                    return Approach::Cancel;
                };
                match (effect, target) {
                    (ItemEffect::PointScrollRestoration(effect), ItemUseTarget::Point(point)) => {
                        near(point, square_i32(world_to_subunits(effect.range)))
                    }
                    (ItemEffect::PointScrollSpeed(effect), ItemUseTarget::Point(point)) => {
                        near(point, square_i32(world_to_subunits(effect.range)))
                    }
                    (ItemEffect::OrbChainLightning(effect), ItemUseTarget::Unit(id)) => {
                        match self.orb_target(handle.team, id) {
                            Some(position) => {
                                near(position, square_i32(world_to_subunits(effect.range)))
                            }
                            None => Approach::Cancel,
                        }
                    }
                    _ => Approach::Cancel,
                }
            }
        }
    }

    fn execute_item_order(
        &mut self,
        catalog: &ItemCatalog,
        handle: &BuilderHandle,
        order: ItemOrder,
    ) {
        match order {
            ItemOrder::Pickup { item } => {
                let Some((entity, ground)) = self.ground_item_entity(item) else {
                    return;
                };
                match self.give_item(catalog, handle.entity, ground.item, ground.charges) {
                    Ok(AcquireOutcome::Stored { .. }) => {
                        self.world.despawn(entity);
                    }
                    Ok(AcquireOutcome::Partial { remaining }) => {
                        self.world
                            .get_mut::<GroundItem>(entity)
                            .expect("ground item exists")
                            .charges = remaining;
                    }
                    // A full inventory leaves the item where it lies.
                    Err(_) => {}
                }
            }
            ItemOrder::Drop { slot, position } => {
                let Ok(stack) = self
                    .inventory_of_mut(handle.entity)
                    .remove(usize::from(slot))
                else {
                    return;
                };
                self.spawn_ground_item(stack.item, stack.charges, position);
            }
            ItemOrder::Give { slot, recipient } => {
                let Ok(target) = self.builder_handle_unchecked(recipient) else {
                    return;
                };
                let Some(stack) = self
                    .inventory_of(handle.entity)
                    .slot(usize::from(slot))
                    .copied()
                else {
                    return;
                };
                let outcome = self.give_item(catalog, target.entity, stack.item, stack.charges);
                let mut giver = self.inventory_of_mut(handle.entity);
                match outcome {
                    Ok(AcquireOutcome::Stored { .. }) => {
                        let _ = giver.remove(usize::from(slot));
                    }
                    Ok(AcquireOutcome::Partial { remaining }) => {
                        if let Some(stack) = giver.slot_mut(usize::from(slot)) {
                            stack.charges = remaining;
                        }
                    }
                    // The recipient has no room, so the item stays with the giver.
                    Err(_) => {}
                }
            }
            ItemOrder::Use { slot, target } => {
                self.use_item_from_slot(catalog, handle, slot, target)
            }
        }
    }

    fn spawn_ground_item(&mut self, item: ItemId, charges: u8, position: SimPoint) {
        let id = self.allocate_id();
        self.world.spawn((
            id,
            GroundItem {
                item,
                charges,
                position,
            },
        ));
    }

    fn use_item_from_slot(
        &mut self,
        catalog: &ItemCatalog,
        handle: &BuilderHandle,
        slot: u8,
        target: ItemUseTarget,
    ) {
        let tick = self.next_tick;
        let Some(stack) = self
            .inventory_of(handle.entity)
            .slot(usize::from(slot))
            .copied()
        else {
            return;
        };
        let Some(definition) = catalog.item(stack.item) else {
            return;
        };
        let Some(effect) = definition.use_effect() else {
            return;
        };
        if self
            .inventory_of(handle.entity)
            .on_cooldown(definition.cooldown_group, tick)
        {
            return;
        }
        let used = match (effect, target) {
            (ItemEffect::PointScrollRestoration(effect), ItemUseTarget::Point(point)) => {
                self.apply_restoration_scroll(handle.team, point, effect);
                true
            }
            (ItemEffect::PointScrollSpeed(effect), ItemUseTarget::Point(point)) => {
                self.apply_speed_scroll(handle.team, point, effect);
                true
            }
            (ItemEffect::OrbChainLightning(effect), ItemUseTarget::Unit(id)) => {
                self.launch_orb_chain(handle, effect, id)
            }
            _ => false,
        };
        if !used {
            return;
        }
        if let ItemEffect::OrbChainLightning(effect) = effect {
            self.inventory_of_mut(handle.entity).start_cooldown(
                definition.cooldown_group,
                tick + seconds_to_ticks(effect.use_cooldown_seconds),
            );
        }
        self.inventory_of_mut(handle.entity)
            .consume_charge(catalog, usize::from(slot))
            .expect("the used slot was occupied");
    }

    /// Friendly units standing inside a circle, in id order.
    fn friendly_units_near(
        &self,
        team: Team,
        center: SimPoint,
        radius: i32,
    ) -> Vec<(SimId, Entity)> {
        let radius_sq = square_i32(world_to_subunits(radius));
        let mut found: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                if entity.get::<BuildingFootprint>().is_some() || entity.get::<Builder>().is_some()
                {
                    return None;
                }
                let id = *entity.get::<SimId>()?;
                let position = entity.get::<Position>()?.0;
                let alive = entity.get::<Health>()?.current > 0;
                let locust = entity
                    .get::<UnitClassifications>()
                    .is_some_and(|flags| flags.locust);
                (alive
                    && !locust
                    && *entity.get::<Team>()? == team
                    && center.distance_sq(position) <= radius_sq)
                    .then_some((id, entity.id()))
            })
            .collect();
        found.sort_unstable_by_key(|(id, _)| *id);
        found
    }

    fn apply_restoration_scroll(
        &mut self,
        team: Team,
        point: SimPoint,
        effect: &PointScrollRestorationEffect,
    ) {
        let tick = self.next_tick;
        let expiry = tick + seconds_to_ticks(effect.duration_seconds);
        for (_, entity) in self.friendly_units_near(team, point, effect.radius) {
            let heal = {
                let status = self.world.get::<StatusState>(entity);
                status.map_or(effect.life, |status| {
                    native_healing_amount(status, effect.life, tick)
                })
            };
            if let Some(mut health) = self.world.get_mut::<Health>(entity) {
                health.current = health.current.saturating_add(heal).min(health.max);
            }
            let mana_maximum = self.unit_mana_maximum(entity);
            if let (Some(maximum), Some(mut mana)) =
                (mana_maximum, self.world.get_mut::<ManaState>(entity))
            {
                mana.current = mana.current.saturating_add(effect.mana).min(maximum);
            }
            if let Some(mut status) = self.world.get_mut::<StatusState>(entity) {
                apply_item_armor_modifier(
                    &mut status,
                    TimedArmorModifier {
                        id: ModifierId(effect.effect_ability),
                        native_buff: item_native_buff(effect.buff),
                        armor_bonus_per_100: effect.armor_bonus.saturating_mul(100),
                        expires_tick: expiry,
                        ..Default::default()
                    },
                );
            }
        }
    }

    fn unit_mana_maximum(&self, entity: Entity) -> Option<i32> {
        let casting = self.world.get::<SpellcastingProfile>(entity);
        casting.map(|profile| profile.mana.maximum).or_else(|| {
            let content = self.world.get::<ContentIdentity>(entity)?;
            crate::content::native_mana_profile_for_version(content.map_version, content.rawcode)
                .map(|profile| profile.maximum)
        })
    }

    fn apply_speed_scroll(&mut self, team: Team, point: SimPoint, effect: &PointScrollSpeedEffect) {
        let expiry = self.next_tick + seconds_to_ticks(effect.duration_seconds);
        let percent = i16::try_from(effect.movement_speed_increase_per_10k / 100)
            .expect("authored speed increase fits a percentage");
        let id = ModifierId(effect.effect_ability);
        for (_, entity) in self.friendly_units_near(team, point, effect.radius) {
            if let Some(mut status) = self.world.get_mut::<StatusState>(entity)
                && status_has_movement_modifier_room(&status, id)
            {
                apply_timed_movement_modifier(&mut status, id, percent, expiry);
            }
        }
    }

    /// Starts the Orb of Lightning's chain on `target`; its level grows every authored step of
    /// round time. Returns whether the chain was launched.
    fn launch_orb_chain(
        &mut self,
        handle: &BuilderHandle,
        effect: &OrbChainLightningEffect,
        target: SimId,
    ) -> bool {
        let Some(position) = self.orb_target(handle.team, target) else {
            return false;
        };
        let tick = self.next_tick;
        let step = u64::from(effect.level_step_minutes.max(1));
        let last = effect.effect_levels.len().saturating_sub(1);
        let level_index = usize::try_from(self.round_minutes(tick) / step)
            .unwrap_or(usize::MAX)
            .min(last);
        let Some(level) = effect.effect_levels.get(level_index) else {
            return false;
        };
        let profile = crate::ChainLightningEffectProfile {
            ability: crate::AbilityId(effect.effect_ability),
            initial_damage: level.damage,
            maximum_targets: level.targets,
            jump_radius: world_to_subunits(level.jump_radius),
            damage_reduction_per_10k: level.damage_reduction_per_target_per_10k,
            targets: AttackTargetMask::from_capabilities(
                effect.targets.ground,
                effect.targets.air,
                effect.targets.structure,
            ),
        };
        self.launch_dummy_chain_lightning(handle.id, handle.team, target, position, profile, tick);
        true
    }

    /// Per-tick monitor of committed walk-then-act orders (run before builders move).
    pub(super) fn advance_item_orders(&mut self) {
        let Some(catalog) = self.item_catalog() else {
            return;
        };
        let mut pending: Vec<(SimId, PendingItemOrder)> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                Some((
                    *entity.get::<SimId>()?,
                    entity.get::<BuilderState>()?.item_order?,
                ))
            })
            .collect();
        pending.sort_unstable_by_key(|(id, _)| *id);
        for (id, pending) in pending {
            let Ok(handle) = self.builder_handle_unchecked(id) else {
                continue;
            };
            match self.item_approach(catalog, &handle, pending.order) {
                Approach::Ready => {
                    self.clear_item_order(handle.entity, true);
                    self.execute_item_order(catalog, &handle, pending.order);
                }
                Approach::Cancel => self.clear_item_order(handle.entity, false),
                Approach::Walk(destination) => {
                    let still_walking = self
                        .world
                        .get::<BuilderState>(handle.entity)
                        .is_some_and(|state| state.destination.is_some());
                    if pending.walking && !still_walking {
                        // The walk ended (blocked by the build region) short of the target.
                        self.clear_item_order(handle.entity, false);
                    } else if let Some(mut state) =
                        self.world.get_mut::<BuilderState>(handle.entity)
                    {
                        state.destination = Some(destination);
                    }
                }
            }
        }
    }

    fn clear_item_order(&mut self, builder: Entity, stop_walking: bool) {
        if let Some(mut state) = self.world.get_mut::<BuilderState>(builder) {
            state.item_order = None;
            if stop_walking {
                state.destination = None;
            }
        }
    }

    // ----------------------------------------------------------------- phases

    /// Opens and restocks the castle shops, retires expired team buffs and spent cooldowns, then
    /// refreshes the auras of carried items and team buffs for this tick.
    pub(super) fn advance_item_system(&mut self) {
        let Some(catalog) = self.item_catalog() else {
            return;
        };
        let tick = self.next_tick;
        let elapsed = tick.saturating_sub(self.round_start_tick);
        for shop in &mut self.items.shops {
            for (slot, entry) in shop.iter_mut().zip(&catalog.shop.entries) {
                let interval = seconds_to_ticks(f64::from(entry.stock.replenish_interval_seconds));
                if !slot.opened {
                    if elapsed >= seconds_to_ticks(f64::from(entry.stock.start_delay_seconds)) {
                        slot.opened = true;
                        slot.stock = entry.stock.initial.min(entry.stock.maximum);
                        slot.restock_tick =
                            (slot.stock < entry.stock.maximum).then_some(tick + interval);
                    }
                    continue;
                }
                if slot.stock < entry.stock.maximum
                    && slot.restock_tick.is_some_and(|due| due <= tick)
                {
                    slot.stock += 1;
                    slot.restock_tick =
                        (slot.stock < entry.stock.maximum).then_some(tick + interval);
                }
            }
        }
        self.items
            .team_buffs
            .retain(|buff| tick < buff.expires_tick);
        let builders: Vec<Entity> = self
            .world
            .iter_entities()
            .filter(|entity| entity.get::<Builder>().is_some())
            .map(|entity| entity.id())
            .collect();
        for entity in builders {
            if let Some(mut inventory) = self.world.get_mut::<Inventory>(entity) {
                inventory.purge_cooldowns(tick);
            }
        }
        self.apply_item_auras(catalog);
    }

    fn apply_item_auras(&mut self, catalog: &'static ItemCatalog) {
        let tick = self.next_tick;
        let expiry = tick + 1;
        let mut carriers: Vec<(SimId, Team, SimPoint, Vec<ItemId>)> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                let inventory = entity.get::<Inventory>()?;
                let items: Vec<ItemId> = inventory.stacks().map(|(_, stack)| stack.item).collect();
                (!items.is_empty()).then(|| {
                    Some((
                        *entity.get::<SimId>()?,
                        *entity.get::<Team>()?,
                        entity.get::<Position>()?.0,
                        items,
                    ))
                })?
            })
            .collect();
        carriers.sort_unstable_by_key(|(id, ..)| *id);
        let mut sources: Vec<AuraSource<'static>> = Vec::new();
        for (_, team, position, items) in &carriers {
            for item in items {
                let Some(definition) = catalog.item(*item) else {
                    continue;
                };
                for effect in &definition.effects {
                    if matches!(
                        effect,
                        ItemEffect::RegenerationAura(_)
                            | ItemEffect::SpeedAura(_)
                            | ItemEffect::DamageAura(_)
                    ) {
                        sources.push(AuraSource {
                            team: *team,
                            position: *position,
                            effect,
                        });
                    }
                }
            }
        }
        let team_buffs: Vec<(Team, &'static crate::items::TeamDamageCarrierEffect)> = self
            .items
            .team_buffs
            .iter()
            .filter_map(|buff| {
                catalog
                    .item(buff.item)?
                    .effects
                    .iter()
                    .find_map(|effect| match effect {
                        ItemEffect::TeamDamageCarrier(carrier) => Some((buff.team, carrier)),
                        _ => None,
                    })
            })
            .collect();
        if sources.is_empty() && team_buffs.is_empty() {
            return;
        }

        let mut units = self.world.query::<(
            &Team,
            &Position,
            &Health,
            &MovementClass,
            Option<&UnitClassifications>,
            &mut StatusState,
            Option<&BuildingFootprint>,
        )>();
        let cell = self.config.navigation_cell_size;
        for (team, position, health, movement, flags, mut status, footprint) in
            units.iter_mut(&mut self.world)
        {
            if health.current <= 0 || flags.is_some_and(|flags| flags.locust) || footprint.is_some()
            {
                continue;
            }
            let invulnerable = flags.is_some_and(|flags| flags.invulnerable);
            apply_unit_item_auras(
                &mut status,
                *team,
                position.0,
                *movement,
                invulnerable,
                &sources,
                &team_buffs,
                expiry,
            );
        }
        let mut buildings = self.world.query::<(
            &Team,
            &BuildingFootprint,
            &Health,
            Option<&UnitClassifications>,
            &mut StatusState,
        )>();
        for (team, footprint, health, flags, mut status) in buildings.iter_mut(&mut self.world) {
            if health.current <= 0 {
                continue;
            }
            let invulnerable = flags.is_some_and(|flags| flags.invulnerable);
            let center = footprint_center_point(*footprint, cell);
            apply_structure_item_auras(&mut status, *team, center, invulnerable, &sources, expiry);
        }
    }

    /// Fires every carried Blast Staff that is ready and lands the missiles that have arrived.
    pub(super) fn resolve_item_projectiles(
        &mut self,
        units: &mut [UnitSnapshot],
        unit_health: &mut [i32],
        positions: &[SimPoint],
        completed_tick: u64,
    ) {
        let Some(catalog) = self.item_catalog() else {
            return;
        };
        self.launch_blast_missiles(catalog, units, unit_health, positions, completed_tick);
        let mut index = 0;
        while index < self.items.impacts.len() {
            if self.items.impacts[index].impact_tick > completed_tick {
                index += 1;
                continue;
            }
            let impact = self.items.impacts.remove(index);
            let Some(target) = find_unit_index(units, impact.target) else {
                continue;
            };
            if unit_health[target] <= 0
                || units[target].classifications.invulnerable
                || units[target].classifications.spell_immune
            {
                continue;
            }
            let damage = self
                .combat_rules
                .damage_rules
                .apply_spell(impact.damage, units[target].armor.armor_type);
            let event_damage = spell_damage_after_defend(units[target], damage, completed_tick);
            super::source_damage_runtime::dispatch(
                impact.source,
                target,
                event_damage,
                super::source_damage_runtime::SourceDamageState {
                    units,
                    health: unit_health,
                    rules: self.combat_rules.damage_rules,
                    tick: completed_tick,
                },
                &self.retired_damage_sources,
            );
            super::damage_thresholds::observe_damage_event(
                &mut units[target],
                unit_health[target],
                event_damage,
            );
            let after = unit_health[target].saturating_sub(event_damage);
            death_healing::commit_split_health_from(
                units,
                unit_health,
                Some(positions),
                target,
                after,
                Some(impact.source),
            );
        }
    }

    fn launch_blast_missiles(
        &mut self,
        catalog: &ItemCatalog,
        units: &[UnitSnapshot],
        unit_health: &[i32],
        positions: &[SimPoint],
        tick: u64,
    ) {
        let mut carriers: Vec<(SimId, Entity, Team, SimPoint)> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                entity.get::<Inventory>()?;
                Some((
                    *entity.get::<SimId>()?,
                    entity.id(),
                    *entity.get::<Team>()?,
                    entity.get::<Position>()?.0,
                ))
            })
            .collect();
        carriers.sort_unstable_by_key(|(id, ..)| *id);
        for (id, entity, team, position) in carriers {
            for slot in 0..INVENTORY_SLOTS {
                let Some(stack) = self.inventory_of(entity).slot(slot).copied() else {
                    continue;
                };
                if tick < stack.ready_tick {
                    continue;
                }
                let Some(blast) = catalog.item(stack.item).and_then(|definition| {
                    definition.effects.iter().find_map(|effect| match effect {
                        ItemEffect::Blast(blast) => Some(blast),
                        _ => None,
                    })
                }) else {
                    continue;
                };
                let candidates =
                    blast_candidates(blast, team, position, units, unit_health, positions);
                if candidates.is_empty() {
                    continue;
                }
                for copy in 0..blast.copies {
                    let pick = deterministic_random(
                        self.config.match_seed,
                        tick,
                        id,
                        RANDOM_PURPOSE_BLAST_STAFF
                            ^ u64::from(stack.item.0)
                            ^ ((slot as u64) << 40),
                        u64::from(copy),
                    ) as usize
                        % candidates.len();
                    let target = candidates[pick];
                    let travel = projectile_travel_ticks(
                        position.distance_sq(positions[target]),
                        speed_per_tick(blast.missile_speed).max(1),
                    );
                    self.items.impacts.push(PendingItemImpact {
                        source: id,
                        source_team: team,
                        target: units[target].id,
                        item: stack.item,
                        damage: blast.damage,
                        launch_position: position,
                        launch_tick: tick,
                        impact_tick: tick + travel,
                    });
                }
                if let Some(stack) = self.inventory_of_mut(entity).slot_mut(slot) {
                    stack.ready_tick = tick + seconds_to_ticks(blast.cooldown_seconds);
                }
            }
        }
    }
}

/// Indices of enemy units a Blast Staff may strike, in id order.
fn blast_candidates(
    blast: &BlastEffect,
    team: Team,
    position: SimPoint,
    units: &[UnitSnapshot],
    unit_health: &[i32],
    positions: &[SimPoint],
) -> Vec<usize> {
    let radius_sq = square_i32(world_to_subunits(blast.radius));
    units
        .iter()
        .enumerate()
        .filter(|(index, unit)| {
            unit_health[*index] > 0
                && unit.team != team
                && !unit.classifications.locust
                && !unit.classifications.invulnerable
                && !unit.classifications.spell_immune
                && match unit.target_movement_class() {
                    MovementClass::Ground => blast.targets.ground,
                    MovementClass::Air => blast.targets.air,
                }
                && position.distance_sq(positions[*index]) <= radius_sq
        })
        .map(|(index, _)| index)
        .collect()
}

fn recipient_class_allowed(targets: &crate::items::ItemTargets, movement: MovementClass) -> bool {
    match movement {
        MovementClass::Ground => targets.ground,
        MovementClass::Air => targets.air,
    }
}

fn invulnerability_allowed(targets: &crate::items::ItemTargets, invulnerable: bool) -> bool {
    if invulnerable {
        targets.invulnerable
    } else {
        targets.vulnerable
    }
}

#[allow(clippy::too_many_arguments)]
fn apply_unit_item_auras(
    status: &mut StatusState,
    team: Team,
    position: SimPoint,
    movement: MovementClass,
    invulnerable: bool,
    sources: &[AuraSource<'_>],
    team_buffs: &[(Team, &crate::items::TeamDamageCarrierEffect)],
    expiry: u64,
) {
    let in_reach = |source: &AuraSource<'_>, radius: i32| {
        source.team == team
            && source.position.distance_sq(position) <= square_i32(world_to_subunits(radius))
    };
    let mut regeneration: Vec<(u32, u32)> = Vec::new();
    let mut speed: Vec<(&crate::items::SpeedAuraEffect, i16, i16)> = Vec::new();
    let mut damage: Vec<&crate::items::DamageAuraEffect> = Vec::new();
    for source in sources {
        match source.effect {
            ItemEffect::RegenerationAura(effect)
                if recipient_class_allowed(&effect.targets, movement)
                    && invulnerability_allowed(&effect.targets, invulnerable)
                    && in_reach(source, effect.radius) =>
            {
                // Each carried rune stacks, like the coral regeneration family.
                match regeneration
                    .iter_mut()
                    .find(|(ability, _)| *ability == effect.ability)
                {
                    Some((_, rate)) => {
                        *rate = rate.saturating_add(effect.regeneration_per_second_per_10k);
                    }
                    None => {
                        regeneration.push((effect.ability, effect.regeneration_per_second_per_10k))
                    }
                }
            }
            ItemEffect::SpeedAura(effect)
                if recipient_class_allowed(&effect.targets, movement)
                    && invulnerability_allowed(&effect.targets, invulnerable)
                    && in_reach(source, effect.radius) =>
            {
                speed.push((effect, effect.movement_percent, effect.attack_speed_percent));
            }
            ItemEffect::DamageAura(effect)
                if recipient_class_allowed(&effect.targets, movement)
                    && invulnerability_allowed(&effect.targets, invulnerable)
                    && in_reach(source, effect.radius) =>
            {
                damage.push(effect);
            }
            _ => {}
        }
    }
    for (ability, rate) in regeneration {
        apply_item_armor_modifier(
            status,
            TimedArmorModifier {
                id: ModifierId(ability),
                regeneration_per_second_per_10k: rate,
                aura_membership: true,
                undispellable: true,
                expires_tick: expiry,
                ..Default::default()
            },
        );
    }
    // One aura family per buff: the strongest source wins, as with the unit speed auras.
    speed.sort_unstable_by_key(|(effect, ..)| (effect.buff, effect.ability));
    for family in speed.chunk_by(|left, right| left.0.buff == right.0.buff) {
        let representative = family[0].0;
        let movement_delta = family
            .iter()
            .map(|(_, movement, _)| *movement)
            .max_by_key(|delta| (delta.unsigned_abs(), *delta))
            .unwrap_or(0);
        let attack_delta = family
            .iter()
            .map(|(_, _, attack)| *attack)
            .max_by_key(|delta| (delta.unsigned_abs(), *delta))
            .unwrap_or(0);
        let id = ModifierId(representative.ability);
        apply_item_armor_modifier(
            status,
            TimedArmorModifier {
                aura_membership: true,
                id,
                native_buff: item_native_buff(representative.buff),
                expires_tick: expiry,
                ..Default::default()
            },
        );
        if status_has_movement_modifier_room(status, id) {
            apply_timed_movement_modifier(status, id, movement_delta, expiry);
        }
        if status_has_attack_speed_modifier_room(status, id) {
            apply_timed_attack_speed_modifier(status, id, attack_delta, expiry);
        }
    }
    damage.sort_unstable_by_key(|effect| (effect.buff, effect.ability));
    for family in damage.chunk_by(|left, right| left.buff == right.buff) {
        let bonus = family
            .iter()
            .map(|effect| effect.damage_per_10k)
            .max()
            .unwrap_or(0);
        apply_item_armor_modifier(
            status,
            TimedArmorModifier {
                aura_membership: true,
                id: ModifierId(family[0].ability),
                native_buff: item_native_buff(family[0].buff),
                damage_bonus_per_10k: bonus,
                expires_tick: expiry,
                ..Default::default()
            },
        );
    }
    for (buff_team, carrier) in team_buffs {
        if *buff_team != team
            || !recipient_class_allowed(&carrier.targets, movement)
            || !invulnerability_allowed(&carrier.targets, invulnerable)
        {
            continue;
        }
        apply_item_armor_modifier(
            status,
            TimedArmorModifier {
                aura_membership: true,
                id: ModifierId(carrier.aura_ability),
                native_buff: item_native_buff(carrier.buff),
                damage_bonus_per_10k: carrier.damage_per_10k,
                expires_tick: expiry,
                ..Default::default()
            },
        );
    }
}

fn apply_structure_item_auras(
    status: &mut StatusState,
    team: Team,
    center: SimPoint,
    invulnerable: bool,
    sources: &[AuraSource<'_>],
    expiry: u64,
) {
    let mut regeneration: Vec<(u32, u32)> = Vec::new();
    for source in sources {
        let ItemEffect::RegenerationAura(effect) = source.effect else {
            continue;
        };
        if !effect.targets.structure
            || !invulnerability_allowed(&effect.targets, invulnerable)
            || source.team != team
            || source.position.distance_sq(center) > square_i32(world_to_subunits(effect.radius))
        {
            continue;
        }
        match regeneration
            .iter_mut()
            .find(|(ability, _)| *ability == effect.ability)
        {
            Some((_, rate)) => *rate = rate.saturating_add(effect.regeneration_per_second_per_10k),
            None => regeneration.push((effect.ability, effect.regeneration_per_second_per_10k)),
        }
    }
    for (ability, rate) in regeneration {
        apply_item_armor_modifier(
            status,
            TimedArmorModifier {
                id: ModifierId(ability),
                regeneration_per_second_per_10k: rate,
                aura_membership: true,
                undispellable: true,
                expires_tick: expiry,
                ..Default::default()
            },
        );
    }
}

#[cfg(test)]
mod tests;
