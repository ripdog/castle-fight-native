use super::*;

/// Script behavior released by a native targeted Parasite. The parent supplies target eligibility,
/// resources and the missile; the child is the map script's independent consequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParasiteChild {
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
