//! Script-selected building bolts, independent of the native parent order trigger.
use super::*;
use crate::building_mechanics::BuildingBoltProfile;
#[cfg(test)]
mod tests;

impl Simulation {
    pub(super) fn cast_building_bolt(
        &mut self,
        source: AbilitySourceSnapshot,
        profile: BuildingBoltProfile,
        sequence: u64,
        units: &mut [UnitSnapshot],
    ) -> bool {
        let bounds =
            crate::building_mechanics::snowveil_for_version(profile.map_version).battlefield_bounds;
        let target = units
            .iter()
            .enumerate()
            .filter(|(_, u)| {
                u.health > 0
                    && u.team != source.team
                    && u.classifications.combat_sapper
                    && u.movement_class == MovementClass::Air
                    && !u.classifications.invulnerable
                    && u.position.x >= bounds[0]
                    && u.position.y >= bounds[1]
                    && u.position.x <= bounds[2]
                    && u.position.y <= bounds[3]
            })
            .min_by_key(|(_, u)| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        profile.bolt.ability,
                        sequence,
                        u.id,
                    ),
                    u.id,
                )
            })
            .map(|(i, _)| i);
        let Some(index) = target else {
            return false;
        };
        let target = &mut units[index];
        if self.check_negative_building_shield(target, profile.map_version, source.id, sequence) {
            return false;
        }
        if self.visible_teams(target.position) & (1 << source.team.0) == 0 {
            self.reveal_area(
                source.team,
                target.position,
                profile.vision_radius,
                u64::from(profile.vision_ticks),
                false,
            );
        }
        let origin = match source.origin {
            AbilitySourceOrigin::Unit(p) => p,
            AbilitySourceOrigin::Building(f) => {
                footprint_center_point(f, self.config.navigation_cell_size)
            }
        };
        if (target.classifications.invisible && !target.visible_to(source.team, self.next_tick))
            || target.classifications.spell_immune
            || target.classifications.invulnerable
            || !profile.bolt.targets.can_target_unit(target.movement_class)
            || origin.distance_sq(target.position) > square_i32(profile.child_range)
        {
            return false;
        }
        self.launch_native_bolt(
            source.id,
            source.team,
            origin,
            target.id,
            target.position,
            profile.bolt,
        );
        true
    }
}
