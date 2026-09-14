use bevy_ecs::prelude::Component;

use crate::{
    damage::{ArmorProfile, DamageType},
    math::SimPoint,
};

pub(crate) const MAX_BOUNCE_COUNT: u8 = 8;
pub(crate) const MAX_BOUNCE_HITS: usize = MAX_BOUNCE_COUNT as usize + 1;
pub(crate) const MAX_TIMED_MOVEMENT_MODIFIERS: usize = 8;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SimId(pub u64);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Team(pub u8);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position(pub SimPoint);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Health {
    pub current: i32,
    pub max: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AttackDelivery {
    Melee,
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
            Self::RangedGuaranteedHit { .. } => 1,
            Self::RangedBallistic { .. } => 2,
            Self::Bounce { .. } => 3,
        }
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PendingAttackEffects {
    pub stun_duration_ticks: u16,
    pub triggered_spell: Option<TriggeredAttackEffect>,
    pub burning_oil: Option<BurningOilEffectProfile>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct GuaranteedHitProjectile {
    pub source: SimId,
    pub source_team: Team,
    pub target: SimId,
    pub damage: i32,
    pub on_hit: PendingAttackEffects,
    pub damage_type: DamageType,
    pub launch_position: SimPoint,
    pub launch_tick: u64,
    pub impact_tick: u64,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BallisticProjectile {
    pub source: SimId,
    pub source_team: Team,
    pub target_mask: AttackTargetMask,
    pub damage: i32,
    pub burning_oil: Option<BurningOilEffectProfile>,
    pub damage_type: DamageType,
    pub launch_position: SimPoint,
    pub destination: SimPoint,
    pub impact_radius: i32,
    pub launch_tick: u64,
    pub impact_tick: u64,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BounceProjectile {
    pub source: SimId,
    pub source_team: Team,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AbilityId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CorpseDefinitionId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CorpseProfile {
    pub definition: CorpseDefinitionId,
    pub lifetime_ticks: Option<u32>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CorpseProducer(pub CorpseProfile);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionCorpseProfile(pub CorpseProfile);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CollisionRadius(pub i32);

#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum MovementClass {
    #[default]
    Ground,
    Air,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
}

impl Default for AttackTargetMask {
    fn default() -> Self {
        Self::ALL
    }
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentIdentity {
    pub rawcode: u32,
    pub name: &'static str,
}

pub const MAX_PASSIVE_UNIT_EFFECTS: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BashEffectProfile {
    pub ability: AbilityId,
    pub chance_per_10k: u16,
    pub bonus_damage: i32,
    pub stun_duration_ticks: u16,
    pub targets: AttackTargetMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EvasionEffectProfile {
    pub ability: AbilityId,
    pub chance_per_10k: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChainLightningEffectProfile {
    pub ability: AbilityId,
    pub initial_damage: i32,
    pub maximum_targets: u8,
    pub jump_radius: i32,
    pub damage_reduction_per_10k: u16,
    pub targets: AttackTargetMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntanglingRootsEffectProfile {
    pub ability: AbilityId,
    pub damage_per_second: i32,
    pub duration_ticks: u16,
    pub targets: AttackTargetMask,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggeredAttackEffect {
    ChainLightning(ChainLightningEffectProfile),
    EntanglingRoots(EntanglingRootsEffectProfile),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TriggeredSpellProcProfile {
    pub ability: AbilityId,
    pub chance_per_10k: u16,
    pub targets: AttackTargetMask,
    pub effect: TriggeredAttackEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PassiveUnitEffect {
    Bash(BashEffectProfile),
    Evasion(EvasionEffectProfile),
    TriggeredSpellProc(TriggeredSpellProcProfile),
    BurningOil(BurningOilEffectProfile),
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UnitGameplayProperties {
    pub content: Option<ContentIdentity>,
    pub corpse: Option<CorpseProfile>,
    pub collision_radius: Option<CollisionRadius>,
    pub movement_class: MovementClass,
    pub mechanical: bool,
    pub build_time_ticks: Option<u32>,
    pub attack_targets: AttackTargetMask,
    pub damage_type: DamageType,
    pub armor: ArmorProfile,
    pub passive_effects: PassiveUnitEffects,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BuildingGameplayProperties {
    pub content: Option<ContentIdentity>,
    pub attack_targets: AttackTargetMask,
    pub damage_type: DamageType,
    pub armor: ArmorProfile,
    pub production_unit: UnitGameplayProperties,
    pub production_spellcasting: Option<SpellcastingProfile>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionContentIdentity(pub ContentIdentity);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionCollisionRadius(pub CollisionRadius);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionMovementClass(pub MovementClass);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionUnitRepairMetadata {
    pub mechanical: bool,
    pub build_time_ticks: Option<u32>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionAttackTargets(pub AttackTargetMask);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionDamageType(pub DamageType);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionArmorProfile(pub ArmorProfile);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionPassiveEffects(pub PassiveUnitEffects);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProductionSpellcastingProfile(pub SpellcastingProfile);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Corpse {
    pub source_unit: SimId,
    pub source_team: Team,
    pub definition: CorpseDefinitionId,
    pub created_tick: u64,
    pub expires_tick: Option<u64>,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AttackCooldown {
    pub remaining: u16,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct AttackSequence(pub u64);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TargetState {
    pub current: Option<SimId>,
    pub direct_retaliation_lock: bool,
    pub ally_defense_lock: bool,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RetaliationState {
    pub attacker: Option<SimId>,
    pub attacked_tick: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum NavigationGoal {
    #[default]
    None,
    Objective(Team),
    Target(SimId),
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct NavigationState {
    pub avoidance_goal: NavigationGoal,
    pub bypass_side: i8,
    pub clear_ticks: u8,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MovementProfile {
    pub speed_per_tick: i32,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuilderProfile {
    pub speed_per_tick: i32,
    pub repair_range: i32,
    pub repair_autocast_range: i32,
    pub repair_time_ratio_numerator: u16,
    pub repair_time_ratio_denominator: u16,
    pub full_repair_duration_ticks: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BuilderLocomotion {
    Foot,
    Hover,
}

#[derive(Component, Debug, Clone, PartialEq, Eq)]
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

#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct BuilderState {
    pub destination: Option<SimPoint>,
    pub repair_target: Option<SimId>,
    pub repair_progress_remainder: u32,
    pub repair_autocast_enabled: bool,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct MechanicalUnit;

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BuildTimeTicks(pub u32);

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpawnTick(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnitTemplate {
    pub health: i32,
    pub attack: AttackProfile,
    pub movement: MovementProfile,
}

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

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionProfile {
    pub initial_delay_ticks: u16,
    pub interval_ticks: u16,
    pub search_radius_cells: u16,
    pub unit: UnitTemplate,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProductionState {
    pub next_spawn_tick: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModifierId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AbilityTargetPolicy {
    RandomEnemyUnit,
    AllEnemyUnits,
    RandomEnemyUnitGlobal,
    RecentlyAttackedFriendlyUnit,
}

impl AbilityTargetPolicy {
    #[must_use]
    pub const fn stable_tag(self) -> u8 {
        match self {
            Self::RandomEnemyUnit => 0,
            Self::AllEnemyUnits => 1,
            Self::RandomEnemyUnitGlobal => 2,
            Self::RecentlyAttackedFriendlyUnit => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    },
    FrostArmor {
        modifier: ModifierId,
        armor_bonus_per_100: i16,
        armor_duration_ticks: u16,
        slow_duration_ticks: u16,
        movement_percent_delta: i16,
        attack_speed_percent_delta: i16,
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
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManaProfile {
    pub maximum: i32,
    pub starting: i32,
    /// Mana regenerated per simulation tick in 1/10,000 mana units.
    pub regen_per_tick_per_10k: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomaticAbilityProfile {
    pub id: AbilityId,
    pub mana_cost: i32,
    pub cooldown_ticks: u16,
    pub range: i32,
    pub target_policy: AbilityTargetPolicy,
    pub effect: AbilityEffect,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpellcastingProfile {
    pub mana: ManaProfile,
    pub ability: AutomaticAbilityProfile,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ManaState {
    pub current: i32,
    pub regen_remainder_per_10k: u16,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutomaticAbilityState {
    pub ready_tick: u64,
    pub cast_sequence: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimedMovementModifier {
    pub id: ModifierId,
    pub percent_delta: i16,
    pub expires_tick: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimedAttackSpeedModifier {
    pub id: ModifierId,
    pub percent_delta: i16,
    pub expires_tick: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimedArmorModifier {
    pub id: ModifierId,
    pub armor_bonus_per_100: i16,
    pub expires_tick: u64,
    pub reactive_slow_duration_ticks: u16,
    pub reactive_movement_percent_delta: i16,
    pub reactive_attack_speed_percent_delta: i16,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TimedDamageOverTime {
    pub id: ModifierId,
    pub damage_per_pulse: i32,
    pub pulse_interval_ticks: u16,
    pub next_pulse_tick: u64,
    pub expires_tick: u64,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BurningOilZone {
    pub source: SimId,
    pub source_team: Team,
    pub center: SimPoint,
    pub profile: BurningOilEffectProfile,
    pub created_tick: u64,
    pub pulse_index: u16,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
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

#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StatusState {
    pub stunned_until_tick: u64,
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
    pub const fn is_stunned(self, tick: u64) -> bool {
        tick < self.stunned_until_tick
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BuildingSpawn {
    pub team: Team,
    pub footprint: BuildingFootprint,
    pub health: i32,
    pub production: Option<ProductionProfile>,
    pub attack: Option<AttackProfile>,
    pub spellcasting: Option<SpellcastingProfile>,
}
