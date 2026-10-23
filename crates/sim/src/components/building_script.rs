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
    /// Kills a random hostile combat body outright with Death damage (which ignores armor).
    DeathPit {
        damage: i32,
        /// Bodies carrying this ability (the tentacle marker) are never chosen.
        excluded_ability: AbilityId,
        /// This unit type is never chosen.
        excluded_type: u32,
    },
    /// Consumes the corpse of a random dying combat body (of either team, never undead or
    /// invulnerable-marked) and creates a skeleton there for the building's player: the rare body
    /// with the retained chance, otherwise one of the common bodies uniformly.
    RaiseFromCorpse {
        map_version: crate::MapVersion,
        rare_body: u32,
        rare_chance_per_10k: u16,
        common_bodies: [u32; 3],
        /// Bodies carrying this marker (the invulnerable ability) are never chosen.
        excluded_ability: AbilityId,
    },
    /// Stores one team charge that the shared damage listener spends on the next qualifying melee
    /// attack of an allied unit.
    ThunderCharge(ThunderCallProfile),
    /// Grants the native A09L energy shield to a random allied combat body anywhere on the map.
    EnergyShield {
        /// The shield ability, whose level is the number of negative building effects it blocks.
        shield_ability: AbilityId,
        /// The level every cast leaves on the chosen body.
        level: u8,
        /// When no unshielded ally exists, any ally is chosen and raised to `level`.
        upgrade_existing: bool,
    },
    /// Banishes (a native ethereal debuff) a random hostile combat body after the shield check.
    Banish {
        debuff: NativeDebuffProfile,
        /// Bodies carrying this ability (the tentacle marker) are never chosen.
        excluded_ability: AbilityId,
        /// This unit type is never chosen.
        excluded_type: u32,
        /// The dummy caster reveals a hidden target to the building's player for this long.
        vision_radius: i32,
        vision_ticks: u16,
    },
    /// Summons a stationary Tornado on a random hostile combat body: `options[0]` while artillery
    /// is enabled, `options[1]` otherwise.
    Tornado {
        options: [TornadoProfile; 2],
        /// Bodies carrying this ability (the tentacle marker) are never chosen.
        excluded_ability: AbilityId,
        /// This unit type is never chosen.
        excluded_type: u32,
    },
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
            Self::EnergyShield {
                shield_ability,
                level,
                upgrade_existing,
            } => vec![
                6,
                u64::from(shield_ability.0),
                u64::from(level),
                u64::from(upgrade_existing),
            ],
            Self::RaiseFromCorpse {
                map_version,
                rare_body,
                rare_chance_per_10k,
                common_bodies,
                excluded_ability,
            } => vec![
                8,
                u64::from(map_version.major),
                u64::from(map_version.minor),
                u64::from(rare_body),
                u64::from(rare_chance_per_10k),
                u64::from(common_bodies[0]),
                u64::from(common_bodies[1]),
                u64::from(common_bodies[2]),
                u64::from(excluded_ability.0),
            ],
            Self::DeathPit {
                damage,
                excluded_ability,
                excluded_type,
            } => vec![
                7,
                damage as u64,
                u64::from(excluded_ability.0),
                u64::from(excluded_type),
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
            Self::Banish {
                debuff,
                excluded_ability,
                excluded_type,
                vision_radius,
                vision_ticks,
            } => {
                let mut words = vec![
                    9,
                    u64::from(excluded_ability.0),
                    u64::from(excluded_type),
                    vision_radius as u64,
                    u64::from(vision_ticks),
                ];
                words.extend(debuff.canonical_words());
                words
            }
            Self::Tornado {
                options,
                excluded_ability,
                excluded_type,
            } => {
                let mut words = vec![10, u64::from(excluded_ability.0), u64::from(excluded_type)];
                for option in options {
                    words.extend(option.canonical_words());
                }
                words
            }
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

/// One damage aura of a Tornado body (`Atdg`): damage per second inside a radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TornadoDamage {
    pub ability: AbilityId,
    pub radius: i32,
    pub damage_per_second: i32,
    /// Movement classes of the combat bodies struck (the `sapper` token); empty when none are.
    pub combat_bodies: AttackTargetMask,
    /// Every mechanical unit is struck regardless of movement class (the `mechanical` token).
    pub mechanical_units: bool,
    /// Enemy structures are struck (the `structure` token).
    pub structures: bool,
    /// Bodies carrying the invulnerable marker are struck (the `invulnerable` token).
    pub affects_invulnerable: bool,
}

/// The Tornado body's hostile slow aura (`Aasl`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TornadoSlow {
    pub ability: AbilityId,
    pub buff: u32,
    pub radius: i32,
    pub movement_percent_delta: i16,
    pub attack_speed_percent_delta: i16,
    pub targets: AttackTargetMask,
    pub affects_invulnerable: bool,
}

/// A summoned Tornado: a stationary locust body whose auras are the whole effect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TornadoProfile {
    /// The Summon Tornado ability (and so the body) this option casts.
    pub ability: AbilityId,
    pub body_rawcode: u32,
    pub duration_ticks: u16,
    /// The damage auras are per-second rates, struck once per this period.
    pub pulse_ticks: u16,
    pub unit_damage: TornadoDamage,
    pub structure_damage: Option<TornadoDamage>,
    pub slow: TornadoSlow,
}

impl TornadoDamage {
    pub(crate) fn canonical_words(self) -> [u64; 7] {
        [
            u64::from(self.ability.0),
            self.radius as u64,
            self.damage_per_second as u64,
            u64::from(self.combat_bodies.bits()),
            u64::from(self.mechanical_units),
            u64::from(self.structures),
            u64::from(self.affects_invulnerable),
        ]
    }
}

impl TornadoProfile {
    pub(crate) fn canonical_words(self) -> Vec<u64> {
        let mut words = vec![
            u64::from(self.ability.0),
            u64::from(self.body_rawcode),
            u64::from(self.duration_ticks),
            u64::from(self.pulse_ticks),
        ];
        words.extend(self.unit_damage.canonical_words());
        words.push(u64::from(self.structure_damage.is_some()));
        if let Some(damage) = self.structure_damage {
            words.extend(damage.canonical_words());
        }
        words.extend([
            u64::from(self.slow.ability.0),
            u64::from(self.slow.buff),
            self.slow.radius as u64,
            self.slow.movement_percent_delta as u64,
            self.slow.attack_speed_percent_delta as u64,
            u64::from(self.slow.targets.bits()),
            u64::from(self.slow.affects_invulnerable),
        ]);
        words
    }
}

/// A live Tornado body. It strikes once per second starting one second after the summon.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TornadoState {
    pub source: SimId,
    pub team: Team,
    pub center: SimPoint,
    pub profile: TornadoProfile,
    pub next_pulse_tick: u64,
    pub expires_tick: u64,
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
