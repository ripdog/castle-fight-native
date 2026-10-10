//! Native Aspy children enter the world once at the structural death boundary.
use super::*;
use crate::native_effects::NativeDeathSpawn;

impl Simulation {
    pub(super) fn spawn_death_children(&mut self, entity: Entity, position: SimPoint) -> usize {
        let source = self.world.entity(entity);
        let Some(content) = source.get::<ContentIdentity>().copied() else {
            return 0;
        };
        if source
            .get::<UnitClassifications>()
            .is_some_and(|flags| flags.illusion)
        {
            return 0;
        }
        let owner = source.get::<Owner>().expect("death-spawn owner").0;
        let team = *source.get::<Team>().expect("death-spawn team");
        let mut profiles: Vec<_> =
            crate::native_effects::death_spawns_for_version(content.map_version)
                .filter(|profile| {
                    crate::content::unit_has_ability_for_version(
                        content.rawcode,
                        profile.ability,
                        content.map_version,
                    )
                })
                .collect();
        profiles.sort_unstable_by_key(|profile| profile.ability);
        profiles
            .into_iter()
            .map(|profile| {
                self.spawn_native_death_children(
                    owner,
                    team,
                    position,
                    content.map_version,
                    profile,
                )
            })
            .sum()
    }

    fn spawn_native_death_children(
        &mut self,
        owner: PlayerId,
        team: Team,
        position: SimPoint,
        version: crate::MapVersion,
        profile: NativeDeathSpawn,
    ) -> usize {
        let kind =
            crate::CastleFightUnitKind::from_rawcode_for_version(profile.child_rawcode, version)
                .expect("death-spawn version")
                .expect("promoted native death-spawn child");
        let mut definition = kind.definition_for_version(version).unwrap().resolved();
        // Aspy creates native summoned units even when the object's class list lacks
        // SUMMONED. The marker ability is a separate script classification.
        definition.properties.classifications.summoned = true;
        for _ in 0..profile.count {
            self.spawn_resolved_unit_unchecked(
                Some(owner),
                UnitSpawn::from_template(team, position, definition.template),
                definition,
            );
        }
        // Entry registers combat and issues the strategic attack order. Neither
        // train-finish nor setupUnit runs, so there is no factory provenance, shrine
        // replication, inherited damage/status, or unrelated timed life.
        usize::from(profile.count)
    }
}

#[cfg(test)]
mod tests;
