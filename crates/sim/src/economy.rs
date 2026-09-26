use bevy_ecs::prelude::Component;
use serde::{Deserialize, Serialize};

pub const RESOURCE_FIXED_SCALE: u64 = 10_000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct EconomyRules {
    pub starting_gold: u32,
    pub starting_lumber: u32,
    pub starting_legendary_points: u16,
    /// Raw gold paid every income interval before progressive tax, in 1/10,000 gold units.
    pub base_income_per_10k: u64,
    pub income_interval_ticks: u32,
    /// Size of one progressive-income-tax bracket, in 1/10,000 gold units.
    pub income_tax_bracket_per_10k: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlayerResources {
    pub gold: u32,
    pub lumber: u32,
    pub legendary_points_used: u16,
    pub legendary_points_cap: u16,
}

impl PlayerResources {
    #[must_use]
    pub const fn legendary_points_available(self) -> u16 {
        self.legendary_points_cap
            .saturating_sub(self.legendary_points_used)
    }
}

#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingEconomyProfile {
    pub gold_cost: u32,
    pub lumber_cost: u32,
    /// Lumber awarded when this building finishes. In Castle Fight this is normally the full gold
    /// cost for zero-lumber buildings and 75% of the gold cost for Siege buildings.
    pub lumber_refund: u32,
    /// Legendary points reserved while this building is ordered, under construction, or active.
    pub legendary_points_cost: u16,
    /// Gold income contributed by this finished building on each income tick, in 1/10,000 gold.
    /// Upgrade definitions include the inherited contribution of their precursor chain.
    pub income_per_10k: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerEconomyView {
    pub resources: PlayerResources,
    pub income: u32,
    pub income_interval_ticks: u32,
    pub income_progress_per_10k: u16,
    pub ticks_until_income: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResourcePurchaseError {
    InsufficientGold { available: u32, required: u32 },
    InsufficientLumber { available: u32, required: u32 },
    InsufficientLegendaryPoints { available: u16, required: u16 },
}

#[must_use]
pub(crate) fn taxed_income_from_fixed(raw_income_per_10k: u64, bracket_per_10k: u64) -> u32 {
    if raw_income_per_10k == 0 {
        return 0;
    }
    if bracket_per_10k == 0 {
        return u32::try_from(raw_income_per_10k / RESOURCE_FIXED_SCALE).unwrap_or(u32::MAX);
    }

    // Castle Fight's default tax walks up to eight equally-sized brackets. The first is untaxed,
    // then each complete bracket loses another 10%, with all income beyond the eighth bracket at
    // 20%. Accumulate the percentage numerator and round only once at the end, matching the map's
    // real-valued sum followed by real_toInt(... + 0.001).
    let mut remaining = raw_income_per_10k;
    let mut bracket = 0u32;
    let mut weighted_income = 0u128;
    while remaining >= bracket_per_10k && bracket < 8 {
        let percent = 100u128 - u128::from(bracket) * 10;
        weighted_income += u128::from(bracket_per_10k) * percent;
        remaining -= bracket_per_10k;
        bracket += 1;
    }
    let percent = 100u128 - u128::from(bracket) * 10;
    weighted_income += u128::from(remaining) * percent;

    let whole_gold = weighted_income / 100 / u128::from(RESOURCE_FIXED_SCALE);
    u32::try_from(whole_gold).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn castle_fight_tax_brackets_match_progressive_formula() {
        let bracket = 25 * RESOURCE_FIXED_SCALE;
        assert_eq!(
            taxed_income_from_fixed(24 * RESOURCE_FIXED_SCALE, bracket),
            24
        );
        assert_eq!(
            taxed_income_from_fixed(25 * RESOURCE_FIXED_SCALE, bracket),
            25
        );
        assert_eq!(
            taxed_income_from_fixed(50 * RESOURCE_FIXED_SCALE, bracket),
            47
        );
        assert_eq!(
            taxed_income_from_fixed(75 * RESOURCE_FIXED_SCALE, bracket),
            67
        );
        assert_eq!(
            taxed_income_from_fixed(250 * RESOURCE_FIXED_SCALE, bracket),
            140
        );
    }
}
