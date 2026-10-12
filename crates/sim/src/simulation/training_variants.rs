use super::*;
use crate::ManaProfile;
use crate::native_effects::{NativeUnitMechanics, native_unit_mechanics_for};

pub(super) struct TrainingVariantSpawn {
    pub owner: PlayerId,
    pub team: Team,
    pub position: SimPoint,
    pub producer: SimId,
    pub version: crate::MapVersion,
    pub root_rawcode: u32,
}

fn with_training_mechanics(
    mut body: ResolvedUnitDefinition,
    mechanics: NativeUnitMechanics,
    native_mana: Option<ManaProfile>,
) -> ResolvedUnitDefinition {
    let mut passives: Vec<_> = body.properties.passive_effects.iter().collect();
    for effect in mechanics.passive_effects.iter() {
        if !passives.contains(&effect) {
            passives.push(effect);
        }
    }
    body.properties.passive_effects = PassiveUnitEffects::from_slice(&passives);
    let mana = native_mana.unwrap_or(ManaProfile::per_second(0, 0, 0));
    let spells = body
        .spellcasting
        .into_iter()
        .chain(
            body.additional_abilities
                .as_ref()
                .into_iter()
                .flat_map(|s| s.iter())
                .map(|ability| SpellcastingProfile { mana, ability }),
        )
        .chain(mechanics.spellcasting)
        .chain(
            mechanics
                .additional_abilities
                .as_ref()
                .into_iter()
                .flat_map(|s| s.iter())
                .map(|ability| SpellcastingProfile { mana, ability }),
        )
        .map(|mut spell| {
            spell.mana = mana;
            spell
        });
    (body.spellcasting, body.additional_abilities) =
        crate::components::compose_spellcasting_profiles(None, spells)
            .expect("verified native training abilities fit their replacement's mana pool");
    body
}

impl Simulation {
    /// Returns the cold replacement definition used by direct CreateUnit shrine copies.
    pub(super) fn apply_training_variant(
        &mut self,
        original: SimId,
        source: TrainingVariantSpawn,
    ) -> Option<ResolvedUnitDefinition> {
        let profile = crate::building_mechanics::training_variants_for_version(source.version);
        if !profile.roots.contains(&source.root_rawcode) {
            return None;
        }
        assert!(crate::content::unit_has_ability_for_version(
            source.root_rawcode,
            AbilityId(profile.marker),
            source.version
        ));
        let (body_rawcode, abilities) = profile.choose(|site, minimum, maximum| {
            let range = u64::from(maximum - minimum) + 1;
            minimum
                + (deterministic_random(
                    self.config.match_seed,
                    self.next_tick,
                    original,
                    0x5452_4149_4e56_4152,
                    site,
                ) % range) as u32
        });
        let cold = crate::castle_fight_content_bundle(source.version)
            .expect("training version")
            .unit_definitions()
            .find(|body| body.rawcode == body_rawcode)
            .expect("promoted training has every complete replacement body")
            .resolved();
        let intrinsic: Vec<_> = abilities
            .iter()
            .copied()
            .filter(|ability| {
                profile.grant_sources.iter().any(|record| {
                    record.key == *ability
                        && record.source().kind == crate::NativeEffectSourceKind::UnitAbility
                })
            })
            .collect();
        let mechanics = native_unit_mechanics_for(source.version, body_rawcode, &intrinsic)
            .expect("verified native training grants");
        let definition = with_training_mechanics(
            cold,
            mechanics,
            crate::content::native_mana_profile_for_version(source.version, body_rawcode),
        );
        let entity = self
            .world
            .iter_entities()
            .find(|entity| entity.get::<SimId>() == Some(&original))
            .expect("newly trained original handle")
            .id();
        self.world.despawn(entity);
        let replacement = self.spawn_resolved_unit_with_provenance(
            Some(source.owner),
            UnitSpawn::from_template(source.team, source.position, definition.template),
            definition,
            Some(source.producer),
        );
        let entity = self
            .world
            .iter_entities()
            .find(|entity| entity.get::<SimId>() == Some(&replacement))
            .expect("native replacement handle")
            .id();
        let mut status = self
            .world
            .get_mut::<StatusState>(entity)
            .expect("replacement status");
        for ability in abilities {
            if let Some(grant) =
                crate::native_effects::permanent_grant_for_version(source.version, ability)
            {
                status.native_grants.insert(grant);
            }
        }
        Some(cold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UnitTemplate;

    #[test]
    fn training_abilities_use_the_cold_body_pool_and_leave_the_clone_definition_unchanged() {
        let cold = ResolvedUnitDefinition {
            template: UnitTemplate {
                health: 77,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 11,
                    range: 10,
                    acquisition_range: 20,
                    cooldown_ticks: 3,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            properties: UnitGameplayProperties::default(),
            spellcasting: None,
            additional_abilities: None,
        };
        let mechanics = NativeUnitMechanics {
            passive_effects: PassiveUnitEffects::single(PassiveUnitEffect::Evasion(
                crate::EvasionEffectProfile {
                    ability: AbilityId(933),
                    chance_per_10k: 1700,
                },
            )),
            spellcasting: Some(SpellcastingProfile {
                mana: ManaProfile::per_second(900, 800, 1000),
                ability: AutomaticAbilityProfile {
                    id: AbilityId(934),
                    mana_cost: 0,
                    cooldown_ticks: 3,
                    range: 10,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnit,
                    effect: AbilityEffect::Damage { amount: 7 },
                },
            }),
            additional_abilities: None,
        };
        let trained = with_training_mechanics(cold, mechanics, None);
        assert_eq!(
            trained.spellcasting.unwrap().mana,
            ManaProfile::per_second(0, 0, 0)
        );
        assert_eq!(trained.properties.passive_effects.iter().count(), 1);
        assert_eq!(trained.template, cold.template);
        assert!(cold.spellcasting.is_none());
        assert_eq!(cold.properties.passive_effects.iter().count(), 0);
        let body_pool = ManaProfile::per_second(13, 7, 2000);
        assert_eq!(
            with_training_mechanics(cold, mechanics, Some(body_pool))
                .spellcasting
                .unwrap()
                .mana,
            body_pool
        );
    }
}
