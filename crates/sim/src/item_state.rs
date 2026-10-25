//! Authoritative item-system state that lives outside a unit's inventory: per-team castle shop
//! stock, timed team buffs from consumed power-ups, missiles in flight, items on the ground and
//! a builder's pending walk-then-act item order.
use crate::components::{SimId, Team};
use crate::items::ItemId;
use crate::math::SimPoint;
use bevy_ecs::prelude::Component;
use serde::{Deserialize, Serialize};

/// Stock of one castle shop entry. The shop starts empty and opens after the authored start
/// delay, then restocks one unit per interval while below the maximum.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShopSlotState {
    pub opened: bool,
    pub stock: u8,
    pub restock_tick: Option<u64>,
    /// First tick on which this button may be pressed again (the authored runtime cooldown).
    pub button_ready_tick: u64,
}

/// A consumed damage power-up's map-wide aura (the source map's timed `e008` carrier).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamItemBuff {
    pub team: Team,
    pub item: ItemId,
    pub expires_tick: u64,
}

/// A Blast Staff projectile committed at launch; damage lands once its flight completes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingItemImpact {
    /// Identity of the missile in flight, so presentation can follow it between snapshots.
    pub id: SimId,
    pub source: SimId,
    pub source_team: Team,
    pub target: SimId,
    pub item: ItemId,
    pub damage: i32,
    pub launch_position: SimPoint,
    pub launch_tick: u64,
    pub impact_tick: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemSystemState {
    /// One shop per team, entries in shop-position order.
    pub shops: [Vec<ShopSlotState>; 2],
    pub team_buffs: Vec<TeamItemBuff>,
    pub impacts: Vec<PendingItemImpact>,
}

impl ItemSystemState {
    /// Fresh state for a round: every shop entry closed and empty.
    #[must_use]
    pub fn for_catalog(catalog: Option<&crate::items::ItemCatalog>) -> Self {
        let entries = catalog.map_or(0, |catalog| catalog.shop.entries.len());
        Self {
            shops: [
                vec![ShopSlotState::default(); entries],
                vec![ShopSlotState::default(); entries],
            ],
            team_buffs: Vec::new(),
            impacts: Vec::new(),
        }
    }
}

/// An item lying on the ground, waiting to be picked up by walking onto it.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroundItem {
    pub item: ItemId,
    pub charges: u8,
    pub position: SimPoint,
}

/// What an item use is aimed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemUseTarget {
    None,
    Point(SimPoint),
    Unit(SimId),
}

/// A builder's committed item action; it walks into range, acts, then goes idle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemOrder {
    Pickup { item: SimId },
    Use { slot: u8, target: ItemUseTarget },
    Drop { slot: u8, position: SimPoint },
    Give { slot: u8, recipient: SimId },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingItemOrder {
    pub order: ItemOrder,
    /// A walk toward the target has been issued; losing the walk without arriving cancels.
    pub walking: bool,
}
