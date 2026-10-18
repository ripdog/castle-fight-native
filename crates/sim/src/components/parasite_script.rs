use super::*;

/// Script behavior released by a native targeted Parasite. The parent supplies target eligibility,
/// resources and the missile; the child is the map script's independent consequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParasiteChild {
    /// The caster berserks itself after a random delay.
    Berserk {
        berserk: KillBerserkProfile,
        minimum_delay_ticks: u16,
        maximum_delay_ticks: u16,
    },
    /// Periodic dummy-cast miss-chance clouds at the parent's target position.
    SandCloud(SandCloudProfile),
    /// Immediate dummy-cast Locust Swarm released at the parent's target position.
    LocustSwarm(LocustSwarmProfile),
    /// Delayed Chaos attack damage equal to a clamped steal, healing the caster by the same
    /// requested amount whether or not the damage is blocked.
    LifeSteal {
        maximum_life: i32,
        minimum_delay_ticks: u16,
        maximum_delay_ticks: u16,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParasiteScriptProfile {
    pub parent: NativeParasiteProfile,
    pub parent_targets: AttackTargetMask,
    pub parent_organic_only: bool,
    pub parent_nonhero_only: bool,
    pub ability: AbilityId,
    pub child: ParasiteChild,
}

impl ParasiteChild {
    pub(crate) fn canonical_words(self) -> [u64; 16] {
        match self {
            Self::LocustSwarm(p) => p.canonical_words(),
            Self::SandCloud(p) => p.canonical_words(),
            Self::Berserk {
                berserk,
                minimum_delay_ticks,
                maximum_delay_ticks,
            } => {
                let mut words = [0; 16];
                words[..8].copy_from_slice(&[
                    4,
                    u64::from(berserk.ability.0),
                    u64::from(berserk.duration_ticks),
                    berserk.movement_percent_delta as u16 as u64,
                    berserk.attack_speed_percent_delta as u16 as u64,
                    berserk.damage_taken_bonus_per_10k as u16 as u64,
                    u64::from(minimum_delay_ticks),
                    u64::from(maximum_delay_ticks),
                ]);
                words
            }
            Self::LifeSteal {
                maximum_life,
                minimum_delay_ticks,
                maximum_delay_ticks,
            } => {
                let mut words = [0; 16];
                words[..4].copy_from_slice(&[
                    1,
                    maximum_life as u64,
                    u64::from(minimum_delay_ticks),
                    u64::from(maximum_delay_ticks),
                ]);
                words
            }
        }
    }
}

impl ParasiteScriptProfile {
    pub(crate) fn canonical_words(self) -> Vec<u64> {
        let mut words = self.parent.canonical_words().to_vec();
        words.extend([
            u64::from(self.parent_targets.bits()),
            u64::from(self.parent_organic_only),
            u64::from(self.parent_nonhero_only),
            u64::from(self.ability.0),
        ]);
        words.extend(self.child.canonical_words());
        words
    }
}

/// A scheduled child callback. It retains only identities; amounts are read when it fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingParasiteScript {
    pub ability: AbilityId,
    pub child: ParasiteChild,
    pub target: SimId,
    pub due_tick: u64,
}

impl PendingParasiteScript {
    pub(crate) fn canonical_words(self) -> Vec<u64> {
        let mut words = vec![u64::from(self.ability.0), self.target.0, self.due_tick];
        words.extend(self.child.canonical_words());
        words
    }
}

pub const MAX_LOCUSTS: usize = 8;

/// Retained Locust Swarm tuning plus the swarm body's weapon, as read from object data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocustSwarmProfile {
    pub ability: AbilityId,
    pub body_rawcode: u32,
    pub count: u8,
    pub release_interval_millis: u16,
    pub max_per_target: u8,
    pub duration_ticks: u16,
    pub radius: i32,
    pub base_damage: i32,
    pub dice_number: u8,
    pub dice_sides: u8,
    pub attack_cooldown_ticks: u16,
    pub flight_speed_per_tick: i32,
    pub structure_damage_per_10k: u16,
    pub targets: AttackTargetMask,
}

impl LocustSwarmProfile {
    pub(crate) fn canonical_words(self) -> [u64; 16] {
        [
            2,
            u64::from(self.ability.0),
            u64::from(self.body_rawcode),
            u64::from(self.count),
            u64::from(self.release_interval_millis),
            u64::from(self.max_per_target),
            u64::from(self.duration_ticks),
            self.radius as u64,
            self.base_damage as u64,
            u64::from(self.dice_number),
            u64::from(self.dice_sides),
            u64::from(self.attack_cooldown_ticks),
            self.flight_speed_per_tick as u64,
            u64::from(self.structure_damage_per_10k),
            u64::from(self.targets.bits()),
            0,
        ]
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocustState {
    /// The enemy the locust is committed to, if any.
    pub target: Option<SimId>,
    /// Tick at which the committed attack lands; zero when none is in flight.
    pub strike_tick: u64,
    /// First tick at which the locust may commit to another target.
    pub ready_tick: u64,
}

/// A stationary dummy-owned swarm. It outlives its caster and belongs to the caster's player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocustSwarmState {
    pub profile: LocustSwarmProfile,
    pub source: SimId,
    pub owner: PlayerId,
    pub team: Team,
    pub center: SimPoint,
    pub start_tick: u64,
    pub expires_tick: u64,
    pub locusts: [LocustState; MAX_LOCUSTS],
}

/// One cloud strength: the dummy's Silence-derived area and the miss-chance debuff it applies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SandCloudVariant {
    pub radius: i32,
    pub debuff: NativeDebuffProfile,
}

/// A script that spawns a short-lived carrier and re-casts its area ability at a fixed period.
/// The caster body type selects the stronger variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SandCloudProfile {
    pub carrier_ticks: u16,
    pub period_ticks: u16,
    pub pulses: u8,
    pub special_caster_rawcode: u32,
    pub standard: SandCloudVariant,
    pub special: SandCloudVariant,
}

impl SandCloudProfile {
    pub(crate) fn canonical_words(self) -> [u64; 16] {
        let mut words = [0; 16];
        words[0] = 3;
        words[1] = u64::from(self.carrier_ticks);
        words[2] = u64::from(self.period_ticks);
        words[3] = u64::from(self.pulses);
        words[4] = u64::from(self.special_caster_rawcode);
        for (offset, variant) in [(5, self.standard), (10, self.special)] {
            words[offset] = variant.radius as u64;
            // Pack the variant's identity: ability, buff, duration, miss chance and movement.
            words[offset + 1] = u64::from(variant.debuff.ability.0);
            words[offset + 2] = u64::from(variant.debuff.buff);
            words[offset + 3] = u64::from(variant.debuff.duration_ticks)
                | u64::from(variant.debuff.hero_duration_ticks) << 16
                | u64::from(variant.debuff.miss_chance_per_10k) << 32
                | u64::from(variant.debuff.targets.bits()) << 48;
            words[offset + 4] = variant.debuff.movement_percent_delta as u16 as u64
                | (variant.debuff.attack_speed_percent_delta as u16 as u64) << 16
                | (variant.debuff.damage_bonus_per_10k as u16 as u64) << 32
                | u64::from(variant.debuff.organic_only) << 48;
        }
        words
    }
}

/// A fixed-position carrier casting its cloud at the retained period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandCloudState {
    pub variant: SandCloudVariant,
    pub source: SimId,
    pub owner: PlayerId,
    pub team: Team,
    pub center: SimPoint,
    pub expires_tick: u64,
    pub next_pulse_tick: u64,
    pub period_ticks: u16,
    pub pulses_remaining: u8,
}
