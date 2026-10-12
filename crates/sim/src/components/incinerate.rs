use super::*;

/// Native passive Incinerate. Compatibility policy is owned by the versioned projection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncinerateProfile {
    pub ability: AbilityId,
    pub buff_rawcode: u32,
    pub bonus_per_hit: i32,
    pub maximum_stacks: u32,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
    pub full_damage: i32,
    pub half_damage: i32,
    pub full_radius: i32,
    pub half_radius: i32,
    pub delay_ticks: u16,
    pub speed_per_tick: i32,
}
impl IncinerateProfile {
    pub(crate) fn canonical_words(self) -> [u64; 12] {
        [
            u64::from(self.ability.0),
            u64::from(self.buff_rawcode),
            self.bonus_per_hit as u64,
            u64::from(self.maximum_stacks),
            u64::from(self.duration_ticks),
            u64::from(self.hero_duration_ticks),
            self.full_damage as u64,
            self.half_damage as u64,
            self.full_radius as u64,
            self.half_radius as u64,
            u64::from(self.delay_ticks),
            self.speed_per_tick as u64,
        ]
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncinerateStatus {
    pub source: SimId,
    pub team: Team,
    pub profile: IncinerateProfile,
    pub stacks: u32,
    pub expires_tick: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct IncinerateExplosion {
    pub source: SimId,
    pub team: Team,
    pub profile: IncinerateProfile,
    pub position: SimPoint,
    pub due_tick: u64,
}
