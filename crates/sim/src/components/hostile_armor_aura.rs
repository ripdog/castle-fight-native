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
}
impl HostileArmorAuraProfile {
    pub(crate) fn canonical_words(self) -> [u64; 6] {
        [
            u64::from(self.ability.0),
            u64::from(self.buff_rawcode),
            self.radius as u64,
            self.armor_bonus_per_100 as u64,
            u64::from(self.targets.bits()),
            u64::from(self.linger_ticks),
        ]
    }
}
