use super::*;

impl Simulation {
    pub fn can_improve_building(&self, source: SimId) -> bool {
        self.building(source).is_some_and(|building| {
            building.health > 0
                && building.construction_complete_tick.is_none()
                && building.status.frozen_until_tick <= self.next_tick
                && building
                    .status
                    .mana_improvement_target
                    .is_none_or(|target| self.building(target).is_none_or(|b| b.health <= 0))
                && building.content.is_some_and(|content| {
                    crate::native_effects::building_improvement_for_version(
                        content.map_version,
                        content.rawcode,
                    )
                    .is_some()
                })
        })
    }

    pub fn improve_building_for_player(
        &mut self,
        controller: PlayerId,
        source: SimId,
        target: SimId,
    ) -> Result<(), BuildingCommandError> {
        if !self.can_player_control_building(controller, source) {
            return Err(BuildingCommandError::NotAuthorized);
        }
        self.refresh_mana_improvement_links();
        let source_view = self
            .building(source)
            .ok_or(BuildingCommandError::SourceNotFound)?;
        if !self.can_improve_building(source) {
            return Err(BuildingCommandError::SourceCannotCast);
        }
        let content = source_view
            .content
            .ok_or(BuildingCommandError::SourceCannotCast)?;
        let profile = crate::native_effects::building_improvement_for_version(
            content.map_version,
            content.rawcode,
        )
        .ok_or(BuildingCommandError::SourceCannotCast)?;
        let target_view = self
            .building(target)
            .ok_or(BuildingCommandError::TargetNotFound)?;
        if target_view.health <= 0
            || source_view.team != target_view.team
            || target_view.production.is_some()
            || target_view
                .content
                .is_some_and(|c| crate::content::building_has_production(c.map_version, c.rawcode))
            || target_view
                .mana_maximum
                .or_else(|| {
                    target_view.content.and_then(|c| {
                        crate::content::building_native_mana_maximum(c.map_version, c.rawcode)
                    })
                })
                .unwrap_or(0)
                < 1
            || target_view
                .status
                .native_grants
                .contains(profile.improvement)
        {
            return Err(BuildingCommandError::InvalidTargetType);
        }
        let origin =
            footprint_center_point(source_view.footprint, self.config.navigation_cell_size);
        let point = footprint_center_point(target_view.footprint, self.config.navigation_cell_size);
        if origin.distance_sq(point) > square_i32(profile.range) {
            return Err(BuildingCommandError::TargetOutOfRange);
        }
        let elemental = target_view.content.is_some_and(|target| {
            crate::content::unit_has_ability_for_version(
                target.rawcode,
                profile.elemental_marker,
                target.map_version,
            )
        });
        let grant = crate::NativeAbilityGrant {
            ability: profile.improvement,
            effect: crate::NativeAbilityGrantEffect::ManaRegeneration(
                profile.regeneration_per_second_per_10k[usize::from(elemental)],
            ),
        };
        let target_entity = self
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&target))
            .unwrap()
            .id();
        let mut status = self
            .world
            .get::<StatusState>(target_entity)
            .copied()
            .unwrap_or_default();
        status.native_grants.insert(grant);
        status.mana_improvement_link = Some(crate::components::ManaImprovementLink {
            source,
            source_position: origin,
            target_position: point,
            ability: profile.ability,
        });
        self.world.entity_mut(target_entity).insert(status);
        let source_entity = self
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source))
            .unwrap()
            .id();
        self.world
            .get_mut::<StatusState>(source_entity)
            .unwrap()
            .mana_improvement_target = Some(target);
        Ok(())
    }

    pub(super) fn refresh_mana_improvement_links(&mut self) {
        let cleared: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                let target = entity.get::<StatusState>()?.mana_improvement_target?;
                self.building(target)
                    .is_none_or(|b| b.health <= 0)
                    .then_some(entity.id())
            })
            .collect();
        for entity in cleared {
            self.world
                .get_mut::<StatusState>(entity)
                .unwrap()
                .mana_improvement_target = None;
        }
    }
}

#[cfg(test)]
mod tests;
