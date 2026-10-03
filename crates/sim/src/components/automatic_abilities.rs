use super::*;

/// An explicit engine capacity, not a map-derived unit limit. Only multi-ability sources
/// carry this component; ordinary unit snapshots remain unchanged in size.
pub const MAX_AUTOMATIC_ABILITIES: usize = 8;
const MAX_ADDITIONAL_AUTOMATIC_ABILITIES: usize = MAX_AUTOMATIC_ABILITIES - 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AbilityConfigurationError {
    MissingSpellcaster,
    CapacityExceeded,
    UnsupportedDelayedSource,
    DuplicateAbility(AbilityId),
    DefinitionChanged(AbilityId),
    IncompatibleManaProfile(AbilityId),
}

impl std::fmt::Display for AbilityConfigurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSpellcaster => f.write_str("source has no spellcasting mana pool"),
            Self::CapacityExceeded => f.write_str("additional automatic ability capacity exceeded"),
            Self::UnsupportedDelayedSource => {
                f.write_str("delayed secondary resurrection requires a combat-unit source")
            }
            Self::DuplicateAbility(id) => write!(f, "duplicate automatic ability {:#010x}", id.0),
            Self::IncompatibleManaProfile(id) => write!(
                f,
                "automatic ability {:#010x} declares a different source mana pool",
                id.0
            ),
            Self::DefinitionChanged(id) => write!(
                f,
                "automatic ability definition changed in place: {:#010x}",
                id.0
            ),
        }
    }
}
impl std::error::Error for AbilityConfigurationError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AutomaticAbilityInstance {
    pub profile: AutomaticAbilityProfile,
    pub state: AutomaticAbilityState,
    pub secondary_resurrection: SecondaryResurrectionState,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SecondaryResurrectionState {
    pub due_tick: u64,
    pub ready_tick: u64,
}

#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "Vec<AutomaticAbilityInstance>",
    into = "Vec<AutomaticAbilityInstance>"
)]
pub struct AdditionalAutomaticAbilities {
    entries: [Option<AutomaticAbilityInstance>; MAX_ADDITIONAL_AUTOMATIC_ABILITIES],
    count: u8,
}

impl AdditionalAutomaticAbilities {
    pub(crate) fn from_definitions(
        definitions: AdditionalAutomaticAbilityDefinitions,
        ready_tick: u64,
    ) -> Self {
        let mut result = Self {
            count: 0,
            entries: [None; MAX_ADDITIONAL_AUTOMATIC_ABILITIES],
        };
        for (index, profile) in definitions.iter().enumerate() {
            result.entries[index] = Some(AutomaticAbilityInstance {
                profile,
                state: AutomaticAbilityState {
                    ready_tick,
                    cast_sequence: 0,
                    autocast_enabled: true,
                    manual_cast_requested: false,
                },
                secondary_resurrection: SecondaryResurrectionState::default(),
            });
            result.count = (index + 1) as u8;
        }
        result.entries[..usize::from(result.count)]
            .sort_unstable_by_key(|entry| entry.expect("populated ability slot").profile.id);
        result
    }

    pub fn iter(&self) -> impl Iterator<Item = &AutomaticAbilityInstance> {
        self.entries[..usize::from(self.count)].iter().flatten()
    }

    pub fn get(&self, id: AbilityId) -> Option<&AutomaticAbilityInstance> {
        self.iter().find(|entry| entry.profile.id == id)
    }

    pub fn get_mut(&mut self, id: AbilityId) -> Option<&mut AutomaticAbilityInstance> {
        self.entries[..usize::from(self.count)]
            .iter_mut()
            .flatten()
            .find(|entry| entry.profile.id == id)
    }
}

impl TryFrom<Vec<AutomaticAbilityInstance>> for AdditionalAutomaticAbilities {
    type Error = AbilityConfigurationError;
    fn try_from(mut entries: Vec<AutomaticAbilityInstance>) -> Result<Self, Self::Error> {
        if entries.len() > MAX_ADDITIONAL_AUTOMATIC_ABILITIES {
            return Err(AbilityConfigurationError::CapacityExceeded);
        }
        entries.sort_unstable_by_key(|entry| entry.profile.id);
        if let Some(pair) = entries
            .windows(2)
            .find(|pair| pair[0].profile.id == pair[1].profile.id)
        {
            return Err(AbilityConfigurationError::DuplicateAbility(
                pair[0].profile.id,
            ));
        }
        let mut result = Self {
            entries: [None; MAX_ADDITIONAL_AUTOMATIC_ABILITIES],
            count: entries.len() as u8,
        };
        for (slot, entry) in result.entries.iter_mut().zip(entries) {
            *slot = Some(entry);
        }
        Ok(result)
    }
}

/// Cold resurrection/production metadata: definitions only, never a dead caster's timers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    try_from = "Vec<AutomaticAbilityProfile>",
    into = "Vec<AutomaticAbilityProfile>"
)]
pub struct AdditionalAutomaticAbilityDefinitions {
    profiles: [Option<AutomaticAbilityProfile>; MAX_ADDITIONAL_AUTOMATIC_ABILITIES],
}

impl AdditionalAutomaticAbilityDefinitions {
    pub fn from_runtime(runtime: AdditionalAutomaticAbilities) -> Self {
        Self {
            profiles: runtime
                .entries
                .map(|entry| entry.map(|entry| entry.profile)),
        }
    }
    pub fn iter(&self) -> impl Iterator<Item = AutomaticAbilityProfile> + '_ {
        self.profiles.iter().flatten().copied()
    }

    pub fn try_from_profiles(
        profiles: impl IntoIterator<Item = AutomaticAbilityProfile>,
    ) -> Result<Self, AbilityConfigurationError> {
        let mut result = Self {
            profiles: [None; MAX_ADDITIONAL_AUTOMATIC_ABILITIES],
        };
        let mut count = 0;
        for profile in profiles {
            if count == MAX_ADDITIONAL_AUTOMATIC_ABILITIES {
                return Err(AbilityConfigurationError::CapacityExceeded);
            }
            result.profiles[count] = Some(profile);
            count += 1;
        }
        let populated = &mut result.profiles[..count];
        populated.sort_unstable_by_key(|profile| profile.expect("populated definition slot").id);
        for pair in populated.windows(2) {
            let id = pair[0].expect("populated definition slot").id;
            if id == pair[1].expect("populated definition slot").id {
                return Err(AbilityConfigurationError::DuplicateAbility(id));
            }
        }
        Ok(result)
    }
}
impl TryFrom<Vec<AutomaticAbilityProfile>> for AdditionalAutomaticAbilityDefinitions {
    type Error = AbilityConfigurationError;
    fn try_from(profiles: Vec<AutomaticAbilityProfile>) -> Result<Self, Self::Error> {
        Self::try_from_profiles(profiles)
    }
}
impl From<AdditionalAutomaticAbilityDefinitions> for Vec<AutomaticAbilityProfile> {
    fn from(value: AdditionalAutomaticAbilityDefinitions) -> Self {
        value.iter().collect()
    }
}

/// Compose translated abilities without losing a native or scripted source. The explicit
/// primary preserves existing control-command identity; otherwise the lowest ID is selected.
pub(crate) fn compose_spellcasting_profiles(
    primary: Option<SpellcastingProfile>,
    profiles: impl IntoIterator<Item = SpellcastingProfile>,
) -> Result<
    (
        Option<SpellcastingProfile>,
        Option<AdditionalAutomaticAbilityDefinitions>,
    ),
    AbilityConfigurationError,
> {
    let preferred = primary.map(|profile| profile.ability.id);
    let mut collected = Vec::new();
    for profile in primary.into_iter().chain(profiles) {
        if collected.len() == MAX_AUTOMATIC_ABILITIES {
            return Err(AbilityConfigurationError::CapacityExceeded);
        }
        collected.push(profile);
    }
    collected.sort_unstable_by_key(|profile| profile.ability.id);
    for pair in collected.windows(2) {
        if pair[0].ability.id == pair[1].ability.id {
            return Err(AbilityConfigurationError::DuplicateAbility(
                pair[0].ability.id,
            ));
        }
    }
    let Some(selected) = collected
        .iter()
        .find(|profile| Some(profile.ability.id) == preferred)
        .or_else(|| collected.first())
        .copied()
    else {
        return Ok((None, None));
    };
    for profile in &collected {
        if profile.mana != selected.mana {
            return Err(AbilityConfigurationError::IncompatibleManaProfile(
                profile.ability.id,
            ));
        }
    }
    let additional = if collected.len() > 1 {
        Some(AdditionalAutomaticAbilityDefinitions::try_from_profiles(
            collected
                .iter()
                .filter(|profile| profile.ability.id != selected.ability.id)
                .map(|profile| profile.ability),
        )?)
    } else {
        None
    };
    Ok((Some(selected), additional))
}

impl From<AdditionalAutomaticAbilities> for Vec<AutomaticAbilityInstance> {
    fn from(value: AdditionalAutomaticAbilities) -> Self {
        value.iter().copied().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(id: u32) -> SpellcastingProfile {
        SpellcastingProfile {
            mana: ManaProfile {
                maximum: 20,
                starting: 10,
                regen_per_tick_per_10k: 3,
            },
            ability: AutomaticAbilityProfile {
                id: AbilityId(id),
                mana_cost: 1,
                cooldown_ticks: 9,
                range: 7,
                target_policy: AbilityTargetPolicy::RandomEnemyUnit,
                effect: AbilityEffect::Damage { amount: 5 },
            },
        }
    }

    #[test]
    fn translated_profile_composition_is_order_independent_and_preserves_explicit_primary() {
        let expected =
            compose_spellcasting_profiles(None, [profile(3), profile(1), profile(2)]).unwrap();
        assert_eq!(
            compose_spellcasting_profiles(None, [profile(2), profile(3), profile(1)]).unwrap(),
            expected
        );
        assert_eq!(expected.0, Some(profile(1)));
        let preferred =
            compose_spellcasting_profiles(Some(profile(3)), [profile(2), profile(1)]).unwrap();
        assert_eq!(preferred.0, Some(profile(3)));
        assert_eq!(
            preferred.1.unwrap().iter().collect::<Vec<_>>(),
            [profile(1).ability, profile(2).ability]
        );
    }

    #[test]
    fn translated_profiles_reject_aliases_mana_conflicts_and_capacity_overflow() {
        assert_eq!(
            compose_spellcasting_profiles(Some(profile(1)), [profile(1)]),
            Err(AbilityConfigurationError::DuplicateAbility(AbilityId(1)))
        );
        let incompatible = SpellcastingProfile {
            mana: ManaProfile {
                maximum: 21,
                ..profile(2).mana
            },
            ..profile(2)
        };
        assert_eq!(
            compose_spellcasting_profiles(Some(profile(1)), [incompatible]),
            Err(AbilityConfigurationError::IncompatibleManaProfile(
                AbilityId(2)
            ))
        );
        assert!(
            compose_spellcasting_profiles(None, (0..MAX_AUTOMATIC_ABILITIES as u32).map(profile))
                .is_ok()
        );
        assert_eq!(
            compose_spellcasting_profiles(None, (0..=MAX_AUTOMATIC_ABILITIES as u32).map(profile)),
            Err(AbilityConfigurationError::CapacityExceeded)
        );
    }

    #[test]
    fn cold_definitions_instantiate_fresh_sorted_runtime_without_carrying_timers() {
        let definitions = AdditionalAutomaticAbilityDefinitions::try_from_profiles([
            profile(3).ability,
            profile(2).ability,
        ])
        .unwrap();
        let runtime = AdditionalAutomaticAbilities::from_definitions(definitions, 17);
        assert_eq!(
            AdditionalAutomaticAbilityDefinitions::from_runtime(runtime),
            definitions
        );
        for entry in runtime.iter() {
            assert_eq!(entry.state.ready_tick, 17);
            assert_eq!(entry.state.cast_sequence, 0);
            assert!(entry.state.autocast_enabled);
            assert!(!entry.state.manual_cast_requested);
            assert_eq!(
                entry.secondary_resurrection,
                SecondaryResurrectionState::default()
            );
        }
    }
}
