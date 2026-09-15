use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MapVersion {
    pub major: u16,
    pub minor: u16,
}

impl MapVersion {
    pub const CASTLE_FIGHT_9_27: Self = Self::new(9, 27);
    pub const CASTLE_FIGHT_9_32: Self = Self::new(9, 32);

    #[must_use]
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }
}

impl fmt::Display for MapVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapVersionParseError;

impl fmt::Display for MapVersionParseError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("map version must be two decimal components such as 9.27")
    }
}

impl std::error::Error for MapVersionParseError {}

impl FromStr for MapVersion {
    type Err = MapVersionParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (major, minor) = value.split_once('.').ok_or(MapVersionParseError)?;
        if major.is_empty() || minor.is_empty() || minor.contains('.') {
            return Err(MapVersionParseError);
        }
        let major = major.parse().map_err(|_| MapVersionParseError)?;
        let minor = minor.parse().map_err(|_| MapVersionParseError)?;
        Ok(Self { major, minor })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapVersionRange {
    pub first: MapVersion,
    pub last: MapVersion,
}

impl MapVersionRange {
    #[must_use]
    pub const fn inclusive(first: MapVersion, last: MapVersion) -> Self {
        assert!(
            first.major < last.major || (first.major == last.major && first.minor <= last.minor)
        );
        Self { first, last }
    }

    #[must_use]
    pub const fn exactly(version: MapVersion) -> Self {
        Self {
            first: version,
            last: version,
        }
    }

    #[must_use]
    pub const fn contains(self, version: MapVersion) -> bool {
        (version.major > self.first.major
            || (version.major == self.first.major && version.minor >= self.first.minor))
            && (version.major < self.last.major
                || (version.major == self.last.major && version.minor <= self.last.minor))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registered_castle_fight_versions_have_stable_numeric_identities() {
        assert_eq!("9.27".parse(), Ok(MapVersion::CASTLE_FIGHT_9_27));
        assert_eq!("9.32".parse(), Ok(MapVersion::CASTLE_FIGHT_9_32));
        assert_ne!(MapVersion::CASTLE_FIGHT_9_27, MapVersion::CASTLE_FIGHT_9_32);
    }
}
