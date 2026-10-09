use crate::{AbilityId, AttackTargetMask, MapVersion, NativeAbilityGrant};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingEnchantmentProfile {
    pub map_version: MapVersion,
    pub trigger_targets: AttackTargetMask,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub heal_if_missing_hp_gt: i32,
    pub heal_amount: i32,
    pub retry_marker: AbilityId,
    pub dispatcher: AbilityId,
    pub dispatcher_option_index: u8,
    pub options: [NativeAbilityGrant; 3],
}

impl BuildingEnchantmentProfile {
    pub(crate) fn canonical_words(self) -> impl Iterator<Item = u64> {
        [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.trigger_targets.bits()),
            u64::from(self.trigger_invulnerable),
            u64::from(self.trigger_spell_immune),
            self.heal_if_missing_hp_gt as u64,
            self.heal_amount as u64,
            u64::from(self.retry_marker.0),
            u64::from(self.dispatcher.0),
            u64::from(self.dispatcher_option_index),
        ]
        .into_iter()
        .chain(
            self.options
                .into_iter()
                .flat_map(NativeAbilityGrant::canonical_words),
        )
    }
}
