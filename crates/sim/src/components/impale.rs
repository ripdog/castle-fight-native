use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImpaleProfile {
    pub ability: AbilityId,
    pub damage: i32,
    pub distance: i32,
    pub radius: i32,
    pub wave_ticks: u16,
    pub air_ticks: u16,
    pub stun_ticks: u16,
    pub hero_stun_ticks: u16,
    pub targets: AttackTargetMask,
}

impl ImpaleProfile {
    pub(crate) fn canonical_words(self) -> [u64; 9] {
        [
            u64::from(self.ability.0),
            self.damage as u64,
            self.distance as u64,
            self.radius as u64,
            u64::from(self.wave_ticks),
            u64::from(self.air_ticks),
            u64::from(self.stun_ticks),
            u64::from(self.hero_stun_ticks),
            u64::from(self.targets.bits()),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImpaleFanProfile {
    pub parent: NativeParasiteProfile,
    pub cast_animation: ActionAnimationKind,
    pub child: ImpaleProfile,
    pub relative_angles_millidegrees: [i32; 4],
    pub cast_point_distance: i32,
    pub dummy_lifetime_ticks: u16,
    pub dummy_rawcode: u32,
    pub dummy_flying: bool,
}

impl ImpaleFanProfile {
    pub(crate) fn visit_canonical_words(self, mut visit: impl FnMut(u64)) {
        for word in self
            .parent
            .canonical_words()
            .into_iter()
            .chain(self.child.canonical_words())
        {
            visit(word);
        }
        for angle in self.relative_angles_millidegrees {
            visit(angle as u64);
        }
        visit(self.cast_point_distance as u64);
        visit(u64::from(self.dummy_lifetime_ticks));
        visit(u64::from(self.dummy_rawcode));
        visit(u64::from(self.dummy_flying));
        visit(u64::from(self.cast_animation.canonical_tag()));
    }
}

/// Suppression starts when the wave arrives; native damage and stun start on landing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ImpaleStatus {
    pub ability: AbilityId,
    pub landed_tick: u64,
    pub expires_tick: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct ImpaleWaveState {
    pub source: SimId,
    pub team: Team,
    pub profile: ImpaleProfile,
    pub origin: SimPoint,
    pub destination: SimPoint,
    pub launch_tick: u64,
    pub traveled: i32,
    pub hit_targets: Vec<SimId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) struct ImpaleLandingState {
    pub source: SimId,
    pub target: SimId,
    pub profile: ImpaleProfile,
    pub due_tick: u64,
}
