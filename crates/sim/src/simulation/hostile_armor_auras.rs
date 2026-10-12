use super::*;

impl Simulation {
    pub(super) fn apply_hostile_armor_auras(&self, units: &mut [UnitSnapshot]) {
        let mut sources = units
            .iter()
            .filter(|u| u.health > 0 && !u.abilities_disabled)
            .flat_map(|u| {
                u.effective_passive_effects().filter_map(move |effect| {
                    let PassiveUnitEffect::HostileArmorAura(p) = effect else {
                        return None;
                    };
                    Some((p, u.id, u.team, u.position))
                })
            })
            .collect::<Vec<_>>();
        sources.sort_unstable_by_key(|(p, id, _, _)| (p.buff_rawcode, p.ability, *id));
        for target in units.iter_mut().filter(|u| {
            u.health > 0 && !u.classifications.locust && !u.classifications.invulnerable
        }) {
            for family in sources.chunk_by(|a, b| a.0.buff_rawcode == b.0.buff_rawcode) {
                // Devotion is a universal aura; armor membership admits magic immunity.
                let Some((p, _, _, _)) = family
                    .iter()
                    .filter(|(p, _, team, position)| {
                        target.team != *team
                            && p.targets.can_target_unit(target.target_movement_class())
                            && position.distance_sq(target.position) <= square_i32(p.radius)
                    })
                    .min_by_key(|(p, id, _, _)| (p.armor_bonus_per_100, p.ability, *id))
                else {
                    continue;
                };
                apply_timed_armor_modifier(
                    &mut target.status,
                    TimedArmorModifier {
                        id: ModifierId(p.ability.0),
                        armor_bonus_per_100: p.armor_bonus_per_100,
                        native_buff: Some(crate::NativeBuffIdentity {
                            rawcode: p.buff_rawcode,
                            positive: false,
                            stealable: false,
                            organic_only: false,
                        }),
                        aura_membership: true,
                        expires_tick: self.next_tick + u64::from(p.linger_ticks.max(1)),
                        ..Default::default()
                    },
                );
            }
        }
    }
}
