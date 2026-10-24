//! Warcraft III-style unit inventory: six slots, charge stacking, swap/merge moves, perishable
//! consumption, completed-set upgrades and per-unit cooldown groups. Pure state and rules, so the
//! authoritative simulation and its tests share one implementation.
use crate::items::{ItemCatalog, ItemDefinition, ItemId};
use bevy_ecs::prelude::Component;
use serde::{Deserialize, Serialize};

/// Every unit in the source map shows the stock six-slot inventory.
pub const INVENTORY_SLOTS: usize = 6;

/// Reach, in world units, within which a builder picks up, drops or hands over an item. The
/// source map leaves this to the engine's default item interaction range, so it is an engine
/// behaviour rather than extracted content (recorded in docs/verification).
pub const ITEM_INTERACTION_RANGE_WORLD_UNITS: i32 = 150;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemStack {
    pub item: ItemId,
    /// Remaining uses of a charged item; permanent items carry zero.
    pub charges: u8,
    /// First tick on which an automatic item ability (Blast Staff) may fire again.
    pub ready_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemCooldown {
    pub group: u32,
    pub ready_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InventoryError {
    UnknownItem,
    InvalidSlot,
    EmptySlot,
    Full,
}

/// What became of an item offered to an inventory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquireOutcome {
    /// The whole offer was stored (merged into stacks and/or placed in free slots).
    Stored { upgraded_to: Option<ItemId> },
    /// Some charges were merged but the rest found no room; the remainder stays with the giver.
    Partial { remaining: u8 },
}

#[derive(Component, Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inventory {
    slots: [Option<ItemStack>; INVENTORY_SLOTS],
    /// Sorted by group; expired entries are purged each tick.
    cooldowns: Vec<ItemCooldown>,
}

impl Inventory {
    #[must_use]
    pub fn slots(&self) -> &[Option<ItemStack>; INVENTORY_SLOTS] {
        &self.slots
    }

    #[must_use]
    pub fn slot(&self, index: usize) -> Option<&ItemStack> {
        self.slots.get(index).and_then(Option::as_ref)
    }

    #[must_use]
    pub fn slot_mut(&mut self, index: usize) -> Option<&mut ItemStack> {
        self.slots.get_mut(index).and_then(Option::as_mut)
    }

    #[must_use]
    pub fn cooldowns(&self) -> &[ItemCooldown] {
        &self.cooldowns
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }

    pub fn stacks(&self) -> impl Iterator<Item = (usize, &ItemStack)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(index, slot)| slot.as_ref().map(|stack| (index, stack)))
    }

    #[must_use]
    pub fn count(&self, item: ItemId) -> usize {
        self.stacks()
            .filter(|(_, stack)| stack.item == item)
            .count()
    }

    fn first_free(&self) -> Option<usize> {
        self.slots.iter().position(Option::is_none)
    }

    /// Whether `item` with `charges` would be stored in full, without mutating.
    #[must_use]
    pub fn can_hold(&self, catalog: &ItemCatalog, item: ItemId, charges: u8) -> bool {
        let Some(definition) = catalog.item(item) else {
            return false;
        };
        if self.first_free().is_some() {
            return true;
        }
        if !definition.stacks() {
            return false;
        }
        let room: u32 = self
            .stacks()
            .filter(|(_, stack)| stack.item == item)
            .map(|(_, stack)| u32::from(definition.stack_maximum.saturating_sub(stack.charges)))
            .sum();
        room >= u32::from(charges)
    }

    /// Offers an item to the inventory. Charged items first fill existing stacks of the same
    /// type (lowest slot first), the remainder takes the first free slot. A completed upgrade
    /// set collapses immediately, as the source map's pickup handler does.
    pub fn acquire(
        &mut self,
        catalog: &ItemCatalog,
        item: ItemId,
        charges: u8,
        ready_tick: u64,
    ) -> Result<AcquireOutcome, InventoryError> {
        let definition = catalog.item(item).ok_or(InventoryError::UnknownItem)?;
        let mut remaining = charges;
        if definition.stacks() {
            for stack in self.slots.iter_mut().flatten() {
                if stack.item != item || remaining == 0 {
                    continue;
                }
                let room = definition.stack_maximum.saturating_sub(stack.charges);
                let moved = room.min(remaining);
                stack.charges += moved;
                remaining -= moved;
            }
            if remaining == 0 {
                return Ok(AcquireOutcome::Stored { upgraded_to: None });
            }
        }
        match self.first_free() {
            Some(index) => {
                self.slots[index] = Some(ItemStack {
                    item,
                    charges: remaining,
                    ready_tick,
                });
            }
            None if remaining < charges => return Ok(AcquireOutcome::Partial { remaining }),
            None => return Err(InventoryError::Full),
        }
        Ok(AcquireOutcome::Stored {
            upgraded_to: self.apply_upgrade(catalog, item, ready_tick),
        })
    }

    /// The source map swaps every copy of the trigger item for one upgraded item the moment the
    /// holder owns the required count: remove each copy in slot order, and place the
    /// replacement in the first free slot as soon as the first copy has left.
    fn apply_upgrade(
        &mut self,
        catalog: &ItemCatalog,
        item: ItemId,
        ready_tick: u64,
    ) -> Option<ItemId> {
        let upgrade = catalog.upgrade_for(item)?;
        if self.count(item) != usize::from(upgrade.required_count) {
            return None;
        }
        let replacement = ItemId(upgrade.replacement_item);
        let mut placed = false;
        for index in 0..INVENTORY_SLOTS {
            if self.slots[index].is_some_and(|stack| stack.item == item) {
                self.slots[index] = None;
                if !placed {
                    let free = self.first_free().expect("a slot was just vacated");
                    self.slots[free] = Some(ItemStack {
                        item: replacement,
                        charges: 0,
                        ready_tick,
                    });
                    placed = true;
                }
            }
        }
        Some(replacement)
    }

    /// Dragging a stack onto another slot: charged stacks of one type merge up to the maximum
    /// (the remainder stays), anything else swaps or moves into an empty slot.
    pub fn move_slot(
        &mut self,
        catalog: &ItemCatalog,
        from: usize,
        to: usize,
    ) -> Result<(), InventoryError> {
        if from >= INVENTORY_SLOTS || to >= INVENTORY_SLOTS {
            return Err(InventoryError::InvalidSlot);
        }
        let source = self.slots[from].ok_or(InventoryError::EmptySlot)?;
        if from == to {
            return Ok(());
        }
        if let Some(target) = self.slots[to]
            && target.item == source.item
            && catalog
                .item(source.item)
                .is_some_and(ItemDefinition::stacks)
        {
            let definition = catalog.item(source.item).expect("checked above");
            let room = definition.stack_maximum.saturating_sub(target.charges);
            if room > 0 {
                let moved = room.min(source.charges);
                self.slots[to].as_mut().expect("target exists").charges += moved;
                let left = source.charges - moved;
                self.slots[from] = (left > 0).then_some(ItemStack {
                    charges: left,
                    ..source
                });
                return Ok(());
            }
        }
        self.slots.swap(from, to);
        Ok(())
    }

    pub fn remove(&mut self, slot: usize) -> Result<ItemStack, InventoryError> {
        self.slots
            .get_mut(slot)
            .ok_or(InventoryError::InvalidSlot)?
            .take()
            .ok_or(InventoryError::EmptySlot)
    }

    /// Spends one charge of a used item. A perishable item disappears with its last charge.
    pub fn consume_charge(
        &mut self,
        catalog: &ItemCatalog,
        slot: usize,
    ) -> Result<(), InventoryError> {
        let stack = self.slot(slot).copied().ok_or(InventoryError::EmptySlot)?;
        let definition = catalog
            .item(stack.item)
            .ok_or(InventoryError::UnknownItem)?;
        if stack.charges > 0 {
            let remaining = stack.charges - 1;
            if remaining == 0 && definition.perishable {
                self.slots[slot] = None;
            } else {
                self.slots[slot].as_mut().expect("slot is occupied").charges = remaining;
            }
        } else if definition.perishable {
            self.slots[slot] = None;
        }
        Ok(())
    }

    #[must_use]
    pub fn cooldown_ready_tick(&self, group: u32) -> Option<u64> {
        self.cooldowns
            .binary_search_by_key(&group, |cooldown| cooldown.group)
            .ok()
            .map(|index| self.cooldowns[index].ready_tick)
    }

    #[must_use]
    pub fn on_cooldown(&self, group: u32, tick: u64) -> bool {
        self.cooldown_ready_tick(group)
            .is_some_and(|ready| tick < ready)
    }

    pub fn start_cooldown(&mut self, group: u32, ready_tick: u64) {
        match self
            .cooldowns
            .binary_search_by_key(&group, |cooldown| cooldown.group)
        {
            Ok(index) => self.cooldowns[index].ready_tick = ready_tick,
            Err(index) => self
                .cooldowns
                .insert(index, ItemCooldown { group, ready_tick }),
        }
    }

    pub fn purge_cooldowns(&mut self, tick: u64) {
        self.cooldowns.retain(|cooldown| tick < cooldown.ready_tick);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{MapVersion, items::item_catalog_for_version};

    fn catalog() -> &'static ItemCatalog {
        item_catalog_for_version(MapVersion::CASTLE_FIGHT_9_27).unwrap()
    }

    fn id(rawcode: &str) -> ItemId {
        ItemId(u32::from_be_bytes(rawcode.as_bytes().try_into().unwrap()))
    }

    #[test]
    fn charged_items_merge_to_the_stack_limit_and_overflow_into_a_new_slot() {
        let stone = id("I005");
        let limit = catalog().item(stone).unwrap().stack_maximum;
        let mut inventory = Inventory::default();
        inventory.acquire(catalog(), stone, limit - 1, 0).unwrap();
        inventory.acquire(catalog(), stone, 3, 0).unwrap();
        assert_eq!(inventory.slot(0).unwrap().charges, limit);
        assert_eq!(inventory.slot(1).unwrap().charges, 2);
    }

    #[test]
    fn a_full_inventory_only_accepts_what_fits_in_existing_stacks() {
        let stone = id("I005");
        let limit = catalog().item(stone).unwrap().stack_maximum;
        let mut inventory = Inventory::default();
        inventory.acquire(catalog(), stone, limit - 1, 0).unwrap();
        for filler in ["I009", "I00A", "I003", "I006", "I001"] {
            inventory.acquire(catalog(), id(filler), 0, 0).unwrap();
        }
        assert!(inventory.can_hold(catalog(), stone, 1));
        assert!(!inventory.can_hold(catalog(), stone, 2));
        assert_eq!(
            inventory.acquire(catalog(), stone, 3, 0),
            Ok(AcquireOutcome::Partial { remaining: 2 })
        );
        assert_eq!(
            inventory.acquire(catalog(), id("I009"), 0, 0),
            Err(InventoryError::Full)
        );
    }

    #[test]
    fn a_completed_upgrade_set_collapses_into_the_first_free_slot() {
        let staff = id("I001");
        let rune = id("I009");
        let mut inventory = Inventory::default();
        inventory.acquire(catalog(), staff, 0, 0).unwrap();
        inventory.acquire(catalog(), rune, 0, 0).unwrap();
        for _ in 0..2 {
            inventory.acquire(catalog(), staff, 0, 0).unwrap();
        }
        let outcome = inventory.acquire(catalog(), staff, 0, 0).unwrap();
        let upgraded = ItemId(catalog().upgrade_for(staff).unwrap().replacement_item);
        assert_eq!(
            outcome,
            AcquireOutcome::Stored {
                upgraded_to: Some(upgraded)
            }
        );
        assert_eq!(inventory.count(staff), 0);
        // The first vacated slot (0) receives the replacement; the rune keeps slot 1.
        assert_eq!(inventory.slot(0).unwrap().item, upgraded);
        assert_eq!(inventory.slot(1).unwrap().item, rune);
        assert_eq!(inventory.stacks().count(), 2);
    }

    #[test]
    fn perishable_items_vanish_with_their_last_charge() {
        let stone = id("I005");
        let mut inventory = Inventory::default();
        inventory.acquire(catalog(), stone, 2, 0).unwrap();
        inventory.consume_charge(catalog(), 0).unwrap();
        assert_eq!(inventory.slot(0).unwrap().charges, 1);
        inventory.consume_charge(catalog(), 0).unwrap();
        assert!(inventory.slot(0).is_none());
    }

    #[test]
    fn moving_onto_a_matching_stack_merges_otherwise_swaps() {
        let stone = id("I005");
        let rune = id("I009");
        let mut inventory = Inventory::default();
        inventory.acquire(catalog(), stone, 2, 0).unwrap();
        inventory.acquire(catalog(), rune, 0, 0).unwrap();
        inventory.move_slot(catalog(), 0, 1).unwrap();
        assert_eq!(inventory.slot(0).unwrap().item, rune);
        assert_eq!(inventory.slot(1).unwrap().item, stone);
        inventory.move_slot(catalog(), 1, 3).unwrap();
        assert!(inventory.slot(1).is_none());
        assert_eq!(inventory.slot(3).unwrap().item, stone);
    }

    #[test]
    fn cooldown_groups_are_independent_and_expire() {
        let mut inventory = Inventory::default();
        inventory.start_cooldown(7, 100);
        inventory.start_cooldown(3, 50);
        assert!(inventory.on_cooldown(7, 99));
        assert!(!inventory.on_cooldown(7, 100));
        inventory.purge_cooldowns(60);
        assert_eq!(inventory.cooldowns().len(), 1);
    }
}
