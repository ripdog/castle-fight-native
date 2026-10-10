use super::*;

pub(super) fn primary_eligible(
    unit: &UnitSnapshot,
    team: Team,
    p: crate::ScriptedRootsProfile,
    tick: u64,
) -> bool {
    unit.health > 0
        && unit.team != team
        && unit.classifications.combat_sapper
        && unit.target_movement_class() == MovementClass::Ground
        && !unit.classifications.invulnerable
        && !unit.classifications.spell_immune
        && !(p.primary_nonhero_only && unit.classifications.hero)
        && unit.visible_to(team, tick)
}

fn excluded(unit: &UnitSnapshot, p: crate::ScriptedRootsProfile) -> bool {
    p.excluded_abilities.iter().any(|&ability| {
        ability != 0
            && crate::content::unit_has_ability_for_version(
                unit.rawcode,
                AbilityId(ability),
                p.map_version,
            )
    })
}

fn has_priority_buff(unit: &UnitSnapshot, p: crate::ScriptedRootsProfile, tick: u64) -> bool {
    let wanted = |buff: u32| buff != 0 && p.priority_buffs.contains(&buff);
    unit.status.armor_modifiers[..usize::from(unit.status.armor_modifier_count)]
        .iter()
        .any(|m| tick < m.expires_tick && m.native_buff.is_some_and(|b| wanted(b.rawcode)))
        || unit.status.movement_modifiers[..usize::from(unit.status.movement_modifier_count)]
            .iter()
            .map(|m| (m.id, m.expires_tick))
            .chain(
                unit.status.damage_over_time[..usize::from(unit.status.damage_over_time_count)]
                    .iter()
                    .map(|m| (m.id, m.expires_tick)),
            )
            .any(|(id, expiry)| {
                tick < expiry
                    && crate::native_effects::native_buff_for_version(p.map_version, id.0)
                        .is_some_and(|b| wanted(b.rawcode))
            })
}

impl Simulation {
    pub(super) fn cast_scripted_roots(
        &mut self,
        source: AbilitySourceSnapshot,
        primary: usize,
        p: crate::ScriptedRootsProfile,
        units: &mut [UnitSnapshot],
    ) -> usize {
        let origin = units[primary].position;
        let mut candidates: Vec<_> = if p.search_radius == 0 {
            vec![primary]
        } else {
            units
                .iter()
                .enumerate()
                .filter(|(_, u)| {
                    u.health > 0
                        && u.team != source.team
                        && u.classifications.combat_sapper
                        && u.target_movement_class() == MovementClass::Ground
                        && !excluded(u, p)
                        && origin.distance_sq(u.position) <= square_i32(p.search_radius)
                })
                .map(|(i, _)| i)
                .collect()
        };
        candidates.sort_unstable_by_key(|&i| {
            (has_priority_buff(&units[i], p, self.next_tick), units[i].id)
        });
        let mut applied = 0;
        // The native helper decrements its selection cap before issuing the child order.
        for index in candidates.into_iter().take(usize::from(p.maximum_targets)) {
            let u = &mut units[index];
            if p.vision_radius > 0 && !u.visible_to(source.team, self.next_tick) {
                self.reveal_area(
                    source.team,
                    u.position,
                    p.vision_radius,
                    u64::from(p.vision_ticks),
                    false,
                );
            }
            if u.health <= 0
                || u.classifications.invulnerable
                || u.classifications.spell_immune
                || (u.classifications.invisible && !u.visible_to(source.team, self.next_tick))
                || !p.roots.targets.can_target_unit(u.target_movement_class())
                || (p.roots.nonhero_only && u.classifications.hero)
                || origin.distance_sq(u.position) > square_i32(p.child_range)
            {
                continue;
            }
            apply_roots_status(
                &mut u.status,
                p.roots,
                self.next_tick,
                u.classifications.hero,
                true,
            );
            applied += 1;
        }
        applied
    }
}

#[cfg(test)]
mod tests;
