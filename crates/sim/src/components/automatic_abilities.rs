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
}
impl TryFrom<Vec<AutomaticAbilityProfile>> for AdditionalAutomaticAbilityDefinitions {
    type Error = AbilityConfigurationError;
    fn try_from(profiles: Vec<AutomaticAbilityProfile>) -> Result<Self, Self::Error> {
        let entries: Vec<_> = profiles
            .into_iter()
            .map(|profile| AutomaticAbilityInstance {
                profile,
                state: AutomaticAbilityState {
                    ready_tick: 0,
                    cast_sequence: 0,
                    autocast_enabled: true,
                    manual_cast_requested: false,
                },
                secondary_resurrection: SecondaryResurrectionState::default(),
            })
            .collect();
        AdditionalAutomaticAbilities::try_from(entries).map(Self::from_runtime)
    }
}
impl From<AdditionalAutomaticAbilityDefinitions> for Vec<AutomaticAbilityProfile> {
    fn from(value: AdditionalAutomaticAbilityDefinitions) -> Self {
        value.iter().collect()
    }
}

impl From<AdditionalAutomaticAbilities> for Vec<AutomaticAbilityInstance> {
    fn from(value: AdditionalAutomaticAbilities) -> Self {
        value.iter().copied().collect()
    }
}
