use std::{fmt, sync::OnceLock};

use serde::Deserialize;

use crate::{
    components::{
        AbilityEffect, AbilityId, AbilityTargetPolicy, AttackTargetMask, BashEffectProfile,
        BurningOilEffectProfile, ChainLightningEffectProfile, EntanglingRootsEffectProfile,
        EvasionEffectProfile, ManaProfile, ModifierId, PassiveUnitEffect, PassiveUnitEffects,
        SpellcastingProfile, TriggeredAttackEffect, TriggeredSpellProcProfile,
    },
    content::CASTLE_FIGHT_SIMULATION_HZ,
    math::SUBUNITS_PER_WORLD_UNIT,
    version::{MapVersion, MapVersionRange},
};

const BINDINGS_JSON: &str = include_str!("../data/castle-fight/native-effect-bindings.json");
const TUNING_9_27_JSON: &str = include_str!("../data/castle-fight/9.27/native-effect-tuning.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeEffectImplementationId {
    WarcraftMarkerOnlyV1,
    WarcraftZeroDamageBarrageV1,
    WarcraftEvasionV1,
    WarcraftBashV1,
    WarcraftOrbSpellProcV1,
    WarcraftChainLightningV1,
    WarcraftEntanglingRootsV1,
    WarcraftBurningOilV1,
    WarcraftFrostArmorV1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeEffectCatalogError {
    UnsupportedMapVersion(MapVersion),
}

impl fmt::Display for NativeEffectCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMapVersion(version) => write!(
                formatter,
                "no native-effect tuning snapshot is available for Castle Fight {version}"
            ),
        }
    }
}

impl std::error::Error for NativeEffectCatalogError {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeUnitMechanics {
    pub passive_effects: PassiveUnitEffects,
    pub spellcasting: Option<SpellcastingProfile>,
}

#[derive(Debug)]
struct NativeEffectCatalog {
    bindings: Vec<OwnedBinding>,
    tuning_9_27: TuningFile,
}

impl NativeEffectCatalog {
    fn load() -> Result<Self, String> {
        let binding_file: BindingFile = serde_json::from_str(BINDINGS_JSON)
            .map_err(|error| format!("invalid native-effect binding registry: {error}"))?;
        if binding_file.schema_version != 1 {
            return Err(format!(
                "unsupported native-effect binding schema {}",
                binding_file.schema_version
            ));
        }

        let mut bindings = Vec::with_capacity(binding_file.bindings.len());
        for binding in binding_file.bindings {
            let valid_from = parse_version(&binding.valid_from)?;
            let valid_through = parse_version(&binding.valid_through)?;
            if valid_through < valid_from {
                return Err(format!(
                    "native-effect binding {} has inverted version range {}..={}",
                    binding.source_key, valid_from, valid_through
                ));
            }
            bindings.push(OwnedBinding {
                source_kind: binding.source_kind,
                source_key: binding.source_key,
                implementation: binding.implementation,
                valid_versions: MapVersionRange::inclusive(valid_from, valid_through),
            });
        }

        for (index, left) in bindings.iter().enumerate() {
            for right in &bindings[index + 1..] {
                if left.source_kind == right.source_kind
                    && left.source_key == right.source_key
                    && ranges_overlap(left.valid_versions, right.valid_versions)
                {
                    return Err(format!(
                        "native-effect bindings overlap for {} {}",
                        left.source_kind, left.source_key
                    ));
                }
            }
        }

        let tuning_9_27: TuningFile = serde_json::from_str(TUNING_9_27_JSON)
            .map_err(|error| format!("invalid Castle Fight 9.27 native-effect tuning: {error}"))?;
        if tuning_9_27.schema_version != 2 {
            return Err(format!(
                "unsupported native-effect tuning schema {}",
                tuning_9_27.schema_version
            ));
        }
        let tuning_version = parse_version(&tuning_9_27.map_version)?;
        if tuning_version != MapVersion::CASTLE_FIGHT_9_27 {
            return Err(format!(
                "9.27 tuning snapshot declares unexpected map version {tuning_version}"
            ));
        }

        for effect in &tuning_9_27.effects {
            rawcode(effect.source_key())?;
            if let Some(unit_rawcode) = effect.unit_rawcode() {
                rawcode(unit_rawcode)?;
            }
            let implementation = bindings
                .iter()
                .find(|binding| {
                    binding.source_kind == effect.source_kind()
                        && binding.source_key == effect.source_key()
                        && binding.valid_versions.contains(tuning_version)
                })
                .map(|binding| binding.implementation)
                .ok_or_else(|| {
                    format!(
                        "tuning for {} {} has no native implementation valid for {tuning_version}",
                        effect.source_kind(),
                        effect.source_key()
                    )
                })?;
            if implementation != effect.expected_implementation() {
                return Err(format!(
                    "tuning kind {} is incompatible with implementation {:?}",
                    effect.kind_name(),
                    implementation
                ));
            }
        }

        Ok(Self {
            bindings,
            tuning_9_27,
        })
    }

    fn tuning(&self, version: MapVersion) -> Option<&TuningFile> {
        (version == MapVersion::CASTLE_FIGHT_9_27).then_some(&self.tuning_9_27)
    }

    fn implementation_for(
        &self,
        source_kind: &str,
        source_key: &str,
        version: MapVersion,
    ) -> Option<NativeEffectImplementationId> {
        self.bindings
            .iter()
            .find(|binding| {
                binding.source_kind == source_kind
                    && binding.source_key == source_key
                    && binding.valid_versions.contains(version)
            })
            .map(|binding| binding.implementation)
    }
}

#[derive(Debug)]
struct OwnedBinding {
    source_kind: String,
    source_key: String,
    implementation: NativeEffectImplementationId,
    valid_versions: MapVersionRange,
}

#[derive(Debug, Deserialize)]
struct BindingFile {
    schema_version: u32,
    bindings: Vec<BindingRecord>,
}

#[derive(Debug, Deserialize)]
struct BindingRecord {
    source_kind: String,
    source_key: String,
    implementation: NativeEffectImplementationId,
    valid_from: String,
    valid_through: String,
    #[allow(dead_code)]
    notes: Option<String>,
}

#[derive(Debug, Deserialize)]
struct TuningFile {
    schema_version: u32,
    map_version: String,
    effects: Vec<TuningEffect>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum TuningEffect {
    Evasion {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        chance_per_10k: u16,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
    Bash {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        chance_per_10k: u16,
        bonus_damage: i32,
        stun_duration_millis: u32,
        targets: String,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
    OrbSpellProc {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        chance_per_10k: u16,
        effect_ability_rawcode: String,
        targets: String,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
    ChainLightning {
        source_kind: String,
        source_key: String,
        initial_damage: i32,
        maximum_targets: u8,
        jump_radius_world: i32,
        damage_reduction_per_10k: u16,
        targets: String,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
    EntanglingRoots {
        source_kind: String,
        source_key: String,
        damage_per_second: i32,
        duration_millis: u32,
        targets: String,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
    BurningOil {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        radius_world: i32,
        full_damage: i32,
        full_interval_millis: u16,
        half_damage: i32,
        half_interval_millis: u16,
        full_duration_millis: u16,
        total_duration_millis: u16,
        target_ground_units: bool,
        target_buildings: bool,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
    FrostArmor {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        mana_maximum: i32,
        mana_starting: i32,
        mana_regen_per_second_per_10k: u32,
        mana_cost: i32,
        cooldown_millis: u32,
        range_world: i32,
        armor_bonus_per_100: i16,
        armor_duration_millis: u32,
        slow_duration_millis: u32,
        movement_percent_delta: i16,
        attack_speed_percent_delta: i16,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
}

impl TuningEffect {
    fn source_kind(&self) -> &str {
        match self {
            Self::Evasion { source_kind, .. }
            | Self::Bash { source_kind, .. }
            | Self::OrbSpellProc { source_kind, .. }
            | Self::ChainLightning { source_kind, .. }
            | Self::EntanglingRoots { source_kind, .. }
            | Self::BurningOil { source_kind, .. }
            | Self::FrostArmor { source_kind, .. } => source_kind,
        }
    }

    fn source_key(&self) -> &str {
        match self {
            Self::Evasion { source_key, .. }
            | Self::Bash { source_key, .. }
            | Self::OrbSpellProc { source_key, .. }
            | Self::ChainLightning { source_key, .. }
            | Self::EntanglingRoots { source_key, .. }
            | Self::BurningOil { source_key, .. }
            | Self::FrostArmor { source_key, .. } => source_key,
        }
    }

    fn unit_rawcode(&self) -> Option<&str> {
        match self {
            Self::Evasion { unit_rawcode, .. }
            | Self::Bash { unit_rawcode, .. }
            | Self::OrbSpellProc { unit_rawcode, .. }
            | Self::BurningOil { unit_rawcode, .. }
            | Self::FrostArmor { unit_rawcode, .. } => Some(unit_rawcode),
            Self::ChainLightning { .. } | Self::EntanglingRoots { .. } => None,
        }
    }

    const fn expected_implementation(&self) -> NativeEffectImplementationId {
        match self {
            Self::Evasion { .. } => NativeEffectImplementationId::WarcraftEvasionV1,
            Self::Bash { .. } => NativeEffectImplementationId::WarcraftBashV1,
            Self::OrbSpellProc { .. } => NativeEffectImplementationId::WarcraftOrbSpellProcV1,
            Self::ChainLightning { .. } => NativeEffectImplementationId::WarcraftChainLightningV1,
            Self::EntanglingRoots { .. } => NativeEffectImplementationId::WarcraftEntanglingRootsV1,
            Self::BurningOil { .. } => NativeEffectImplementationId::WarcraftBurningOilV1,
            Self::FrostArmor { .. } => NativeEffectImplementationId::WarcraftFrostArmorV1,
        }
    }

    const fn kind_name(&self) -> &'static str {
        match self {
            Self::Evasion { .. } => "evasion",
            Self::Bash { .. } => "bash",
            Self::OrbSpellProc { .. } => "orb-spell-proc",
            Self::ChainLightning { .. } => "chain-lightning",
            Self::EntanglingRoots { .. } => "entangling-roots",
            Self::BurningOil { .. } => "burning-oil",
            Self::FrostArmor { .. } => "frost-armor",
        }
    }
}

fn catalog() -> &'static NativeEffectCatalog {
    static CATALOG: OnceLock<NativeEffectCatalog> = OnceLock::new();
    CATALOG.get_or_init(|| NativeEffectCatalog::load().expect("invalid native-effect content"))
}

pub fn native_unit_mechanics_for(
    version: MapVersion,
    unit_rawcode: u32,
) -> Result<NativeUnitMechanics, NativeEffectCatalogError> {
    let catalog = catalog();
    let tuning = catalog
        .tuning(version)
        .ok_or(NativeEffectCatalogError::UnsupportedMapVersion(version))?;
    let mut passive_effects = Vec::new();
    let mut spellcasting = None;

    for effect in &tuning.effects {
        let Some(effect_unit_rawcode) = effect.unit_rawcode() else {
            continue;
        };
        if rawcode(effect_unit_rawcode).expect("validated unit rawcode") != unit_rawcode {
            continue;
        }
        match effect {
            TuningEffect::FrostArmor { .. } => {
                assert!(
                    spellcasting.is_none(),
                    "one automatic spell per verification unit"
                );
                spellcasting = Some(build_spellcasting(effect));
            }
            TuningEffect::Evasion { .. }
            | TuningEffect::Bash { .. }
            | TuningEffect::OrbSpellProc { .. }
            | TuningEffect::BurningOil { .. } => {
                passive_effects.push(build_passive_effect(tuning, effect));
            }
            TuningEffect::ChainLightning { .. } | TuningEffect::EntanglingRoots { .. } => {}
        }
    }

    Ok(NativeUnitMechanics {
        passive_effects: PassiveUnitEffects::from_slice(&passive_effects),
        spellcasting,
    })
}

#[must_use]
pub fn native_effect_implementation_for(
    source_kind: &str,
    source_key: &str,
    version: MapVersion,
) -> Option<NativeEffectImplementationId> {
    catalog().implementation_for(source_kind, source_key, version)
}

fn build_passive_effect(tuning: &TuningFile, effect: &TuningEffect) -> PassiveUnitEffect {
    match effect {
        TuningEffect::Evasion {
            source_key,
            chance_per_10k,
            ..
        } => {
            assert!(*chance_per_10k <= 10_000, "Evasion chance exceeds 100%");
            PassiveUnitEffect::Evasion(EvasionEffectProfile {
                ability: AbilityId(rawcode(source_key).expect("validated Evasion rawcode")),
                chance_per_10k: *chance_per_10k,
            })
        }
        TuningEffect::Bash {
            source_key,
            chance_per_10k,
            bonus_damage,
            stun_duration_millis,
            targets,
            ..
        } => {
            assert!(*chance_per_10k <= 10_000, "Bash chance exceeds 100%");
            assert!(*bonus_damage >= 0, "Bash bonus damage must be non-negative");
            PassiveUnitEffect::Bash(BashEffectProfile {
                ability: AbilityId(rawcode(source_key).expect("validated Bash rawcode")),
                chance_per_10k: *chance_per_10k,
                bonus_damage: *bonus_damage,
                stun_duration_ticks: exact_millis_to_ticks(*stun_duration_millis, "Bash duration"),
                targets: target_mask(targets),
            })
        }
        TuningEffect::OrbSpellProc {
            source_key,
            chance_per_10k,
            effect_ability_rawcode,
            targets,
            ..
        } => {
            assert!(*chance_per_10k <= 10_000, "orb proc chance exceeds 100%");
            let child = tuning
                .effects
                .iter()
                .find(|candidate| {
                    candidate.source_kind() == "ability-effect"
                        && candidate.source_key() == effect_ability_rawcode
                })
                .unwrap_or_else(|| {
                    panic!("missing tuning for orb effect {effect_ability_rawcode}")
                });
            PassiveUnitEffect::TriggeredSpellProc(TriggeredSpellProcProfile {
                ability: AbilityId(rawcode(source_key).expect("validated orb rawcode")),
                chance_per_10k: *chance_per_10k,
                targets: target_mask(targets),
                effect: build_triggered_effect(child),
            })
        }
        TuningEffect::BurningOil {
            source_key,
            radius_world,
            full_damage,
            full_interval_millis,
            half_damage,
            half_interval_millis,
            full_duration_millis,
            total_duration_millis,
            target_ground_units,
            target_buildings,
            ..
        } => PassiveUnitEffect::BurningOil(BurningOilEffectProfile {
            ability: AbilityId(rawcode(source_key).expect("validated Burning Oil rawcode")),
            radius: world(*radius_world),
            full_damage: *full_damage,
            full_interval_millis: *full_interval_millis,
            half_damage: *half_damage,
            half_interval_millis: *half_interval_millis,
            full_duration_millis: *full_duration_millis,
            total_duration_millis: *total_duration_millis,
            target_ground_units: *target_ground_units,
            target_buildings: *target_buildings,
        }),
        TuningEffect::ChainLightning { .. }
        | TuningEffect::EntanglingRoots { .. }
        | TuningEffect::FrostArmor { .. } => {
            panic!("effect {} is not a unit passive", effect.kind_name())
        }
    }
}

fn build_triggered_effect(effect: &TuningEffect) -> TriggeredAttackEffect {
    match effect {
        TuningEffect::ChainLightning {
            source_key,
            initial_damage,
            maximum_targets,
            jump_radius_world,
            damage_reduction_per_10k,
            targets,
            ..
        } => TriggeredAttackEffect::ChainLightning(ChainLightningEffectProfile {
            ability: AbilityId(rawcode(source_key).expect("validated Chain Lightning rawcode")),
            initial_damage: *initial_damage,
            maximum_targets: *maximum_targets,
            jump_radius: world(*jump_radius_world),
            damage_reduction_per_10k: *damage_reduction_per_10k,
            targets: target_mask(targets),
        }),
        TuningEffect::EntanglingRoots {
            source_key,
            damage_per_second,
            duration_millis,
            targets,
            ..
        } => TriggeredAttackEffect::EntanglingRoots(EntanglingRootsEffectProfile {
            ability: AbilityId(rawcode(source_key).expect("validated Entangling Roots rawcode")),
            damage_per_second: *damage_per_second,
            duration_ticks: exact_millis_to_ticks(*duration_millis, "Entangling Roots duration"),
            targets: target_mask(targets),
        }),
        _ => panic!("{} cannot be used as an orb effect", effect.kind_name()),
    }
}

fn build_spellcasting(effect: &TuningEffect) -> SpellcastingProfile {
    let TuningEffect::FrostArmor {
        source_key,
        mana_maximum,
        mana_starting,
        mana_regen_per_second_per_10k,
        mana_cost,
        cooldown_millis,
        range_world,
        armor_bonus_per_100,
        armor_duration_millis,
        slow_duration_millis,
        movement_percent_delta,
        attack_speed_percent_delta,
        ..
    } = effect
    else {
        panic!("{} is not an automatic spell", effect.kind_name());
    };
    let hz = u32::try_from(CASTLE_FIGHT_SIMULATION_HZ).expect("simulation Hz is positive");
    assert_eq!(
        *mana_regen_per_second_per_10k % hz,
        0,
        "mana regeneration must map exactly to fixed-point ticks"
    );
    SpellcastingProfile {
        mana: ManaProfile {
            maximum: *mana_maximum,
            starting: *mana_starting,
            regen_per_tick_per_10k: *mana_regen_per_second_per_10k / hz,
        },
        ability: crate::components::AutomaticAbilityProfile {
            id: AbilityId(rawcode(source_key).expect("validated Frost Armor rawcode")),
            mana_cost: *mana_cost,
            cooldown_ticks: exact_millis_to_ticks(*cooldown_millis, "Frost Armor cooldown"),
            range: world(*range_world),
            target_policy: AbilityTargetPolicy::RecentlyAttackedFriendlyUnit,
            effect: AbilityEffect::FrostArmor {
                modifier: ModifierId(rawcode(source_key).expect("validated Frost Armor rawcode")),
                armor_bonus_per_100: *armor_bonus_per_100,
                armor_duration_ticks: exact_millis_to_ticks(
                    *armor_duration_millis,
                    "Frost Armor duration",
                ),
                slow_duration_ticks: exact_millis_to_ticks(
                    *slow_duration_millis,
                    "Frost Armor slow duration",
                ),
                movement_percent_delta: *movement_percent_delta,
                attack_speed_percent_delta: *attack_speed_percent_delta,
            },
        },
    }
}

fn target_mask(value: &str) -> AttackTargetMask {
    match value {
        "ground-units" => AttackTargetMask::GROUND_UNITS,
        "air-ground-units" => AttackTargetMask::AIR_AND_GROUND,
        other => panic!("unsupported native-effect target mask {other}"),
    }
}

fn exact_millis_to_ticks(millis: u32, label: &str) -> u16 {
    let tick_numerator = u64::from(millis)
        * u64::try_from(CASTLE_FIGHT_SIMULATION_HZ).expect("simulation Hz is positive");
    assert_eq!(
        tick_numerator % 1_000,
        0,
        "{label} must map exactly onto simulation ticks"
    );
    u16::try_from(tick_numerator / 1_000).expect("duration exceeds u16 tick range")
}

const fn world(world_units: i32) -> i32 {
    world_units * SUBUNITS_PER_WORLD_UNIT
}

fn parse_version(value: &str) -> Result<MapVersion, String> {
    value
        .parse()
        .map_err(|error| format!("invalid map version {value:?}: {error}"))
}

fn rawcode(value: &str) -> Result<u32, String> {
    let bytes: [u8; 4] = value
        .as_bytes()
        .try_into()
        .map_err(|_| format!("rawcode must contain exactly four bytes: {value:?}"))?;
    Ok(u32::from_be_bytes(bytes))
}

const fn ranges_overlap(left: MapVersionRange, right: MapVersionRange) -> bool {
    left.contains(right.first)
        || left.contains(right.last)
        || right.contains(left.first)
        || right.contains(left.last)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_slice_bindings_are_version_scoped() {
        for (kind, key) in [
            ("unit-ability", "A0CV"),
            ("unit-ability", "A03N"),
            ("unit-ability", "A00U"),
            ("unit-ability", "A05K"),
            ("unit-ability", "A01B"),
            ("ability-effect", "A05X"),
            ("unit-ability", "A049"),
            ("ability-effect", "A03W"),
            ("unit-ability", "A02J"),
            ("unit-ability", "A03Z"),
        ] {
            assert!(native_effect_implementation_for(kind, key, MapVersion::new(9, 27)).is_some());
            assert_eq!(
                native_effect_implementation_for(kind, key, MapVersion::new(9, 28)),
                None
            );
        }
    }

    #[test]
    fn current_unit_mechanics_are_loaded_from_927_tuning() {
        let ranger =
            native_unit_mechanics_for(MapVersion::CASTLE_FIGHT_9_27, u32::from_be_bytes(*b"e003"))
                .unwrap();
        assert!(matches!(
            ranger.passive_effects.iter().next(),
            Some(PassiveUnitEffect::Evasion(EvasionEffectProfile {
                chance_per_10k: 1_500,
                ..
            }))
        ));

        let troll =
            native_unit_mechanics_for(MapVersion::CASTLE_FIGHT_9_27, u32::from_be_bytes(*b"n015"))
                .unwrap();
        let spellcasting = troll.spellcasting.expect("Ice Troll must cast Frost Armor");
        assert_eq!(spellcasting.mana.maximum, 250);
        assert_eq!(spellcasting.mana.starting, 150);
        assert_eq!(spellcasting.mana.regen_per_tick_per_10k, 500);
        assert_eq!(spellcasting.ability.mana_cost, 35);
        assert_eq!(spellcasting.ability.cooldown_ticks, 240);
    }
}
