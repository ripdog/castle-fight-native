use super::*;

impl Simulation {
    /// Resolve both sides before committing resources. Recompute against the
    /// canonical current phase at release, so competing casters cannot copy a buff.
    pub(super) fn native_buff_transfer_plan(
        &self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        donor_index: usize,
        units: &[UnitSnapshot],
    ) -> Option<(usize, usize)> {
        let AbilityEffect::SpellSteal { recipient_radius } = ability.effect else {
            return None;
        };
        let donor = &units[donor_index];
        if donor.health <= 0
            || donor.classifications.invulnerable
            || self.ability_source_distance_sq(source.origin, donor.position)
                > square_i32(ability.range)
            || (donor.team != source.team && !donor.visible_to(source.team, self.next_tick))
        {
            return None;
        }
        let positive = donor.team != source.team;
        let count = usize::from(donor.status.armor_modifier_count);
        donor.status.armor_modifiers[..count]
            .iter()
            .enumerate()
            .filter_map(|(modifier_index, modifier)| {
                let buff = modifier.native_buff?;
                if !buff.stealable
                    || buff.positive != positive
                    || self.next_tick >= modifier.expires_tick
                {
                    return None;
                }
                let recipient = units
                    .iter()
                    .enumerate()
                    .filter(|(_, candidate)| {
                        candidate.health > 0
                            && !candidate.classifications.invulnerable
                            && (candidate.team == source.team) == positive
                            && (!buff.organic_only || !candidate.mechanical)
                            && (positive
                                || (!candidate.classifications.spell_immune
                                    && candidate.visible_to(source.team, self.next_tick)))
                            && donor.position.distance_sq(candidate.position)
                                <= square_i32(recipient_radius)
                            && usize::from(candidate.status.armor_modifier_count)
                                < MAX_TIMED_ARMOR_MODIFIERS
                            && !candidate.status.armor_modifiers
                                [..usize::from(candidate.status.armor_modifier_count)]
                                .iter()
                                .any(|active| {
                                    active
                                        .native_buff
                                        .is_some_and(|active| active.rawcode == buff.rawcode)
                                        && self.next_tick < active.expires_tick
                                })
                    })
                    .min_by_key(|(_, candidate)| {
                        (donor.position.distance_sq(candidate.position), candidate.id)
                    })?;
                Some((buff.rawcode, modifier.id, modifier_index, recipient.0))
            })
            .min()
            .map(|(_, _, modifier, recipient)| (modifier, recipient))
    }

    pub(super) fn transfer_native_buff(
        &self,
        source: AbilitySourceSnapshot,
        ability: AutomaticAbilityProfile,
        donor_index: usize,
        units: &mut [UnitSnapshot],
    ) -> bool {
        let Some((modifier_index, recipient_index)) =
            self.native_buff_transfer_plan(source, ability, donor_index, units)
        else {
            return false;
        };
        let status = &mut units[donor_index].status;
        let mut transferred = status.armor_modifiers[modifier_index];
        let count = usize::from(status.armor_modifier_count);
        status
            .armor_modifiers
            .copy_within(modifier_index + 1..count, modifier_index);
        status.armor_modifiers[count - 1] = TimedArmorModifier::default();
        status.armor_modifier_count -= 1;
        if transferred.revealed_to.is_some() {
            transferred.revealed_to = Some(source.team);
        }
        apply_timed_armor_modifier(&mut units[recipient_index].status, transferred);
        true
    }
}
