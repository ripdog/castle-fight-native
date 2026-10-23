use super::*;

/// The Bear and Ancient Bear's target-damage listener (`handleTargetDamageEffects`): every damage
/// event may start a Feral Rage, and the first event below the health threshold starts the
/// one-time hibernation (retreat, sleep, regeneration).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FeralBearProfile {
    /// The shared runtime-damage marker that gates the listener.
    pub ability: AbilityId,
    pub rage_chance_per_10k: u16,
    pub rage_duration_ticks: u16,
    /// The dummy Inner Fire: a damage bonus on the bear.
    pub rage_damage_ability: AbilityId,
    pub rage_damage_buff: NativeBuffIdentity,
    pub rage_damage_bonus_per_10k: i16,
    /// The dummy Bloodlust: an attack speed bonus on the bear.
    pub rage_speed_ability: AbilityId,
    pub rage_speed_buff: NativeBuffIdentity,
    pub rage_attack_speed_percent_delta: i16,
    /// The listener starts the hibernation when the life at an event is below this.
    pub hibernate_below_health: i32,
    pub retreat_ticks: u16,
    pub retreat_speed_per_tick: i32,
    pub sleep_ticks: u16,
    pub sleep_regeneration_ability: AbilityId,
    pub sleep_regeneration_per_second_per_10k: u32,
    /// The item attack bonus installed at sleep and spent by the first damage the bear deals.
    pub wake_bonus: Option<NativeAbilityGrant>,
}

impl FeralBearProfile {
    pub(crate) fn canonical_words(self) -> [u64; 20] {
        let (bonus, bonus_words) = self
            .wake_bonus
            .map_or((0, [0; 5]), |grant| (1, grant.canonical_words()));
        let buff = |buff: NativeBuffIdentity| {
            u64::from(buff.rawcode)
                | u64::from(buff.positive) << 32
                | u64::from(buff.stealable) << 33
                | u64::from(buff.organic_only) << 34
        };
        [
            u64::from(self.ability.0),
            u64::from(self.rage_chance_per_10k) | u64::from(self.rage_duration_ticks) << 16,
            u64::from(self.rage_damage_ability.0),
            buff(self.rage_damage_buff),
            self.rage_damage_bonus_per_10k as u16 as u64,
            u64::from(self.rage_speed_ability.0),
            buff(self.rage_speed_buff),
            self.rage_attack_speed_percent_delta as u16 as u64,
            self.hibernate_below_health as u64,
            u64::from(self.retreat_ticks) | u64::from(self.sleep_ticks) << 16,
            self.retreat_speed_per_tick as u64,
            u64::from(self.sleep_regeneration_ability.0),
            u64::from(self.sleep_regeneration_per_second_per_10k),
            bonus,
            bonus_words[0],
            bonus_words[1],
            bonus_words[2],
            bonus_words[3],
            bonus_words[4],
            0,
        ]
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BearPhase {
    /// The hibernate marker is still installed.
    #[default]
    Ready,
    /// Walking home at the retreat speed until the sleep begins.
    Retreating { until_tick: u64 },
    /// Asleep (regenerating) until the tick, or until the next damage event wakes it.
    Asleep { until_tick: u64 },
    /// The marker is spent; the bear never hibernates again.
    Spent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeralBearState {
    pub profile: FeralBearProfile,
    pub phase: BearPhase,
    /// Distinguishes the deterministic Feral Rage rolls of successive damage events.
    pub event_sequence: u64,
    /// Damage events observed since the last resolution and the lowest life among them.
    pub pending_events: u8,
    pub pending_lowest_health: i32,
    /// The wake bonus is installed and waits for the first damage the bear deals.
    pub bonus_pending: bool,
}

impl FeralBearState {
    pub(crate) fn new(profile: FeralBearProfile) -> Self {
        Self {
            profile,
            phase: BearPhase::Ready,
            event_sequence: 0,
            pending_events: 0,
            pending_lowest_health: i32::MAX,
            bonus_pending: false,
        }
    }
}
