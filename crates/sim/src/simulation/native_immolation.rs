use super::*;

impl Simulation {
    pub(super) fn resolve_permanent_immolation(&self, units: &mut [UnitSnapshot]) {
        let sources = units
            .iter()
            .filter(|u| u.health > 0 && !u.abilities_disabled)
            .flat_map(|u| {
                u.effective_passive_effects().filter_map(move |effect| {
                    let PassiveUnitEffect::PermanentImmolation(profile) = effect else {
                        return None;
                    };
                    Some((u.id, u.team, u.position, u.spawn_tick, profile))
                })
            })
            .collect::<Vec<_>>();
        for (source, team, position, created, profile) in sources {
            let age = self.next_tick.saturating_sub(created);
            let pulse = age > 0 && age.is_multiple_of(u64::from(profile.interval_ticks));
            for i in 0..units.len() {
                let target = units[i];
                if target.health <= 0
                    || target.team == team
                    || target.classifications.locust
                    || target.classifications.invulnerable
                    || target.classifications.spell_immune
                    || (profile.organic_only && target.mechanical)
                    || !profile
                        .targets
                        .can_target_unit(target.target_movement_class())
                    || position.distance_sq(target.position) > square_i32(profile.radius)
                {
                    continue;
                }
                apply_timed_armor_modifier(
                    &mut units[i].status,
                    TimedArmorModifier {
                        id: ModifierId(profile.ability.0),
                        native_buff: Some(crate::NativeBuffIdentity {
                            rawcode: profile.buff_rawcode,
                            positive: false,
                            stealable: false,
                            organic_only: profile.organic_only,
                        }),
                        aura_membership: true,
                        undispellable: true,
                        expires_tick: self.next_tick + 1,
                        ..Default::default()
                    },
                );
                if pulse {
                    apply_ability_effect_to_unit(
                        &mut units[i],
                        AbilityEffect::Damage {
                            amount: profile.damage,
                        },
                        team,
                        self.next_tick,
                        self.combat_rules.damage_rules,
                    );
                    death_healing::observe_inline_death_from(units, i, target.health, Some(source));
                }
            }
        }
    }
}
