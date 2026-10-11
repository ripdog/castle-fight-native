use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PurgeProfile {
    pub ability: AbilityId,
    pub range: i32,
    pub targets: AttackTargetMask,
    pub movement_factor_per_10k: u32,
    pub minimum_speed_per_tick: i32,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
    pub pause_ticks: u16,
    pub hero_pause_ticks: u16,
    pub summoned_damage: i32,
}
impl PurgeProfile {
    pub(crate) fn canonical_words(self) -> [u64; 10] {
        [
            u64::from(self.ability.0),
            self.range as u64,
            u64::from(self.targets.bits()),
            u64::from(self.movement_factor_per_10k),
            self.minimum_speed_per_tick as u64,
            u64::from(self.duration_ticks),
            u64::from(self.hero_duration_ticks),
            u64::from(self.pause_ticks),
            u64::from(self.hero_pause_ticks),
            self.summoned_damage as u64,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SirenPurgeProfile {
    pub parent: NativeParasiteProfile,
    pub child: PurgeProfile,
    pub summon_ability: AbilityId,
    pub summon_delay_ticks: u16,
    pub resume_delay_ticks: u16,
    pub parent_nonhero_only: bool,
}
impl SirenPurgeProfile {
    pub(crate) fn visit_canonical_words(self, mut visit: impl FnMut(u64)) {
        for word in self
            .parent
            .canonical_words()
            .into_iter()
            .chain(self.child.canonical_words())
        {
            visit(word);
        }
        for word in [
            u64::from(self.summon_ability.0),
            u64::from(self.summon_delay_ticks),
            u64::from(self.resume_delay_ticks),
            u64::from(self.parent_nonhero_only),
        ] {
            visit(word);
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeSummonProfile {
    pub map_version: crate::MapVersion,
    pub ability: AbilityId,
    pub body_rawcode: u32,
    pub count: u8,
    pub duration_ticks: u16,
    pub forward_distance: i32,
}
impl NativeSummonProfile {
    pub(crate) fn canonical_words(self) -> [u64; 7] {
        [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.ability.0),
            u64::from(self.body_rawcode),
            u64::from(self.count),
            u64::from(self.duration_ticks),
            self.forward_distance as u64,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PurgeState {
    pub profile: PurgeProfile,
    pub applied_tick: u64,
    pub pause_until_tick: u64,
    pub expires_tick: u64,
}

/// The zero-damage native ACpa parent still delivers an undispellable death summon buff.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeParasiteProfile {
    pub buff_rawcode: u32,
    pub map_version: crate::MapVersion,
    pub speed_per_tick: i32,
    pub affects_invulnerable: bool,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
    pub body_rawcode: u32,
    pub count: u8,
    pub body_duration_ticks: u16,
}
impl NativeParasiteProfile {
    pub(crate) fn canonical_words(self) -> [u64; 10] {
        [
            self.buff_rawcode.into(),
            self.map_version.major.into(),
            self.map_version.minor.into(),
            self.speed_per_tick as u64,
            u64::from(self.affects_invulnerable),
            self.duration_ticks.into(),
            self.hero_duration_ticks.into(),
            self.body_rawcode.into(),
            self.count.into(),
            self.body_duration_ticks.into(),
        ]
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct NativeParasiteState {
    pub source: SimId,
    pub owner: PlayerId,
    pub team: Team,
    pub target: SimId,
    pub profile: NativeParasiteProfile,
    pub position: SimPoint,
    pub buff_until_tick: Option<u64>,
}
