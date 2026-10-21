//! Power Plant: building auras applied from the timers phase (so production sees them), a global
//! periodic disable-cleanse, and the augmentation of units trained by a covered building.
use super::*;
use crate::building_mechanics::{PowerPlantProfile, power_plant_for_version};
use crate::components::SpellResistanceEffectProfile;

fn aura_modifier(ability: AbilityId, buff: u32, expires_tick: u64) -> TimedArmorModifier {
    TimedArmorModifier {
        id: ModifierId(ability.0),
        native_buff: Some(crate::NativeBuffIdentity {
            rawcode: buff,
            positive: true,
            stealable: false,
            organic_only: false,
        }),
        aura_membership: true,
        undispellable: true,
        expires_tick,
        ..Default::default()
    }
}

impl Simulation {
    pub(super) fn refresh_power_plants_for_timers(&mut self) {
        let tick = self.next_tick;
        let cell = self.config.navigation_cell_size;
        let mut query = self
            .world
            .try_query_filtered::<(
                Entity,
                &ContentIdentity,
                &Team,
                &BuildingFootprint,
                &Health,
                Option<&AttackProfile>,
            ), bevy_ecs::query::Without<BuildingConstruction>>()
            .expect("simulation component types are registered at world construction");
        let buildings: Vec<_> = query
            .iter(&self.world)
            .filter(|(.., health, _)| health.current > 0)
            .map(|(entity, content, team, footprint, _, attack)| {
                (
                    entity,
                    *content,
                    *team,
                    footprint_center_point(*footprint, cell),
                    attack.is_some_and(|attack| attack.delivery != AttackDelivery::Melee),
                )
            })
            .collect();
        let plants: Vec<(&'static PowerPlantProfile, Team, SimPoint)> = buildings
            .iter()
            .filter_map(|(_, content, team, center, _)| {
                let profile = power_plant_for_version(content.map_version)?;
                (content.rawcode == profile.source_rawcode).then_some((profile, *team, *center))
            })
            .collect();
        if plants.is_empty() {
            return;
        }
        let mut grants: Vec<(Entity, Vec<TimedArmorModifier>)> = Vec::new();
        for (entity, content, team, center, ranged) in &buildings {
            let mut modifiers = Vec::new();
            for (profile, plant_team, plant_center) in &plants {
                if plant_team != team {
                    continue;
                }
                let distance_sq = plant_center.distance_sq(*center);
                let is_plant = content.rawcode == profile.source_rawcode;
                if distance_sq <= square_i32(profile.armor.radius) {
                    let mut modifier =
                        aura_modifier(profile.armor.ability, profile.armor.buff, tick + 1);
                    modifier.armor_bonus_per_100 = profile.armor.bonus_per_100;
                    modifiers.push(modifier);
                }
                if !is_plant && distance_sq <= square_i32(profile.mana.radius) {
                    let mut modifier =
                        aura_modifier(profile.mana.ability, profile.mana.buff, tick + 1);
                    modifier.mana_regeneration_per_second_per_10k =
                        profile.mana.regeneration_per_second_per_10k;
                    modifiers.push(modifier);
                }
                if !is_plant && *ranged && distance_sq <= square_i32(profile.drums.radius) {
                    let mut modifier =
                        aura_modifier(profile.drums.ability, profile.drums.buff, tick + 1);
                    modifier.damage_bonus_per_10k = profile.drums.damage_per_10k;
                    modifiers.push(modifier);
                }
            }
            if !modifiers.is_empty() {
                grants.push((*entity, modifiers));
            }
        }
        for (entity, modifiers) in grants {
            let mut status = self
                .world
                .get::<StatusState>(entity)
                .copied()
                .unwrap_or_default();
            for modifier in modifiers {
                apply_timed_armor_modifier(&mut status, modifier);
            }
            self.world.entity_mut(entity).insert(status);
        }
        // The cleanse is one global script timer, not a per-plant clock.
        for (profile, _, plant_center) in &plants {
            let cleanse = profile.cleanse;
            if tick == 0 || tick % u64::from(cleanse.interval_ticks) != 0 {
                continue;
            }
            for (entity, content, _, center, _) in &buildings {
                if content.rawcode == profile.source_rawcode
                    || plant_center.distance_sq(*center) > square_i32(cleanse.radius)
                {
                    continue;
                }
                if let Some(mut status) = self.world.get_mut::<StatusState>(*entity) {
                    status.frozen_until_tick = 0;
                    status.frozen_ability = None;
                }
            }
        }
    }

    /// The augmentation a building's production earns, when that building carries the plant's buff.
    pub(super) fn power_armor_for_producer(
        &self,
        producer: Entity,
        version: crate::MapVersion,
    ) -> Option<&'static crate::building_mechanics::PowerPlantSpawnProfile> {
        let spawn = &power_plant_for_version(version)?.spawn;
        let status = self.world.get::<StatusState>(producer)?;
        status.armor_modifiers[..usize::from(status.armor_modifier_count)]
            .iter()
            .any(|modifier| {
                // Production runs before the timers phase refreshes the aura, so it reads the
                // modifier granted by the previous tick, which expires exactly now.
                self.next_tick <= modifier.expires_tick
                    && modifier
                        .native_buff
                        .is_some_and(|buff| buff.rawcode == spawn.requires_buff)
            })
            .then_some(spawn)
    }

    /// `applyPowerArmor`: permanent maximum life, a spell-damage reduction for bodies without a
    /// spell defense, and the two status bonuses applied by [`Self::apply_power_armor_status`].
    pub(super) fn augment_power_armor_definition(
        spawn: &crate::building_mechanics::PowerPlantSpawnProfile,
        definition: &mut ResolvedUnitDefinition,
        version: crate::MapVersion,
    ) {
        definition.template.health = definition
            .template
            .health
            .saturating_add(spawn.max_life_bonus);
        let excluded = definition.properties.content.is_some_and(|content| {
            spawn.spell_resist_exclusions.iter().any(|ability| {
                crate::content::unit_has_ability_for_version(content.rawcode, *ability, version)
            })
        });
        if !excluded {
            let mut effects: Vec<_> = definition.properties.passive_effects.iter().collect();
            effects.push(PassiveUnitEffect::SpellResistance(
                SpellResistanceEffectProfile {
                    ability: spawn.spell_resist_ability,
                    damage_taken_per_10k: spawn.spell_damage_taken_per_10k,
                },
            ));
            definition.properties.passive_effects = PassiveUnitEffects::from_slice(&effects);
        }
    }

    pub(super) fn apply_power_armor_status(
        &mut self,
        unit: SimId,
        spawn: &crate::building_mechanics::PowerPlantSpawnProfile,
    ) {
        let Some(entity) = self
            .world
            .iter_entities()
            .find(|entity| {
                entity.get::<SimId>() == Some(&unit) && entity.contains::<MovementProfile>()
            })
            .map(|entity| entity.id())
        else {
            return;
        };
        let Some(mut status) = self.world.get_mut::<StatusState>(entity) else {
            return;
        };
        for (ability, armor, damage) in [
            (spawn.armor_ability, spawn.armor_bonus_per_100, 0),
            (spawn.damage_ability, 0, spawn.damage_per_10k),
        ] {
            apply_timed_armor_modifier(
                &mut status,
                TimedArmorModifier {
                    id: ModifierId(ability.0),
                    armor_bonus_per_100: armor,
                    damage_bonus_per_10k: damage,
                    undispellable: true,
                    expires_tick: u64::MAX,
                    ..Default::default()
                },
            );
        }
    }
}
