use super::*;

/// Native Grab Tree (`Agra`, "War Club"): at spawn the body grabs a tree and its first attacks use
/// the club weapon, which replaces the ordinary one entirely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WarClubProfile {
    pub ability: AbilityId,
    pub damage: i32,
    pub range: i32,
    pub cooldown_ticks: u16,
    pub targets: AttackTargetMask,
    pub damage_type: DamageType,
    pub max_attacks: u16,
    /// Time from entering the map until the club is attached.
    pub attach_delay_ticks: u16,
    /// Time from entering the map until the script resumes the lane order.
    pub resume_delay_ticks: u16,
}

impl WarClubProfile {
    pub(crate) fn canonical_words(self) -> [u64; 8] {
        [
            u64::from(self.ability.0),
            self.damage as u64,
            self.range as u64,
            u64::from(self.cooldown_ticks)
                | u64::from(self.max_attacks) << 16
                | u64::from(self.attach_delay_ticks) << 32
                | u64::from(self.resume_delay_ticks) << 48,
            u64::from(self.targets.bits()),
            u64::from(self.damage_type.stable_tag()),
            0,
            0,
        ]
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ClubPhase {
    /// Walking to the tree and picking it up; the body does not act.
    #[default]
    Gripping,
    /// Armed from the given attack sequence number.
    Armed {
        start_sequence: u64,
    },
    Spent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WarClubState {
    pub profile: WarClubProfile,
    pub resume_tick: u64,
    pub phase: ClubPhase,
}
