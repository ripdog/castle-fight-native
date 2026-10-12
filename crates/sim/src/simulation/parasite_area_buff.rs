use super::*;
use crate::ParasiteAreaBuffProfile;

pub(super) fn primary_eligible(
    u: &UnitSnapshot,
    team: Team,
    p: ParasiteAreaBuffProfile,
    tick: u64,
) -> bool {
    u.health > 0
        && u.team != team
        && u.classifications.combat_sapper
        && u.target_movement_class() == MovementClass::Ground
        && !u.classifications.invulnerable
        && !u.classifications.spell_immune
        && !(p.parent_nonhero_only && u.classifications.uses_hero_spell_rules())
        && u.visible_to(team, tick)
}

impl Simulation {
    pub(super) fn cast_parasite_area_buff(
        &mut self,
        source: AbilitySourceSnapshot,
        target: usize,
        p: ParasiteAreaBuffProfile,
        units: &mut [UnitSnapshot],
    ) -> usize {
        let caster = find_unit_index(units, source.id).expect("native buff caster");
        let origin = units[caster].position;
        self.launch_native_parasite(source, units[caster].owner, units[target].id, p.parent);
        let mut applied = 0;
        for recipient in units {
            if origin.distance_sq(recipient.position) <= square_i32(p.radius)
                && p.targets.can_target_unit(recipient.target_movement_class())
                && apply_ability_effect_to_unit(
                    recipient,
                    p.buff_effect(),
                    source.team,
                    self.next_tick,
                    self.combat_rules.damage_rules,
                )
            {
                applied += 1;
            }
        }
        applied
    }
}
