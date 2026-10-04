use bevy_ecs::prelude::Component;

mod native_actions;
pub use native_actions::{HealingWaveProfile, NativeBoltProfile};
pub(crate) use native_actions::{HealingWaveState, NativeAction, NativeBoltState};
mod automatic_abilities;
pub(crate) use automatic_abilities::compose_spellcasting_profiles;
pub use automatic_abilities::{
    AbilityConfigurationError, AdditionalAutomaticAbilities, AdditionalAutomaticAbilityDefinitions,
    AutomaticAbilityInstance, MAX_AUTOMATIC_ABILITIES, SecondaryResurrectionState,
};
use serde::{Deserialize, Serialize};

use crate::{
    damage::{ArmorProfile, DamageType},
    economy::BuildingEconomyProfile,
    math::SimPoint,
};

pub(crate) const MAX_BOUNCE_COUNT: u8 = 8;
pub(crate) const MAX_BOUNCE_HITS: usize = MAX_BOUNCE_COUNT as usize + 1;
pub(crate) const MAX_TIMED_MOVEMENT_MODIFIERS: usize = 8;

#[derive(
    Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct SimId(pub u64);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Team(pub u8);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PlayerId(pub u8);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Owner(pub PlayerId);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position(pub SimPoint);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    pub current: i32,
    pub max: i32,
}

#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct HealthRegeneration {
    /// Hit points regenerated per second in 1/10,000 HP units.
    pub per_second_per_10k: u32,
    /// Fixed-point numerator retained across simulation ticks. The denominator is
    /// `10_000 * CASTLE_FIGHT_SIMULATION_HZ`.
    pub remainder_per_10k_hz: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AttackDelivery {
    Melee,
    RangedInstant,
    RangedGuaranteedHit {
        speed_per_tick: i32,
    },
    RangedBallistic {
        speed_per_tick: i32,
        impact_radius: i32,
    },
    Bounce {
        speed_per_tick: i32,
        bounce_range: i32,
        max_bounces: u8,
        damage_percent_per_bounce: u16,
        allow_repeat_targets: bool,
    },
}

impl AttackDelivery {
    #[must_use]
    pub const fn stable_tag(self) -> u8 {
        match self {
            Self::Melee => 0,
            Self::RangedInstant => 4,
            Self::RangedGuaranteedHit { .. } => 1,
            Self::RangedBallistic { .. } => 2,
            Self::Bounce { .. } => 3,
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttackProfile {
    pub delivery: AttackDelivery,
    pub damage: i32,
    pub range: i32,
    pub acquisition_range: i32,
    pub cooldown_ticks: u16,
}

impl AttackProfile {
    #[must_use]
    pub fn range_sq(self) -> u64 {
        let range = i64::from(self.range);
        (range * range) as u64
    }

    #[must_use]
    pub fn acquisition_range_sq(self) -> u64 {
        let range = i64::from(self.acquisition_range);
        (range * range) as u64
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecondaryAttackProfile {
    pub primary_targets: AttackTargetMask,
    pub attack: AttackProfile,
    pub targets: AttackTargetMask,
    pub damage_type: DamageType,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PendingAttackEffects {
    pub stun_duration_ticks: u16,
    pub triggered_spell: Option<TriggeredAttackEffect>,
    pub burning_oil: Option<BurningOilEffectProfile>,
    pub splash_falloff: Option<SplashFalloffProfile>,
    pub feedback: Option<FeedbackEffectProfile>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct GuaranteedHitProjectile {
    pub source: SimId,
    pub source_team: Team,
    pub source_is_building: bool,
    pub target: SimId,
    pub damage: i32,
    pub on_hit: PendingAttackEffects,
    pub damage_type: DamageType,
    pub speed_per_tick: i32,
    pub launch_position: SimPoint,
    pub launch_tick: u64,
    pub impact_tick: u64,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ReflectedProjectile {
    /// Original attacker. Kept as the visual source so the reflected missile retains its art.
    pub original_source: SimId,
    /// Unit whose Defend state reflected the projectile and owns the returned damage.
    pub reflector: SimId,
    pub reflector_team: Team,
    pub target: SimId,
    pub damage: i32,
    pub damage_type: DamageType,
    pub launch_position: SimPoint,
    pub launch_tick: u64,
    pub impact_tick: u64,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BallisticProjectile {
    pub source: SimId,
    pub source_team: Team,
    pub target_mask: AttackTargetMask,
    pub damage: i32,
    pub burning_oil: Option<BurningOilEffectProfile>,
    pub splash_falloff: Option<SplashFalloffProfile>,
    pub damage_type: DamageType,
    pub launch_position: SimPoint,
    pub destination: SimPoint,
    pub impact_radius: i32,
    pub launch_tick: u64,
    pub impact_tick: u64,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BounceProjectile {
    pub source: SimId,
    pub source_team: Team,
    pub source_is_building: bool,
    pub target_mask: AttackTargetMask,
    pub target: SimId,
    pub damage: i32,
    pub damage_type: DamageType,
    pub launch_position: SimPoint,
    pub launch_tick: u64,
    pub impact_tick: u64,
    pub speed_per_tick: i32,
    pub bounce_range: i32,
    pub remaining_bounces: u8,
    pub bounce_index: u8,
    pub damage_percent_per_bounce: u16,
    pub allow_repeat_targets: bool,
    pub hit_targets: [SimId; MAX_BOUNCE_HITS],
    pub hit_count: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct AbilityId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CorpseDefinitionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorpseProfile {
    pub definition: CorpseDefinitionId,
    pub decay_start_ticks: u32,
    pub lifetime_ticks: Option<u32>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CorpseProducer(pub CorpseProfile);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionCorpseProfile(pub CorpseProfile);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CollisionRadius(pub i32);

#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MovementClass {
    #[default]
    Ground,
    Air,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AttackTargetMask(u8);

impl AttackTargetMask {
    const GROUND_UNIT_BIT: u8 = 1 << 0;
    const AIR_UNIT_BIT: u8 = 1 << 1;
    const BUILDING_BIT: u8 = 1 << 2;

    pub const GROUND_UNITS: Self = Self(Self::GROUND_UNIT_BIT);
    pub const AIR_UNITS: Self = Self(Self::AIR_UNIT_BIT);
    pub const BUILDINGS: Self = Self(Self::BUILDING_BIT);
    pub const GROUND_AND_BUILDINGS: Self = Self(Self::GROUND_UNIT_BIT | Self::BUILDING_BIT);
    pub const AIR_AND_GROUND: Self = Self(Self::GROUND_UNIT_BIT | Self::AIR_UNIT_BIT);
    pub const ALL: Self = Self(Self::GROUND_UNIT_BIT | Self::AIR_UNIT_BIT | Self::BUILDING_BIT);

    #[must_use]
    pub const fn from_capabilities(ground_units: bool, air_units: bool, buildings: bool) -> Self {
        Self(
            ((ground_units as u8) * Self::GROUND_UNIT_BIT)
                | ((air_units as u8) * Self::AIR_UNIT_BIT)
                | ((buildings as u8) * Self::BUILDING_BIT),
        )
    }

    #[must_use]
    pub const fn can_target_unit(self, movement_class: MovementClass) -> bool {
        let bit = match movement_class {
            MovementClass::Ground => Self::GROUND_UNIT_BIT,
            MovementClass::Air => Self::AIR_UNIT_BIT,
        };
        self.0 & bit != 0
    }

    #[must_use]
    pub const fn can_target_buildings(self) -> bool {
        self.0 & Self::BUILDING_BIT != 0
    }

    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    #[must_use]
    pub const fn union(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

impl Default for AttackTargetMask {
    fn default() -> Self {
        Self::ALL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GameplayBundleIdentity {
    pub schema_version: u32,
    pub gameplay_hash: u64,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContentIdentity {
    pub rawcode: u32,
    #[serde(skip, default)]
    pub name: &'static str,
}

pub const MAX_PASSIVE_UNIT_EFFECTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BashEffectProfile {
    pub ability: AbilityId,
    pub chance_per_10k: u16,
    pub bonus_damage: i32,
    pub stun_duration_ticks: u16,
    pub targets: AttackTargetMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvasionEffectProfile {
    pub ability: AbilityId,
    pub chance_per_10k: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DefendEffectProfile {
    pub ability: AbilityId,
    /// Fraction of ordinary ranged attack damage retained while Defend is active.
    pub ranged_damage_taken_per_10k: u16,
    /// Fraction of spell damage retained while Defend is active.
    pub spell_damage_taken_per_10k: u16,
    /// Chance for a directed Pierce projectile to be deflected.
    pub deflect_chance_per_10k: u16,
    /// Fraction of Pierce damage retained by the defender on a successful deflection.
    pub deflected_pierce_damage_taken_per_10k: u16,
    /// Castle Fight waits briefly after spawn before ordering Defend on.
    pub activation_delay_ticks: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChainLightningEffectProfile {
    pub ability: AbilityId,
    pub initial_damage: i32,
    pub maximum_targets: u8,
    pub jump_radius: i32,
    pub damage_reduction_per_10k: u16,
    pub targets: AttackTargetMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntanglingRootsEffectProfile {
    pub ability: AbilityId,
    pub damage_per_second: i32,
    pub duration_ticks: u16,
    pub targets: AttackTargetMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggeredAttackEffect {
    ChainLightning(ChainLightningEffectProfile),
    EntanglingRoots(EntanglingRootsEffectProfile),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TriggeredSpellProcProfile {
    pub ability: AbilityId,
    pub chance_per_10k: u16,
    pub targets: AttackTargetMask,
    pub effect: TriggeredAttackEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BurningOilEffectProfile {
    pub ability: AbilityId,
    pub radius: i32,
    pub full_damage: i32,
    pub full_interval_millis: u16,
    pub half_damage: i32,
    pub half_interval_millis: u16,
    pub full_duration_millis: u16,
    pub total_duration_millis: u16,
    pub target_ground_units: bool,
    pub target_buildings: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CriticalStrikeEffectProfile {
    pub ability: AbilityId,
    pub chance_per_10k: u16,
    pub damage_multiplier_per_10k: u16,
    pub targets: AttackTargetMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SplashFalloffProfile {
    pub full_radius: i32,
    pub medium_radius: i32,
    pub outer_radius: i32,
    pub medium_damage_per_10k: u16,
    pub outer_damage_per_10k: u16,
    pub targets: AttackTargetMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleaveEffectProfile {
    pub ability: AbilityId,
    pub radius: i32,
    pub damage_per_10k: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraEffectProfile {
    pub ability: AbilityId,
    pub radius: i32,
    pub armor_bonus_per_100: i16,
    pub mana_regeneration_per_second_per_10k: u32,
    pub suspend_during_spell_cooldown: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpellResistanceEffectProfile {
    pub ability: AbilityId,
    pub damage_taken_per_10k: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FeedbackEffectProfile {
    pub ability: AbilityId,
    pub maximum_mana_drained: i32,
    pub damage_per_mana_per_10k: u16,
    pub summoned_damage: i32,
    pub targets: AttackTargetMask,
}

/// Intrinsic target classifications, independent of armor type or presentation.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitClassifications {
    pub hero: bool,
    pub summoned: bool,
    pub spell_immune: bool,
    /// Script UNIT_TYPE_SAPPER and not a structure.
    pub combat_sapper: bool,
    /// Native Avul marker. Distinct from magic immunity.
    pub invulnerable: bool,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionUnitClassifications(pub UnitClassifications);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PassiveUnitEffect {
    Bash(BashEffectProfile),
    CriticalStrike(CriticalStrikeEffectProfile),
    SplashFalloff(SplashFalloffProfile),
    Evasion(EvasionEffectProfile),
    Defend(DefendEffectProfile),
    TriggeredSpellProc(TriggeredSpellProcProfile),
    BurningOil(BurningOilEffectProfile),
    Cleave(CleaveEffectProfile),
    Aura(AuraEffectProfile),
    SpellResistance(SpellResistanceEffectProfile),
    Feedback(FeedbackEffectProfile),
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PassiveUnitEffects {
    effects: [Option<PassiveUnitEffect>; MAX_PASSIVE_UNIT_EFFECTS],
    count: u8,
}

impl PassiveUnitEffects {
    pub const EMPTY: Self = Self {
        effects: [None; MAX_PASSIVE_UNIT_EFFECTS],
        count: 0,
    };

    #[must_use]
    pub const fn single(effect: PassiveUnitEffect) -> Self {
        let mut effects = [None; MAX_PASSIVE_UNIT_EFFECTS];
        effects[0] = Some(effect);
        Self { effects, count: 1 }
    }

    #[must_use]
    pub fn from_slice(effects: &[PassiveUnitEffect]) -> Self {
        assert!(
            effects.len() <= MAX_PASSIVE_UNIT_EFFECTS,
            "too many passive effects for one unit"
        );
        let mut stored = [None; MAX_PASSIVE_UNIT_EFFECTS];
        for (slot, effect) in stored.iter_mut().zip(effects.iter().copied()) {
            *slot = Some(effect);
        }
        Self {
            effects: stored,
            count: u8::try_from(effects.len()).expect("passive effect count fits u8"),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = PassiveUnitEffect> + '_ {
        self.effects[..usize::from(self.count)]
            .iter()
            .map(|effect| effect.expect("active passive effect slot must be populated"))
    }

    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.count == 0
    }
}

impl Default for PassiveUnitEffects {
    fn default() -> Self {
        Self::EMPTY
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitGameplayProperties {
    pub content: Option<ContentIdentity>,
    pub health_regen_per_second_per_10k: u32,
    pub corpse: Option<CorpseProfile>,
    pub collision_radius: Option<CollisionRadius>,
    pub movement_class: MovementClass,
    pub mechanical: bool,
    pub classifications: UnitClassifications,
    pub build_time_ticks: Option<u32>,
    pub repair_time_ticks: Option<u32>,
    pub attack_targets: AttackTargetMask,
    pub secondary_attack: Option<SecondaryAttackProfile>,
    pub damage_type: DamageType,
    pub armor: ArmorProfile,
    pub passive_effects: PassiveUnitEffects,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingGameplayProperties {
    pub content: Option<ContentIdentity>,
    /// Authoritative construction duration. `None` keeps generic/synthetic building spawns
    /// immediate; Castle Fight content supplies this from the versioned map object data.
    pub construction_time_ticks: Option<u32>,
    pub repair_time_ticks: Option<u32>,
    pub attack_targets: AttackTargetMask,
    pub damage_type: DamageType,
    pub armor: ArmorProfile,
    pub economy: Option<BuildingEconomyProfile>,
    pub production_unit: UnitGameplayProperties,
    pub production_spellcasting: Option<SpellcastingProfile>,
    pub production_additional_abilities: Option<AdditionalAutomaticAbilityDefinitions>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionContentIdentity(pub ContentIdentity);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionCollisionRadius(pub CollisionRadius);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionMovementClass(pub MovementClass);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ProductionUnitRepairMetadata {
    pub mechanical: bool,
    pub build_time_ticks: Option<u32>,
    pub repair_time_ticks: Option<u32>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionAttackTargets(pub AttackTargetMask);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionSecondaryAttack(pub SecondaryAttackProfile);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionDamageType(pub DamageType);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionArmorProfile(pub ArmorProfile);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionPassiveEffects(pub PassiveUnitEffects);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionHealthRegeneration(pub u32);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionSpellcastingProfile(pub SpellcastingProfile);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionAdditionalAutomaticAbilities(pub AdditionalAutomaticAbilityDefinitions);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Corpse {
    pub source_unit: SimId,
    pub source_owner: PlayerId,
    pub source_team: Team,
    pub definition: CorpseDefinitionId,
    pub created_tick: u64,
    pub decay_start_tick: u64,
    pub expires_tick: Option<u64>,
    pub resurrection: Option<ResolvedUnitDefinition>,
}

impl Corpse {
    #[must_use]
    pub(crate) const fn is_usable_at(self, tick: u64) -> bool {
        tick >= self.decay_start_tick
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct AttackCooldown {
    pub remaining: u16,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct AttackSequence(pub u64);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TargetState {
    pub current: Option<SimId>,
    pub direct_retaliation_lock: bool,
    pub ally_defense_lock: bool,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct RetaliationState {
    pub attacker: Option<SimId>,
    pub attacked_tick: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) enum NavigationGoal {
    #[default]
    None,
    Objective(Team),
    Target(SimId),
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub(crate) struct NavigationState {
    pub avoidance_goal: NavigationGoal,
    pub bypass_side: i8,
    pub clear_ticks: u8,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MovementProfile {
    pub speed_per_tick: i32,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuilderProfile {
    pub speed_per_tick: i32,
    pub build_range: i32,
    pub repair_range: i32,
    pub repair_autocast_range: i32,
    pub repair_time_ratio_numerator: u16,
    pub repair_time_ratio_denominator: u16,
    pub full_repair_duration_ticks: u16,
    pub blink_range: i32,
    pub blink_boundary_inset: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BuilderLocomotion {
    Foot,
    Hover,
}

#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuilderConfiguration {
    pub appearance: ContentIdentity,
    pub locomotion: BuilderLocomotion,
    pub build_catalog: Vec<u32>,
}

impl BuilderConfiguration {
    #[must_use]
    pub fn allows_building(&self, rawcode: u32) -> bool {
        self.build_catalog.contains(&rawcode)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuilderSpawn {
    pub team: Team,
    pub position: SimPoint,
    pub profile: BuilderProfile,
    pub configuration: BuilderConfiguration,
    pub repair_autocast_enabled: bool,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Builder;

#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BuilderState {
    pub destination: Option<SimPoint>,
    pub follow_target: Option<SimId>,
    pub repair_target: Option<SimId>,
    pub repair_progress_remainder: u32,
    pub repair_autocast_enabled: bool,
}

#[derive(Component, Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct BuilderBuildOrder {
    pub building: BuildingSpawn,
    pub properties: BuildingGameplayProperties,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub(crate) struct BuildingRuntimeState {
    pub production: Option<ProductionState>,
    pub attack_cooldown: Option<AttackCooldown>,
    pub target: Option<TargetState>,
    pub spawn_tick: Option<SpawnTick>,
    pub mana: Option<ManaState>,
    pub ability_state: Option<AutomaticAbilityState>,
    pub additional_abilities: Option<AdditionalAutomaticAbilities>,
    pub status: Option<StatusState>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct BuildingUpgradeSource {
    pub building: BuildingSpawn,
    pub properties: BuildingGameplayProperties,
    pub health: Health,
    pub runtime: BuildingRuntimeState,
}

#[derive(Component, Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct BuildingConstruction {
    pub started_tick: u64,
    pub complete_tick: u64,
    pub building: BuildingSpawn,
    pub properties: BuildingGameplayProperties,
    pub upgrade_from: Option<BuildingUpgradeSource>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MechanicalUnit;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BuildTimeTicks(pub u32);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RepairTimeTicks(pub u32);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpawnTick(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitTemplate {
    pub health: i32,
    pub attack: AttackProfile,
    pub movement: MovementProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedUnitDefinition {
    pub template: UnitTemplate,
    pub properties: UnitGameplayProperties,
    pub spellcasting: Option<SpellcastingProfile>,
    pub additional_abilities: Option<AdditionalAutomaticAbilityDefinitions>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ResurrectionProfile(pub ResolvedUnitDefinition);

#[derive(Debug, Clone, Copy)]
pub struct UnitSpawn {
    pub team: Team,
    pub position: SimPoint,
    pub health: i32,
    pub attack: AttackProfile,
    pub movement: MovementProfile,
}

impl UnitSpawn {
    #[must_use]
    pub const fn from_template(team: Team, position: SimPoint, template: UnitTemplate) -> Self {
        Self {
            team,
            position,
            health: template.health,
            attack: template.attack,
            movement: template.movement,
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingFootprint {
    pub min_x: i32,
    pub min_y: i32,
    pub width: u16,
    pub height: u16,
}

impl BuildingFootprint {
    #[must_use]
    pub const fn new(min_x: i32, min_y: i32, width: u16, height: u16) -> Self {
        Self {
            min_x,
            min_y,
            width,
            height,
        }
    }

    #[must_use]
    pub const fn max_x(self) -> i32 {
        self.min_x + self.width as i32 - 1
    }

    #[must_use]
    pub const fn max_y(self) -> i32 {
        self.min_y + self.height as i32 - 1
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionProfile {
    pub initial_delay_ticks: u16,
    pub interval_ticks: u16,
    pub search_radius_cells: u16,
    pub unit: UnitTemplate,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProductionState {
    pub next_spawn_tick: u64,
    /// WC3-style visible training slots. A running producer replenishes to two after each attempt.
    pub queued: u8,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize,
)]
pub struct ModifierId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AbilityTargetPolicy {
    RandomEnemyUnit,
    AllEnemyUnits,
    RandomEnemyUnitGlobal,
    RecentlyAttackedFriendlyUnit,
    WoundedFriendlyUnit,
    RandomGroundEnemyUnit,
    AllFriendlyUnits,
    RandomCorpse,
    RandomEnemyBasePoint,
    NearestEnemyInCombat,
    FlyingEnemyUnit,
    RandomEnemyUnitOrBuilding,
}

impl AbilityTargetPolicy {
    #[must_use]
    pub const fn stable_tag(self) -> u8 {
        match self {
            Self::RandomEnemyUnit => 0,
            Self::AllEnemyUnits => 1,
            Self::RandomEnemyUnitGlobal => 2,
            Self::RecentlyAttackedFriendlyUnit => 3,
            Self::WoundedFriendlyUnit => 4,
            Self::RandomGroundEnemyUnit => 5,
            Self::AllFriendlyUnits => 6,
            Self::RandomCorpse => 7,
            Self::RandomEnemyBasePoint => 8,
            Self::NearestEnemyInCombat => 9,
            Self::FlyingEnemyUnit => 10,
            Self::RandomEnemyUnitOrBuilding => 11,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AreaDamageOrigin {
    Caster,
    Target,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AbilityEffect {
    Damage {
        amount: i32,
    },
    Stun {
        duration_ticks: u16,
    },
    ModifyMovementSpeedPercent {
        modifier: ModifierId,
        percent_delta: i16,
        duration_ticks: u16,
    },
    AreaDamage {
        amount: i32,
        radius: i32,
        origin: AreaDamageOrigin,
    },
    FrostArmor {
        modifier: ModifierId,
        armor_bonus_per_100: i16,
        armor_duration_ticks: u16,
        slow_duration_ticks: u16,
        movement_percent_delta: i16,
        attack_speed_percent_delta: i16,
    },
    HolyAid {
        modifier: ModifierId,
        healing: i32,
        armor_bonus_per_100: i16,
        regeneration_per_second_per_10k: u32,
        duration_ticks: u16,
        permanent_max_health_bonus: i32,
        resurrection_count: u8,
        resurrection_radius: i32,
        resurrection_mana_cost: i32,
        resurrection_cooldown_ticks: u16,
        resurrection_delay_ticks: u16,
    },
    Prayer {
        modifier: ModifierId,
        healing: i32,
        mana_restored: i32,
        armor_bonus_per_100: i16,
        damage_bonus_per_10k: u16,
        duration_ticks: u16,
        radius: i32,
        resurrection_count: u8,
        resurrection_radius: i32,
    },
    HolyFervour {
        modifier: ModifierId,
        radius: i32,
        duration_ticks: u16,
    },
    Purification {
        damage: i32,
        radius: i32,
        consume_radius: i32,
        reveal_radius: i32,
        reveal_duration_ticks: u16,
    },
    SolarStrike {
        profile: NativeBoltProfile,
        radius: i32,
        maximum_targets: u8,
    },
    PhoenixFire(NativeBoltProfile),
    HealingWave(HealingWaveProfile),
    FaerieFire {
        modifier: ModifierId,
        armor_reduction_per_100: i16,
        duration_ticks: u16,
        hero_duration_ticks: u16,
    },
    ArtilleryBombardment {
        min_damage: i32,
        max_damage: i32,
        speed_per_tick: i32,
        splash: SplashFalloffProfile,
        burning_oil: BurningOilEffectProfile,
    },
}

impl AbilityEffect {
    #[must_use]
    pub const fn stable_tag(self) -> u8 {
        match self {
            Self::Damage { .. } => 0,
            Self::Stun { .. } => 1,
            Self::ModifyMovementSpeedPercent { .. } => 2,
            Self::AreaDamage { .. } => 3,
            Self::FrostArmor { .. } => 4,
            Self::HolyAid { .. } => 5,
            Self::Prayer { .. } => 6,
            Self::HolyFervour { .. } => 7,
            Self::Purification { .. } => 8,
            Self::ArtilleryBombardment { .. } => 9,
            Self::FaerieFire { .. } => 10,
            Self::HealingWave(_) => 11,
            Self::SolarStrike { .. } => 12,
            Self::PhoenixFire(_) => 13,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaProfile {
    pub maximum: i32,
    pub starting: i32,
    /// Mana regenerated per simulation tick in 1/10,000 mana units.
    pub regen_per_tick_per_10k: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomaticAbilityProfile {
    pub id: AbilityId,
    pub mana_cost: i32,
    pub cooldown_ticks: u16,
    pub range: i32,
    pub target_policy: AbilityTargetPolicy,
    pub effect: AbilityEffect,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpellcastingProfile {
    pub mana: ManaProfile,
    pub ability: AutomaticAbilityProfile,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaState {
    pub current: i32,
    pub regen_remainder_per_10k: u16,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomaticAbilityState {
    pub ready_tick: u64,
    pub cast_sequence: u64,
    pub autocast_enabled: bool,
    pub manual_cast_requested: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedMovementModifier {
    pub id: ModifierId,
    pub percent_delta: i16,
    pub expires_tick: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedAttackSpeedModifier {
    pub id: ModifierId,
    pub percent_delta: i16,
    pub expires_tick: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedArmorModifier {
    pub id: ModifierId,
    pub armor_bonus_per_100: i16,
    pub regeneration_per_second_per_10k: u32,
    pub mana_regeneration_per_second_per_10k: u32,
    pub damage_bonus_per_10k: u16,
    pub expires_tick: u64,
    pub reactive_slow_duration_ticks: u16,
    pub reactive_movement_percent_delta: i16,
    pub reactive_attack_speed_percent_delta: i16,
    pub revealed_to: Option<Team>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedDamageOverTime {
    pub id: ModifierId,
    pub damage_per_pulse: i32,
    pub pulse_interval_ticks: u16,
    pub next_pulse_tick: u64,
    pub expires_tick: u64,
    /// Phoenix Fire's last one-second pulse lands at buff expiry, before removal.
    pub final_pulse_at_expiry: bool,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BurningOilZone {
    pub source: SimId,
    pub source_team: Team,
    pub center: SimPoint,
    pub profile: BurningOilEffectProfile,
    pub created_tick: u64,
    pub pulse_index: u16,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ChainLightningState {
    pub source: SimId,
    pub source_team: Team,
    pub profile: ChainLightningEffectProfile,
    pub started_tick: u64,
    pub next_jump_index: u8,
    pub current_target: SimId,
    pub last_position: SimPoint,
    pub next_damage: i32,
    pub hit_targets: [SimId; MAX_BOUNCE_HITS],
    pub hit_count: u8,
}

pub const MAX_TIMED_ATTACK_SPEED_MODIFIERS: usize = 8;
pub const MAX_TIMED_ARMOR_MODIFIERS: usize = 8;
pub const MAX_TIMED_DAMAGE_OVER_TIME: usize = 4;

#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusState {
    pub stunned_until_tick: u64,
    pub ability_retreat_start_tick: u64,
    pub ability_retreat_end_tick: u64,
    pub secondary_resurrection_due_tick: u64,
    pub secondary_resurrection_ready_tick: u64,
    pub secondary_resurrection_ability: Option<AbilityId>,
    pub permanent_holy_health_bonus: bool,
    pub movement_modifiers: [TimedMovementModifier; MAX_TIMED_MOVEMENT_MODIFIERS],
    pub movement_modifier_count: u8,
    pub attack_speed_modifiers: [TimedAttackSpeedModifier; MAX_TIMED_ATTACK_SPEED_MODIFIERS],
    pub attack_speed_modifier_count: u8,
    pub armor_modifiers: [TimedArmorModifier; MAX_TIMED_ARMOR_MODIFIERS],
    pub armor_modifier_count: u8,
    pub damage_over_time: [TimedDamageOverTime; MAX_TIMED_DAMAGE_OVER_TIME],
    pub damage_over_time_count: u8,
}

impl StatusState {
    #[must_use]
    pub fn is_revealed_to(&self, team: Team, tick: u64) -> bool {
        self.armor_modifiers[..usize::from(self.armor_modifier_count)]
            .iter()
            .any(|modifier| modifier.revealed_to == Some(team) && tick < modifier.expires_tick)
    }

    #[must_use]
    pub const fn is_stunned(self, tick: u64) -> bool {
        tick < self.stunned_until_tick
    }

    #[must_use]
    pub fn effective_armor_points_per_100(&self, armor: ArmorProfile) -> i32 {
        let count = usize::from(self.armor_modifier_count);
        debug_assert!(count <= MAX_TIMED_ARMOR_MODIFIERS);
        self.armor_modifiers[..count].iter().fold(
            i32::from(armor.armor_points) * 100,
            |total, modifier| {
                total
                    .checked_add(i32::from(modifier.armor_bonus_per_100))
                    .expect("effective armor overflow")
            },
        )
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BuildingSpawn {
    pub team: Team,
    pub footprint: BuildingFootprint,
    pub health: i32,
    pub production: Option<ProductionProfile>,
    pub attack: Option<AttackProfile>,
    pub spellcasting: Option<SpellcastingProfile>,
}
