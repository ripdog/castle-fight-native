use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::OnceLock,
};

use serde::Deserialize;

use crate::{
    components::{
        AbilityConfigurationError, AbilityEffect, AbilityId, AbilityTargetPolicy,
        AdditionalAutomaticAbilityDefinitions, AttackTargetMask, BashEffectProfile,
        BurningOilEffectProfile, ChainLightningEffectProfile, CleaveEffectProfile,
        CriticalStrikeEffectProfile, DefendEffectProfile, EntanglingRootsEffectProfile,
        EvasionEffectProfile, FeedbackEffectProfile, FrostAttackEffectProfile, ManaProfile,
        ModifierId, PassiveUnitEffect, PassiveUnitEffects, SpellResistanceEffectProfile,
        SpellcastingProfile, TriggeredAttackEffect, TriggeredSpellProcProfile,
        compose_spellcasting_profiles,
    },
    content::CASTLE_FIGHT_SIMULATION_HZ,
    math::SUBUNITS_PER_WORLD_UNIT,
    version::{MapVersion, MapVersionRange},
};

const BINDINGS_JSON: &str = include_str!("../data/castle-fight/native-effect-bindings.json");
const TUNING_9_27_JSON: &str = include_str!("../data/castle-fight/9.27/native-effect-tuning.json");

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeEffectImplementationId {
    WarcraftPermanentImmolationV1,
    WarcraftSpikedShellV1,
    WarcraftHardenedSkinV1,
    WarcraftDeathSpawnV1,
    WarcraftCompanionProductionV1,
    WarcraftSilverGladeV1,
    WarcraftStarfallV1,
    WarcraftWildernessV1,
    WarcraftCorruptionV1,
    WarcraftResistantSkinV1,
    WarcraftBuildingImprovementV1,
    WarcraftBuildingDamageProcV1,
    WarcraftWeatherWaveV1,
    WarcraftSourceDamageDispatcherV1,
    WarcraftHealthScaledAftershockV1,
    WarcraftDamageRetaliationV1,
    WarcraftForkedLightningV1,
    WarcraftAuraCarrierV1,
    WarcraftStasisTrapV1,
    WarcraftPermanentAbilityGrantsV1,
    WarcraftAcidBombV1,
    WarcraftPoisonV1,
    WarcraftDamageThresholdV1,
    WarcraftKillBerserkV1,
    WarcraftMarkerOnlyV1,
    WarcraftZeroDamageBarrageV1,
    WarcraftEvasionV1,
    WarcraftDefendV1,
    WarcraftBashV1,
    WarcraftOrbSpellProcV1,
    WarcraftChainLightningV1,
    WarcraftEntanglingRootsV1,
    WarcraftBurningOilV1,
    WarcraftFrostArmorV1,
    WarcraftCriticalStrikeV1,
    WarcraftCleaveV1,
    WarcraftFrostAttackV1,
    WarcraftPulverizeV1,
    WarcraftNativeAuraV1,
    WarcraftSpeedAuraV1,
    WarcraftSphereV1,
    WarcraftScriptDamageControllerV1,
    WarcraftSelfDestructV1,
    WarcraftMirrorImageV1,
    WarcraftLavaSpawnV1,
    WarcraftConeWaveV1,
    WarcraftScriptedAutomaticV1,
    WarcraftHumanSupportV1,
    WarcraftHumanPassiveV1,
    WarcraftHumanUtilityV1,
    WarcraftSpellResistanceV1,
    WarcraftFeedbackV1,
    WarcraftFaerieFireV1,
    WarcraftElvenAutomaticV1,
    WarcraftBuildingHexV1,
    WarcraftSnowveilV1,
    WarcraftHostileArmorAuraV1,
    WarcraftIncinerateV1,
    WarcraftManaAttackOrbV1,
    WarcraftDeathExplosionV1,
    WarcraftNativeImpaleV1,
    WarcraftNativeTargetDebuffV1,
    WarcraftEraserV1,
    WarcraftMagicRuinV1,
    WarcraftFlameStrikeV1,
    WarcraftBuildingAttackProxyV1,
    WarcraftHailstoneV1,
    WarcraftBuildingBoltV1,
    WarcraftWorldFreezerV1,
    WarcraftBarrageV1,
    WarcraftPersistentCarrierV1,
    WarcraftNagaEarthquakeV1,
    WarcraftNagaCharmV1,
    WarcraftNativeBuildingAurasV1,
    WarcraftNagaTidalWaveV1,
}

impl NativeEffectImplementationId {
    #[must_use]
    pub const fn stable_tag(self) -> u8 {
        match self {
            Self::WarcraftPermanentImmolationV1 => 96,
            Self::WarcraftNagaEarthquakeV1 => 92,
            Self::WarcraftNagaCharmV1 => 93,
            Self::WarcraftNativeBuildingAurasV1 => 94,
            Self::WarcraftNagaTidalWaveV1 => 95,
            Self::WarcraftSpikedShellV1 => 90,
            Self::WarcraftHardenedSkinV1 => 91,
            Self::WarcraftDeathSpawnV1 => 89,
            Self::WarcraftCompanionProductionV1 => 88,
            Self::WarcraftStarfallV1 => 87,
            Self::WarcraftWildernessV1 => 86,
            Self::WarcraftSilverGladeV1 => 85,
            Self::WarcraftCorruptionV1 => 84,
            Self::WarcraftResistantSkinV1 => 83,
            Self::WarcraftBuildingImprovementV1 => 79,
            Self::WarcraftBuildingDamageProcV1 => 82,
            Self::WarcraftWeatherWaveV1 => 81,
            Self::WarcraftSourceDamageDispatcherV1 => 80,
            Self::WarcraftHealthScaledAftershockV1 => 78,
            Self::WarcraftDamageRetaliationV1 => 76,
            Self::WarcraftForkedLightningV1 => 77,
            Self::WarcraftAuraCarrierV1 => 36,
            Self::WarcraftStasisTrapV1 => 35,
            Self::WarcraftPoisonV1 => 68,
            Self::WarcraftDamageThresholdV1 => 67,
            Self::WarcraftKillBerserkV1 => 66,
            Self::WarcraftMarkerOnlyV1 => 0,
            Self::WarcraftZeroDamageBarrageV1 => 1,
            Self::WarcraftEvasionV1 => 2,
            Self::WarcraftDefendV1 => 3,
            Self::WarcraftBashV1 => 4,
            Self::WarcraftOrbSpellProcV1 => 5,
            Self::WarcraftChainLightningV1 => 6,
            Self::WarcraftEntanglingRootsV1 => 7,
            Self::WarcraftBurningOilV1 => 8,
            Self::WarcraftFrostArmorV1 => 9,
            Self::WarcraftCriticalStrikeV1 => 10,
            Self::WarcraftCleaveV1 => 18,
            Self::WarcraftFrostAttackV1 => 20,
            Self::WarcraftPulverizeV1 => 21,
            Self::WarcraftNativeAuraV1 => 22,
            Self::WarcraftSpeedAuraV1 => 69,
            Self::WarcraftSphereV1 => 70,
            Self::WarcraftScriptDamageControllerV1 => 71,
            Self::WarcraftSelfDestructV1 => 72,
            Self::WarcraftMirrorImageV1 => 73,
            Self::WarcraftLavaSpawnV1 => 74,
            Self::WarcraftConeWaveV1 => 75,
            Self::WarcraftScriptedAutomaticV1 => 19,
            Self::WarcraftHumanSupportV1 => 11,
            Self::WarcraftHumanPassiveV1 => 12,
            Self::WarcraftHumanUtilityV1 => 13,
            Self::WarcraftSpellResistanceV1 => 14,
            Self::WarcraftFeedbackV1 => 15,
            Self::WarcraftFaerieFireV1 => 16,
            Self::WarcraftElvenAutomaticV1 => 17,
            Self::WarcraftBuildingHexV1 => 24,
            Self::WarcraftSnowveilV1 => 25,
            Self::WarcraftHostileArmorAuraV1 => 103,
            Self::WarcraftIncinerateV1 => 104,
            Self::WarcraftManaAttackOrbV1 => 105,
            Self::WarcraftDeathExplosionV1 => 106,
            Self::WarcraftNativeImpaleV1 => 102,
            Self::WarcraftNativeTargetDebuffV1 => 101,
            Self::WarcraftEraserV1 => 100,
            Self::WarcraftMagicRuinV1 => 99,
            Self::WarcraftFlameStrikeV1 => 98,
            Self::WarcraftBuildingAttackProxyV1 => 97,
            Self::WarcraftHailstoneV1 => 26,
            Self::WarcraftBuildingBoltV1 => 27,
            Self::WarcraftPermanentAbilityGrantsV1 => 34,
            Self::WarcraftAcidBombV1 => 33,
            Self::WarcraftWorldFreezerV1 => 28,
            Self::WarcraftBarrageV1 => 64,
            Self::WarcraftPersistentCarrierV1 => 65,
        }
    }

    const fn requires_tuning(self) -> bool {
        !matches!(
            self,
            Self::WarcraftResistantSkinV1
                | Self::WarcraftCompanionProductionV1
                | Self::WarcraftMarkerOnlyV1
                | Self::WarcraftZeroDamageBarrageV1
                | Self::WarcraftHumanSupportV1
                | Self::WarcraftHumanPassiveV1
                | Self::WarcraftHumanUtilityV1
                | Self::WarcraftBarrageV1
                | Self::WarcraftPersistentCarrierV1
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum NativeEffectSourceKind {
    UnitAbility,
    AbilityEffect,
}

impl NativeEffectSourceKind {
    #[must_use]
    pub const fn stable_tag(self) -> u8 {
        match self {
            Self::UnitAbility => 0,
            Self::AbilityEffect => 1,
        }
    }

    const fn as_str(self) -> &'static str {
        match self {
            Self::UnitAbility => "unit-ability",
            Self::AbilityEffect => "ability-effect",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        match value {
            "unit-ability" => Some(Self::UnitAbility),
            "ability-effect" => Some(Self::AbilityEffect),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeEffectSource {
    pub kind: NativeEffectSourceKind,
    pub key: u32,
}

impl NativeEffectSource {
    #[must_use]
    pub const fn new(kind: NativeEffectSourceKind, key: u32) -> Self {
        Self { kind, key }
    }
}

impl fmt::Display for NativeEffectSource {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} {}",
            self.kind.as_str(),
            display_rawcode(self.key)
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ResolvedNativeEffectBinding {
    pub source: NativeEffectSource,
    pub implementation: NativeEffectImplementationId,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeEffectResolveError {
    UnsupportedMapVersion(MapVersion),
    MissingBinding(NativeEffectSource),
    AmbiguousBinding(NativeEffectSource),
    MissingTuning(NativeEffectSource),
    IncompatibleTuning {
        source: NativeEffectSource,
        expected: NativeEffectImplementationId,
        selected: NativeEffectImplementationId,
    },
    DependencyCycle(NativeEffectSource),
}

impl fmt::Display for NativeEffectResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMapVersion(version) => write!(
                formatter,
                "no native-effect tuning snapshot is available for Castle Fight {version}"
            ),
            Self::MissingBinding(source) => {
                write!(formatter, "missing native-effect binding for {source}")
            }
            Self::AmbiguousBinding(source) => {
                write!(formatter, "multiple native-effect bindings match {source}")
            }
            Self::MissingTuning(source) => {
                write!(formatter, "missing native-effect tuning for {source}")
            }
            Self::IncompatibleTuning {
                source,
                expected,
                selected,
            } => write!(
                formatter,
                "native-effect tuning for {source} requires {expected:?}, selected {selected:?}"
            ),
            Self::DependencyCycle(source) => {
                write!(formatter, "native-effect dependency cycle reaches {source}")
            }
        }
    }
}

impl std::error::Error for NativeEffectResolveError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeEffectCatalogError {
    UnsupportedMapVersion(MapVersion),
    InvalidAutomaticAbilities {
        unit: u32,
        reason: AbilityConfigurationError,
    },
}

impl fmt::Display for NativeEffectCatalogError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedMapVersion(version) => write!(
                formatter,
                "no native-effect tuning snapshot is available for Castle Fight {version}"
            ),
            Self::InvalidAutomaticAbilities { unit, reason } => write!(
                formatter,
                "invalid automatic abilities for unit {}: {reason}",
                display_rawcode(*unit)
            ),
        }
    }
}

impl std::error::Error for NativeEffectCatalogError {}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NativeUnitMechanics {
    pub passive_effects: PassiveUnitEffects,
    pub spellcasting: Option<SpellcastingProfile>,
    pub additional_abilities: Option<AdditionalAutomaticAbilityDefinitions>,
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
            let source_kind =
                NativeEffectSourceKind::parse(&binding.source_kind).ok_or_else(|| {
                    format!(
                        "unsupported native-effect source kind {:?} for {}",
                        binding.source_kind, binding.source_key
                    )
                })?;
            bindings.push(OwnedBinding {
                source: NativeEffectSource::new(source_kind, rawcode(&binding.source_key)?),
                implementation: binding.implementation,
                valid_versions: MapVersionRange::inclusive(valid_from, valid_through),
            });
        }

        for (index, left) in bindings.iter().enumerate() {
            for right in &bindings[index + 1..] {
                if left.source == right.source
                    && ranges_overlap(left.valid_versions, right.valid_versions)
                {
                    return Err(format!(
                        "native-effect bindings overlap for {}",
                        left.source
                    ));
                }
            }
        }

        let tuning_9_27: TuningFile = serde_json::from_str(TUNING_9_27_JSON)
            .map_err(|error| format!("invalid Castle Fight 9.27 native-effect tuning: {error}"))?;
        if tuning_9_27.schema_version != 3 {
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
            effect.dependency_sources()?;
            let source = effect.source()?;
            let implementation = bindings
                .iter()
                .find(|binding| {
                    binding.source == source && binding.valid_versions.contains(tuning_version)
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
        let kind = NativeEffectSourceKind::parse(source_kind)?;
        let key = rawcode(source_key).ok()?;
        self.bindings
            .iter()
            .find(|binding| {
                binding.source == NativeEffectSource::new(kind, key)
                    && binding.valid_versions.contains(version)
            })
            .map(|binding| binding.implementation)
    }

    fn resolve_requirements(
        &self,
        version: MapVersion,
        roots: &[NativeEffectSource],
    ) -> Result<Vec<ResolvedNativeEffectBinding>, NativeEffectResolveError> {
        let tuning = self
            .tuning(version)
            .ok_or(NativeEffectResolveError::UnsupportedMapVersion(version))?;
        let mut resolved = BTreeMap::new();
        let mut visiting = BTreeSet::new();
        let roots = roots.iter().copied().collect::<BTreeSet<_>>();
        for root in roots {
            self.resolve_requirement(version, tuning, root, &mut visiting, &mut resolved)?;
        }
        Ok(resolved.into_values().collect())
    }

    fn resolve_requirement(
        &self,
        version: MapVersion,
        tuning: &TuningFile,
        source: NativeEffectSource,
        visiting: &mut BTreeSet<NativeEffectSource>,
        resolved: &mut BTreeMap<NativeEffectSource, ResolvedNativeEffectBinding>,
    ) -> Result<(), NativeEffectResolveError> {
        if resolved.contains_key(&source) {
            return Ok(());
        }
        if !visiting.insert(source) {
            return Err(NativeEffectResolveError::DependencyCycle(source));
        }

        let matching = self
            .bindings
            .iter()
            .filter(|binding| binding.source == source && binding.valid_versions.contains(version))
            .collect::<Vec<_>>();
        let selected = match matching.as_slice() {
            [] => return Err(NativeEffectResolveError::MissingBinding(source)),
            [binding] => binding.implementation,
            _ => return Err(NativeEffectResolveError::AmbiguousBinding(source)),
        };

        let effects = tuning
            .effects
            .iter()
            .filter(|effect| effect.source().ok() == Some(source))
            .collect::<Vec<_>>();
        if effects.is_empty() && selected.requires_tuning() {
            return Err(NativeEffectResolveError::MissingTuning(source));
        }

        for effect in effects {
            let expected = effect.expected_implementation();
            if expected != selected {
                return Err(NativeEffectResolveError::IncompatibleTuning {
                    source,
                    expected,
                    selected,
                });
            }
            for dependency in effect
                .dependency_sources()
                .expect("native-effect dependencies validated at catalog load")
            {
                self.resolve_requirement(version, tuning, dependency, visiting, resolved)?;
            }
        }

        visiting.remove(&source);
        resolved.insert(
            source,
            ResolvedNativeEffectBinding {
                source,
                implementation: selected,
            },
        );
        Ok(())
    }
}

#[derive(Debug, Clone)]
struct OwnedBinding {
    source: NativeEffectSource,
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

#[derive(Debug, Clone, Deserialize)]
struct TuningFile {
    schema_version: u32,
    map_version: String,
    buff_families: Vec<NativeBuffRecord>,
    unit_death_rules: Vec<NativeUnitDeathRule>,
    effects: Vec<TuningEffect>,
}

#[derive(Debug, Clone, Deserialize)]
struct NativeBuffRecord {
    source_key: String,
    buff: crate::NativeBuffIdentity,
}

/// Source-owned family metadata also covers native child buffs cast by script.
pub(crate) fn native_buff_for_version(
    version: MapVersion,
    source: u32,
) -> Option<crate::NativeBuffIdentity> {
    native_buff_from_tuning(catalog().tuning(version)?, source)
}

fn native_buff_from_tuning(tuning: &TuningFile, source: u32) -> Option<crate::NativeBuffIdentity> {
    tuning.buff_families.iter().find_map(|record| {
        (rawcode(&record.source_key).ok() == Some(source)).then_some(record.buff)
    })
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum TuningEffect {
    DeathExplosion {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        profile: crate::DeathExplosionProfile,
    },
    ManaAttackOrb {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        profile: crate::ManaAttackOrbProfile,
    },
    Incinerate {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        profile: crate::IncinerateProfile,
    },
    HostileArmorAura {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        profile: crate::HostileArmorAuraProfile,
    },
    PermanentImmolation {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        profile: crate::components::PermanentImmolationProfile,
    },
    NativeWeatherWave {
        source_kind: String,
        source_key: String,
        profile: crate::building_mechanics::WeatherWaveProfile,
    },
    NativeEnchantmentGrant {
        source_kind: String,
        source_key: String,
        grant: crate::NativeAbilityGrant,
        dependencies: Vec<String>,
    },
    SourceDamageDispatcher {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        ability: crate::AbilityId,
    },
    BuildingImprovement {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        effect_ability_rawcode: String,
        profile: crate::building_mechanics::BuildingImprovementProfile,
    },
    BuildingManaRegeneration {
        source_kind: String,
        source_key: String,
        levels: [u32; 2],
    },
    HealthScaledAftershock {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        profile: crate::HealthScaledAftershockProfile,
    },
    DamageRetaliation {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        effect_ability_rawcode: String,
        profile: crate::DamageRetaliationProfile,
    },
    RetaliationProjectile {
        source_kind: String,
        source_key: String,
        profiles: [crate::NativeBoltProfile; 2],
    },
    ForkedLightning {
        source_kind: String,
        source_key: String,
        profile: crate::ForkedLightningProfile,
    },
    LavaSpawn {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        profile: crate::components::LavaSpawnProfile,
    },
    ConeWave {
        source_kind: String,
        source_key: String,
        profile: crate::components::ConeWaveProfile,
    },
    MirrorImage {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        profile: crate::components::MirrorImageProfile,
    },
    DamageCharge {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        effect_ability_rawcode: String,
        profile: crate::components::DamageChargeProfile,
    },
    SelfDestruct {
        source_kind: String,
        source_key: String,
        profile: crate::components::SelfDestructProfile,
    },
    DeathSpawn {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        child_rawcode: u32,
        count: u8,
    },
    Poison {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        profile: crate::PoisonProfile,
    },
    DamageThreshold {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        thresholds: [crate::components::DamageThreshold; 3],
    },
    KillBerserk {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        duration_ticks: u16,
        movement_percent_delta: i16,
        attack_speed_percent_delta: i16,
        damage_taken_bonus_per_10k: i16,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
    Corruption {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        profile: crate::components::CorruptionProfile,
    },
    HardenedSkin {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        profile: crate::NativeAbilityGrant,
    },
    SpikedShell {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        profile: crate::components::SpikedShellProfile,
    },
    Sphere {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        profile: crate::components::SphereProfile,
    },
    SpeedAura {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        profile: crate::components::SpeedAuraProfile,
    },
    NativeAura {
        melee_damage_return_per_10k: u32,
        damage_bonus_per_10k: i16,
        armor_bonus_per_100: i16,
        affects_melee: bool,
        affects_ranged: bool,
        linger_ticks: u16,
        #[serde(default)]
        self_only: bool,
        #[serde(default)]
        melee_life_steal_per_10k: u32,
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        radius_world: i32,
        mana_regeneration_per_second_per_10k: u32,
    },
    Pulverize {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        chance_per_10k: u16,
        damage: i32,
        full_radius_world: i32,
        half_radius_world: i32,
        targets: String,
    },
    FrostAttack {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        duration_millis: u32,
        hero_duration_millis: u32,
        movement_percent_delta: i16,
        attack_speed_percent_delta: i16,
        targets: String,
    },
    ScriptedAutomatic {
        #[serde(default)]
        master_profile: Option<Box<crate::MasterProjectileProfile>>,
        #[serde(default)]
        dependencies: Vec<String>,
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        spellcasting: SpellcastingProfile,
        mana_regen_per_second_per_10k: u32,
        effect_ability_rawcode: Option<String>,
    },
    ElvenAutomatic {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        spellcasting: SpellcastingProfile,
        mana_regen_per_second_per_10k: u32,
        effect_ability_rawcode: Option<String>,
    },
    Feedback {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        maximum_mana_drained: i32,
        damage_per_mana_per_10k: u16,
        summoned_damage: i32,
        targets: String,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
    FaerieFire {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        mana_maximum: i32,
        mana_starting: i32,
        mana_regen_per_second_per_10k: u32,
        mana_cost: i32,
        cooldown_millis: u32,
        range_world: i32,
        armor_reduction_per_100: i16,
        duration_millis: u32,
        hero_duration_millis: u32,
        always_autocast: bool,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
    SpellResistance {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        damage_taken_per_10k: u16,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
    Cleave {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        radius_world: i32,
        damage_per_10k: u16,
        targets: AttackTargetMask,
    },
    CriticalStrike {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        chance_per_10k: u16,
        damage_multiplier_per_10k: u16,
        targets: String,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
    Evasion {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        chance_per_10k: u16,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
    Defend {
        source_kind: String,
        source_key: String,
        unit_rawcode: String,
        ranged_damage_taken_per_10k: u16,
        spell_damage_taken_per_10k: u16,
        deflect_chance_per_10k: u16,
        deflected_pierce_damage_taken_per_10k: u16,
        activation_delay_millis: u32,
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
        hero_stun_duration_millis: u32,
        targets: String,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
    NativeImpale {
        source_kind: String,
        source_key: String,
        profile: crate::ImpaleProfile,
        #[allow(dead_code)]
        provenance: serde_json::Value,
    },
    NativeTargetDebuff {
        source_kind: String,
        source_key: String,
        profile: crate::NativeDebuffProfile,
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
        hero_duration_millis: u32,
        nonhero_only: bool,
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
    fn source(&self) -> Result<NativeEffectSource, String> {
        let kind = NativeEffectSourceKind::parse(self.source_kind()).ok_or_else(|| {
            format!(
                "unsupported native-effect source kind {:?}",
                self.source_kind()
            )
        })?;
        Ok(NativeEffectSource::new(kind, rawcode(self.source_key())?))
    }

    fn dependency_sources(&self) -> Result<Vec<NativeEffectSource>, String> {
        let mut result = Vec::new();
        if let Self::ScriptedAutomatic { dependencies, .. }
        | Self::NativeEnchantmentGrant { dependencies, .. } = self
        {
            for code in dependencies {
                result.push(NativeEffectSource::new(
                    NativeEffectSourceKind::AbilityEffect,
                    rawcode(code)?,
                ));
            }
        }
        if let Self::ScriptedAutomatic { spellcasting, .. } = self
            && let crate::AbilityEffect::BuildingEnchantment(p) = spellcasting.ability.effect
        {
            result.push(NativeEffectSource::new(
                NativeEffectSourceKind::UnitAbility,
                p.dispatcher.0,
            ));
        }
        let effect_ability_rawcode = match self {
            Self::BuildingImprovement {
                effect_ability_rawcode,
                ..
            }
            | Self::DamageRetaliation {
                effect_ability_rawcode,
                ..
            }
            | Self::DamageCharge {
                effect_ability_rawcode,
                ..
            }
            | Self::OrbSpellProc {
                effect_ability_rawcode,
                ..
            } => effect_ability_rawcode,
            Self::ScriptedAutomatic {
                effect_ability_rawcode: Some(effect),
                ..
            }
            | Self::ElvenAutomatic {
                effect_ability_rawcode: Some(effect),
                ..
            } => effect,
            _ => return Ok(result),
        };
        result.push(NativeEffectSource::new(
            NativeEffectSourceKind::AbilityEffect,
            rawcode(effect_ability_rawcode)?,
        ));
        Ok(result)
    }

    fn source_kind(&self) -> &str {
        match self {
            Self::DeathExplosion { source_kind, .. }
            | Self::ManaAttackOrb { source_kind, .. }
            | Self::Incinerate { source_kind, .. }
            | Self::HostileArmorAura { source_kind, .. }
            | Self::NativeImpale { source_kind, .. }
            | Self::NativeTargetDebuff { source_kind, .. }
            | Self::NativeWeatherWave { source_kind, .. }
            | Self::SourceDamageDispatcher { source_kind, .. }
            | Self::NativeEnchantmentGrant { source_kind, .. }
            | Self::BuildingImprovement { source_kind, .. }
            | Self::BuildingManaRegeneration { source_kind, .. }
            | Self::HealthScaledAftershock { source_kind, .. }
            | Self::DamageRetaliation { source_kind, .. }
            | Self::RetaliationProjectile { source_kind, .. }
            | Self::ForkedLightning { source_kind, .. }
            | Self::LavaSpawn { source_kind, .. }
            | Self::ConeWave { source_kind, .. }
            | Self::MirrorImage { source_kind, .. }
            | Self::DamageCharge { source_kind, .. }
            | Self::SelfDestruct { source_kind, .. }
            | Self::DeathSpawn { source_kind, .. }
            | Self::Poison { source_kind, .. }
            | Self::DamageThreshold { source_kind, .. }
            | Self::KillBerserk { source_kind, .. }
            | Self::Corruption { source_kind, .. }
            | Self::SpikedShell { source_kind, .. }
            | Self::HardenedSkin { source_kind, .. }
            | Self::PermanentImmolation { source_kind, .. }
            | Self::Sphere { source_kind, .. }
            | Self::SpeedAura { source_kind, .. }
            | Self::NativeAura { source_kind, .. }
            | Self::Pulverize { source_kind, .. }
            | Self::FrostAttack { source_kind, .. }
            | Self::ScriptedAutomatic { source_kind, .. }
            | Self::ElvenAutomatic { source_kind, .. }
            | Self::Feedback { source_kind, .. }
            | Self::FaerieFire { source_kind, .. }
            | Self::SpellResistance { source_kind, .. }
            | Self::Evasion { source_kind, .. }
            | Self::Cleave { source_kind, .. }
            | Self::CriticalStrike { source_kind, .. }
            | Self::Defend { source_kind, .. }
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
            Self::DeathExplosion { source_key, .. }
            | Self::ManaAttackOrb { source_key, .. }
            | Self::Incinerate { source_key, .. }
            | Self::HostileArmorAura { source_key, .. }
            | Self::NativeImpale { source_key, .. }
            | Self::NativeTargetDebuff { source_key, .. }
            | Self::NativeWeatherWave { source_key, .. }
            | Self::SourceDamageDispatcher { source_key, .. }
            | Self::NativeEnchantmentGrant { source_key, .. }
            | Self::BuildingImprovement { source_key, .. }
            | Self::BuildingManaRegeneration { source_key, .. }
            | Self::HealthScaledAftershock { source_key, .. }
            | Self::DamageRetaliation { source_key, .. }
            | Self::RetaliationProjectile { source_key, .. }
            | Self::ForkedLightning { source_key, .. }
            | Self::LavaSpawn { source_key, .. }
            | Self::ConeWave { source_key, .. }
            | Self::MirrorImage { source_key, .. }
            | Self::DamageCharge { source_key, .. }
            | Self::SelfDestruct { source_key, .. }
            | Self::DeathSpawn { source_key, .. }
            | Self::Poison { source_key, .. }
            | Self::DamageThreshold { source_key, .. }
            | Self::KillBerserk { source_key, .. }
            | Self::Corruption { source_key, .. }
            | Self::SpikedShell { source_key, .. }
            | Self::HardenedSkin { source_key, .. }
            | Self::PermanentImmolation { source_key, .. }
            | Self::Sphere { source_key, .. }
            | Self::SpeedAura { source_key, .. }
            | Self::NativeAura { source_key, .. }
            | Self::Pulverize { source_key, .. }
            | Self::FrostAttack { source_key, .. }
            | Self::ScriptedAutomatic { source_key, .. }
            | Self::ElvenAutomatic { source_key, .. }
            | Self::Feedback { source_key, .. }
            | Self::FaerieFire { source_key, .. }
            | Self::SpellResistance { source_key, .. }
            | Self::Evasion { source_key, .. }
            | Self::Cleave { source_key, .. }
            | Self::CriticalStrike { source_key, .. }
            | Self::Defend { source_key, .. }
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
            Self::DeathExplosion { unit_rawcode, .. }
            | Self::ManaAttackOrb { unit_rawcode, .. }
            | Self::Incinerate { unit_rawcode, .. }
            | Self::HostileArmorAura { unit_rawcode, .. }
            | Self::SourceDamageDispatcher { unit_rawcode, .. }
            | Self::BuildingImprovement { unit_rawcode, .. }
            | Self::HealthScaledAftershock { unit_rawcode, .. }
            | Self::DamageRetaliation { unit_rawcode, .. }
            | Self::LavaSpawn { unit_rawcode, .. }
            | Self::MirrorImage { unit_rawcode, .. }
            | Self::DamageCharge { unit_rawcode, .. }
            | Self::DeathSpawn { unit_rawcode, .. }
            | Self::Poison { unit_rawcode, .. }
            | Self::DamageThreshold { unit_rawcode, .. }
            | Self::KillBerserk { unit_rawcode, .. }
            | Self::Corruption { unit_rawcode, .. }
            | Self::SpikedShell { unit_rawcode, .. }
            | Self::HardenedSkin { unit_rawcode, .. }
            | Self::PermanentImmolation { unit_rawcode, .. }
            | Self::Sphere { unit_rawcode, .. }
            | Self::SpeedAura { unit_rawcode, .. }
            | Self::NativeAura { unit_rawcode, .. }
            | Self::Pulverize { unit_rawcode, .. }
            | Self::FrostAttack { unit_rawcode, .. }
            | Self::ScriptedAutomatic { unit_rawcode, .. }
            | Self::ElvenAutomatic { unit_rawcode, .. }
            | Self::Feedback { unit_rawcode, .. }
            | Self::FaerieFire { unit_rawcode, .. }
            | Self::SpellResistance { unit_rawcode, .. }
            | Self::Evasion { unit_rawcode, .. }
            | Self::Cleave { unit_rawcode, .. }
            | Self::CriticalStrike { unit_rawcode, .. }
            | Self::Defend { unit_rawcode, .. }
            | Self::Bash { unit_rawcode, .. }
            | Self::OrbSpellProc { unit_rawcode, .. }
            | Self::BurningOil { unit_rawcode, .. }
            | Self::FrostArmor { unit_rawcode, .. } => Some(unit_rawcode),
            Self::NativeImpale { .. }
            | Self::NativeTargetDebuff { .. }
            | Self::NativeWeatherWave { .. }
            | Self::NativeEnchantmentGrant { .. }
            | Self::BuildingManaRegeneration { .. }
            | Self::RetaliationProjectile { .. }
            | Self::ForkedLightning { .. }
            | Self::ConeWave { .. }
            | Self::SelfDestruct { .. }
            | Self::ChainLightning { .. }
            | Self::EntanglingRoots { .. } => None,
        }
    }

    const fn expected_implementation(&self) -> NativeEffectImplementationId {
        match self {
            Self::DeathExplosion { .. } => NativeEffectImplementationId::WarcraftDeathExplosionV1,
            Self::ManaAttackOrb { .. } => NativeEffectImplementationId::WarcraftManaAttackOrbV1,
            Self::Incinerate { .. } => NativeEffectImplementationId::WarcraftIncinerateV1,
            Self::HostileArmorAura { .. } => {
                NativeEffectImplementationId::WarcraftHostileArmorAuraV1
            }
            Self::NativeImpale { .. } => NativeEffectImplementationId::WarcraftNativeImpaleV1,
            Self::NativeTargetDebuff { .. } => {
                NativeEffectImplementationId::WarcraftNativeTargetDebuffV1
            }
            Self::NativeWeatherWave { .. } => NativeEffectImplementationId::WarcraftWeatherWaveV1,
            Self::NativeEnchantmentGrant { .. } => {
                NativeEffectImplementationId::WarcraftPermanentAbilityGrantsV1
            }
            Self::SourceDamageDispatcher { .. } => {
                NativeEffectImplementationId::WarcraftSourceDamageDispatcherV1
            }
            Self::ElvenAutomatic { .. } => NativeEffectImplementationId::WarcraftElvenAutomaticV1,
            Self::ScriptedAutomatic { .. } => {
                NativeEffectImplementationId::WarcraftScriptedAutomaticV1
            }
            Self::SpellResistance { .. } => NativeEffectImplementationId::WarcraftSpellResistanceV1,
            Self::Feedback { .. } => NativeEffectImplementationId::WarcraftFeedbackV1,
            Self::FaerieFire { .. } => NativeEffectImplementationId::WarcraftFaerieFireV1,
            Self::Evasion { .. } => NativeEffectImplementationId::WarcraftEvasionV1,
            Self::CriticalStrike { .. } => NativeEffectImplementationId::WarcraftCriticalStrikeV1,
            Self::Cleave { .. } => NativeEffectImplementationId::WarcraftCleaveV1,
            Self::FrostAttack { .. } => NativeEffectImplementationId::WarcraftFrostAttackV1,
            Self::Pulverize { .. } => NativeEffectImplementationId::WarcraftPulverizeV1,
            Self::DeathSpawn { .. } => NativeEffectImplementationId::WarcraftDeathSpawnV1,
            Self::Poison { .. } => NativeEffectImplementationId::WarcraftPoisonV1,
            Self::DamageThreshold { .. } => NativeEffectImplementationId::WarcraftDamageThresholdV1,
            Self::KillBerserk { .. } => NativeEffectImplementationId::WarcraftKillBerserkV1,
            Self::NativeAura { .. } => NativeEffectImplementationId::WarcraftNativeAuraV1,
            Self::SpeedAura { .. } => NativeEffectImplementationId::WarcraftSpeedAuraV1,
            Self::Corruption { .. } => NativeEffectImplementationId::WarcraftCorruptionV1,
            Self::SpikedShell { .. } => NativeEffectImplementationId::WarcraftSpikedShellV1,
            Self::HardenedSkin { .. } => NativeEffectImplementationId::WarcraftHardenedSkinV1,
            Self::PermanentImmolation { .. } => {
                NativeEffectImplementationId::WarcraftPermanentImmolationV1
            }
            Self::Sphere { .. } => NativeEffectImplementationId::WarcraftSphereV1,
            Self::BuildingImprovement { .. } | Self::BuildingManaRegeneration { .. } => {
                NativeEffectImplementationId::WarcraftBuildingImprovementV1
            }
            Self::HealthScaledAftershock { .. } => {
                NativeEffectImplementationId::WarcraftHealthScaledAftershockV1
            }
            Self::DamageRetaliation { .. } | Self::RetaliationProjectile { .. } => {
                NativeEffectImplementationId::WarcraftDamageRetaliationV1
            }
            Self::ForkedLightning { .. } => NativeEffectImplementationId::WarcraftForkedLightningV1,
            Self::LavaSpawn { .. } => NativeEffectImplementationId::WarcraftLavaSpawnV1,
            Self::ConeWave { .. } => NativeEffectImplementationId::WarcraftConeWaveV1,
            Self::MirrorImage { .. } => NativeEffectImplementationId::WarcraftMirrorImageV1,
            Self::DamageCharge { .. } => {
                NativeEffectImplementationId::WarcraftScriptDamageControllerV1
            }
            Self::SelfDestruct { .. } => NativeEffectImplementationId::WarcraftSelfDestructV1,
            Self::Defend { .. } => NativeEffectImplementationId::WarcraftDefendV1,
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
            Self::DeathExplosion { .. } => "death-explosion",
            Self::ManaAttackOrb { .. } => "mana-attack-orb",
            Self::Incinerate { .. } => "incinerate",
            Self::HostileArmorAura { .. } => "hostile-armor-aura",
            Self::NativeImpale { .. } => "native-impale",
            Self::NativeTargetDebuff { .. } => "native-target-debuff",
            Self::NativeWeatherWave { .. } => "native-weather-wave",
            Self::NativeEnchantmentGrant { .. } => "native-enchantment-grant",
            Self::SourceDamageDispatcher { .. } => "source-damage-dispatcher",
            Self::ElvenAutomatic { .. } => "elven-automatic",
            Self::ScriptedAutomatic { .. } => "scripted-automatic",
            Self::SpellResistance { .. } => "spell-resistance",
            Self::Feedback { .. } => "feedback",
            Self::FaerieFire { .. } => "faerie-fire",
            Self::Evasion { .. } => "evasion",
            Self::CriticalStrike { .. } => "critical-strike",
            Self::Cleave { .. } => "cleave",
            Self::FrostAttack { .. } => "frost-attack",
            Self::Pulverize { .. } => "pulverize",
            Self::DeathSpawn { .. } => "death-spawn",
            Self::Poison { .. } => "poison",
            Self::DamageThreshold { .. } => "damage-threshold",
            Self::KillBerserk { .. } => "kill-berserk",
            Self::NativeAura { .. } => "native-aura",
            Self::SpeedAura { .. } => "speed-aura",
            Self::Corruption { .. } => "corruption",
            Self::SpikedShell { .. } => "spiked-shell",
            Self::HardenedSkin { .. } => "hardened-skin",
            Self::PermanentImmolation { .. } => "permanent-immolation",
            Self::Sphere { .. } => "sphere",
            Self::BuildingImprovement { .. } => "building-improvement",
            Self::BuildingManaRegeneration { .. } => "building-mana-regeneration",
            Self::HealthScaledAftershock { .. } => "health-scaled-aftershock",
            Self::DamageRetaliation { .. } => "damage-retaliation",
            Self::RetaliationProjectile { .. } => "retaliation-projectile",
            Self::ForkedLightning { .. } => "forked-lightning",
            Self::LavaSpawn { .. } => "lava-spawn",
            Self::ConeWave { .. } => "cone-wave",
            Self::MirrorImage { .. } => "mirror-image",
            Self::DamageCharge { .. } => "damage-charge",
            Self::SelfDestruct { .. } => "self-destruct",
            Self::Defend { .. } => "defend",
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

pub fn resolve_native_effect_requirements(
    version: MapVersion,
    roots: &[NativeEffectSource],
) -> Result<Vec<ResolvedNativeEffectBinding>, NativeEffectResolveError> {
    let (eraser_roots, roots): (Vec<_>, Vec<_>) = roots
        .iter()
        .copied()
        .partition(|s| crate::building_mechanics::eraser_source(version, *s));
    let (ruin_roots, roots): (Vec<_>, Vec<_>) = roots
        .iter()
        .copied()
        .partition(|s| crate::building_mechanics::magic_ruin_source(version, *s));
    let (flame_roots, roots): (Vec<_>, Vec<_>) = roots
        .iter()
        .copied()
        .partition(|s| crate::building_mechanics::flame_strike_source(version, *s));
    let (proxy_roots, roots): (Vec<_>, Vec<_>) = roots
        .iter()
        .copied()
        .partition(|s| crate::building_mechanics::attack_proxy_source(version, *s));
    let (tidal_roots, roots): (Vec<_>, Vec<_>) = roots
        .iter()
        .copied()
        .partition(|s| crate::building_mechanics::tidal_source(version, *s));
    let (coral_roots, roots): (Vec<_>, Vec<_>) = roots
        .iter()
        .copied()
        .partition(|s| crate::building_mechanics::coral_source(version, *s));
    let (oracle_roots, roots): (Vec<_>, Vec<_>) = roots
        .into_iter()
        .partition(|s| crate::building_mechanics::oracle_source(version, *s));
    let (pyramid_roots, roots): (Vec<_>, Vec<_>) = roots
        .into_iter()
        .partition(|s| crate::building_mechanics::pyramid_source(version, *s));
    let (silver_roots, roots): (Vec<_>, Vec<_>) = roots
        .iter()
        .copied()
        .partition(|s| crate::building_mechanics::silver_source(version, *s));
    let (wilderness_roots, roots): (Vec<_>, Vec<_>) = roots
        .into_iter()
        .partition(|s| crate::building_mechanics::wilderness_source(version, *s));
    let (starfall_roots, roots): (Vec<_>, Vec<_>) = roots
        .into_iter()
        .partition(|s| crate::building_mechanics::starfall_source(version, *s));
    let (proc_roots, roots): (Vec<_>, Vec<_>) = roots
        .iter()
        .copied()
        .partition(|source| crate::building_mechanics::damage_proc_source(version, *source));
    let (stasis_roots, roots): (Vec<_>, Vec<_>) = roots
        .iter()
        .copied()
        .partition(|source| crate::building_mechanics::stasis_source_for_version(*source, version));
    let (aura_roots, roots): (Vec<_>, Vec<_>) = roots.into_iter().partition(|source| {
        crate::building_mechanics::aura_carrier_source_for_version(*source, version)
    });
    let (grant_roots, roots): (Vec<_>, Vec<_>) = roots.into_iter().partition(|source| {
        crate::building_mechanics::ceremonial_source_for_version(*source, version)
    });
    let (acid_roots, roots): (Vec<_>, Vec<_>) = roots.into_iter().partition(|source| {
        crate::building_mechanics::serpent_source_for_version(*source, version)
    });
    let (field_roots, roots): (Vec<_>, Vec<_>) = roots.into_iter().partition(|source| {
        crate::building_mechanics::world_freezer_source_for_version(*source, version)
    });
    let (bolt_roots, roots): (Vec<_>, Vec<_>) = roots.into_iter().partition(|source| {
        crate::building_mechanics::building_bolt_source_for_version(*source, version)
    });
    let (launcher_roots, roots): (Vec<_>, Vec<_>) = roots.into_iter().partition(|source| {
        crate::building_mechanics::launcher_source_for_version(*source, version)
    });
    let (snow_roots, roots): (Vec<_>, Vec<_>) = roots
        .into_iter()
        .partition(|source| crate::building_mechanics::snow_source_for_version(*source, version));
    let (building_roots, ordinary_roots): (Vec<_>, Vec<_>) = roots
        .iter()
        .copied()
        .partition(|source| crate::building_mechanics::hex_source_for_version(*source, version));
    let mut resolved = catalog().resolve_requirements(version, &ordinary_roots)?;
    if !eraser_roots.is_empty() {
        resolved.push(crate::building_mechanics::eraser_binding(version));
    }
    if !ruin_roots.is_empty() {
        resolved.extend(crate::building_mechanics::magic_ruin_bindings(version));
    }
    if !flame_roots.is_empty() {
        resolved.extend(crate::building_mechanics::flame_strike_bindings(version));
    }
    if !proxy_roots.is_empty() {
        resolved.extend(crate::building_mechanics::attack_proxy_bindings(version));
    }
    if !tidal_roots.is_empty() {
        resolved.extend(crate::building_mechanics::tidal_bindings(version));
    }
    if !coral_roots.is_empty() {
        resolved.extend(crate::building_mechanics::coral_bindings(version));
    }
    if !oracle_roots.is_empty() {
        resolved.extend(crate::building_mechanics::oracle_bindings(version));
    }
    if !pyramid_roots.is_empty() {
        resolved.extend(crate::building_mechanics::pyramid_bindings(version));
    }
    if !starfall_roots.is_empty() {
        resolved.extend(crate::building_mechanics::starfall_bindings(version));
    }
    if !wilderness_roots.is_empty() {
        resolved.extend(crate::building_mechanics::wilderness_bindings(version));
    }
    if !silver_roots.is_empty() {
        resolved.extend(crate::building_mechanics::silver_bindings(version));
    }
    if !proc_roots.is_empty() {
        resolved.extend(crate::building_mechanics::damage_proc_bindings(version));
    }
    if !aura_roots.is_empty() {
        resolved.extend(
            crate::building_mechanics::aura_carrier_bindings_for_version(version, &aura_roots),
        );
    }
    if !stasis_roots.is_empty() {
        resolved.extend(crate::building_mechanics::stasis_bindings_for_version(
            version,
        ));
    }
    if !grant_roots.is_empty() {
        resolved.extend(crate::building_mechanics::ceremonial_bindings_for_version(
            version,
        ));
    }
    if !acid_roots.is_empty() {
        resolved.extend(crate::building_mechanics::serpent_bindings_for_version(
            version,
        ));
    }
    if !field_roots.is_empty() {
        resolved.extend(crate::building_mechanics::world_freezer_bindings_for_version(version));
    }
    if !launcher_roots.is_empty() {
        resolved.extend(crate::building_mechanics::launcher_bindings_for_version(
            version,
        ));
    }
    if !bolt_roots.is_empty() {
        resolved.extend(crate::building_mechanics::building_bolt_bindings_for_version(version));
    }
    if !snow_roots.is_empty() {
        resolved.extend(crate::building_mechanics::snow_bindings_for_version(
            version,
        ));
    }
    if !building_roots.is_empty() {
        resolved.extend(crate::building_mechanics::hex_bindings_for_version(version));
    }
    resolved.sort_unstable_by_key(|binding| binding.source);
    Ok(resolved)
}

pub(crate) fn permanent_grant_for_version(
    version: MapVersion,
    ability: u32,
) -> Option<crate::NativeAbilityGrant> {
    catalog()
        .tuning(version)?
        .effects
        .iter()
        .find_map(|effect| match effect {
            TuningEffect::NativeEnchantmentGrant {
                source_key, grant, ..
            } if rawcode(source_key).ok() == Some(ability) => Some(*grant),
            _ => None,
        })
}

pub(crate) fn canonical_projection_for_version(version: MapVersion) -> &'static [u8] {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    TUNING_9_27_JSON.as_bytes()
}

pub(crate) fn script_granted_ability_sources_for(
    version: MapVersion,
    unit_rawcode: u32,
) -> impl Iterator<Item = NativeEffectSource> {
    catalog()
        .tuning(version)
        .into_iter()
        .flat_map(move |tuning| {
            tuning
                .effects
                .iter()
                .filter(move |effect| {
                    matches!(effect, TuningEffect::DamageThreshold { unit_rawcode: source_unit, .. }
                if rawcode(source_unit).ok() == Some(unit_rawcode))
                })
                .map(|effect| effect.source().expect("validated script-granted source"))
        })
        .chain(crate::building_mechanics::damage_proc_granted_sources(
            version,
            unit_rawcode,
        ))
        .chain(crate::building_mechanics::training_granted_sources(
            version,
            unit_rawcode,
        ))
}

pub fn native_unit_mechanics_for(
    version: MapVersion,
    unit_rawcode: u32,
    ability_rawcodes: &[u32],
) -> Result<NativeUnitMechanics, NativeEffectCatalogError> {
    let catalog = catalog();
    let tuning = catalog
        .tuning(version)
        .ok_or(NativeEffectCatalogError::UnsupportedMapVersion(version))?;
    native_unit_mechanics_from_tuning(tuning, unit_rawcode, ability_rawcodes)
}

fn native_unit_mechanics_from_tuning(
    tuning: &TuningFile,
    unit_rawcode: u32,
    ability_rawcodes: &[u32],
) -> Result<NativeUnitMechanics, NativeEffectCatalogError> {
    let mut seen = BTreeSet::new();
    let abilities = ability_rawcodes
        .iter()
        .copied()
        .filter(|ability| seen.insert(*ability));
    let mut passive_effects = Vec::new();
    let mut automatic_profiles = Vec::new();

    for ability in abilities {
        let source = NativeEffectSource::new(NativeEffectSourceKind::UnitAbility, ability);
        let mut passive = None;
        let mut automatic_spell = None;
        for effect in tuning
            .effects
            .iter()
            .filter(|effect| effect.source().ok() == Some(source))
            .filter(|effect| !matches!(effect, TuningEffect::DamageCharge { unit_rawcode: code, .. } | TuningEffect::DamageRetaliation { unit_rawcode: code, .. } | TuningEffect::HealthScaledAftershock { unit_rawcode: code, .. } if rawcode(code).ok() != Some(unit_rawcode)))
        {
            match effect {
                TuningEffect::BuildingImprovement { .. } | TuningEffect::DeathSpawn { .. } => {},
                TuningEffect::ScriptedAutomatic { .. }
                | TuningEffect::ElvenAutomatic { .. }
                | TuningEffect::FrostArmor { .. }
                | TuningEffect::FaerieFire { .. } => {
                    let candidate = build_spellcasting(tuning, effect);
                    if let Some(existing) = automatic_spell {
                        assert_eq!(
                            existing,
                            candidate,
                            "ability {} has conflicting tuning rows while resolving unit {}",
                            display_rawcode(ability),
                            display_rawcode(unit_rawcode)
                        );
                    } else {
                        automatic_spell = Some(candidate);
                    }
                }
                TuningEffect::SourceDamageDispatcher { .. } | TuningEffect::HealthScaledAftershock { .. } | TuningEffect::DamageRetaliation { .. } | TuningEffect::LavaSpawn { .. } | TuningEffect::MirrorImage { .. }
                | TuningEffect::DamageCharge { .. }
                | TuningEffect::Poison { .. }
                | TuningEffect::KillBerserk { .. }
                | TuningEffect::Corruption { .. }
                | TuningEffect::SpikedShell { .. }
                | TuningEffect::HardenedSkin { .. }
                | TuningEffect::DeathExplosion { .. }
                | TuningEffect::ManaAttackOrb { .. }
                | TuningEffect::Incinerate { .. }
                | TuningEffect::HostileArmorAura { .. }
                | TuningEffect::PermanentImmolation { .. }
                | TuningEffect::Sphere { .. }
                | TuningEffect::SpeedAura { .. }
                | TuningEffect::NativeAura { .. }
                | TuningEffect::Pulverize { .. }
                | TuningEffect::FrostAttack { .. }
                | TuningEffect::Feedback { .. }
                | TuningEffect::SpellResistance { .. }
                | TuningEffect::Evasion { .. }
                | TuningEffect::Cleave { .. }
                | TuningEffect::CriticalStrike { .. }
                | TuningEffect::Defend { .. }
                | TuningEffect::Bash { .. }
                | TuningEffect::OrbSpellProc { .. }
                | TuningEffect::BurningOil { .. } => {
                    let candidate = build_passive_effect(tuning, effect);
                    if let Some(existing) = passive {
                        assert_eq!(
                            existing,
                            candidate,
                            "ability {} has conflicting tuning rows while resolving unit {}",
                            display_rawcode(ability),
                            display_rawcode(unit_rawcode)
                        );
                    } else {
                        passive = Some(candidate);
                    }
                }
                TuningEffect::NativeImpale { .. } | TuningEffect::NativeTargetDebuff { .. } | TuningEffect::NativeWeatherWave { .. } | TuningEffect::NativeEnchantmentGrant { .. } | TuningEffect::BuildingManaRegeneration { .. } | TuningEffect::RetaliationProjectile { .. } | TuningEffect::ForkedLightning { .. }
                | TuningEffect::SelfDestruct { .. }
                | TuningEffect::DamageThreshold { .. }
                | TuningEffect::ConeWave { .. }
        | TuningEffect::ChainLightning { .. }
                | TuningEffect::EntanglingRoots { .. } => {
                    unreachable!("ability-effect tuning cannot match a unit-ability source")
                }
            }
        }

        if let Some(effect) = passive {
            passive_effects.push(effect);
        }
        if let Some(candidate) = automatic_spell {
            automatic_profiles.push(candidate);
        }
    }

    for effect in tuning.effects.iter().filter(|effect| {
        matches!(effect, TuningEffect::DamageThreshold { unit_rawcode: source_unit, .. }
            if rawcode(source_unit).ok() == Some(unit_rawcode))
    }) {
        passive_effects.push(build_passive_effect(tuning, effect));
    }
    let (spellcasting, additional_abilities) =
        compose_spellcasting_profiles(None, automatic_profiles).map_err(|reason| {
            NativeEffectCatalogError::InvalidAutomaticAbilities {
                unit: unit_rawcode,
                reason,
            }
        })?;
    Ok(NativeUnitMechanics {
        passive_effects: PassiveUnitEffects::from_slice(&passive_effects),
        spellcasting,
        additional_abilities,
    })
}

#[must_use]
pub fn native_effect_implementation_for(
    source_kind: &str,
    source_key: &str,
    version: MapVersion,
) -> Option<NativeEffectImplementationId> {
    let source = NativeEffectSourceKind::parse(source_kind)
        .zip(rawcode(source_key).ok())
        .map(|(kind, key)| NativeEffectSource::new(kind, key));
    if source.is_some_and(|s| crate::building_mechanics::eraser_source(version, s)) {
        return Some(NativeEffectImplementationId::WarcraftEraserV1);
    }
    if source.is_some_and(|s| crate::building_mechanics::magic_ruin_source(version, s)) {
        return Some(NativeEffectImplementationId::WarcraftMagicRuinV1);
    }
    if source.is_some_and(|s| crate::building_mechanics::flame_strike_source(version, s)) {
        return Some(NativeEffectImplementationId::WarcraftFlameStrikeV1);
    }
    if source.is_some_and(|s| crate::building_mechanics::attack_proxy_source(version, s)) {
        return Some(NativeEffectImplementationId::WarcraftBuildingAttackProxyV1);
    }
    if source.is_some_and(|s| crate::building_mechanics::tidal_source(version, s)) {
        return Some(NativeEffectImplementationId::WarcraftNagaTidalWaveV1);
    }
    if source.is_some_and(|s| crate::building_mechanics::coral_source(version, s)) {
        return Some(NativeEffectImplementationId::WarcraftNativeBuildingAurasV1);
    }
    if source.is_some_and(|s| crate::building_mechanics::oracle_source(version, s)) {
        return Some(NativeEffectImplementationId::WarcraftNagaCharmV1);
    }
    if source.is_some_and(|s| crate::building_mechanics::pyramid_source(version, s)) {
        return Some(NativeEffectImplementationId::WarcraftNagaEarthquakeV1);
    }
    if source.is_some_and(|source| crate::building_mechanics::starfall_source(version, source)) {
        return Some(NativeEffectImplementationId::WarcraftStarfallV1);
    }
    if source.is_some_and(|source| crate::building_mechanics::wilderness_source(version, source)) {
        return Some(NativeEffectImplementationId::WarcraftWildernessV1);
    }
    if source.is_some_and(|source| crate::building_mechanics::silver_source(version, source)) {
        return Some(NativeEffectImplementationId::WarcraftSilverGladeV1);
    }
    if source.is_some_and(|source| crate::building_mechanics::damage_proc_source(version, source)) {
        return Some(NativeEffectImplementationId::WarcraftBuildingDamageProcV1);
    }
    if source
        .is_some_and(|source| crate::building_mechanics::hex_source_for_version(source, version))
    {
        return Some(NativeEffectImplementationId::WarcraftBuildingHexV1);
    }
    if source
        .is_some_and(|source| crate::building_mechanics::snow_source_for_version(source, version))
    {
        return Some(NativeEffectImplementationId::WarcraftSnowveilV1);
    }
    if source.is_some_and(|source| {
        crate::building_mechanics::launcher_source_for_version(source, version)
    }) {
        return Some(NativeEffectImplementationId::WarcraftHailstoneV1);
    }
    if source.is_some_and(|source| {
        crate::building_mechanics::building_bolt_source_for_version(source, version)
    }) {
        return Some(NativeEffectImplementationId::WarcraftBuildingBoltV1);
    }
    if source.is_some_and(|source| {
        crate::building_mechanics::world_freezer_source_for_version(source, version)
    }) {
        return Some(NativeEffectImplementationId::WarcraftWorldFreezerV1);
    }
    if source.is_some_and(|source| {
        crate::building_mechanics::serpent_source_for_version(source, version)
    }) {
        return Some(NativeEffectImplementationId::WarcraftAcidBombV1);
    }
    if source.is_some_and(|source| {
        crate::building_mechanics::ceremonial_source_for_version(source, version)
    }) {
        return Some(NativeEffectImplementationId::WarcraftPermanentAbilityGrantsV1);
    }
    if source
        .is_some_and(|source| crate::building_mechanics::stasis_source_for_version(source, version))
    {
        return Some(NativeEffectImplementationId::WarcraftStasisTrapV1);
    }
    if source.is_some_and(|source| {
        crate::building_mechanics::aura_carrier_source_for_version(source, version)
    }) {
        return Some(NativeEffectImplementationId::WarcraftAuraCarrierV1);
    }
    catalog().implementation_for(source_kind, source_key, version)
}

fn build_passive_effect(tuning: &TuningFile, effect: &TuningEffect) -> PassiveUnitEffect {
    match effect {
        TuningEffect::DeathExplosion { profile, .. } => {
            assert!(profile.delay_ticks > 0);
            PassiveUnitEffect::DeathExplosion(*profile)
        }
        TuningEffect::ManaAttackOrb { profile, .. } => PassiveUnitEffect::ManaAttackOrb(*profile),
        TuningEffect::Incinerate { profile, .. } => PassiveUnitEffect::Incinerate(*profile),
        TuningEffect::HostileArmorAura { profile, .. } => {
            PassiveUnitEffect::HostileArmorAura(*profile)
        }
        TuningEffect::DeathSpawn { .. } => {
            unreachable!("death spawn is consumed at structural commit")
        }
        TuningEffect::SourceDamageDispatcher { ability, .. } => {
            PassiveUnitEffect::SourceDamageDispatcher(*ability)
        }
        TuningEffect::DamageRetaliation {
            profile,
            effect_ability_rawcode,
            ..
        } => {
            let child = tuning
                .effects
                .iter()
                .find_map(|effect| match effect {
                    TuningEffect::RetaliationProjectile {
                        source_key,
                        profiles,
                        ..
                    } if source_key == effect_ability_rawcode => Some(profiles),
                    _ => None,
                })
                .expect("validated native retaliation dependency");
            assert!(child.contains(&profile.projectile));
            PassiveUnitEffect::DamageRetaliation(*profile)
        }
        TuningEffect::HealthScaledAftershock { profile, .. } => {
            PassiveUnitEffect::HealthScaledAftershock(*profile)
        }
        TuningEffect::LavaSpawn { profile, .. } => PassiveUnitEffect::LavaSpawn(*profile),
        TuningEffect::MirrorImage { profile, .. } => PassiveUnitEffect::MirrorImage(*profile),
        TuningEffect::DamageCharge {
            profile,
            effect_ability_rawcode,
            ..
        } => {
            let child = tuning
                .effects
                .iter()
                .find_map(|effect| match effect {
                    TuningEffect::SelfDestruct {
                        source_key,
                        profile,
                        ..
                    } if source_key == effect_ability_rawcode => Some(profile),
                    _ => None,
                })
                .expect("validated charge explosion dependency");
            assert_eq!(profile.explosion, *child);
            PassiveUnitEffect::DamageCharge(*profile)
        }
        TuningEffect::Poison { profile, .. } => {
            assert!(profile.damage_per_second >= 0 && profile.duration_ticks > 0);
            PassiveUnitEffect::Poison(*profile)
        }
        TuningEffect::DamageThreshold {
            source_key,
            thresholds,
            ..
        } => {
            assert!(thresholds.iter().all(|t| t.hp_ratio_below_per_10k > 0
                && t.hp_ratio_below_per_10k <= 10_000
                && t.attack_bonus >= 0));
            assert!(
                thresholds
                    .windows(2)
                    .all(|pair| pair[0].hp_ratio_below_per_10k > pair[1].hp_ratio_below_per_10k)
            );
            PassiveUnitEffect::DamageThreshold(crate::components::DamageThresholdProfile {
                ability: AbilityId(rawcode(source_key).expect("damage threshold source")),
                thresholds: *thresholds,
            })
        }
        TuningEffect::KillBerserk {
            source_key,
            duration_ticks,
            movement_percent_delta,
            attack_speed_percent_delta,
            damage_taken_bonus_per_10k,
            ..
        } => {
            assert!(
                *duration_ticks > 0
                    && *movement_percent_delta >= 0
                    && *attack_speed_percent_delta >= 0
                    && *damage_taken_bonus_per_10k >= 0
            );
            PassiveUnitEffect::KillBerserk(crate::components::KillBerserkProfile {
                ability: AbilityId(rawcode(source_key).expect("Berserk source")),
                duration_ticks: *duration_ticks,
                movement_percent_delta: *movement_percent_delta,
                attack_speed_percent_delta: *attack_speed_percent_delta,
                damage_taken_bonus_per_10k: *damage_taken_bonus_per_10k,
            })
        }
        TuningEffect::Corruption { profile, .. } => PassiveUnitEffect::Corruption(*profile),
        TuningEffect::SpikedShell { profile, .. } => PassiveUnitEffect::SpikedShell(*profile),
        TuningEffect::HardenedSkin { profile, .. } => PassiveUnitEffect::HardenedSkin(*profile),
        TuningEffect::PermanentImmolation { profile, .. } => {
            assert!(profile.interval_ticks > 0 && profile.damage >= 0 && profile.radius >= 0);
            PassiveUnitEffect::PermanentImmolation(*profile)
        }
        TuningEffect::Sphere { profile, .. } => {
            assert!((1..=3).contains(&profile.attachment_count) && profile.speed_per_tick >= 0);
            PassiveUnitEffect::Sphere(*profile)
        }
        TuningEffect::SpeedAura { profile, .. } => {
            assert!(profile.radius >= 0 && profile.buff_rawcode != 0);
            PassiveUnitEffect::SpeedAura(*profile)
        }
        TuningEffect::NativeAura {
            melee_damage_return_per_10k,
            damage_bonus_per_10k,
            armor_bonus_per_100,
            affects_melee,
            affects_ranged,
            linger_ticks,
            self_only,
            melee_life_steal_per_10k,
            source_key,
            radius_world,
            mana_regeneration_per_second_per_10k,
            ..
        } => PassiveUnitEffect::Aura(crate::components::AuraEffectProfile {
            self_only: *self_only,
            melee_life_steal_per_10k: *melee_life_steal_per_10k,
            melee_damage_return_per_10k: *melee_damage_return_per_10k,
            damage_bonus_per_10k: *damage_bonus_per_10k,
            affects_melee: *affects_melee,
            affects_ranged: *affects_ranged,
            linger_ticks: *linger_ticks,
            ability: AbilityId(rawcode(source_key).expect("aura source")),
            radius: world(*radius_world),
            armor_bonus_per_100: *armor_bonus_per_100,
            mana_regeneration_per_second_per_10k: *mana_regeneration_per_second_per_10k,
            suspend_during_spell_cooldown: false,
        }),
        TuningEffect::Pulverize {
            source_key,
            chance_per_10k,
            damage,
            full_radius_world,
            half_radius_world,
            targets,
            ..
        } => {
            assert!(
                *chance_per_10k <= 10_000
                    && *damage >= 0
                    && *full_radius_world >= 0
                    && *half_radius_world >= *full_radius_world
            );
            PassiveUnitEffect::Pulverize(crate::components::PulverizeEffectProfile {
                ability: AbilityId(rawcode(source_key).expect("Pulverize source")),
                chance_per_10k: *chance_per_10k,
                damage: *damage,
                full_radius: world(*full_radius_world),
                half_radius: world(*half_radius_world),
                targets: target_mask(targets),
            })
        }
        TuningEffect::FrostAttack {
            source_key,
            duration_millis,
            hero_duration_millis,
            movement_percent_delta,
            attack_speed_percent_delta,
            targets,
            ..
        } => PassiveUnitEffect::FrostAttack(FrostAttackEffectProfile {
            ability: AbilityId(rawcode(source_key).expect("validated Frost Attack rawcode")),
            duration_ticks: u16::try_from(
                (u64::from(*duration_millis) * CASTLE_FIGHT_SIMULATION_HZ as u64).div_ceil(1000),
            )
            .expect("Frost duration fits"),
            hero_duration_ticks: u16::try_from(
                (u64::from(*hero_duration_millis) * CASTLE_FIGHT_SIMULATION_HZ as u64)
                    .div_ceil(1000),
            )
            .expect("hero Frost duration fits"),
            movement_percent_delta: *movement_percent_delta,
            attack_speed_percent_delta: *attack_speed_percent_delta,
            targets: target_mask(targets),
        }),
        TuningEffect::Cleave {
            source_key,
            radius_world,
            damage_per_10k,
            targets,
            ..
        } => {
            assert!(*radius_world >= 0 && *damage_per_10k <= 10_000);
            PassiveUnitEffect::Cleave(CleaveEffectProfile {
                ability: AbilityId(rawcode(source_key).expect("validated cleave rawcode")),
                radius: world(*radius_world),
                damage_per_10k: *damage_per_10k,
                targets: *targets,
            })
        }

        TuningEffect::Feedback {
            source_key,
            maximum_mana_drained,
            damage_per_mana_per_10k,
            summoned_damage,
            targets,
            ..
        } => {
            assert!(*maximum_mana_drained >= 0 && *summoned_damage >= 0);
            PassiveUnitEffect::Feedback(FeedbackEffectProfile {
                ability: AbilityId(rawcode(source_key).expect("validated Feedback rawcode")),
                maximum_mana_drained: *maximum_mana_drained,
                damage_per_mana_per_10k: *damage_per_mana_per_10k,
                summoned_damage: *summoned_damage,
                targets: target_mask(targets),
            })
        }
        TuningEffect::SpellResistance {
            source_key,
            damage_taken_per_10k,
            ..
        } => {
            assert!(
                *damage_taken_per_10k <= 10_000,
                "spell damage taken exceeds 100%"
            );
            PassiveUnitEffect::SpellResistance(SpellResistanceEffectProfile {
                ability: AbilityId(
                    rawcode(source_key).expect("validated spell resistance rawcode"),
                ),
                damage_taken_per_10k: *damage_taken_per_10k,
            })
        }
        TuningEffect::CriticalStrike {
            source_key,
            chance_per_10k,
            damage_multiplier_per_10k,
            targets,
            ..
        } => {
            assert!(
                *chance_per_10k <= 10_000,
                "Critical Strike chance exceeds 100%"
            );
            assert!(
                *damage_multiplier_per_10k >= 10_000,
                "Critical Strike must not reduce damage"
            );
            PassiveUnitEffect::CriticalStrike(CriticalStrikeEffectProfile {
                ability: AbilityId(rawcode(source_key).expect("validated Critical Strike rawcode")),
                chance_per_10k: *chance_per_10k,
                damage_multiplier_per_10k: *damage_multiplier_per_10k,
                targets: target_mask(targets),
            })
        }
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
        TuningEffect::Defend {
            source_key,
            ranged_damage_taken_per_10k,
            spell_damage_taken_per_10k,
            deflect_chance_per_10k,
            deflected_pierce_damage_taken_per_10k,
            activation_delay_millis,
            ..
        } => {
            for (name, value) in [
                ("ranged damage taken", *ranged_damage_taken_per_10k),
                ("spell damage taken", *spell_damage_taken_per_10k),
                ("deflect chance", *deflect_chance_per_10k),
                (
                    "deflected Pierce damage taken",
                    *deflected_pierce_damage_taken_per_10k,
                ),
            ] {
                assert!(value <= 10_000, "Defend {name} exceeds 100%");
            }
            PassiveUnitEffect::Defend(DefendEffectProfile {
                ability: AbilityId(rawcode(source_key).expect("validated Defend rawcode")),
                ranged_damage_taken_per_10k: *ranged_damage_taken_per_10k,
                spell_damage_taken_per_10k: *spell_damage_taken_per_10k,
                deflect_chance_per_10k: *deflect_chance_per_10k,
                deflected_pierce_damage_taken_per_10k: *deflected_pierce_damage_taken_per_10k,
                activation_delay_ticks: exact_millis_to_ticks(
                    *activation_delay_millis,
                    "Defend activation delay",
                ),
            })
        }
        TuningEffect::Bash {
            source_key,
            chance_per_10k,
            bonus_damage,
            stun_duration_millis,
            hero_stun_duration_millis,
            targets,
            ..
        } => {
            assert!(*chance_per_10k <= 10_000, "Bash chance exceeds 100%");
            assert!(*bonus_damage >= 0, "Bash bonus damage must be non-negative");
            PassiveUnitEffect::Bash(BashEffectProfile {
                ability: AbilityId(rawcode(source_key).expect("validated Bash rawcode")),
                chance_per_10k: *chance_per_10k,
                bonus_damage: *bonus_damage,
                stun_duration_ticks: ceil_millis_to_ticks(*stun_duration_millis),
                hero_stun_duration_ticks: ceil_millis_to_ticks(*hero_stun_duration_millis),
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
                effect: build_triggered_effect(
                    child,
                    AbilityId(rawcode(source_key).expect("validated orb rawcode")),
                ),
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
        TuningEffect::NativeImpale { .. }
        | TuningEffect::NativeTargetDebuff { .. }
        | TuningEffect::NativeWeatherWave { .. }
        | TuningEffect::NativeEnchantmentGrant { .. }
        | TuningEffect::BuildingImprovement { .. }
        | TuningEffect::BuildingManaRegeneration { .. }
        | TuningEffect::ConeWave { .. }
        | TuningEffect::SelfDestruct { .. }
        | TuningEffect::ScriptedAutomatic { .. }
        | TuningEffect::ElvenAutomatic { .. }
        | TuningEffect::RetaliationProjectile { .. }
        | TuningEffect::ForkedLightning { .. }
        | TuningEffect::ChainLightning { .. }
        | TuningEffect::EntanglingRoots { .. }
        | TuningEffect::FrostArmor { .. }
        | TuningEffect::FaerieFire { .. } => {
            panic!("effect {} is not a unit passive", effect.kind_name())
        }
    }
}

fn build_triggered_effect(effect: &TuningEffect, parent: AbilityId) -> TriggeredAttackEffect {
    match effect {
        TuningEffect::NativeTargetDebuff { profile, .. } => TriggeredAttackEffect::NativeDebuff {
            parent,
            profile: *profile,
        },
        TuningEffect::ForkedLightning { profile, .. } => {
            TriggeredAttackEffect::ForkedLightning(*profile)
        }
        TuningEffect::ConeWave { profile, .. } => TriggeredAttackEffect::ConeWave(*profile),
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
            hero_duration_millis,
            nonhero_only,
            targets,
            ..
        } => TriggeredAttackEffect::EntanglingRoots(EntanglingRootsEffectProfile {
            ability: AbilityId(rawcode(source_key).expect("validated Entangling Roots rawcode")),
            damage_per_second: *damage_per_second,
            duration_ticks: exact_millis_to_ticks(*duration_millis, "Entangling Roots duration"),
            hero_duration_ticks: u16::try_from(
                (u64::from(*hero_duration_millis) * CASTLE_FIGHT_SIMULATION_HZ as u64)
                    .div_ceil(1000),
            )
            .expect("root hero duration fits"),
            nonhero_only: *nonhero_only,
            targets: target_mask(targets),
        }),
        _ => panic!("{} cannot be used as an orb effect", effect.kind_name()),
    }
}

fn build_spellcasting(tuning: &TuningFile, effect: &TuningEffect) -> SpellcastingProfile {
    if let TuningEffect::ScriptedAutomatic {
        spellcasting,
        mana_regen_per_second_per_10k,
        ..
    }
    | TuningEffect::ElvenAutomatic {
        spellcasting,
        mana_regen_per_second_per_10k,
        ..
    } = effect
    {
        assert!(*mana_regen_per_second_per_10k <= 10_000_000);
        assert_eq!(
            spellcasting.mana.regen_per_tick_per_10k,
            (1 << 31) | *mana_regen_per_second_per_10k,
            "generated automatic profile must retain its exact per-second rate unit"
        );
        if let crate::AbilityEffect::BuildingWeather(p) = spellcasting.ability.effect {
            for expected in p.options.into_iter().flatten() {
                assert!(tuning.effects.iter().any(|child| matches!(child, TuningEffect::NativeWeatherWave { profile, .. } if *profile == expected)));
            }
        }
        if let crate::AbilityEffect::ImpaleFan(p) = spellcasting.ability.effect {
            assert!(
                tuning.effects.iter().any(|child| matches!(child,
                TuningEffect::NativeImpale { profile, .. } if *profile == p.child)),
                "generated Impale fan must match its retained native child"
            );
        }
        if let crate::AbilityEffect::BuildingEnchantment(p) = spellcasting.ability.effect {
            for expected in p.options {
                assert!(
                    tuning.effects.iter().any(|child| matches!(child,
                    TuningEffect::NativeEnchantmentGrant { grant, .. } if *grant == expected)),
                    "generated enchantment option must match its retained native child"
                );
            }
        }
        let mut profile = *spellcasting;
        profile.mana = ManaProfile::per_second(
            profile.mana.maximum,
            profile.mana.starting,
            *mana_regen_per_second_per_10k,
        );
        return profile;
    }
    if let TuningEffect::FaerieFire {
        source_key,
        mana_maximum,
        mana_starting,
        mana_regen_per_second_per_10k,
        mana_cost,
        cooldown_millis,
        range_world,
        armor_reduction_per_100,
        duration_millis,
        hero_duration_millis,
        always_autocast,
        ..
    } = effect
    {
        let id = rawcode(source_key).expect("validated Faerie Fire rawcode");
        return SpellcastingProfile {
            mana: ManaProfile::per_second(
                *mana_maximum,
                *mana_starting,
                *mana_regen_per_second_per_10k,
            ),
            ability: crate::components::AutomaticAbilityProfile {
                id: AbilityId(id),
                mana_cost: *mana_cost,
                cooldown_ticks: exact_millis_to_ticks(*cooldown_millis, "Faerie Fire cooldown"),
                range: world(*range_world),
                target_policy: if *always_autocast {
                    AbilityTargetPolicy::RandomEnemyDebuff
                } else {
                    AbilityTargetPolicy::NearestEnemyInCombat
                },
                effect: AbilityEffect::FaerieFire {
                    native_buff: native_buff_from_tuning(tuning, id),
                    modifier: ModifierId(id),
                    armor_reduction_per_100: *armor_reduction_per_100,
                    duration_ticks: exact_millis_to_ticks(*duration_millis, "Faerie Fire duration"),
                    hero_duration_ticks: exact_millis_to_ticks(
                        *hero_duration_millis,
                        "Faerie Fire hero duration",
                    ),
                },
            },
        };
    }
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
    SpellcastingProfile {
        mana: ManaProfile::per_second(
            *mana_maximum,
            *mana_starting,
            *mana_regen_per_second_per_10k,
        ),
        ability: crate::components::AutomaticAbilityProfile {
            id: AbilityId(rawcode(source_key).expect("validated Frost Armor rawcode")),
            mana_cost: *mana_cost,
            cooldown_ticks: exact_millis_to_ticks(*cooldown_millis, "Frost Armor cooldown"),
            range: world(*range_world),
            target_policy: AbilityTargetPolicy::RecentlyAttackedFriendlyUnit,
            effect: AbilityEffect::FrostArmor {
                native_buff: native_buff_from_tuning(
                    tuning,
                    rawcode(source_key).expect("Frost Armor source"),
                ),
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
        "air-units" => AttackTargetMask::AIR_UNITS,
        "air-ground-units" => AttackTargetMask::AIR_AND_GROUND,
        "buildings" => AttackTargetMask::BUILDINGS,
        "air-ground-units-and-buildings" => AttackTargetMask::ALL,
        "ground-units-and-buildings" => AttackTargetMask::GROUND_AND_BUILDINGS,
        other => panic!("unsupported native-effect target mask {other}"),
    }
}

/// Native durations may lie between fixed ticks. Expire on the first tick at or
/// after the authored deadline, never earlier (e.g. a quarter second at 30 Hz).
fn ceil_millis_to_ticks(millis: u32) -> u16 {
    let numerator = u64::from(millis)
        * u64::try_from(CASTLE_FIGHT_SIMULATION_HZ).expect("positive simulation Hz");
    u16::try_from(numerator.div_ceil(1_000)).expect("duration exceeds u16 tick range")
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

fn display_rawcode(value: u32) -> String {
    let bytes = value.to_be_bytes();
    if bytes.iter().all(u8::is_ascii_graphic) {
        String::from_utf8_lossy(&bytes).into_owned()
    } else {
        format!("0x{value:08x}")
    }
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

/// Intrinsic ANlm split state is installed by the native summoner, not an ability in the body's inventory.
pub(crate) fn master_projectile_profile_for_version(
    version: MapVersion,
) -> &'static crate::MasterProjectileProfile {
    catalog()
        .tuning(version)
        .expect("registered Master version")
        .effects
        .iter()
        .find_map(|effect| {
            if let TuningEffect::ScriptedAutomatic {
                master_profile: Some(profile),
                ..
            } = effect
            {
                Some(profile.as_ref())
            } else {
                None
            }
        })
        .expect("registered Master source tuning")
}

pub(crate) fn uses_damage_charge_mana_for_version(version: MapVersion, body_rawcode: u32) -> bool {
    catalog().tuning(version).is_some_and(|tuning| tuning.effects.iter().any(|effect| matches!(effect, TuningEffect::DamageCharge { unit_rawcode, .. } if rawcode(unit_rawcode).ok() == Some(body_rawcode))))
}

pub(crate) fn lava_spawn_profile_for_version(
    version: MapVersion,
    body_rawcode: u32,
) -> Option<crate::components::LavaSpawnProfile> {
    catalog()
        .tuning(version)?
        .effects
        .iter()
        .find_map(|effect| match effect {
            TuningEffect::LavaSpawn { profile, .. } if profile.body_rawcode == body_rawcode => {
                Some(*profile)
            }
            _ => None,
        })
}

/// Manual building abilities are separate from automatic combat-unit casting.
pub fn building_improvement_for_version(
    version: MapVersion,
    rawcode: u32,
) -> Option<crate::building_mechanics::BuildingImprovementProfile> {
    catalog()
        .tuning(version)?
        .effects
        .iter()
        .find_map(|effect| {
            if let TuningEffect::BuildingImprovement { profile, .. } = effect {
                if profile.source_rawcode != rawcode {
                    return None;
                }
                let levels = catalog()
                    .tuning(version)?
                    .effects
                    .iter()
                    .find_map(|effect| {
                        if let TuningEffect::BuildingManaRegeneration {
                            source_key, levels, ..
                        } = effect
                        {
                            (u32::from_be_bytes(source_key.as_bytes().try_into().ok()?)
                                == profile.improvement.0)
                                .then_some(levels)
                        } else {
                            None
                        }
                    })
                    .expect("validated mana improvement dependency");
                assert_eq!(*levels, profile.regeneration_per_second_per_10k);
                Some(*profile)
            } else {
                None
            }
        })
}

pub(crate) fn aftershock_for_version(
    version: MapVersion,
    body: u32,
) -> Option<crate::HealthScaledAftershockProfile> {
    catalog()
        .tuning(version)?
        .effects
        .iter()
        .find_map(|e| match e {
            TuningEffect::HealthScaledAftershock {
                unit_rawcode,
                profile,
                ..
            } if rawcode(unit_rawcode).ok() == Some(body) => Some(*profile),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(kind: NativeEffectSourceKind, key: &str) -> NativeEffectSource {
        NativeEffectSource::new(kind, rawcode(key).unwrap())
    }

    #[test]
    fn resolver_is_root_order_independent_and_includes_indirect_effects() {
        let first = resolve_native_effect_requirements(
            MapVersion::CASTLE_FIGHT_9_27,
            &[
                source(NativeEffectSourceKind::UnitAbility, "A01B"),
                source(NativeEffectSourceKind::UnitAbility, "A049"),
            ],
        )
        .unwrap();
        let second = resolve_native_effect_requirements(
            MapVersion::CASTLE_FIGHT_9_27,
            &[
                source(NativeEffectSourceKind::UnitAbility, "A049"),
                source(NativeEffectSourceKind::UnitAbility, "A01B"),
            ],
        )
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(
            first
                .iter()
                .map(|binding| binding.source)
                .collect::<Vec<_>>(),
            vec![
                source(NativeEffectSourceKind::UnitAbility, "A01B"),
                source(NativeEffectSourceKind::UnitAbility, "A049"),
                source(NativeEffectSourceKind::AbilityEffect, "A03W"),
                source(NativeEffectSourceKind::AbilityEffect, "A05X"),
            ]
        );
    }

    #[test]
    fn resolver_does_not_depend_on_binding_registry_order() {
        let roots = [
            source(NativeEffectSourceKind::UnitAbility, "A01B"),
            source(NativeEffectSourceKind::UnitAbility, "A049"),
            source(NativeEffectSourceKind::UnitAbility, "A03Z"),
        ];
        let original = NativeEffectCatalog::load().unwrap();
        let expected = original
            .resolve_requirements(MapVersion::CASTLE_FIGHT_9_27, &roots)
            .unwrap();
        let mut reversed = NativeEffectCatalog::load().unwrap();
        reversed.bindings.reverse();
        assert_eq!(
            reversed
                .resolve_requirements(MapVersion::CASTLE_FIGHT_9_27, &roots)
                .unwrap(),
            expected
        );
    }

    #[test]
    fn resolver_rejects_missing_indirect_effect_binding() {
        let mut test_catalog = NativeEffectCatalog::load().unwrap();
        let child = source(NativeEffectSourceKind::AbilityEffect, "A05X");
        test_catalog
            .bindings
            .retain(|binding| binding.source != child);

        assert_eq!(
            test_catalog.resolve_requirements(
                MapVersion::CASTLE_FIGHT_9_27,
                &[source(NativeEffectSourceKind::UnitAbility, "A01B")],
            ),
            Err(NativeEffectResolveError::MissingBinding(child))
        );
    }

    #[test]
    fn resolver_accepts_explicit_no_runtime_marker_without_tuning() {
        let marker = source(NativeEffectSourceKind::UnitAbility, "A0CV");
        let resolved =
            resolve_native_effect_requirements(MapVersion::CASTLE_FIGHT_9_27, &[marker]).unwrap();
        assert_eq!(
            resolved,
            vec![ResolvedNativeEffectBinding {
                source: marker,
                implementation: NativeEffectImplementationId::WarcraftMarkerOnlyV1,
            }]
        );
    }

    #[test]
    fn resolver_rejects_ambiguous_binding() {
        let mut test_catalog = NativeEffectCatalog::load().unwrap();
        let evasion = source(NativeEffectSourceKind::UnitAbility, "A00U");
        let duplicate = test_catalog
            .bindings
            .iter()
            .find(|binding| binding.source == evasion)
            .unwrap()
            .clone();
        test_catalog.bindings.push(duplicate);

        assert_eq!(
            test_catalog.resolve_requirements(MapVersion::CASTLE_FIGHT_9_27, &[evasion]),
            Err(NativeEffectResolveError::AmbiguousBinding(evasion))
        );
    }

    #[test]
    fn resolver_rejects_dependency_cycles() {
        let cyclic = source(NativeEffectSourceKind::AbilityEffect, "A0ZZ");
        let test_catalog = NativeEffectCatalog {
            bindings: vec![OwnedBinding {
                source: cyclic,
                implementation: NativeEffectImplementationId::WarcraftOrbSpellProcV1,
                valid_versions: MapVersionRange::exactly(MapVersion::CASTLE_FIGHT_9_27),
            }],
            tuning_9_27: TuningFile {
                schema_version: 3,
                buff_families: Vec::new(),
                unit_death_rules: Vec::new(),
                map_version: "9.27".to_owned(),
                effects: vec![TuningEffect::OrbSpellProc {
                    source_kind: "ability-effect".to_owned(),
                    source_key: "A0ZZ".to_owned(),
                    unit_rawcode: "hfoo".to_owned(),
                    chance_per_10k: 1_000,
                    effect_ability_rawcode: "A0ZZ".to_owned(),
                    targets: "air-ground-units".to_owned(),
                    provenance: serde_json::Value::Null,
                }],
            },
        };

        assert_eq!(
            test_catalog.resolve_requirements(MapVersion::CASTLE_FIGHT_9_27, &[cyclic]),
            Err(NativeEffectResolveError::DependencyCycle(cyclic))
        );
    }

    #[test]
    fn unit_mechanics_do_not_depend_on_tuning_file_order() {
        let tuning = catalog().tuning_9_27.clone();
        let mut reversed = tuning.clone();
        reversed.effects.reverse();

        for (unit_rawcode, abilities) in [
            (
                u32::from_be_bytes(*b"e003"),
                vec![
                    u32::from_be_bytes(*b"A0CV"),
                    u32::from_be_bytes(*b"A03N"),
                    u32::from_be_bytes(*b"A00U"),
                ],
            ),
            (
                u32::from_be_bytes(*b"h03A"),
                vec![u32::from_be_bytes(*b"A00U"), u32::from_be_bytes(*b"A03G")],
            ),
            (
                u32::from_be_bytes(*b"o001"),
                vec![u32::from_be_bytes(*b"A0CV"), u32::from_be_bytes(*b"A02J")],
            ),
            (
                u32::from_be_bytes(*b"n015"),
                vec![
                    u32::from_be_bytes(*b"A0CV"),
                    u32::from_be_bytes(*b"A049"),
                    u32::from_be_bytes(*b"A03Z"),
                ],
            ),
            (
                u32::from_be_bytes(*b"h016"),
                vec![u32::from_be_bytes(*b"A05K"), u32::from_be_bytes(*b"A01B")],
            ),
        ] {
            assert_eq!(
                native_unit_mechanics_from_tuning(&tuning, unit_rawcode, &abilities),
                native_unit_mechanics_from_tuning(&reversed, unit_rawcode, &abilities),
            );
        }
    }

    #[test]
    fn native_importer_collects_multiple_translated_abilities_and_rejects_mana_conflicts() {
        let frost = |key: &str, maximum| TuningEffect::FrostArmor {
            source_kind: "unit-ability".to_owned(),
            source_key: key.to_owned(),
            unit_rawcode: "TEST".to_owned(),
            mana_maximum: maximum,
            mana_starting: 80,
            mana_regen_per_second_per_10k: 3000,
            mana_cost: 4,
            cooldown_millis: 1000,
            range_world: 80,
            armor_bonus_per_100: 100,
            armor_duration_millis: 1000,
            slow_duration_millis: 1000,
            movement_percent_delta: -10,
            attack_speed_percent_delta: -10,
            provenance: serde_json::Value::Null,
        };
        let unit = u32::from_be_bytes(*b"TEST");
        let abilities = [u32::from_be_bytes(*b"F002"), u32::from_be_bytes(*b"F001")];
        let mut tuning = TuningFile {
            schema_version: 3,
            buff_families: Vec::new(),
            unit_death_rules: Vec::new(),
            map_version: "9.27".to_owned(),
            effects: vec![frost("F002", 100), frost("F001", 100)],
        };
        let expected = native_unit_mechanics_from_tuning(&tuning, unit, &abilities).unwrap();
        assert_eq!(
            expected.spellcasting.unwrap().ability.id,
            AbilityId(abilities[1])
        );
        assert_eq!(
            expected
                .additional_abilities
                .unwrap()
                .iter()
                .map(|profile| profile.id)
                .collect::<Vec<_>>(),
            [AbilityId(abilities[0])]
        );
        tuning.effects.reverse();
        assert_eq!(
            native_unit_mechanics_from_tuning(&tuning, unit, &[abilities[1], abilities[0]])
                .unwrap(),
            expected
        );
        tuning.effects = vec![frost("F001", 100), frost("F002", 200)];
        assert_eq!(
            native_unit_mechanics_from_tuning(&tuning, unit, &abilities),
            Err(NativeEffectCatalogError::InvalidAutomaticAbilities {
                unit,
                reason: AbilityConfigurationError::IncompatibleManaProfile(AbilityId(abilities[0]))
            })
        );
    }

    #[test]
    fn shared_ability_rawcode_reuses_verified_tuning_for_new_unit_users() {
        let mechanics = native_unit_mechanics_for(
            MapVersion::CASTLE_FIGHT_9_27,
            u32::from_be_bytes(*b"zzzz"),
            &[u32::from_be_bytes(*b"A00U")],
        )
        .unwrap();
        assert_eq!(
            mechanics.passive_effects.iter().collect::<Vec<_>>(),
            vec![PassiveUnitEffect::Evasion(EvasionEffectProfile {
                ability: AbilityId(u32::from_be_bytes(*b"A00U")),
                chance_per_10k: 1_500,
            })]
        );
    }

    #[test]
    fn current_slice_bindings_are_version_scoped() {
        for (kind, key) in [
            ("unit-ability", "A0CV"),
            ("unit-ability", "A03N"),
            ("unit-ability", "A00U"),
            ("unit-ability", "A03G"),
            ("unit-ability", "A05K"),
            ("unit-ability", "A01B"),
            ("ability-effect", "A05X"),
            ("unit-ability", "A049"),
            ("ability-effect", "A03W"),
            ("unit-ability", "A02J"),
            ("unit-ability", "A03Z"),
            ("unit-ability", "A09A"),
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
        let ranger = native_unit_mechanics_for(
            MapVersion::CASTLE_FIGHT_9_27,
            u32::from_be_bytes(*b"e003"),
            &[
                u32::from_be_bytes(*b"A0CV"),
                u32::from_be_bytes(*b"A03N"),
                u32::from_be_bytes(*b"A00U"),
            ],
        )
        .unwrap();
        assert!(matches!(
            ranger.passive_effects.iter().next(),
            Some(PassiveUnitEffect::Evasion(EvasionEffectProfile {
                chance_per_10k: 1_500,
                ..
            }))
        ));

        let defender = native_unit_mechanics_for(
            MapVersion::CASTLE_FIGHT_9_27,
            u32::from_be_bytes(*b"h03A"),
            &[u32::from_be_bytes(*b"A00U"), u32::from_be_bytes(*b"A03G")],
        )
        .unwrap();
        let defender_effects = defender.passive_effects.iter().collect::<Vec<_>>();
        assert!(defender_effects.iter().any(|effect| matches!(
            effect,
            PassiveUnitEffect::Evasion(EvasionEffectProfile {
                chance_per_10k: 1_500,
                ..
            })
        )));
        assert!(defender_effects.iter().any(|effect| matches!(
            effect,
            PassiveUnitEffect::Defend(DefendEffectProfile {
                ranged_damage_taken_per_10k: 4_000,
                spell_damage_taken_per_10k: 5_000,
                deflect_chance_per_10k: 5_000,
                deflected_pierce_damage_taken_per_10k: 0,
                activation_delay_ticks: 21,
                ..
            })
        )));

        let troll = native_unit_mechanics_for(
            MapVersion::CASTLE_FIGHT_9_27,
            u32::from_be_bytes(*b"n015"),
            &[
                u32::from_be_bytes(*b"A0CV"),
                u32::from_be_bytes(*b"A049"),
                u32::from_be_bytes(*b"A03Z"),
            ],
        )
        .unwrap();
        let spellcasting = troll.spellcasting.expect("Ice Troll must cast Frost Armor");
        assert_eq!(spellcasting.mana.maximum, 250);
        assert_eq!(spellcasting.mana.starting, 150);
        let TuningEffect::FrostArmor {
            mana_regen_per_second_per_10k,
            ..
        } = catalog()
            .tuning_9_27
            .effects
            .iter()
            .find(|effect| effect.source_key() == "A03Z")
            .unwrap()
        else {
            panic!("source Frost Armor tuning")
        };
        assert_eq!(
            (0..CASTLE_FIGHT_SIMULATION_HZ as u64)
                .map(|tick| spellcasting
                    .mana
                    .regeneration_at_tick(tick, CASTLE_FIGHT_SIMULATION_HZ as u32))
                .sum::<u32>(),
            *mana_regen_per_second_per_10k
        );
        assert_eq!(spellcasting.ability.mana_cost, 35);
        assert_eq!(spellcasting.ability.cooldown_ticks, 240);
    }
}

/// Audited native ancestry binding; this flag does not make the handle an actual hero.
pub(crate) fn resistant_skin_for_version(version: MapVersion, abilities: &[u32]) -> bool {
    abilities.iter().any(|ability| {
        catalog().bindings.iter().any(|binding| {
            binding.source == NativeEffectSource::new(NativeEffectSourceKind::UnitAbility, *ability)
                && binding.valid_versions.contains(version)
                && binding.implementation == NativeEffectImplementationId::WarcraftResistantSkinV1
        })
    })
}

#[derive(Debug, Clone, Copy, Default, Deserialize)]
pub(crate) struct NativeUnitDeathRule {
    pub rawcode: u32,
    pub heal_percent: u16,
    pub retaliation_damage: i32,
}
pub(crate) fn unit_death_rule_for_version(
    version: MapVersion,
    rawcode: u32,
) -> Option<NativeUnitDeathRule> {
    catalog()
        .tuning(version)?
        .unit_death_rules
        .iter()
        .find(|r| r.rawcode == rawcode)
        .copied()
}

pub(crate) fn unit_death_rules_for_version(
    version: MapVersion,
) -> impl Iterator<Item = NativeUnitDeathRule> {
    catalog()
        .tuning(version)
        .into_iter()
        .flat_map(|t| t.unit_death_rules.iter().copied())
}

/// Aspy is consumed at the authoritative death boundary; it has no active resource
/// clock or per-handle counter. The inventory and versioned tuning remain canonical.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeDeathSpawn {
    pub ability: AbilityId,
    pub child_rawcode: u32,
    pub count: u8,
}

pub(crate) fn death_spawns_for_version(
    version: MapVersion,
) -> impl Iterator<Item = NativeDeathSpawn> {
    catalog()
        .tuning(version)
        .into_iter()
        .flat_map(move |tuning| {
            tuning
                .effects
                .iter()
                .filter_map(move |effect| match effect {
                    TuningEffect::DeathSpawn {
                        source_key,
                        child_rawcode,
                        count,
                        ..
                    } => {
                        assert!(*count > 0, "native death spawn needs a positive count");
                        Some(NativeDeathSpawn {
                            ability: AbilityId(
                                rawcode(source_key).expect("validated death-spawn source"),
                            ),
                            child_rawcode: *child_rawcode,
                            count: *count,
                        })
                    }
                    _ => None,
                })
        })
}
