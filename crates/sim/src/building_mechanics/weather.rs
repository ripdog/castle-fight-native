use crate::{AbilityId, AttackTargetMask, MapVersion};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct WeatherWaveProfile {
    pub ability: AbilityId,
    pub radius: i32,
    pub damage: i32,
    pub maximum_damage: i32,
    pub building_damage_per_10k: u16,
    pub waves: u16,
    pub shards: u16,
    pub interval_ticks: u16,
    pub first_impact_ticks: u16,
    pub damage_per_second: i32,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
    pub buff_rawcode: u32,
    pub wave_buff_rawcode: u32,
    pub targets: AttackTargetMask,
}
impl WeatherWaveProfile {
    pub(crate) fn canonical_words(self) -> [u64; 15] {
        [
            u64::from(self.ability.0),
            self.radius as u64,
            self.damage as u64,
            self.maximum_damage as u64,
            u64::from(self.building_damage_per_10k),
            u64::from(self.waves),
            u64::from(self.shards),
            u64::from(self.interval_ticks),
            u64::from(self.first_impact_ticks),
            self.damage_per_second as u64,
            u64::from(self.duration_ticks),
            u64::from(self.hero_duration_ticks),
            u64::from(self.buff_rawcode),
            u64::from(self.wave_buff_rawcode),
            u64::from(self.targets.bits()),
        ]
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingWeatherProfile {
    pub map_version: MapVersion,
    pub trigger_targets: AttackTargetMask,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub select_ground: bool,
    pub selector_excluded_ability: u32,
    pub option_count: u8,
    pub dummy_lifetime_ticks: u16,
    pub options: [[WeatherWaveProfile; 2]; 2],
}
impl BuildingWeatherProfile {
    pub(crate) fn canonical_words(self) -> impl Iterator<Item = u64> {
        [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.trigger_targets.bits()),
            u64::from(self.trigger_invulnerable),
            u64::from(self.trigger_spell_immune),
            u64::from(self.select_ground),
            u64::from(self.selector_excluded_ability),
            u64::from(self.option_count),
            u64::from(self.dummy_lifetime_ticks),
        ]
        .into_iter()
        .chain(
            self.options
                .into_iter()
                .flatten()
                .flat_map(WeatherWaveProfile::canonical_words),
        )
    }
}
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct WeatherState {
    pub map_version: MapVersion,
    pub source: crate::SimId,
    pub team: crate::Team,
    pub center: crate::SimPoint,
    pub profile: WeatherWaveProfile,
    pub started_tick: u64,
    pub expires_tick: u64,
    pub next_wave: u16,
    pub next_visual: u16,
}
impl WeatherState {
    pub(crate) fn canonical_words(&self) -> impl Iterator<Item = u64> {
        [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            self.source.0,
            u64::from(self.team.0),
            self.center.x as u64,
            self.center.y as u64,
            self.started_tick,
            self.expires_tick,
            u64::from(self.next_wave),
            u64::from(self.next_visual),
        ]
        .into_iter()
        .chain(self.profile.canonical_words())
    }
}
