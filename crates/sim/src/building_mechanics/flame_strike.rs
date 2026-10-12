use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FlameStrikeProfile {
    pub ability: AbilityId,
    pub buff: u32,
    pub radius: i32,
    pub cast_ticks: u16,
    pub full_damage: i32,
    pub full_interval_millis: u16,
    pub half_damage: i32,
    pub half_interval_millis: u16,
    pub building_damage_per_10k: u16,
    pub maximum_damage: i32,
    pub full_duration_millis: u16,
    pub duration_millis: u16,
    pub targets: crate::AttackTargetMask,
}
impl FlameStrikeProfile {
    pub(crate) fn canonical_words(self) -> [u64; 13] {
        [
            u64::from(self.ability.0),
            u64::from(self.buff),
            self.radius as u64,
            u64::from(self.cast_ticks),
            self.full_damage as u64,
            u64::from(self.full_interval_millis),
            self.half_damage as u64,
            u64::from(self.half_interval_millis),
            u64::from(self.building_damage_per_10k),
            self.maximum_damage as u64,
            u64::from(self.full_duration_millis),
            u64::from(self.duration_millis),
            u64::from(self.targets.bits()),
        ]
    }
    pub(crate) fn pulse(self, index: u16) -> Option<(u64, i32)> {
        let full = self
            .full_duration_millis
            .div_ceil(self.full_interval_millis);
        let millis = if index < full {
            u64::from(index) * u64::from(self.full_interval_millis)
        } else {
            u64::from(full) * u64::from(self.full_interval_millis)
                + u64::from(index - full + 1) * u64::from(self.half_interval_millis)
        };
        (millis <= u64::from(self.duration_millis)).then_some((
            millis,
            if index < full {
                self.full_damage
            } else {
                self.half_damage
            },
        ))
    }
    pub(crate) fn strength(self, elapsed_ticks: u64) -> i64 {
        let full_end = u64::from(
            self.full_duration_millis
                .div_ceil(self.full_interval_millis),
        ) * u64::from(self.full_interval_millis);
        if elapsed_ticks * 1000 < full_end * CASTLE_FIGHT_SIMULATION_HZ as u64 {
            i64::from(self.full_damage) * 1000 / i64::from(self.full_interval_millis)
        } else {
            i64::from(self.half_damage) * 1000 / i64::from(self.half_interval_millis)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingFlameStrikeProfile {
    pub map_version: MapVersion,
    pub parent: crate::NativeParasiteProfile,
    pub trigger_targets: crate::AttackTargetMask,
    pub trigger_mechanical_only: bool,
    pub selector_exclusions: [AbilityId; 2],
    pub dummy_rawcode: u32,
    pub dummy_flying: bool,
    pub dummy_ticks: u16,
    pub options: [FlameStrikeProfile; 2],
}
impl BuildingFlameStrikeProfile {
    pub(crate) fn canonical_words(self) -> impl Iterator<Item = u64> {
        self.parent
            .canonical_words()
            .into_iter()
            .chain([
                u64::from(self.map_version.major),
                u64::from(self.map_version.minor),
                u64::from(self.trigger_targets.bits()),
                u64::from(self.trigger_mechanical_only),
                u64::from(self.selector_exclusions[0].0),
                u64::from(self.selector_exclusions[1].0),
                u64::from(self.dummy_rawcode),
                u64::from(self.dummy_flying),
                u64::from(self.dummy_ticks),
            ])
            .chain(
                self.options
                    .into_iter()
                    .flat_map(FlameStrikeProfile::canonical_words),
            )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct FlameStrikeState {
    pub map_version: MapVersion,
    pub source: crate::SimId,
    pub team: crate::Team,
    pub center: crate::SimPoint,
    pub profile: FlameStrikeProfile,
    pub released_tick: u64,
    pub expires_tick: u64,
    pub next_pulse: u16,
}
impl FlameStrikeState {
    pub(crate) fn canonical_words(&self) -> impl Iterator<Item = u64> {
        [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            self.source.0,
            u64::from(self.team.0),
            self.center.x as u64,
            self.center.y as u64,
            self.released_tick,
            self.expires_tick,
            u64::from(self.next_pulse),
        ]
        .into_iter()
        .chain(self.profile.canonical_words())
    }
}

pub(crate) fn flame_strike_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static PROFILE: OnceLock<SpellcastingProfile> = OnceLock::new();
    *PROFILE.get_or_init(|| {
        serde_json::from_value(evidence(version)["flame_strike_spellcasting"].clone())
            .expect("retained Flame Strike projection")
    })
}
pub(crate) fn flame_strike_bindings(
    version: MapVersion,
) -> [crate::ResolvedNativeEffectBinding; 3] {
    let spell = flame_strike_spellcasting_for_version(version);
    let AbilityEffect::BuildingFlameStrike(p) = spell.ability.effect else {
        unreachable!()
    };
    [
        (crate::NativeEffectSourceKind::UnitAbility, spell.ability.id),
        (
            crate::NativeEffectSourceKind::AbilityEffect,
            p.options[0].ability,
        ),
        (
            crate::NativeEffectSourceKind::AbilityEffect,
            p.options[1].ability,
        ),
    ]
    .map(|(kind, ability)| crate::ResolvedNativeEffectBinding {
        source: crate::NativeEffectSource::new(kind, ability.0),
        implementation: crate::NativeEffectImplementationId::WarcraftFlameStrikeV1,
    })
}
pub(crate) fn flame_strike_source(version: MapVersion, source: crate::NativeEffectSource) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && flame_strike_bindings(version)
            .iter()
            .any(|b| b.source == source)
}
