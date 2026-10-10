use super::*;

pub const MAX_PERMANENT_NATIVE_ABILITIES: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NativeAbilityGrantEffect {
    HardenedSkin {
        chance_per_10k: u16,
        ignored_damage: i32,
        minimum_damage: i32,
    },
    SpellResistance(u16),
    AttackDamage(i32),
    AttackSpeed(i16),
    MaximumLife(i32),
    Armor(i16),
    CriticalStrike {
        chance_per_10k: u16,
        multiplier_per_10k: u16,
        targets: AttackTargetMask,
    },
    Evasion {
        chance_per_10k: u16,
    },
    Demolish {
        chance_per_10k: u16,
        multiplier_per_10k: u16,
        targets: AttackTargetMask,
    },
    ManaRegeneration(u32),
    Speed {
        attack: i16,
        movement: i16,
        buff_rawcode: u32,
    },
    SpellBookArmor {
        bonus_per_100: i16,
        child: AbilityId,
        badge: AbilityId,
    },
    SourceDamageSplash {
        dispatcher: AbilityId,
        damage_per_10k: u16,
        radius: i32,
    },
    SourceDamageDispatcher {
        aftershock: Option<HealthScaledAftershockProfile>,
    },
    AuraMarker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NativeAbilityGrant {
    pub ability: AbilityId,
    pub effect: NativeAbilityGrantEffect,
}

impl NativeAbilityGrant {
    pub(crate) fn canonical_words(self) -> [u64; 5] {
        use NativeAbilityGrantEffect::*;
        let (tag, values) = match self.effect {
            HardenedSkin {
                chance_per_10k,
                ignored_damage,
                minimum_damage,
            } => (
                13,
                [
                    u64::from(chance_per_10k),
                    ignored_damage as u64,
                    minimum_damage as u64,
                ],
            ),
            SpellResistance(factor) => (14, [u64::from(factor), 0, 0]),
            AttackDamage(amount) => (0, [amount as u64, 0, 0]),
            AttackSpeed(amount) => (1, [amount as u64, 0, 0]),
            MaximumLife(amount) => (2, [amount as u64, 0, 0]),
            Armor(amount) => (3, [amount as u64, 0, 0]),
            CriticalStrike {
                chance_per_10k,
                multiplier_per_10k,
                targets,
            } => (
                4,
                [
                    u64::from(chance_per_10k),
                    u64::from(multiplier_per_10k),
                    u64::from(targets.bits()),
                ],
            ),
            Evasion { chance_per_10k } => (5, [u64::from(chance_per_10k), 0, 0]),
            Demolish {
                chance_per_10k,
                multiplier_per_10k,
                targets,
            } => (
                6,
                [
                    u64::from(chance_per_10k),
                    u64::from(multiplier_per_10k),
                    u64::from(targets.bits()),
                ],
            ),
            ManaRegeneration(amount) => (8, [u64::from(amount), 0, 0]),
            Speed {
                attack,
                movement,
                buff_rawcode,
            } => (9, [attack as u64, movement as u64, u64::from(buff_rawcode)]),
            SourceDamageSplash {
                dispatcher,
                damage_per_10k,
                radius,
            } => (
                10,
                [
                    u64::from(dispatcher.0),
                    u64::from(damage_per_10k),
                    radius as u64,
                ],
            ),
            SpellBookArmor {
                bonus_per_100,
                child,
                badge,
            } => (
                11,
                [bonus_per_100 as u64, u64::from(child.0), u64::from(badge.0)],
            ),
            SourceDamageDispatcher { aftershock } => (
                12,
                aftershock.map_or([0; 3], HealthScaledAftershockProfile::canonical_words),
            ),
            AuraMarker => (7, [0, 0, 0]),
        };
        [
            u64::from(self.ability.0),
            tag,
            values[0],
            values[1],
            values[2],
        ]
    }

    fn passive(self) -> Option<PassiveUnitEffect> {
        match self.effect {
            NativeAbilityGrantEffect::SpellResistance(damage_taken_per_10k) => Some(
                PassiveUnitEffect::SpellResistance(SpellResistanceEffectProfile {
                    ability: self.ability,
                    damage_taken_per_10k,
                }),
            ),
            NativeAbilityGrantEffect::CriticalStrike {
                chance_per_10k,
                multiplier_per_10k,
                targets,
            } => Some(PassiveUnitEffect::CriticalStrike(
                CriticalStrikeEffectProfile {
                    ability: self.ability,
                    chance_per_10k,
                    damage_multiplier_per_10k: multiplier_per_10k,
                    targets,
                },
            )),
            NativeAbilityGrantEffect::Evasion { chance_per_10k } => {
                Some(PassiveUnitEffect::Evasion(EvasionEffectProfile {
                    ability: self.ability,
                    chance_per_10k,
                }))
            }
            _ => None,
        }
    }
}

/// Identity-scoped native ability grants, distinct from dispellable buffs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Vec<NativeAbilityGrant>", into = "Vec<NativeAbilityGrant>")]
pub struct NativeAbilityGrants {
    entries: [Option<NativeAbilityGrant>; MAX_PERMANENT_NATIVE_ABILITIES],
    count: u8,
}

impl NativeAbilityGrants {
    pub fn from_slice(grants: &[NativeAbilityGrant]) -> Self {
        let mut result = Self::default();
        for grant in grants {
            result.insert(*grant);
        }
        result
    }

    pub fn iter(&self) -> impl Iterator<Item = NativeAbilityGrant> + '_ {
        self.entries[..usize::from(self.count)]
            .iter()
            .map(|entry| entry.expect("active grant slot"))
    }

    pub fn contains(&self, ability: AbilityId) -> bool {
        self.iter().any(|grant| grant.ability == ability || matches!(grant.effect,
            NativeAbilityGrantEffect::SpellBookArmor { child, badge, .. } if ability == child || ability == badge))
    }

    /// UnitAddAbility leaves an already installed identity unchanged.
    pub(crate) fn insert(&mut self, grant: NativeAbilityGrant) -> bool {
        if self.contains(grant.ability) {
            return false;
        }
        let count = usize::from(self.count);
        assert!(
            count < MAX_PERMANENT_NATIVE_ABILITIES,
            "native ability grant capacity exhausted"
        );
        let slot = self
            .iter()
            .take_while(|entry| entry.ability < grant.ability)
            .count();
        self.entries.copy_within(slot..count, slot + 1);
        self.entries[slot] = Some(grant);
        self.count += 1;
        true
    }

    /// Return the revoked native life adjustment; other payloads are computed from survivors.
    pub(crate) fn remove(&mut self, abilities: &[AbilityId]) -> i32 {
        let mut life = 0;
        let mut write = 0;
        for read in 0..usize::from(self.count) {
            let grant = self.entries[read].expect("active grant slot");
            if abilities.contains(&grant.ability) {
                if let NativeAbilityGrantEffect::MaximumLife(amount) = grant.effect {
                    life += amount;
                }
            } else {
                self.entries[write] = Some(grant);
                write += 1;
            }
        }
        self.entries[write..].fill(None);
        self.count = write as u8;
        life
    }

    pub(crate) fn passive_effects(&self) -> impl Iterator<Item = PassiveUnitEffect> + '_ {
        self.iter().filter_map(NativeAbilityGrant::passive)
    }

    pub fn attack_damage(&self) -> i32 {
        self.iter()
            .filter_map(|grant| match grant.effect {
                NativeAbilityGrantEffect::AttackDamage(amount) => Some(amount),
                _ => None,
            })
            .sum()
    }

    pub fn attack_speed_percent(&self) -> i32 {
        self.iter()
            .filter_map(|grant| match grant.effect {
                NativeAbilityGrantEffect::AttackSpeed(amount) => Some(i32::from(amount)),
                _ => None,
            })
            .sum()
    }

    pub fn mana_regeneration_per_second_per_10k(&self) -> u32 {
        self.iter()
            .filter_map(|grant| match grant.effect {
                NativeAbilityGrantEffect::ManaRegeneration(amount) => Some(amount),
                _ => None,
            })
            .sum()
    }

    pub fn armor_bonus_per_100(&self) -> i32 {
        self.iter()
            .filter_map(|grant| match grant.effect {
                NativeAbilityGrantEffect::Armor(amount) => Some(i32::from(amount)),
                NativeAbilityGrantEffect::SpellBookArmor { bonus_per_100, .. } => {
                    Some(i32::from(bonus_per_100))
                }
                _ => None,
            })
            .sum()
    }
}

impl From<NativeAbilityGrants> for Vec<NativeAbilityGrant> {
    fn from(value: NativeAbilityGrants) -> Self {
        value.iter().collect()
    }
}
impl TryFrom<Vec<NativeAbilityGrant>> for NativeAbilityGrants {
    type Error = &'static str;
    fn try_from(value: Vec<NativeAbilityGrant>) -> Result<Self, Self::Error> {
        if value.len() > MAX_PERMANENT_NATIVE_ABILITIES {
            return Err("native grant capacity exceeded");
        }
        let mut result = Self::default();
        for grant in value {
            match grant.effect {
                NativeAbilityGrantEffect::HardenedSkin {
                    chance_per_10k,
                    ignored_damage,
                    minimum_damage,
                } if chance_per_10k > 10000 || ignored_damage < 0 || minimum_damage < 0 => {
                    return Err("invalid native Hardened Skin");
                }
                NativeAbilityGrantEffect::SpellResistance(factor) if factor > 10000 => {
                    return Err("invalid native resistance");
                }
                NativeAbilityGrantEffect::CriticalStrike {
                    chance_per_10k,
                    multiplier_per_10k,
                    ..
                }
                | NativeAbilityGrantEffect::Demolish {
                    chance_per_10k,
                    multiplier_per_10k,
                    ..
                } if chance_per_10k > 10_000 || multiplier_per_10k < 10_000 => {
                    return Err("invalid native weapon multiplier");
                }
                NativeAbilityGrantEffect::Evasion { chance_per_10k } if chance_per_10k > 10_000 => {
                    return Err("invalid native evasion chance");
                }
                _ => {}
            }
            if !result.insert(grant) {
                return Err("duplicate native grant identity");
            }
        }
        Ok(result)
    }
}
