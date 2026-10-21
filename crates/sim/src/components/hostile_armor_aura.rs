use super::*;

/// Native flat hostile Devotion Aura, including shared-family strongest-source selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostileArmorAuraProfile {
    pub ability: AbilityId,
    pub buff_rawcode: u32,
    pub radius: i32,
    pub armor_bonus_per_100: i16,
    pub targets: AttackTargetMask,
    pub linger_ticks: u16,
    /// Script-owned extra non-attack damage for recipients carrying this aura's buff.
    #[serde(default)]
    pub spell_damage_taken_bonus_per_10k: u16,
    /// Command Aura: a signed change to the recipients' attack damage (negative on enemies).
    #[serde(default)]
    pub damage_bonus_per_10k: i16,
    /// Mechanical enemies are not recipients (the native mask lists `organic`).
    #[serde(default)]
    pub organic_only: bool,
}
impl HostileArmorAuraProfile {
    pub(crate) fn canonical_words(self) -> [u64; 9] {
        [
            u64::from(self.ability.0),
            u64::from(self.buff_rawcode),
            self.radius as u64,
            self.armor_bonus_per_100 as u64,
            u64::from(self.targets.bits()),
            u64::from(self.linger_ticks),
            u64::from(self.spell_damage_taken_bonus_per_10k),
            self.damage_bonus_per_10k as u64,
            u64::from(self.organic_only),
        ]
    }
}
