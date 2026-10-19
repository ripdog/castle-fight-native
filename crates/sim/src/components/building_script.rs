use super::*;

/// Map-script consequence of a native building trigger. The parent (an inert Parasite on an
/// enemy) only decides when the building fires; the child chooses its own recipients.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BuildingChild {
    /// Heals life and mana of a random wounded ally, preferring badly wounded ones.
    Replenish {
        life: i32,
        mana: i32,
        /// Life missing above which an ally is preferred first.
        severe_missing_life: i32,
        /// Mana percentage (per 10k) strictly between zero and this bound qualifies last.
        mana_below_per_10k: u16,
    },
    /// Creates an illusion of a random vulnerable enemy combat body for the building's player.
    Hallucination {
        illusion_ability: AbilityId,
        illusion_ticks: u16,
        damage_dealt_per_10k: u16,
        damage_taken_per_10k: u16,
        /// Bodies carrying this ability (the tentacle marker) are never copied.
        excluded_ability: AbilityId,
    },
    /// Burns the mana of a random hostile combat body and combusts it for a multiple of the burn,
    /// preferring bodies holding more than the priority threshold.
    ManaBurn {
        /// Maximum mana burned by one cast.
        cap: i32,
        /// Bodies with strictly more mana than this are chosen first.
        priority_above: i32,
        /// Combustion damage per point of mana actually burned.
        damage_per_mana: i32,
        /// Bodies carrying this ability (the tentacle marker) are never chosen.
        excluded_ability: AbilityId,
    },
    /// Stores one team charge that the shared damage listener spends on the next qualifying melee
    /// attack of an allied unit.
    ThunderCharge(ThunderCallProfile),
    /// A map-wide debuff plus a pulsing immolation on every hostile unit for a fixed time.
    Sandstorm {
        debuff: NativeDebuffProfile,
        pulse_ability: AbilityId,
        pulse_damage: i32,
        pulse_interval_ticks: u16,
        duration_ticks: u16,
        targets: AttackTargetMask,
    },
}

impl BuildingChild {
    pub(crate) fn canonical_words(self) -> Vec<u64> {
        match self {
            Self::Replenish {
                life,
                mana,
                severe_missing_life,
                mana_below_per_10k,
            } => vec![
                1,
                life as u64,
                mana as u64,
                severe_missing_life as u64,
                u64::from(mana_below_per_10k),
            ],
            Self::Hallucination {
                illusion_ability,
                illusion_ticks,
                damage_dealt_per_10k,
                damage_taken_per_10k,
                excluded_ability,
            } => vec![
                2,
                u64::from(illusion_ability.0),
                u64::from(illusion_ticks),
                u64::from(damage_dealt_per_10k),
                u64::from(damage_taken_per_10k),
                u64::from(excluded_ability.0),
            ],
            Self::ThunderCharge(profile) => {
                let mut words = vec![5];
                words.extend(profile.canonical_words());
                words
            }
            Self::ManaBurn {
                cap,
                priority_above,
                damage_per_mana,
                excluded_ability,
            } => vec![
                4,
                cap as u64,
                priority_above as u64,
                damage_per_mana as u64,
                u64::from(excluded_ability.0),
            ],
            Self::Sandstorm {
                debuff,
                pulse_ability,
                pulse_damage,
                pulse_interval_ticks,
                duration_ticks,
                targets,
            } => {
                let mut words = vec![
                    3,
                    u64::from(pulse_ability.0),
                    pulse_damage as u64,
                    u64::from(pulse_interval_ticks),
                    u64::from(duration_ticks),
                    u64::from(targets.bits()),
                ];
                words.extend(debuff.canonical_words());
                words
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BuildingScriptProfile {
    pub ability: AbilityId,
    pub trigger_targets: AttackTargetMask,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub trigger_mechanical_only: bool,
    pub child: BuildingChild,
}

impl BuildingScriptProfile {
    pub(crate) fn canonical_words(self) -> Vec<u64> {
        let mut words = vec![
            u64::from(self.ability.0),
            u64::from(self.trigger_targets.bits()),
            u64::from(self.trigger_invulnerable),
            u64::from(self.trigger_spell_immune),
            u64::from(self.trigger_mechanical_only),
        ];
        words.extend(self.child.canonical_words());
        words
    }
}

/// A map-wide pulsing damage actor owned by the casting player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SandstormState {
    pub source: SimId,
    pub team: Team,
    pub ability: AbilityId,
    pub damage: i32,
    pub targets: AttackTargetMask,
    pub interval_ticks: u16,
    pub next_pulse_tick: u64,
    pub expires_tick: u64,
}

/// Call of Thunder: the damage listener's lightning strike around the damaged target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ThunderCallProfile {
    pub ability: AbilityId,
    /// The attack's reduced damage must exceed this to consume a charge.
    pub minimum_event_damage_exclusive: i32,
    /// The attacker's first weapon range must lie strictly between these bounds...
    pub weapon_range_lower_exclusive: i32,
    pub weapon_range_upper_exclusive: i32,
    /// ...and its second weapon's range strictly below the upper bound.
    pub strike_minimum: i32,
    pub strike_maximum: i32,
    pub strike_multiplier: i32,
    pub radius: i32,
    pub flying_per_10k: u16,
    /// Attackers carrying this marker ability strike with the reduced factor below.
    pub special_source_ability: AbilityId,
    pub special_source_per_10k: u16,
}

impl ThunderCallProfile {
    pub(crate) fn canonical_words(self) -> [u64; 11] {
        [
            u64::from(self.ability.0),
            self.minimum_event_damage_exclusive as u64,
            self.weapon_range_lower_exclusive as u64,
            self.weapon_range_upper_exclusive as u64,
            self.strike_minimum as u64,
            self.strike_maximum as u64,
            self.strike_multiplier as u64,
            self.radius as u64,
            u64::from(self.flying_per_10k),
            u64::from(self.special_source_ability.0),
            u64::from(self.special_source_per_10k),
        ]
    }
}

/// Team-indexed stored charges and the damage listener's same-instant source guard.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThunderCallState {
    pub profile: ThunderCallProfile,
    pub charges: [u32; 2],
    /// The source that just consumed a charge cannot consume another within the same tick.
    pub guard: Option<(SimId, u64)>,
}
