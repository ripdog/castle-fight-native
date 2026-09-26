use std::{
    collections::{BTreeMap, BTreeSet},
    fmt,
    sync::OnceLock,
};

use serde::Deserialize;

use crate::{
    components::{
        AbilityEffect, AbilityId, AbilityTargetPolicy, AttackTargetMask, BashEffectProfile,
        BurningOilEffectProfile, ChainLightningEffectProfile, CriticalStrikeEffectProfile,
        DefendEffectProfile, EntanglingRootsEffectProfile, EvasionEffectProfile, ManaProfile,
        ModifierId, PassiveUnitEffect, PassiveUnitEffects, SpellcastingProfile,
        TriggeredAttackEffect, TriggeredSpellProcProfile,
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
    WarcraftHumanSupportV1,
    WarcraftHumanPassiveV1,
}

impl NativeEffectImplementationId {
    #[must_use]
    pub const fn stable_tag(self) -> u8 {
        match self {
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
            Self::WarcraftHumanSupportV1 => 11,
            Self::WarcraftHumanPassiveV1 => 12,
        }
    }

    const fn requires_tuning(self) -> bool {
        !matches!(
            self,
            Self::WarcraftMarkerOnlyV1
                | Self::WarcraftZeroDamageBarrageV1
                | Self::WarcraftHumanSupportV1
                | Self::WarcraftHumanPassiveV1
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
            effect.dependency_source()?;
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
            if let Some(dependency) = effect
                .dependency_source()
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
    effects: Vec<TuningEffect>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum TuningEffect {
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
    fn source(&self) -> Result<NativeEffectSource, String> {
        let kind = NativeEffectSourceKind::parse(self.source_kind()).ok_or_else(|| {
            format!(
                "unsupported native-effect source kind {:?}",
                self.source_kind()
            )
        })?;
        Ok(NativeEffectSource::new(kind, rawcode(self.source_key())?))
    }

    fn dependency_source(&self) -> Result<Option<NativeEffectSource>, String> {
        let Self::OrbSpellProc {
            effect_ability_rawcode,
            ..
        } = self
        else {
            return Ok(None);
        };
        Ok(Some(NativeEffectSource::new(
            NativeEffectSourceKind::AbilityEffect,
            rawcode(effect_ability_rawcode)?,
        )))
    }

    fn source_kind(&self) -> &str {
        match self {
            Self::Evasion { source_kind, .. }
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
            Self::Evasion { source_key, .. }
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
            Self::Evasion { unit_rawcode, .. }
            | Self::CriticalStrike { unit_rawcode, .. }
            | Self::Defend { unit_rawcode, .. }
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
            Self::CriticalStrike { .. } => NativeEffectImplementationId::WarcraftCriticalStrikeV1,
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
            Self::Evasion { .. } => "evasion",
            Self::CriticalStrike { .. } => "critical-strike",
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
    catalog().resolve_requirements(version, roots)
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
    Ok(native_unit_mechanics_from_tuning(
        tuning,
        unit_rawcode,
        ability_rawcodes,
    ))
}

fn native_unit_mechanics_from_tuning(
    tuning: &TuningFile,
    unit_rawcode: u32,
    ability_rawcodes: &[u32],
) -> NativeUnitMechanics {
    let abilities = ability_rawcodes.iter().copied().collect::<BTreeSet<_>>();
    let mut passive_effects = Vec::new();
    let mut spellcasting = None;

    for ability in abilities {
        let source = NativeEffectSource::new(NativeEffectSourceKind::UnitAbility, ability);
        let mut passive = None;
        let mut automatic_spell = None;
        for effect in tuning
            .effects
            .iter()
            .filter(|effect| effect.source().ok() == Some(source))
        {
            match effect {
                TuningEffect::FrostArmor { .. } => {
                    let candidate = build_spellcasting(effect);
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
                TuningEffect::Evasion { .. }
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
                TuningEffect::ChainLightning { .. } | TuningEffect::EntanglingRoots { .. } => {
                    unreachable!("ability-effect tuning cannot match a unit-ability source")
                }
            }
        }

        if let Some(effect) = passive {
            passive_effects.push(effect);
        }
        if let Some(candidate) = automatic_spell {
            assert!(
                spellcasting.replace(candidate).is_none(),
                "unit {} has more than one automatic spell in the current native primitive",
                display_rawcode(unit_rawcode)
            );
        }
    }

    NativeUnitMechanics {
        passive_effects: PassiveUnitEffects::from_slice(&passive_effects),
        spellcasting,
    }
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
        "air-units" => AttackTargetMask::AIR_UNITS,
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
                schema_version: 2,
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
        assert_eq!(spellcasting.mana.regen_per_tick_per_10k, 500);
        assert_eq!(spellcasting.ability.mana_cost, 35);
        assert_eq!(spellcasting.ability.cooldown_ticks, 240);
    }
}
