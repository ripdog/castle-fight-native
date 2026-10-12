use super::{MapVersion, evidence};
use crate::{NativeEffectSource, NativeEffectSourceKind};

#[derive(Debug, serde::Deserialize)]
pub(crate) struct TrainingBodyChoice {
    pub threshold_exclusive: u32,
    pub body: u32,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct TrainingGrantChoice {
    pub threshold_exclusive: u32,
    pub ability: u32,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct TrainingGrantGroup {
    pub minimum: u32,
    pub maximum: u32,
    pub choices: Vec<TrainingGrantChoice>,
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct TrainingGrantSource {
    kind: String,
    pub key: u32,
}

impl TrainingGrantSource {
    pub fn source(&self) -> NativeEffectSource {
        let kind = match self.kind.as_str() {
            "unit-ability" => NativeEffectSourceKind::UnitAbility,
            "ability-effect" => NativeEffectSourceKind::AbilityEffect,
            _ => unreachable!("verified training source kind"),
        };
        NativeEffectSource::new(kind, self.key)
    }
}

#[derive(Debug, serde::Deserialize)]
pub(crate) struct TrainingVariantsProfile {
    pub marker: u32,
    pub roots: Vec<u32>,
    pub body_minimum: u32,
    pub body_maximum: u32,
    pub rare_bodies: Vec<TrainingBodyChoice>,
    pub common_minimum: u32,
    pub common_maximum: u32,
    pub common_bodies: Vec<u32>,
    pub grant_groups: Vec<TrainingGrantGroup>,
    pub grant_sources: Vec<TrainingGrantSource>,
}

impl TrainingVariantsProfile {
    pub fn bodies(&self) -> impl Iterator<Item = u32> + '_ {
        self.rare_bodies
            .iter()
            .map(|choice| choice.body)
            .chain(self.common_bodies.iter().copied())
    }

    pub fn choose(&self, mut roll: impl FnMut(u64, u32, u32) -> u32) -> (u32, Vec<u32>) {
        let body_roll = roll(0, self.body_minimum, self.body_maximum);
        let body = self
            .rare_bodies
            .iter()
            .find(|choice| body_roll < choice.threshold_exclusive)
            .map_or_else(
                || {
                    let index =
                        roll(1, self.common_minimum, self.common_maximum) - self.common_minimum;
                    self.common_bodies[index as usize]
                },
                |choice| choice.body,
            );
        let mut abilities = Vec::with_capacity(self.grant_groups.len());
        for (index, group) in self.grant_groups.iter().enumerate() {
            let value = roll(2 + index as u64, group.minimum, group.maximum);
            if let Some(choice) = group
                .choices
                .iter()
                .find(|choice| value < choice.threshold_exclusive)
                && !abilities.contains(&choice.ability)
            {
                abilities.push(choice.ability);
            }
        }
        (body, abilities)
    }
}

pub(crate) fn training_variants_for_version(
    version: MapVersion,
) -> &'static TrainingVariantsProfile {
    static PROFILE: std::sync::OnceLock<TrainingVariantsProfile> = std::sync::OnceLock::new();
    let data = evidence(version);
    PROFILE.get_or_init(|| {
        serde_json::from_value(data["production_training_variants"].clone())
            .expect("verified native training variants")
    })
}

pub(crate) fn training_granted_sources(
    version: MapVersion,
    unit: u32,
) -> impl Iterator<Item = NativeEffectSource> {
    let profile = training_variants_for_version(version);
    profile
        .grant_sources
        .iter()
        .filter(move |_| profile.roots.contains(&unit) || profile.bodies().any(|body| body == unit))
        .map(TrainingGrantSource::source)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn independent_rolls_keep_rare_boundaries_fallback_and_duplicate_native_grants_distinct() {
        let profile: TrainingVariantsProfile = serde_json::from_value(serde_json::json!({
            "marker": 1, "roots": [10], "body_minimum": 0, "body_maximum": 11,
            "rare_bodies": [{"threshold_exclusive": 2, "body": 20}, {"threshold_exclusive": 5, "body": 21}],
            "common_minimum": 0, "common_maximum": 1, "common_bodies": [22, 23],
            "grant_groups": [
                {"minimum": 0, "maximum": 9, "choices": [{"threshold_exclusive": 2, "ability": 30}, {"threshold_exclusive": 4, "ability": 31}]},
                {"minimum": 0, "maximum": 9, "choices": [{"threshold_exclusive": 3, "ability": 30}]}
            ], "grant_sources": []
        })).unwrap();
        for (body_roll, expected) in [(0, 20), (1, 20), (2, 21), (4, 21), (5, 23), (11, 23)] {
            let mut sites = Vec::new();
            let result = profile.choose(|site, _, _| {
                sites.push(site);
                match site {
                    0 => body_roll,
                    1 => 1,
                    _ => 0,
                }
            });
            assert_eq!(result, (expected, vec![30]));
            assert_eq!(
                sites,
                if body_roll < 5 {
                    vec![0, 2, 3]
                } else {
                    vec![0, 1, 2, 3]
                }
            );
        }
        assert_eq!(
            profile
                .choose(|site, _, _| match site {
                    0 => 0,
                    2 => 2,
                    _ => 3,
                })
                .1,
            vec![31]
        );
        assert!(
            profile
                .choose(|site, _, _| if site == 0 { 0 } else { 9 })
                .1
                .is_empty()
        );
    }
}
