use bevy_ecs::prelude::Component;
use std::collections::VecDeque;

mod support_orders;
pub(crate) use support_orders::{SupportOrderQueue, SupportTargetCache};
pub use support_orders::{SupportOrderRules, SupportOrderState, WindWalkProfile, WindWalkState};
mod native_grants;
pub use native_grants::{
    MAX_PERMANENT_NATIVE_ABILITIES, NativeAbilityGrant, NativeAbilityGrantEffect,
    NativeAbilityGrants,
};
mod master_projectiles;
pub(crate) use master_projectiles::{
    MasterFan, MasterMinion, MasterMover, MasterParasite, MasterProjectileState,
};
pub use master_projectiles::{MasterMoverProfile, MasterProjectileProfile, NativeNovaProfile};
mod native_summons;
pub(crate) use native_summons::NativeParasiteState;
pub use native_summons::{
    NativeParasiteProfile, NativeSummonProfile, PurgeProfile, PurgeState, SirenPurgeProfile,
};
mod native_actions;
pub(crate) use native_actions::{
    AcidBombMissileState, AuraCarrierState, ConeWaveState, EnsnareMissileState, HailstoneState,
    HealingWaveState, NativeAction, NativeBoltState, PoisonState, SphereFlightState,
    StasisTrapState,
};
pub use native_actions::{
    AcidBombProfile, AcidBombStatus, EnsnareProfile, EnsnareState, HealingWaveProfile,
    NativeBoltProfile,
};
mod automatic_abilities;
mod mana;
pub(crate) use automatic_abilities::compose_spellcasting_profiles;
pub use automatic_abilities::{
    AbilityConfigurationError, AdditionalAutomaticAbilities, AdditionalAutomaticAbilityDefinitions,
    AutomaticAbilityInstance, MAX_AUTOMATIC_ABILITIES, SecondaryResurrectionState,
};
pub use mana::ManaRegeneration;
pub(crate) use mana::per_second_increment;
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
    /// Targeted missile followed by a directed, travelling damage spill behind its impact.
    Line {
        speed_per_tick: i32,
        minimum_range: i32,
        spill_distance: i32,
        spill_radius: i32,
        damage_retention_per_10k: u16,
        spill_targets: AttackTargetMask,
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
            Self::Line { .. } => 5,
        }
    }
}

/// Versioned native action totals (point plus backswing) and independent attack damage points.
/// Each source duration is rounded once to simulation ticks.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionTimingProfile {
    pub primary_attack_ticks: u16,
    pub primary_attack_point_ticks: u16,
    pub secondary_attack_ticks: u16,
    pub secondary_attack_point_ticks: u16,
    pub cast_ticks: u16,
    #[serde(default)]
    pub cast_point_ticks: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionAnimationKind {
    Attack,
    Cast,
    CastSlam,
    CastThrow,
}
impl ActionAnimationKind {
    #[must_use]
    pub const fn is_cast(self) -> bool {
        matches!(self, Self::Cast | Self::CastSlam | Self::CastThrow)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionAnimationState {
    pub kind: ActionAnimationKind,
    pub started_tick: u64,
    pub until_tick: u64,
}

impl ActionAnimationState {
    #[must_use]
    pub const fn is_active(self, tick: u64) -> bool {
        tick >= self.started_tick && tick < self.until_tick
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
    pub fn in_range(self, distance_sq: u64) -> bool {
        let minimum = match self.delivery {
            AttackDelivery::Line { minimum_range, .. } => minimum_range,
            _ => 0,
        };
        distance_sq >= (i64::from(minimum) * i64::from(minimum)) as u64
            && distance_sq <= self.range_sq()
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
    pub corruption: Option<CorruptionProfile>,
    pub poison: Option<PoisonProfile>,
    pub stun_duration_ticks: u16,
    pub triggered_spell: Option<TriggeredAttackEffect>,
    pub burning_oil: Option<BurningOilEffectProfile>,
    pub splash_falloff: Option<SplashFalloffProfile>,
    pub feedback: Option<FeedbackEffectProfile>,
    pub frost: Option<FrostAttackEffectProfile>,
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
    pub frost: Option<FrostAttackEffectProfile>,
    pub damage_type: DamageType,
    pub launch_position: SimPoint,
    pub destination: SimPoint,
    pub impact_radius: i32,
    pub launch_tick: u64,
    pub impact_tick: u64,
}

#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct LineProjectile {
    pub source: SimId,
    /// Retained independently of the source lifetime so rejoin can bind missile art.
    pub source_rawcode: Option<u32>,
    pub source_team: Team,
    pub target: SimId,
    pub damage: i32,
    pub damage_type: DamageType,
    pub delivery: AttackDelivery,
    pub launch_position: SimPoint,
    pub launch_tick: u64,
    pub primary_impact_tick: u64,
    pub impact_tick: u64,
    /// None until the primary missile arrives; thereafter the spill ray is fixed.
    pub spill_origin: Option<SimPoint>,
    pub destination: SimPoint,
    pub hit_targets: Vec<SimId>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct BounceProjectile {
    pub source: SimId,
    pub source_rawcode: Option<u32>,
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
    /// Retained definition version, including cold future-spawn and upgrade identities.
    pub map_version: crate::version::MapVersion,
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
    /// Hero duration is independently authored; it is not inferred from armor type.
    pub hero_stun_duration_ticks: u16,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntanglingRootsEffectProfile {
    pub ability: AbilityId,
    pub damage_per_second: i32,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
    pub nonhero_only: bool,
    pub targets: AttackTargetMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScriptedRootsProfile {
    pub map_version: crate::MapVersion,
    pub roots: EntanglingRootsEffectProfile,
    pub primary_nonhero_only: bool,
    pub search_radius: i32,
    pub maximum_targets: u8,
    pub child_range: i32,
    pub excluded_abilities: [u32; 2],
    pub priority_buffs: [u32; 2],
    pub vision_radius: i32,
    pub vision_ticks: u16,
}

impl ScriptedRootsProfile {
    pub(crate) fn canonical_words(self) -> [u64; 17] {
        [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.roots.ability.0),
            self.roots.damage_per_second as u64,
            u64::from(self.roots.duration_ticks),
            u64::from(self.roots.hero_duration_ticks),
            u64::from(self.roots.nonhero_only),
            u64::from(self.roots.targets.bits()),
            u64::from(self.primary_nonhero_only),
            self.search_radius as u64,
            u64::from(self.maximum_targets),
            self.child_range as u64,
            u64::from(self.excluded_abilities[0]),
            u64::from(self.excluded_abilities[1]),
            u64::from(self.priority_buffs[0]) | (u64::from(self.priority_buffs[1]) << 32),
            self.vision_radius as u64,
            u64::from(self.vision_ticks),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriggeredAttackEffect {
    ForkedLightning(ForkedLightningProfile),
    ConeWave(ConeWaveProfile),
    ChainLightning(ChainLightningEffectProfile),
    EntanglingRoots(EntanglingRootsEffectProfile),
}

/// Simultaneous native lightning branches in a caster-oriented expanding cone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkedLightningProfile {
    pub ability: AbilityId,
    pub damage: i32,
    pub maximum_targets: u8,
    pub distance: i32,
    pub initial_radius: i32,
    pub final_radius: i32,
    pub targets: AttackTargetMask,
}

impl ForkedLightningProfile {
    pub(crate) fn canonical_words(self) -> [u64; 7] {
        [
            u64::from(self.ability.0),
            self.damage as u64,
            u64::from(self.maximum_targets),
            self.distance as u64,
            self.initial_radius as u64,
            self.final_radius as u64,
            u64::from(self.targets.bits()),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct HealthScaledAftershockProfile {
    pub marker: AbilityId,
    pub dispatcher: AbilityId,
    pub maximum_damage: i32,
}

impl HealthScaledAftershockProfile {
    pub(crate) fn canonical_words(self) -> [u64; 3] {
        [
            u64::from(self.marker.0),
            u64::from(self.dispatcher.0),
            self.maximum_damage as u64,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageRetaliationProfile {
    pub marker: AbilityId,
    pub chance_per_10k: u16,
    pub range: i32,
    pub organic_only: bool,
    pub projectile: NativeBoltProfile,
}

impl DamageRetaliationProfile {
    pub(crate) fn canonical_words(self) -> [u64; 13] {
        let p = self.projectile;
        [
            u64::from(self.marker.0),
            u64::from(self.chance_per_10k),
            self.range as u64,
            u64::from(self.organic_only),
            u64::from(p.ability.0),
            p.damage as u64,
            u64::from(p.stun_ticks),
            u64::from(p.hero_stun_ticks),
            p.damage_per_second as u64,
            u64::from(p.duration_ticks),
            p.speed_per_tick as u64,
            u64::from(p.cleanse),
            u64::from(p.targets.bits()),
        ]
    }
}

/// Native traveling cone (Carrion Swarm / Breath of Fire family), independent of its trigger target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConeWaveProfile {
    pub ability: AbilityId,
    pub damage: i32,
    pub maximum_damage: i32,
    pub distance: i32,
    pub initial_radius: i32,
    pub final_radius: i32,
    pub speed_per_tick: i32,
    pub targets: AttackTargetMask,
}

impl ConeWaveProfile {
    pub(crate) fn canonical_words(self) -> [u64; 8] {
        [
            u64::from(self.ability.0),
            self.damage as u64,
            self.maximum_damage as u64,
            self.distance as u64,
            self.initial_radius as u64,
            self.final_radius as u64,
            self.speed_per_tick as u64,
            u64::from(self.targets.bits()),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LavaSpawnProfile {
    pub ability: AbilityId,
    pub body_rawcode: u32,
    pub child_rawcode: u32,
    pub attacks_to_split: u16,
    pub split_delay_ticks: u16,
    pub maximum_splits: u8,
    pub healing_per_10k: u16,
    pub order_refresh_ticks: u16,
    pub targets: AttackTargetMask,
}

impl LavaSpawnProfile {
    pub(crate) fn canonical_words(self) -> [u64; 9] {
        [
            u64::from(self.ability.0),
            u64::from(self.body_rawcode),
            u64::from(self.child_rawcode),
            u64::from(self.attacks_to_split),
            u64::from(self.split_delay_ticks),
            u64::from(self.maximum_splits),
            u64::from(self.healing_per_10k),
            u64::from(self.order_refresh_ticks),
            u64::from(self.targets.bits()),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct LavaSpawnState {
    pub profile: LavaSpawnProfile,
    pub landed_attacks: u16,
    pub splits: u8,
    pub split_tick: Option<u64>,
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
pub struct PulverizeEffectProfile {
    pub ability: AbilityId,
    pub chance_per_10k: u16,
    pub damage: i32,
    pub full_radius: i32,
    pub half_radius: i32,
    pub targets: AttackTargetMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FrostAttackEffectProfile {
    pub ability: AbilityId,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
    pub movement_percent_delta: i16,
    pub attack_speed_percent_delta: i16,
    pub targets: AttackTargetMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CleaveEffectProfile {
    pub ability: AbilityId,
    pub radius: i32,
    pub damage_per_10k: u16,
    pub targets: AttackTargetMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuraEffectProfile {
    pub melee_damage_return_per_10k: u32,
    pub damage_bonus_per_10k: i16,
    pub affects_melee: bool,
    pub affects_ranged: bool,
    pub linger_ticks: u16,
    pub ability: AbilityId,
    pub radius: i32,
    pub armor_bonus_per_100: i16,
    pub mana_regeneration_per_second_per_10k: u32,
    pub suspend_during_spell_cooldown: bool,
}

/// Script-ordered native Mirror Image. Values are version-scoped source projections.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirrorImageProfile {
    pub ability: AbilityId,
    pub chance_per_10k: u16,
    pub cooldown_ticks: u16,
    pub split_ticks: u16,
    pub lifetime_ticks: u16,
    pub displacement: i32,
    pub images: u8,
    pub damage_dealt_per_10k: u16,
    pub damage_taken_per_10k: u16,
}

impl MirrorImageProfile {
    pub(crate) fn canonical_words(self) -> [u64; 9] {
        [
            u64::from(self.ability.0),
            u64::from(self.chance_per_10k),
            u64::from(self.cooldown_ticks),
            u64::from(self.split_ticks),
            u64::from(self.lifetime_ticks),
            self.displacement as u64,
            u64::from(self.images),
            u64::from(self.damage_dealt_per_10k),
            u64::from(self.damage_taken_per_10k),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MirrorImageState {
    pub profile: MirrorImageProfile,
    pub event_sequence: u64,
    pub ready_tick: u64,
    pub cast_tick: Option<u64>,
    pub split_tick: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct IllusionState {
    pub source: SimId,
    pub ability: AbilityId,
    pub expires_tick: u64,
    pub damage_dealt_per_10k: u16,
    pub damage_taken_per_10k: u16,
}

/// Native Sphere owns visual-only missiles and attachment cooldowns, never damage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SphereProfile {
    pub ability: AbilityId,
    pub attachment_count: u8,
    pub cooldown_ticks: u16,
    pub speed_per_tick: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SphereAttachmentState {
    pub ability: AbilityId,
    pub hidden_until: [u64; 3],
    pub next_slot: u8,
}

/// Native Asds: physical ability damage uses armor points without attack-class multipliers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelfDestructProfile {
    pub ability: AbilityId,
    pub full_radius: i32,
    pub full_damage: i32,
    pub partial_radius: i32,
    pub partial_damage: i32,
    pub building_damage_per_10k: u16,
    pub targets: AttackTargetMask,
}

impl SelfDestructProfile {
    pub(crate) fn canonical_words(self) -> [u64; 7] {
        [
            u64::from(self.ability.0),
            self.full_radius as u64,
            self.full_damage as u64,
            self.partial_radius as u64,
            self.partial_damage as u64,
            u64::from(self.building_damage_per_10k),
            u64::from(self.targets.bits()),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageChargeProfile {
    pub marker: AbilityId,
    pub mana_maximum: i32,
    pub mana_starting: i32,
    pub mana_regen_per_second_per_10k: i32,
    pub threshold: i32,
    pub armed_speed_per_tick: i32,
    pub explosion: SelfDestructProfile,
}

impl DamageChargeProfile {
    pub(crate) fn canonical_words(self) -> [u64; 13] {
        let mut words = [0; 13];
        words[..6].copy_from_slice(&[
            u64::from(self.marker.0),
            self.mana_maximum as u64,
            self.mana_starting as u64,
            self.mana_regen_per_second_per_10k as u64,
            self.threshold as u64,
            self.armed_speed_per_tick as u64,
        ]);
        words[6..].copy_from_slice(&self.explosion.canonical_words());
        words
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageChargeState {
    pub profile: DamageChargeProfile,
    /// Exact signed regeneration and damage accumulation, denominator simulation Hz * 10,000.
    pub mana_per_10k_hz: i64,
    pub armed: bool,
    pub detonation_tick: Option<u64>,
}

/// Intrinsic zero-linger native Endurance Aura. Buff identity controls stacking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeedAuraProfile {
    pub ability: AbilityId,
    pub buff_rawcode: u32,
    pub radius: i32,
    pub targets: AttackTargetMask,
    pub hostile: bool,
    pub organic_only: bool,
    pub affects_invulnerable: bool,
    pub movement_percent_delta: i16,
    pub attack_speed_percent_delta: i16,
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

/// Native AIcb on-hit armor family, independent of spell damage/immunity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CorruptionProfile {
    pub ability: AbilityId,
    pub buff: NativeBuffIdentity,
    pub armor_reduction_per_100: i16,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
    pub targets: AttackTargetMask,
}
impl CorruptionProfile {
    pub(crate) fn canonical_words(self) -> [u64; 9] {
        [
            u64::from(self.ability.0),
            u64::from(self.buff.rawcode),
            self.buff.positive as u64,
            self.buff.stealable as u64,
            self.buff.organic_only as u64,
            self.armor_reduction_per_100 as u64,
            u64::from(self.duration_ticks),
            u64::from(self.hero_duration_ticks),
            u64::from(self.targets.bits()),
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeHealProfile {
    pub amount: i32,
    pub targets: AttackTargetMask,
    pub organic_only: bool,
    pub nonancient_only: bool,
    pub affects_invulnerable: bool,
    pub resume_attack_ticks: u16,
}
impl NativeHealProfile {
    pub(crate) fn canonical_words(self) -> [u64; 6] {
        [
            self.amount as u64,
            u64::from(self.targets.bits()),
            self.organic_only as u64,
            self.nonancient_only as u64,
            self.affects_invulnerable as u64,
            u64::from(self.resume_attack_ticks),
        ]
    }
}

/// Intrinsic target classifications, independent of armor type or presentation.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitClassifications {
    /// Native melee-attacker handle classification, independent of the current damage event.
    #[serde(default)]
    pub melee_attacker: bool,
    pub hero: bool,
    /// Native Arsk: hero targeting/duration rules for negative spells only.
    pub resistant_skin: bool,
    /// Present only while the handle belongs to the native Neutral Hostile creep group.
    pub neutral_hostile_creep_level: Option<u16>,
    pub ancient: bool,
    pub summoned: bool,
    pub spell_immune: bool,
    /// Native invisibility; revelation is tracked independently by status buff identity.
    #[serde(default)]
    pub invisible: bool,
    /// Script UNIT_TYPE_SAPPER and not a structure.
    pub combat_sapper: bool,
    /// Native Avul marker. Distinct from magic immunity.
    pub invulnerable: bool,
    pub legendary: bool,
    pub summoned_marker: bool,
    /// Native IsUnitIllusion handle classification (not the summoned marker).
    pub illusion: bool,
}

impl UnitClassifications {
    #[must_use]
    pub const fn uses_hero_spell_rules(self) -> bool {
        self.hero || self.resistant_skin
    }
}

/// Per-handle script flags, never part of a cold resurrection definition.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ShrineRevivalState {
    pub revived: bool,
    pub suppress_next_death: bool,
    /// Native resurrection preserves the original handle's script identity.
    pub death_identity: Option<SimId>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct DelayedShrineRevival {
    /// Supporting script version, independent of the replacement's optional content identity.
    pub map_version: crate::MapVersion,
    pub source_unit: SimId,
    pub owner: PlayerId,
    pub team: Team,
    pub position: SimPoint,
    pub due_tick: u64,
    pub death_generation: u64,
    pub definition: ResolvedUnitDefinition,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionUnitClassifications(pub UnitClassifications);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PassiveUnitEffect {
    Corruption(CorruptionProfile),
    SourceDamageDispatcher(AbilityId),
    HealthScaledAftershock(HealthScaledAftershockProfile),
    DamageRetaliation(DamageRetaliationProfile),
    LavaSpawn(LavaSpawnProfile),
    MirrorImage(MirrorImageProfile),
    DamageCharge(DamageChargeProfile),
    Poison(PoisonProfile),
    DamageThreshold(DamageThresholdProfile),
    KillBerserk(KillBerserkProfile),
    Bash(BashEffectProfile),
    CriticalStrike(CriticalStrikeEffectProfile),
    SplashFalloff(SplashFalloffProfile),
    Evasion(EvasionEffectProfile),
    Defend(DefendEffectProfile),
    TriggeredSpellProc(TriggeredSpellProcProfile),
    BurningOil(BurningOilEffectProfile),
    Cleave(CleaveEffectProfile),
    Aura(AuraEffectProfile),
    SpeedAura(SpeedAuraProfile),
    Sphere(SphereProfile),
    SpellResistance(SpellResistanceEffectProfile),
    Feedback(FeedbackEffectProfile),
    FrostAttack(FrostAttackEffectProfile),
    Pulverize(PulverizeEffectProfile),
    SpikedShell(SpikedShellProfile),
    HardenedSkin(NativeAbilityGrant),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpikedShellProfile {
    pub ability: AbilityId,
    pub returned_damage_per_10k: u32,
    pub received_damage_per_10k: u32,
    pub armor_bonus_per_100: i32,
}

impl SpikedShellProfile {
    pub(crate) const fn canonical_words(self) -> [u64; 4] {
        [
            self.ability.0 as u64,
            self.returned_damage_per_10k as u64,
            self.received_damage_per_10k as u64,
            self.armor_bonus_per_100 as i64 as u64,
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KillBerserkProfile {
    pub ability: AbilityId,
    pub duration_ticks: u16,
    pub movement_percent_delta: i16,
    pub attack_speed_percent_delta: i16,
    pub damage_taken_bonus_per_10k: i16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageThreshold {
    pub hp_ratio_below_per_10k: u16,
    pub attack_bonus: i32,
    pub vertex_rgba: [u8; 4],
    pub adds_if_absent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageThresholdProfile {
    pub ability: AbilityId,
    pub thresholds: [DamageThreshold; 3],
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DamageThresholdState {
    pub ability: Option<AbilityId>,
    pub ability_level: u8,
    pub color_level: u8,
    pub attack_bonus: i32,
    pub vertex_rgba: Option<[u8; 4]>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoisonProfile {
    pub ability: AbilityId,
    /// Native poison uses its second authored buff when stacking flags are set.
    pub buff_rawcode: u32,
    pub damage_per_second: i32,
    pub movement_percent_delta: i16,
    pub attack_speed_percent_delta: i16,
    pub duration_ticks: u16,
    pub hero_duration_ticks: u16,
    pub targets: AttackTargetMask,
    pub stack_damage: bool,
    pub can_kill: bool,
}

pub const MAX_NATIVE_POISON_BUFFS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoisonStatus {
    pub profile: PoisonProfile,
    pub total_damage_per_second: i32,
    pub buffs: [Option<PoisonBuffStatus>; MAX_NATIVE_POISON_BUFFS],
    /// Distinguishes a cleansed family from a new application in the same tick.
    pub generation: SimId,
    pub sources: u32,
    pub expires_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoisonBuffStatus {
    pub ability: AbilityId,
    pub buff_rawcode: u32,
    pub damage_per_second: i32,
    pub expires_tick: u64,
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

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = PassiveUnitEffect> + '_ {
        self.effects[..usize::from(self.count)]
            .iter()
            .map(|effect| effect.expect("active passive effect slot must be populated"))
    }

    pub(crate) fn without_defend(self) -> Self {
        let mut result = self;
        result.retain(|effect| !matches!(effect, PassiveUnitEffect::Defend(_)));
        result
    }

    /// Remove identity-scoped permanent abilities without reallocating or reordering survivors.
    pub fn retain(&mut self, mut keep: impl FnMut(PassiveUnitEffect) -> bool) {
        let count = usize::from(self.count);
        let mut write = 0;
        for read in 0..count {
            let effect = self.effects[read].expect("active passive effect slot must be populated");
            if keep(effect) {
                self.effects[write] = Some(effect);
                write += 1;
            }
        }
        self.effects[write..count].fill(None);
        self.count = u8::try_from(write).expect("passive effect count fits u8");
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
    pub action_timing: ActionTimingProfile,
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
    pub action_timing: ActionTimingProfile,
    pub secondary_attack: Option<SecondaryAttackProfile>,
    pub passive_effects: PassiveUnitEffects,
    pub content: Option<ContentIdentity>,
    /// Intrinsic structure flags, independent of the classifications of produced children.
    pub classifications: UnitClassifications,
    /// Authoritative construction duration. `None` keeps generic/synthetic building spawns
    /// immediate; Castle Fight content supplies this from the versioned map object data.
    pub construction_time_ticks: Option<u32>,
    pub repair_time_ticks: Option<u32>,
    pub health_regen_per_second_per_10k: u32,
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
pub(crate) struct ProductionActionTiming(pub ActionTimingProfile);

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
    pub vertex_rgba: Option<[u8; 4]>,
    pub source_unit: SimId,
    pub source_owner: PlayerId,
    pub source_team: Team,
    pub definition: CorpseDefinitionId,
    pub created_tick: u64,
    pub decay_start_tick: u64,
    pub expires_tick: Option<u64>,
    pub resurrection: Option<ResolvedUnitDefinition>,
    pub shrine_state: ShrineRevivalState,
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
    pub order_queue_capacity: u16,
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub(crate) struct BuilderBuildOrder {
    pub building: BuildingSpawn,
    pub properties: BuildingGameplayProperties,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) enum BuilderOrder {
    Build(Box<BuilderBuildOrder>),
    Move(SimPoint),
    Follow(SimId),
    Repair(SimId),
    Blink(SimPoint),
    Stop,
}

impl BuilderOrder {
    pub(crate) fn build(&self) -> Option<&BuilderBuildOrder> {
        if let Self::Build(build) = self {
            Some(build)
        } else {
            None
        }
    }
}

#[derive(Component, Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct BuilderOrderQueue(pub(crate) VecDeque<BuilderOrder>);

impl BuilderOrderQueue {
    pub(crate) fn committed_cost(&self) -> BuildingEconomyProfile {
        self.0
            .iter()
            .filter_map(|order| order.build().and_then(|build| build.properties.economy))
            .fold(BuildingEconomyProfile::default(), |mut total, cost| {
                total.gold_cost = total
                    .gold_cost
                    .checked_add(cost.gold_cost)
                    .expect("queued gold cost overflow");
                total.lumber_cost = total
                    .lumber_cost
                    .checked_add(cost.lumber_cost)
                    .expect("queued lumber cost overflow");
                total.legendary_points_cost = total
                    .legendary_points_cost
                    .checked_add(cost.legendary_points_cost)
                    .expect("queued legendary cost overflow");
                total
            })
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub(crate) struct BuildingRuntimeState {
    pub passive_effects: PassiveUnitEffects,
    pub classifications: UnitClassifications,
    pub production: Option<ProductionState>,
    pub attack_cooldown: Option<AttackCooldown>,
    pub target: Option<TargetState>,
    pub spawn_tick: Option<SpawnTick>,
    pub mana: Option<ManaState>,
    pub health_regeneration: Option<HealthRegeneration>,
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
    /// Issued exclusively by a source-owned script controller.
    ScriptedController,
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
    /// Native Always Autocast debuffs consider any viable hostile in range.
    NativeBuildingSpellTrigger,
    RandomEnemyDebuff,
    /// Native positive autocast buffs select unbuffed allies engaged in combat.
    FriendlyUnitInCombat,
    /// Native Spell Steal selects a transferable hostile buff or friendly debuff.
    NativeBuffDonor,
}

impl AbilityTargetPolicy {
    #[must_use]
    pub const fn stable_tag(self) -> u8 {
        match self {
            Self::ScriptedController => 23,
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
            Self::NativeBuildingSpellTrigger => 12,
            Self::RandomEnemyDebuff => 20,
            Self::FriendlyUnitInCombat => 21,
            Self::NativeBuffDonor => 22,
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
    Wilderness(crate::building_mechanics::WildernessProfile),
    SilverGlade(crate::building_mechanics::SilverGladeProfile),
    Heal(NativeHealProfile),
    WindWalk(WindWalkProfile),
    ScriptedRoots(ScriptedRootsProfile),
    /// Released native weather art has its own lifetime, independent of logical caster life.
    NativeWeatherVisual {
        duration_ticks: u16,
    },
    Starfall(crate::building_mechanics::StarfallProfile),
    BuildingWeather(crate::building_mechanics::BuildingWeatherProfile),
    BuildingEnchantment(crate::building_mechanics::BuildingEnchantmentProfile),
    MasterProjectiles(crate::MapVersion),
    BuildingAura(crate::building_mechanics::BuildingAuraProfile),
    BuildingStasis(crate::building_mechanics::BuildingStasisProfile),
    BuildingPermanentGrant(crate::building_mechanics::BuildingPermanentGrantProfile),
    BuildingAcidBomb(crate::building_mechanics::BuildingAcidBombProfile),
    Ensnare {
        profile: EnsnareProfile,
        search_radius: i32,
        maximum_targets: u8,
    },
    SpellSteal {
        recipient_radius: i32,
    },
    StatBuff {
        modifier: ModifierId,
        buff: NativeBuffIdentity,
        armor_bonus_per_100: i16,
        damage_bonus_per_10k: i16,
        regeneration_per_second_per_10k: u32,
        duration_ticks: u16,
        hero_duration_ticks: u16,
        autocast_range: i32,
    },
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
    FrostNova {
        ability: AbilityId,
        radius: i32,
        primary_damage: i32,
        area_damage: i32,
        duration_ticks: u16,
        hero_duration_ticks: u16,
        movement_percent_delta: i16,
        attack_speed_percent_delta: i16,
        targets: AttackTargetMask,
    },
    AreaDebuff {
        ability: AbilityId,
        radius: i32,
        armor_delta_per_100: i16,
        damage_delta_per_10k: i16,
        duration_ticks: u16,
        hero_duration_ticks: u16,
        targets: AttackTargetMask,
    },
    AreaStun {
        ability: AbilityId,
        damage: i32,
        radius: i32,
        stun_ticks: u16,
        hero_stun_ticks: u16,
        targets: AttackTargetMask,
    },
    AreaDamage {
        amount: i32,
        radius: i32,
        origin: AreaDamageOrigin,
    },
    FrostArmor {
        native_buff: Option<NativeBuffIdentity>,
        modifier: ModifierId,
        armor_bonus_per_100: i16,
        armor_duration_ticks: u16,
        slow_duration_ticks: u16,
        movement_percent_delta: i16,
        attack_speed_percent_delta: i16,
    },
    HolyAid {
        native_buff: Option<NativeBuffIdentity>,
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
        native_buff: Option<NativeBuffIdentity>,
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
    Hailstone(crate::building_mechanics::HailstoneProfile),
    BuildingBolt(crate::building_mechanics::BuildingBoltProfile),
    WorldFreezer(crate::building_mechanics::WorldFreezerProfile),
    Snowfall {
        map_version: crate::MapVersion,
    },
    Hex {
        profile: crate::building_mechanics::HexEffectProfile,
    },
    FaerieFire {
        native_buff: Option<NativeBuffIdentity>,
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
    SirenPurge(SirenPurgeProfile),
    NativeSummon(NativeSummonProfile),
    NagaEarthquake(crate::building_mechanics::NagaEarthquakeProfile),
    NagaCharm(crate::building_mechanics::NagaCharmProfile),
    NagaTidalWave(crate::building_mechanics::NagaTidalWaveProfile),
}

impl AbilityEffect {
    /// Autonomous native firing does not issue an interruptible caster order.
    /// Ability disable (for example a morph) still suppresses it.
    pub(crate) const fn ignores_order_interruptions(self) -> bool {
        matches!(self, Self::PhoenixFire(_))
    }

    #[must_use]
    pub const fn stable_tag(self) -> u8 {
        match self {
            Self::SirenPurge(_) => 45,
            Self::NativeSummon(_) => 46,
            Self::NagaEarthquake(_) => 47,
            Self::NagaCharm(_) => 48,
            Self::NagaTidalWave(_) => 49,
            Self::Heal(_) => 41,
            Self::SilverGlade(_) => 42,
            Self::Wilderness(_) => 43,
            Self::WindWalk(_) => 40,
            Self::ScriptedRoots(_) => 39,
            Self::Starfall(_) => 44,
            Self::NativeWeatherVisual { .. } => 38,
            Self::BuildingWeather(_) => 37,
            Self::BuildingEnchantment(_) => 36,
            Self::BuildingAura(_) => 28,
            Self::BuildingStasis(_) => 27,
            Self::BuildingPermanentGrant(_) => 26,
            Self::BuildingAcidBomb(_) => 25,
            Self::Ensnare { .. } => 24,
            Self::StatBuff { .. } => 22,
            Self::SpellSteal { .. } => 23,
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
            Self::Hex { .. } => 14,
            Self::AreaStun { .. } => 15,
            Self::AreaDebuff { .. } => 16,
            Self::MasterProjectiles(_) => 35,
            Self::FrostNova { .. } => 17,
            Self::Snowfall { .. } => 18,
            Self::Hailstone(_) => 19,
            Self::BuildingBolt(_) => 20,
            Self::WorldFreezer(_) => 21,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaProfile {
    pub maximum: i32,
    pub starting: i32,
    /// Legacy per-tick rate. Use `ManaProfile::per_second` for exact per-second rates;
    /// the high bit tags that encoding without changing existing snapshot shapes.
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
    pub undispellable: bool,
    pub id: ModifierId,
    pub percent_delta: i16,
    pub expires_tick: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedAttackSpeedModifier {
    pub undispellable: bool,
    pub id: ModifierId,
    pub percent_delta: i16,
    pub expires_tick: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedArmorModifier {
    pub melee_damage_return_per_10k: u32,
    pub undispellable: bool,
    /// Last completed tick’s intrinsic aura membership, visible until the next resample.
    pub aura_membership: bool,
    pub id: ModifierId,
    pub native_buff: Option<NativeBuffIdentity>,
    pub armor_bonus_per_100: i16,
    pub regeneration_per_second_per_10k: u32,
    pub mana_regeneration_per_second_per_10k: u32,
    pub damage_bonus_per_10k: i16,
    pub damage_taken_bonus_per_10k: i16,
    pub expires_tick: u64,
    pub reactive_slow_duration_ticks: u16,
    pub reactive_movement_percent_delta: i16,
    pub reactive_attack_speed_percent_delta: i16,
    pub revealed_to: Option<Team>,
}

/// Native buff family controls replacement independently of its granting ability.
/// Script bonuses and intrinsic auras have no such transferable identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeBuffIdentity {
    pub rawcode: u32,
    pub positive: bool,
    pub stealable: bool,
    pub organic_only: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimedDamageOverTime {
    pub source: Option<SimId>,
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

/// Committed ordinary attack windup. The target cannot silently change before release.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingAttackState {
    pub target: SimId,
    pub release_tick: u64,
}

/// Stable target identity retained while an ordered spell plays through its cast point.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingCastTarget {
    Unit(SimId),
    Building { id: SimId, position: SimPoint },
    AllEnemyUnits,
    AllFriendlyUnits,
    Corpse { id: SimId, position: SimPoint },
    Point(SimPoint),
}

/// Committed unit spell windup. Resources and gameplay effects resolve at `release_tick`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingCastState {
    pub ability: AutomaticAbilityProfile,
    pub cast_sequence: u64,
    pub target: PendingCastTarget,
    pub release_tick: u64,
}

pub const MAX_TIMED_ATTACK_SPEED_MODIFIERS: usize = 8;
pub const MAX_TIMED_ARMOR_MODIFIERS: usize = 8;
pub const MAX_TIMED_DAMAGE_OVER_TIME: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManaImprovementLink {
    pub source: SimId,
    pub source_position: SimPoint,
    pub target_position: SimPoint,
    pub ability: AbilityId,
}

#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StatusState {
    pub support_order: Option<SupportOrderState>,
    pub wind_walk: Option<WindWalkState>,
    /// A consumed manual improvement ability; target death restores it.
    pub mana_improvement_target: Option<SimId>,
    /// Native lightning and the permanent grant survive generator death.
    pub mana_improvement_link: Option<ManaImprovementLink>,
    pub retaliation_sequence: u64,
    pub hardened_skin_sequence: u64,
    /// Last authoritative order/movement heading, normalized by consumers.
    pub facing_direction: SimPoint,
    pub spawn_building: Option<SimId>,
    pub lava_spawn: Option<LavaSpawnState>,
    pub summon_order_refresh_tick: Option<u64>,
    pub mirror_image: Option<MirrorImageState>,
    pub illusion: Option<IllusionState>,
    pub damage_charge: Option<DamageChargeState>,
    pub spheres: [Option<SphereAttachmentState>; 4],
    pub purge: Option<PurgeState>,
    pub acid_bomb: Option<AcidBombStatus>,
    pub poison: Option<PoisonStatus>,
    pub damage_threshold: DamageThresholdState,
    pub ensnare: Option<EnsnareState>,
    pub action_animation: Option<ActionAnimationState>,
    pub pending_attack: Option<PendingAttackState>,
    pub pending_cast: Option<PendingCastState>,
    pub stunned_until_tick: u64,
    pub native_stun_until_tick: u64,
    pub native_stun_ability: Option<AbilityId>,
    pub rooted_until_tick: u64,
    /// Freezing Breath additionally suspends native building activities.
    pub frozen_until_tick: u64,
    pub frozen_ability: Option<AbilityId>,
    /// Independent script/order recovery, not a removable native stun buff.
    pub order_recovery_until_tick: u64,
    pub ability_retreat_start_tick: u64,
    pub ability_retreat_end_tick: u64,
    pub secondary_resurrection_due_tick: u64,
    pub secondary_resurrection_ready_tick: u64,
    pub secondary_resurrection_ability: Option<AbilityId>,
    pub native_grants: NativeAbilityGrants,
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
    pub fn wind_walk_active(&self, tick: u64) -> bool {
        self.wind_walk.is_some_and(|w| tick < w.expires_tick)
    }
    pub fn wind_walk_invisible(&self, tick: u64) -> bool {
        self.wind_walk
            .is_some_and(|w| tick >= w.cloaked_tick && tick < w.expires_tick)
    }
    #[must_use]
    pub fn is_performing_action(&self, tick: u64) -> bool {
        self.action_animation
            .is_some_and(|action| action.is_active(tick))
    }

    #[must_use]
    pub fn is_casting(&self, tick: u64) -> bool {
        self.action_animation
            .is_some_and(|action| action.kind.is_cast() && action.is_active(tick))
    }

    pub(crate) fn begin_action_animation(
        &mut self,
        kind: ActionAnimationKind,
        tick: u64,
        duration_ticks: u16,
    ) {
        if kind.is_cast() {
            self.pending_attack = None;
        }
        if duration_ticks > 0 {
            self.action_animation = Some(ActionAnimationState {
                kind,
                started_tick: tick,
                until_tick: tick
                    .checked_add(u64::from(duration_ticks))
                    .expect("action animation tick overflow"),
            });
        }
    }

    #[must_use]
    pub fn is_revealed_to(&self, team: Team, tick: u64) -> bool {
        self.armor_modifiers[..usize::from(self.armor_modifier_count)]
            .iter()
            .any(|modifier| modifier.revealed_to == Some(team) && tick < modifier.expires_tick)
    }

    #[must_use]
    pub const fn attacks_disabled(self, tick: u64) -> bool {
        tick < self.action_pause_until_tick() || tick < self.rooted_until_tick
    }

    #[must_use]
    pub const fn is_stunned(self, tick: u64) -> bool {
        tick < self.action_pause_until_tick()
    }

    pub const fn action_pause_until_tick(self) -> u64 {
        let purge = match self.purge {
            Some(p) => p.pause_until_tick,
            None => 0,
        };
        if purge > self.stunned_until_tick {
            purge
        } else {
            self.stunned_until_tick
        }
    }

    #[must_use]
    pub fn effective_armor_points_per_100(&self, armor: ArmorProfile) -> i32 {
        let count = usize::from(self.armor_modifier_count);
        debug_assert!(count <= MAX_TIMED_ARMOR_MODIFIERS);
        self.armor_modifiers[..count].iter().fold(
            i32::from(armor.armor_points) * 100 + self.native_grants.armor_bonus_per_100()
                - self
                    .acid_bomb
                    .map_or(0, |acid| i32::from(acid.profile.armor_penalty_per_100)),
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
