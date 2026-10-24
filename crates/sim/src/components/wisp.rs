use super::*;

/// Call of Nature: the script releases pre-placed forest wisps from the owner's side, one per
/// period, alternating between the two corner pools of the owner's team.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WispReleaseProfile {
    pub map_version: crate::MapVersion,
    pub wisp_rawcode: u32,
    pub releases: u8,
    pub period_ticks: u16,
    pub life_ticks: u16,
    /// The pools stand at `±min(x_limit, battlefield half-width)`.
    pub x_limit: i32,
    /// ...and at `±(battlefield half-height - y_inset)`.
    pub y_inset: i32,
}

impl WispReleaseProfile {
    pub(crate) fn canonical_words(self) -> [u64; 8] {
        [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.wisp_rawcode),
            u64::from(self.releases),
            u64::from(self.period_ticks),
            u64::from(self.life_ticks),
            self.x_limit as u64,
            self.y_inset as u64,
        ]
    }
}

/// An in-progress wisp release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WispReleaseState {
    pub owner: PlayerId,
    pub team: Team,
    pub profile: WispReleaseProfile,
    pub iteration: u8,
    pub next_tick: u64,
}

/// The slow aura left where a wisp died (`Aasl`), refreshed on every tick it lasts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeathSlowField {
    pub ability: AbilityId,
    pub buff: u32,
    pub radius: i32,
    pub movement_percent_delta: i16,
    pub attack_speed_percent_delta: i16,
    pub targets: AttackTargetMask,
    pub affects_invulnerable: bool,
    pub duration_ticks: u16,
}

/// The death blast of a bearer: spell damage to hostile combat bodies in range, then a slow field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DeathBurstRule {
    pub damage: i32,
    pub radius: i32,
    /// Bodies carrying this ability (the tentacle marker) are untouched.
    pub excluded_ability: AbilityId,
    /// Bodies of this unit type are untouched.
    pub excluded_type: u32,
    pub slow: DeathSlowField,
}

impl DeathBurstRule {
    pub(crate) fn canonical_words(self) -> [u64; 12] {
        [
            self.damage as u64,
            self.radius as u64,
            u64::from(self.excluded_ability.0),
            u64::from(self.excluded_type),
            u64::from(self.slow.ability.0),
            u64::from(self.slow.buff),
            self.slow.radius as u64,
            self.slow.movement_percent_delta as u64,
            self.slow.attack_speed_percent_delta as u64,
            u64::from(self.slow.targets.bits()),
            u64::from(self.slow.affects_invulnerable),
            u64::from(self.slow.duration_ticks),
        ]
    }
}

/// A live death blast: damage once at `due_tick`, then the slow field until `expires_tick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeathBurstState {
    pub map_version: crate::MapVersion,
    pub source: SimId,
    pub team: Team,
    pub center: SimPoint,
    pub rule: DeathBurstRule,
    pub struck: bool,
    pub expires_tick: u64,
}
