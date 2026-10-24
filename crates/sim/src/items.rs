//! Version-scoped Castle Fight item catalog: the castle shop, inventory rules and item effects.
//!
//! Source of truth is `data/castle-fight/9.27/items-r1.json`, generated reproducibly from the
//! extracted resolved tables by `tools/wc3-map/project_items.py`. Nothing here restates a map
//! value: callers look definitions up through [`item_catalog_for_version`].
use crate::{CASTLE_FIGHT_SIMULATION_HZ, MapVersion, SUBUNITS_PER_WORLD_UNIT};
use serde::{Deserialize, Deserializer, Serialize};
use std::sync::OnceLock;

/// A four-character item rawcode packed big-endian, matching the map's integer item ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ItemId(pub u32);

impl ItemId {
    /// The rawcode text, for diagnostics and presentation lookups.
    #[must_use]
    pub fn rawcode(self) -> String {
        String::from_utf8_lossy(&self.0.to_be_bytes()).into_owned()
    }
}

fn rawcode_id<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    let text = String::deserialize(deserializer)?;
    rawcode_from_str(&text).ok_or_else(|| serde::de::Error::custom(format!("bad rawcode {text:?}")))
}

fn rawcode_from_str(text: &str) -> Option<u32> {
    let bytes: [u8; 4] = text.as_bytes().try_into().ok()?;
    Some(u32::from_be_bytes(bytes))
}

/// The authored target list of an item ability (`ground,air,friend,structure,...`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ItemTargets {
    pub ground: bool,
    pub air: bool,
    pub structure: bool,
    pub friend: bool,
    pub enemy: bool,
    pub invulnerable: bool,
    pub vulnerable: bool,
}

impl<'de> Deserialize<'de> for ItemTargets {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        let mut targets = Self::default();
        for token in text.split(',').map(str::trim) {
            match token {
                "ground" => targets.ground = true,
                "air" => targets.air = true,
                "structure" => targets.structure = true,
                "friend" => targets.friend = true,
                "enemy" | "enemies" => targets.enemy = true,
                "invu" | "invulnerable" => targets.invulnerable = true,
                "vuln" | "vulnerable" => targets.vulnerable = true,
                // Relations/qualifiers whose absence of effect is already modelled by the
                // classes above: `self` is covered by `friend`, `nonsapper` is the default
                // for these auras, and `neutral` never matches a player's units.
                "self" | "nonsapper" | "neutral" => {}
                other => {
                    return Err(serde::de::Error::custom(format!(
                        "unsupported item target qualifier {other:?}"
                    )));
                }
            }
        }
        Ok(targets)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemClass {
    Permanent,
    Charged,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemUpgrade {
    #[serde(deserialize_with = "rawcode_id")]
    pub trigger_item: u32,
    pub required_count: u8,
    #[serde(deserialize_with = "rawcode_id")]
    pub replacement_item: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InventoryRules {
    pub slots: u8,
    pub upgrades: Vec<ItemUpgrade>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShopStock {
    pub maximum: u8,
    pub initial: u8,
    pub start_delay_seconds: u32,
    pub replenish_interval_seconds: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShopEntry {
    pub position: u8,
    pub slot: u8,
    #[serde(deserialize_with = "rawcode_id")]
    pub item: u32,
    #[serde(deserialize_with = "rawcode_id")]
    pub button_ability: u32,
    pub hotkey: String,
    pub button_icon: String,
    pub runtime_button_cooldown_seconds: f64,
    pub stock: ShopStock,
}

/// A command-card cell, column then row from the top left.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShopButtonPosition {
    pub x: u8,
    pub y: u8,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShopDefinition {
    #[serde(deserialize_with = "rawcode_id")]
    pub spellbook_ability: u32,
    pub spellbook_hotkey: String,
    pub spellbook_columns: u8,
    pub spellbook_position: ShopButtonPosition,
    pub filler_position: u8,
    pub entries: Vec<ShopEntry>,
}

impl ShopDefinition {
    /// The command-card cell of a shop entry: the spellbook lays its abilities out in list order
    /// across `spellbook_columns`, with the filler button occupying its own list position.
    #[must_use]
    pub fn card_cell(&self, entry: usize) -> ShopButtonPosition {
        let listed = entry + usize::from(entry >= usize::from(self.filler_position));
        let columns = usize::from(self.spellbook_columns);
        ShopButtonPosition {
            x: u8::try_from(listed % columns).expect("spellbook column fits u8"),
            y: u8::try_from(listed / columns).expect("spellbook row fits u8"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlastEffect {
    #[serde(deserialize_with = "rawcode_id")]
    pub ability: u32,
    pub copies: u8,
    pub damage: i32,
    pub radius: i32,
    pub cooldown_seconds: f64,
    pub missile_speed: u32,
    pub missile_art: String,
    pub targets: ItemTargets,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DamageAuraEffect {
    #[serde(deserialize_with = "rawcode_id")]
    pub ability: u32,
    #[serde(deserialize_with = "rawcode_id")]
    pub buff: u32,
    pub radius: i32,
    pub damage_per_10k: i16,
    pub targets: ItemTargets,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeedAuraEffect {
    #[serde(deserialize_with = "rawcode_id")]
    pub ability: u32,
    #[serde(deserialize_with = "rawcode_id")]
    pub buff: u32,
    pub radius: i32,
    pub movement_percent: i16,
    pub attack_speed_percent: i16,
    pub targets: ItemTargets,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegenerationAuraEffect {
    #[serde(deserialize_with = "rawcode_id")]
    pub ability: u32,
    pub radius: i32,
    pub regeneration_per_second_per_10k: u32,
    pub targets: ItemTargets,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TeamDamageCarrierEffect {
    #[serde(deserialize_with = "rawcode_id")]
    pub aura_ability: u32,
    #[serde(deserialize_with = "rawcode_id")]
    pub buff: u32,
    pub radius: i32,
    pub damage_per_10k: i16,
    #[serde(deserialize_with = "rawcode_id")]
    pub carrier_unit: u32,
    pub carrier_lifetime_seconds: f64,
    pub targets: ItemTargets,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LegendaryPointEffect {
    pub legendary_points: u16,
    pub refund_gold_when_inactive: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointScrollRestorationEffect {
    #[serde(deserialize_with = "rawcode_id")]
    pub trigger_ability: u32,
    #[serde(deserialize_with = "rawcode_id")]
    pub effect_ability: u32,
    pub range: i32,
    pub radius: i32,
    pub duration_seconds: f64,
    #[serde(deserialize_with = "rawcode_id")]
    pub buff: u32,
    pub targets: ItemTargets,
    #[serde(deserialize_with = "rawcode_id")]
    pub carrier_unit: u32,
    pub carrier_lifetime_seconds: f64,
    pub armor_bonus: i16,
    pub life: i32,
    pub mana: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointScrollSpeedEffect {
    #[serde(deserialize_with = "rawcode_id")]
    pub trigger_ability: u32,
    #[serde(deserialize_with = "rawcode_id")]
    pub effect_ability: u32,
    pub range: i32,
    pub radius: i32,
    pub duration_seconds: f64,
    #[serde(deserialize_with = "rawcode_id")]
    pub buff: u32,
    pub targets: ItemTargets,
    #[serde(deserialize_with = "rawcode_id")]
    pub carrier_unit: u32,
    pub carrier_lifetime_seconds: f64,
    pub movement_speed_increase_per_10k: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrbEffectLevel {
    pub level: u8,
    pub targets: u8,
    pub damage: i32,
    pub damage_reduction_per_target_per_10k: u16,
    pub jump_radius: i32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrbChainLightningEffect {
    #[serde(deserialize_with = "rawcode_id")]
    pub trigger_ability: u32,
    #[serde(deserialize_with = "rawcode_id")]
    pub effect_ability: u32,
    pub range: i32,
    pub use_cooldown_seconds: f64,
    pub level_step_minutes: u32,
    pub dummy_recycle_delay_seconds: f64,
    pub missile_speed: u32,
    pub targets: ItemTargets,
    pub effect_levels: Vec<OrbEffectLevel>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ItemEffect {
    Blast(BlastEffect),
    DamageAura(DamageAuraEffect),
    SpeedAura(SpeedAuraEffect),
    RegenerationAura(RegenerationAuraEffect),
    TeamDamageCarrier(TeamDamageCarrierEffect),
    LegendaryPoint(LegendaryPointEffect),
    PointScrollRestoration(PointScrollRestorationEffect),
    PointScrollSpeed(PointScrollSpeedEffect),
    OrbChainLightning(OrbChainLightningEffect),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemDefinition {
    #[serde(deserialize_with = "rawcode_id")]
    pub rawcode: u32,
    pub rawcode_integer: u32,
    pub name: String,
    pub icon: String,
    pub tooltip_title: String,
    pub tooltip: String,
    pub gold_cost: u32,
    pub lumber_cost: u32,
    pub class: ItemClass,
    pub initial_charges: u8,
    pub stack_maximum: u8,
    #[serde(deserialize_with = "rawcode_id")]
    pub cooldown_group: u32,
    pub usable: bool,
    pub perishable: bool,
    pub powerup: bool,
    pub droppable: bool,
    pub drop_on_death: bool,
    pub pawnable: bool,
    pub sellable: bool,
    pub effects: Vec<ItemEffect>,
}

impl ItemDefinition {
    #[must_use]
    pub const fn id(&self) -> ItemId {
        ItemId(self.rawcode)
    }

    /// Charged items stack up to their authored maximum; permanent items never merge.
    #[must_use]
    pub fn stacks(&self) -> bool {
        self.class == ItemClass::Charged && self.stack_maximum > 1
    }

    /// The effect that fires when the item is used from its slot, if it has one.
    #[must_use]
    pub fn use_effect(&self) -> Option<&ItemEffect> {
        self.usable
            .then(|| {
                self.effects.iter().find(|effect| {
                    matches!(
                        effect,
                        ItemEffect::PointScrollRestoration(_)
                            | ItemEffect::PointScrollSpeed(_)
                            | ItemEffect::OrbChainLightning(_)
                    )
                })
            })
            .flatten()
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemCatalog {
    pub map_version: String,
    pub retained_revision: String,
    pub source: serde_json::Value,
    pub inventory: InventoryRules,
    pub shop: ShopDefinition,
    pub items: Vec<ItemDefinition>,
}

impl ItemCatalog {
    #[must_use]
    pub fn item(&self, id: ItemId) -> Option<&ItemDefinition> {
        self.items.iter().find(|item| item.rawcode == id.0)
    }

    #[must_use]
    pub fn shop_entry(&self, position: u8) -> Option<&ShopEntry> {
        self.shop
            .entries
            .iter()
            .find(|entry| entry.position == position)
    }

    /// The upgrade a completed set of identical items collapses into, if any.
    #[must_use]
    pub fn upgrade_for(&self, trigger: ItemId) -> Option<&ItemUpgrade> {
        self.inventory
            .upgrades
            .iter()
            .find(|upgrade| upgrade.trigger_item == trigger.0)
    }
}

#[must_use]
pub fn item_catalog_for_version(version: MapVersion) -> Option<&'static ItemCatalog> {
    static CATALOG: OnceLock<ItemCatalog> = OnceLock::new();
    (version == MapVersion::CASTLE_FIGHT_9_27).then(|| {
        CATALOG.get_or_init(|| {
            let catalog: ItemCatalog =
                serde_json::from_str(include_str!("../data/castle-fight/9.27/items-r1.json"))
                    .expect("validated retained item projection");
            for item in &catalog.items {
                assert_eq!(item.rawcode, item.rawcode_integer);
            }
            catalog
        })
    })
}

/// Whole simulation ticks covering an authored duration, rounding partial ticks up.
#[must_use]
pub fn seconds_to_ticks(seconds: f64) -> u64 {
    (seconds * f64::from(CASTLE_FIGHT_SIMULATION_HZ)).ceil() as u64
}

/// Authored world units per second to subunits per tick.
#[must_use]
pub fn speed_per_tick(world_units_per_second: u32) -> i32 {
    (f64::from(world_units_per_second) * f64::from(SUBUNITS_PER_WORLD_UNIT)
        / f64::from(CASTLE_FIGHT_SIMULATION_HZ))
    .round() as i32
}

/// Authored world distance to subunits.
#[must_use]
pub const fn world_to_subunits(world_units: i32) -> i32 {
    world_units * SUBUNITS_PER_WORLD_UNIT
}
