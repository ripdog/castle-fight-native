use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MasterMoverProfile {
    pub rawcode: u32,
    pub step: i32,
    pub radius: i32,
    pub budget: i32,
    pub decrement: i32,
    pub interval_millis: u16,
    pub height: i32,
}
impl MasterMoverProfile {
    fn words(self) -> [u64; 7] {
        [
            self.rawcode.into(),
            self.step as u64,
            self.radius as u64,
            self.budget as u64,
            self.decrement as u64,
            self.interval_millis.into(),
            self.height as u64,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeNovaProfile {
    pub ability: AbilityId,
    pub radius: i32,
    pub primary_damage: i32,
    pub area_damage: i32,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
    pub movement_percent_delta: i16,
    pub attack_speed_percent_delta: i16,
}
impl NativeNovaProfile {
    pub(crate) fn effect(self, primary: bool) -> AbilityEffect {
        AbilityEffect::FrostNova {
            ability: self.ability,
            radius: self.radius,
            primary_damage: 0,
            area_damage: self.area_damage + if primary { self.primary_damage } else { 0 },
            duration_ticks: self.duration_ticks,
            hero_duration_ticks: self.hero_duration_ticks,
            movement_percent_delta: self.movement_percent_delta,
            attack_speed_percent_delta: self.attack_speed_percent_delta,
            targets: AttackTargetMask::AIR_AND_GROUND,
        }
    }
    fn words(self) -> [u64; 8] {
        [
            self.ability.0.into(),
            self.radius as u64,
            self.primary_damage as u64,
            self.area_damage as u64,
            self.duration_ticks.into(),
            self.hero_duration_ticks.into(),
            self.movement_percent_delta as u64,
            self.attack_speed_percent_delta as u64,
        ]
    }
}

/// Release-facing movers scale only with the caster owner's completed source buildings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MasterProjectileProfile {
    pub ability: AbilityId,
    /// Fire, earth, lightning, water, wind; zero entries are padding, never identities.
    pub buildings: [[u32; 3]; 5],
    pub lightning: MasterMoverProfile,
    pub frost: MasterMoverProfile,
    pub damage: i32,
    pub damage_count_cap: u16,
    pub branch_roll_size: u16,
    pub lightning_roll_below: u16,
    pub lightning_initial_count: u16,
    pub lightning_count_divisor: u16,
    pub split_initial_count: u16,
    pub split_count_divisor: u16,
    pub lane_spacing: i32,
    pub forward_offset: i32,
    pub split_angle_millidegrees: i32,
    pub stub_budget: i32,
    pub frost_initial_count: u16,
    pub frost_count_factor: u16,
    pub fan_spread_millidegrees: i32,
    pub fan_interval_millis: u16,
    pub nova: NativeNovaProfile,
    pub nova_reveal_radius: i32,
    pub nova_reveal_ticks: u16,
    pub parasite_speed: i32,
    pub parasite_duration_ticks: u16,
    pub parasite_child_rawcode: u32,
    pub parasite_child_duration_ticks: u16,
}
impl MasterProjectileProfile {
    pub(crate) fn validate(self) {
        assert!(self.branch_roll_size > self.lightning_roll_below);
        assert!(self.lightning_count_divisor > 0 && self.split_count_divisor > 0);
        assert!(self.fan_interval_millis > 0 && self.parasite_speed > 0);
        for p in [self.lightning, self.frost] {
            assert!(
                p.step > 0
                    && p.radius > 0
                    && p.budget > 0
                    && p.decrement > 0
                    && p.interval_millis > 0
            );
        }
    }
    pub(crate) fn canonical_words(&self) -> impl Iterator<Item = u64> + '_ {
        [
            self.ability.0.into(),
            self.damage as u64,
            self.damage_count_cap.into(),
            self.branch_roll_size.into(),
            self.lightning_roll_below.into(),
            self.lightning_initial_count.into(),
            self.lightning_count_divisor.into(),
            self.split_initial_count.into(),
            self.split_count_divisor.into(),
            self.lane_spacing as u64,
            self.forward_offset as u64,
            self.split_angle_millidegrees as u64,
            self.stub_budget as u64,
            self.frost_initial_count.into(),
            self.frost_count_factor.into(),
            self.fan_spread_millidegrees as u64,
            self.fan_interval_millis.into(),
            self.nova_reveal_radius as u64,
            self.nova_reveal_ticks.into(),
            self.parasite_speed as u64,
            self.parasite_duration_ticks.into(),
            self.parasite_child_rawcode.into(),
            self.parasite_child_duration_ticks.into(),
        ]
        .into_iter()
        .chain(self.buildings.iter().flatten().copied().map(u64::from))
        .chain(self.lightning.words())
        .chain(self.frost.words())
        .chain(self.nova.words())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MasterMover {
    pub id: SimId,
    pub source: SimId,
    pub owner: PlayerId,
    pub team: Team,
    pub position_fractional: [i64; 2],
    pub direction: SimPoint,
    pub damage: i32,
    pub budget: i32,
    pub splits: u16,
    pub stage: i8,
    pub last_hit: Option<SimId>,
    pub last_original_lane: bool,
    pub born_tick: u64,
}
impl MasterMover {
    pub(crate) fn position(&self) -> SimPoint {
        SimPoint::new(
            (self.position_fractional[0] / 1_000_000) as i32,
            (self.position_fractional[1] / 1_000_000) as i32,
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MasterFan {
    pub source: SimId,
    pub owner: PlayerId,
    pub team: Team,
    pub direction: SimPoint,
    pub damage: i32,
    pub remaining: u32,
    pub sequence: u64,
    pub due_millis: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MasterParasite {
    pub source: SimId,
    pub owner: PlayerId,
    pub team: Team,
    pub target: SimId,
    pub position: SimPoint,
    pub buff_until_tick: Option<u64>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MasterMinion {
    pub id: SimId,
    pub source: SimId,
    pub owner: PlayerId,
    pub team: Team,
    pub position: SimPoint,
    pub expires_tick: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct MasterProjectileState {
    pub profile: Box<MasterProjectileProfile>,
    pub map_version: crate::MapVersion,
    pub lightning: Vec<MasterMover>,
    pub frost: Vec<MasterMover>,
    pub fans: Vec<MasterFan>,
    pub parasites: Vec<MasterParasite>,
    pub minions: Vec<MasterMinion>,
    /// Native shared timers keep phase while casts append to an active array.
    pub lightning_due_millis: u64,
    pub frost_due_millis: u64,
}
impl MasterProjectileState {
    pub(crate) fn visit_canonical_words(&self, mut write: impl FnMut(u64)) {
        for word in self.profile.canonical_words() {
            write(word);
        }
        write(self.map_version.major.into());
        write(self.map_version.minor.into());
        write(self.lightning_due_millis);
        write(self.frost_due_millis);
        for movers in [&self.lightning, &self.frost] {
            write(movers.len() as u64);
            for m in movers {
                for w in [
                    m.id.0,
                    m.source.0,
                    m.owner.0.into(),
                    m.team.0.into(),
                    m.position_fractional[0] as u64,
                    m.position_fractional[1] as u64,
                    m.direction.x as u64,
                    m.direction.y as u64,
                    m.damage as u64,
                    m.budget as u64,
                    m.splits.into(),
                    m.stage as u64,
                    m.last_hit.map_or(0, |id| id.0),
                    u64::from(m.last_original_lane),
                    m.born_tick,
                ] {
                    write(w);
                }
            }
        }
        write(self.fans.len() as u64);
        for f in &self.fans {
            for w in [
                f.source.0,
                f.owner.0.into(),
                f.team.0.into(),
                f.direction.x as u64,
                f.direction.y as u64,
                f.damage as u64,
                f.remaining.into(),
                f.sequence,
                f.due_millis,
            ] {
                write(w);
            }
        }
        write(self.parasites.len() as u64);
        for p in &self.parasites {
            for w in [
                p.source.0,
                p.owner.0.into(),
                p.team.0.into(),
                p.target.0,
                p.position.x as u64,
                p.position.y as u64,
                u64::from(p.buff_until_tick.is_some()),
                p.buff_until_tick.unwrap_or(0),
            ] {
                write(w);
            }
        }
        write(self.minions.len() as u64);
        for m in &self.minions {
            for w in [
                m.id.0,
                m.source.0,
                m.owner.0.into(),
                m.team.0.into(),
                m.position.x as u64,
                m.position.y as u64,
                m.expires_tick,
            ] {
                write(w);
            }
        }
    }
}
