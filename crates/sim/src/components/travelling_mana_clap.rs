use super::*;

/// Independent native Parasite order and a fixed-point delayed script callback.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TravellingManaClapProfile {
    pub parent: NativeParasiteProfile,
    pub parent_nonhero_only: bool,
    pub mover_rawcode: u32,
    pub callback_speed_per_second: i32,
    pub mover_speed_per_tick: i32,
    pub mover_has_visible_model: bool,
    pub mover_height_world_units: i32,
    pub mover_duration_ticks: u16,
    pub clap: NativeNovaProfile,
    pub burn_radius: i32,
    pub maximum_mana_burn: i32,
    pub excluded_ability: AbilityId,
    pub feedback_visual_ability: AbilityId,
}

impl TravellingManaClapProfile {
    pub(crate) fn canonical_words(self) -> impl Iterator<Item = u64> {
        [
            u64::from(self.parent_nonhero_only),
            self.mover_rawcode.into(),
            self.callback_speed_per_second as u64,
            self.mover_speed_per_tick as u64,
            u64::from(self.mover_has_visible_model),
            self.mover_height_world_units as u64,
            self.mover_duration_ticks.into(),
            self.burn_radius as u64,
            self.maximum_mana_burn as u64,
            self.excluded_ability.0.into(),
            self.feedback_visual_ability.0.into(),
            self.clap.ability.0.into(),
            self.clap.radius as u64,
            self.clap.primary_damage as u64,
            self.clap.area_damage as u64,
            self.clap.duration_ticks.into(),
            self.clap.hero_duration_ticks.into(),
            self.clap.movement_percent_delta as u64,
            self.clap.attack_speed_percent_delta as u64,
        ]
        .into_iter()
        .chain(self.parent.canonical_words())
    }

    pub(crate) fn clap_effect(self) -> AbilityEffect {
        AbilityEffect::FrostNova {
            ability: self.clap.ability,
            radius: self.clap.radius,
            primary_damage: self.clap.primary_damage,
            area_damage: self.clap.area_damage,
            duration_ticks: self.clap.duration_ticks,
            hero_duration_ticks: self.clap.hero_duration_ticks,
            movement_percent_delta: self.clap.movement_percent_delta,
            attack_speed_percent_delta: self.clap.attack_speed_percent_delta,
            targets: AttackTargetMask::GROUND_UNITS,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TravellingManaClapState {
    pub source: SimId,
    pub owner: PlayerId,
    pub team: Team,
    pub origin: SimPoint,
    pub destination: SimPoint,
    pub position: SimPoint,
    pub position_tick: u64,
    pub created_tick: u64,
    pub due_tick: u64,
    pub profile: TravellingManaClapProfile,
    pub impacted: bool,
}
