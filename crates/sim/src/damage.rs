use std::{error::Error, fmt};

use bevy_ecs::prelude::Component;

pub const DAMAGE_MULTIPLIER_SCALE: u16 = 10_000;
const ARMOR_EXP_SCALE: i128 = 1_000_000_000;
const STOCK_ARMOR_FACTOR_PER_10K: u16 = 600;

#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DamageType {
    #[default]
    Normal,
    Pierce,
    Siege,
    Magic,
    Chaos,
    Spells,
    Hero,
}

impl DamageType {
    pub const COUNT: usize = 7;

    #[must_use]
    pub const fn stable_tag(self) -> u8 {
        match self {
            Self::Normal => 0,
            Self::Pierce => 1,
            Self::Siege => 2,
            Self::Magic => 3,
            Self::Chaos => 4,
            Self::Spells => 5,
            Self::Hero => 6,
        }
    }

    const fn index(self) -> usize {
        self.stable_tag() as usize
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ArmorType {
    Small,
    Medium,
    Large,
    Fortified,
    Normal,
    Hero,
    Divine,
    #[default]
    Unarmored,
}

impl ArmorType {
    pub const COUNT: usize = 8;

    #[must_use]
    pub const fn stable_tag(self) -> u8 {
        match self {
            Self::Small => 0,
            Self::Medium => 1,
            Self::Large => 2,
            Self::Fortified => 3,
            Self::Normal => 4,
            Self::Hero => 5,
            Self::Divine => 6,
            Self::Unarmored => 7,
        }
    }

    const fn index(self) -> usize {
        self.stable_tag() as usize
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ArmorProfile {
    pub armor_type: ArmorType,
    pub armor_points: i16,
}

impl ArmorProfile {
    pub const UNARMORED: Self = Self::new(ArmorType::Unarmored, 0);

    #[must_use]
    pub const fn new(armor_type: ArmorType, armor_points: i16) -> Self {
        Self {
            armor_type,
            armor_points,
        }
    }
}

impl Default for ArmorProfile {
    fn default() -> Self {
        Self::UNARMORED
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DamageRules {
    armor_factor_per_10k: u16,
    bonuses_per_10k: [[u16; ArmorType::COUNT]; DamageType::COUNT],
}

impl DamageRules {
    #[must_use]
    pub const fn warcraft_frozen_throne() -> Self {
        Self {
            armor_factor_per_10k: STOCK_ARMOR_FACTOR_PER_10K,
            bonuses_per_10k: [
                [10_000, 15_000, 10_000, 7_000, 10_000, 10_000, 500, 10_000],
                [20_000, 7_500, 10_000, 3_500, 10_000, 5_000, 500, 15_000],
                [10_000, 5_000, 10_000, 15_000, 10_000, 5_000, 500, 15_000],
                [12_500, 7_500, 20_000, 3_500, 10_000, 5_000, 500, 10_000],
                [10_000; ArmorType::COUNT],
                [10_000, 10_000, 10_000, 10_000, 10_000, 7_500, 500, 10_000],
                [10_000, 10_000, 10_000, 5_000, 10_000, 10_000, 500, 10_000],
            ],
        }
    }

    pub fn from_wc3_misc_text(text: &str) -> Result<Self, DamageRulesLoadError> {
        let mut rules = Self::warcraft_frozen_throne();
        let mut in_misc = false;
        for raw_line in text.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with("//") || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') && line.ends_with(']') {
                in_misc = line.eq_ignore_ascii_case("[Misc]");
                continue;
            }
            if !in_misc {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            let value = value.trim();
            match key {
                "DefenseArmor" => {
                    rules.armor_factor_per_10k = parse_decimal_per_10k(value, key)?;
                }
                "DamageBonusNormal" => {
                    rules.set_row(DamageType::Normal, parse_bonus_row(value, key)?);
                }
                "DamageBonusPierce" => {
                    rules.set_row(DamageType::Pierce, parse_bonus_row(value, key)?);
                }
                "DamageBonusSiege" => {
                    rules.set_row(DamageType::Siege, parse_bonus_row(value, key)?);
                }
                "DamageBonusMagic" => {
                    rules.set_row(DamageType::Magic, parse_bonus_row(value, key)?);
                }
                "DamageBonusChaos" => {
                    rules.set_row(DamageType::Chaos, parse_bonus_row(value, key)?);
                }
                "DamageBonusSpells" => {
                    rules.set_row(DamageType::Spells, parse_bonus_row(value, key)?);
                }
                "DamageBonusHero" => {
                    rules.set_row(DamageType::Hero, parse_bonus_row(value, key)?);
                }
                _ => {}
            }
        }
        Ok(rules)
    }

    #[must_use]
    pub const fn armor_factor_per_10k(self) -> u16 {
        self.armor_factor_per_10k
    }

    #[must_use]
    pub const fn bonus_per_10k(self, damage_type: DamageType, armor_type: ArmorType) -> u16 {
        self.bonuses_per_10k[damage_type.index()][armor_type.index()]
    }

    #[must_use]
    pub fn apply_attack(
        self,
        raw_damage: i32,
        damage_type: DamageType,
        armor: ArmorProfile,
    ) -> i32 {
        self.apply_attack_with_armor_per_100(
            raw_damage,
            damage_type,
            armor.armor_type,
            i32::from(armor.armor_points) * 100,
        )
    }

    #[must_use]
    pub fn apply_attack_with_armor_per_100(
        self,
        raw_damage: i32,
        damage_type: DamageType,
        armor_type: ArmorType,
        armor_points_per_100: i32,
    ) -> i32 {
        if raw_damage <= 0 {
            return raw_damage;
        }
        let bonus = i128::from(self.bonus_per_10k(damage_type, armor_type));
        if bonus == 0 {
            return 0;
        }
        let raw = i128::from(raw_damage);
        let factor = i128::from(self.armor_factor_per_10k);
        let adjusted = if armor_points_per_100 >= 0 {
            let denominator = i128::from(DAMAGE_MULTIPLIER_SCALE) * 100
                + factor * i128::from(armor_points_per_100);
            round_ratio(raw * bonus * 100, denominator)
        } else if armor_points_per_100 % 100 == 0 {
            let base = i128::from(DAMAGE_MULTIPLIER_SCALE) - factor;
            let mut remaining = u32::try_from((-armor_points_per_100) / 100)
                .expect("negative armor magnitude exceeds u32");
            let mut power = ARMOR_EXP_SCALE;
            while remaining > 0 {
                power = round_ratio(power * base, i128::from(DAMAGE_MULTIPLIER_SCALE));
                remaining -= 1;
            }
            let armor_multiplier = 2 * ARMOR_EXP_SCALE - power;
            round_ratio(
                raw * bonus * armor_multiplier,
                i128::from(DAMAGE_MULTIPLIER_SCALE) * ARMOR_EXP_SCALE,
            )
        } else {
            panic!("fractional negative armor is not yet supported");
        };
        i32::try_from(adjusted.max(1)).expect("adjusted damage exceeds i32")
    }

    #[must_use]
    pub fn apply_spell(self, raw_damage: i32, armor_type: ArmorType) -> i32 {
        if raw_damage <= 0 {
            return raw_damage;
        }
        let bonus = i128::from(self.bonus_per_10k(DamageType::Spells, armor_type));
        if bonus == 0 {
            return 0;
        }
        let adjusted = round_ratio(
            i128::from(raw_damage) * bonus,
            i128::from(DAMAGE_MULTIPLIER_SCALE),
        );
        i32::try_from(adjusted.max(1)).expect("adjusted spell damage exceeds i32")
    }

    const fn set_row(&mut self, damage_type: DamageType, row: [u16; ArmorType::COUNT]) {
        self.bonuses_per_10k[damage_type.index()] = row;
    }
}

impl Default for DamageRules {
    fn default() -> Self {
        Self::warcraft_frozen_throne()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DamageRulesLoadError {
    InvalidNumber { key: String, value: String },
    WrongColumnCount { key: String, found: usize },
}

impl fmt::Display for DamageRulesLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidNumber { key, value } => {
                write!(f, "invalid decimal value {value:?} for {key}")
            }
            Self::WrongColumnCount { key, found } => write!(
                f,
                "{key} must contain exactly {} armor columns, found {found}",
                ArmorType::COUNT
            ),
        }
    }
}

impl Error for DamageRulesLoadError {}

fn parse_bonus_row(
    value: &str,
    key: &str,
) -> Result<[u16; ArmorType::COUNT], DamageRulesLoadError> {
    let values = value.split(',').map(str::trim).collect::<Vec<_>>();
    if values.len() != ArmorType::COUNT {
        return Err(DamageRulesLoadError::WrongColumnCount {
            key: key.to_owned(),
            found: values.len(),
        });
    }
    let mut row = [0; ArmorType::COUNT];
    for (index, value) in values.into_iter().enumerate() {
        row[index] = parse_decimal_per_10k(value, key)?;
    }
    Ok(row)
}

fn parse_decimal_per_10k(value: &str, key: &str) -> Result<u16, DamageRulesLoadError> {
    let (whole, fractional) = value.split_once('.').unwrap_or((value, ""));
    let whole: u32 = whole
        .parse()
        .map_err(|_| DamageRulesLoadError::InvalidNumber {
            key: key.to_owned(),
            value: value.to_owned(),
        })?;
    if fractional.len() > 4 || !fractional.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(DamageRulesLoadError::InvalidNumber {
            key: key.to_owned(),
            value: value.to_owned(),
        });
    }
    let mut fraction = if fractional.is_empty() {
        0
    } else {
        fractional
            .parse::<u32>()
            .map_err(|_| DamageRulesLoadError::InvalidNumber {
                key: key.to_owned(),
                value: value.to_owned(),
            })?
    };
    for _ in fractional.len()..4 {
        fraction *= 10;
    }
    let scaled = whole
        .checked_mul(u32::from(DAMAGE_MULTIPLIER_SCALE))
        .and_then(|whole| whole.checked_add(fraction))
        .ok_or_else(|| DamageRulesLoadError::InvalidNumber {
            key: key.to_owned(),
            value: value.to_owned(),
        })?;
    u16::try_from(scaled).map_err(|_| DamageRulesLoadError::InvalidNumber {
        key: key.to_owned(),
        value: value.to_owned(),
    })
}

fn round_ratio(numerator: i128, denominator: i128) -> i128 {
    debug_assert!(numerator >= 0);
    debug_assert!(denominator > 0);
    (numerator + denominator / 2) / denominator
}

#[cfg(test)]
mod tests {
    use super::*;

    const CASTLE_FIGHT_MISC: &str =
        include_str!("../../../docs/original_map/extracted/war3mapMisc.txt");

    #[test]
    fn castle_fight_misc_loads_exact_damage_rows_and_stock_armor_factor() {
        let rules = DamageRules::from_wc3_misc_text(CASTLE_FIGHT_MISC).unwrap();
        assert_eq!(rules.armor_factor_per_10k(), 600);
        assert_eq!(
            rules.bonus_per_10k(DamageType::Normal, ArmorType::Small),
            7_000
        );
        assert_eq!(
            rules.bonus_per_10k(DamageType::Normal, ArmorType::Medium),
            17_500
        );
        assert_eq!(
            rules.bonus_per_10k(DamageType::Pierce, ArmorType::Small),
            17_500
        );
        assert_eq!(
            rules.bonus_per_10k(DamageType::Magic, ArmorType::Large),
            17_500
        );
        assert_eq!(
            rules.bonus_per_10k(DamageType::Siege, ArmorType::Fortified),
            16_000
        );
        assert_eq!(
            rules.bonus_per_10k(DamageType::Spells, ArmorType::Hero),
            7_000
        );
        assert_eq!(
            rules.bonus_per_10k(DamageType::Hero, ArmorType::Divine),
            4_000
        );
        assert_eq!(
            rules.bonus_per_10k(DamageType::Chaos, ArmorType::Fortified),
            10_000
        );

        let armor_types = [
            ArmorType::Small,
            ArmorType::Medium,
            ArmorType::Large,
            ArmorType::Fortified,
            ArmorType::Normal,
            ArmorType::Hero,
            ArmorType::Divine,
            ArmorType::Unarmored,
        ];
        let rows = [
            (
                DamageType::Hero,
                [11_000, 11_000, 11_000, 6_000, 11_000, 6_000, 4_000, 11_000],
            ),
            (
                DamageType::Magic,
                [10_000, 7_000, 17_500, 4_000, 10_000, 6_000, 2_500, 10_500],
            ),
            (
                DamageType::Normal,
                [7_000, 17_500, 10_000, 5_000, 10_000, 6_000, 2_500, 10_500],
            ),
            (
                DamageType::Pierce,
                [17_500, 10_000, 7_000, 4_500, 10_000, 6_000, 2_500, 10_500],
            ),
            (
                DamageType::Siege,
                [7_000, 7_000, 7_000, 16_000, 8_000, 4_000, 2_000, 10_000],
            ),
            (
                DamageType::Spells,
                [10_000, 10_000, 10_000, 10_000, 10_000, 7_000, 2_500, 10_000],
            ),
        ];
        for (damage_type, expected) in rows {
            for (armor_type, expected_bonus) in armor_types.into_iter().zip(expected) {
                assert_eq!(rules.bonus_per_10k(damage_type, armor_type), expected_bonus);
            }
        }
    }

    #[test]
    fn positive_and_negative_armor_use_warcraft_formula() {
        let rules = DamageRules::warcraft_frozen_throne();
        assert_eq!(
            rules.apply_attack(
                100,
                DamageType::Chaos,
                ArmorProfile::new(ArmorType::Unarmored, 5),
            ),
            77
        );
        assert_eq!(
            rules.apply_attack(
                100,
                DamageType::Chaos,
                ArmorProfile::new(ArmorType::Unarmored, -5),
            ),
            127
        );
    }

    #[test]
    fn typed_unarmored_baseline_preserves_raw_damage() {
        let rules = DamageRules::warcraft_frozen_throne();
        assert_eq!(
            rules.apply_attack(123, DamageType::Normal, ArmorProfile::UNARMORED),
            123
        );
    }

    #[test]
    fn spell_damage_uses_defense_type_but_ignores_numeric_armor() {
        let rules = DamageRules::from_wc3_misc_text(CASTLE_FIGHT_MISC).unwrap();
        assert_eq!(rules.apply_spell(100, ArmorType::Large), 100);
        assert_eq!(rules.apply_spell(100, ArmorType::Hero), 70);
    }
}
