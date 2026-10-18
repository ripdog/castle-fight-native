//! Native `AEfk` Fan of Knives issued by a source-owned damage listener.
use super::*;

impl Simulation {
    /// Caster-centered hostile fan. Each eligible unit takes the retained per-target damage
    /// unless that would exceed the retained total cap, in which case the cap is divided evenly
    /// (integer floor) between the units hit. A nonzero target limit keeps the nearest units,
    /// ties resolved by stable identity.
    pub(super) fn cast_fan_of_knives(
        &self,
        source: AbilitySourceSnapshot,
        effect: AbilityEffect,
        units: &mut [UnitSnapshot],
    ) -> usize {
        let AbilityEffect::FanOfKnives {
            ability,
            damage,
            maximum_total_damage,
            maximum_targets,
            radius,
            targets,
            organic_only,
        } = effect
        else {
            unreachable!("Fan of Knives resolver requires its own effect");
        };
        let AbilitySourceOrigin::Unit(center) = source.origin else {
            unreachable!("Fan of Knives originates at a unit");
        };
        let radius_sq = square_i32(radius);
        let mut recipients = units
            .iter()
            .enumerate()
            .filter(|(_, target)| {
                target.team != source.team
                    && center.distance_sq(target.position) <= radius_sq
                    && super::status::fan_of_knives_target_eligible(target, targets, organic_only)
            })
            .map(|(index, target)| (center.distance_sq(target.position), target.id, index))
            .collect::<Vec<_>>();
        if maximum_targets > 0 {
            recipients.sort_unstable();
            recipients.truncate(usize::from(maximum_targets));
        }
        let count = i32::try_from(recipients.len()).expect("bounded Fan of Knives recipient count");
        if count == 0 {
            return 0;
        }
        let per_target = if damage.checked_mul(count).is_none_or(|total| total > maximum_total_damage)
        {
            maximum_total_damage / count
        } else {
            damage
        };
        let effect = AbilityEffect::FanOfKnives {
            ability,
            damage: per_target,
            maximum_total_damage,
            maximum_targets,
            radius,
            targets,
            organic_only,
        };
        let mut applied = 0;
        for (_, _, index) in recipients {
            if self.apply_source_ability_to_unit(source.id, source.team, index, effect, units) {
                applied += 1;
            }
        }
        applied
    }
}
