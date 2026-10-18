use super::*;

/// Script behavior released by a native targeted Parasite. The parent supplies target eligibility,
/// resources and the missile; the child is the map script's independent consequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ParasiteChild {
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
    pub(crate) fn canonical_words(self) -> [u64; 4] {
        match self {
            Self::LifeSteal {
                maximum_life,
                minimum_delay_ticks,
                maximum_delay_ticks,
            } => [
                1,
                maximum_life as u64,
                u64::from(minimum_delay_ticks),
                u64::from(maximum_delay_ticks),
            ],
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
    pub(crate) fn canonical_words(self) -> [u64; 8] {
        let child = self.child.canonical_words();
        [
            u64::from(self.ability.0),
            self.target.0,
            self.due_tick,
            child[0],
            child[1],
            child[2],
            child[3],
            0,
        ]
    }
}
