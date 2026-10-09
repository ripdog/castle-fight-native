use super::*;

impl Simulation {
    pub(super) fn apply_speed_auras(&self, units: &mut [UnitSnapshot]) {
        let mut sources: Vec<_> = units
            .iter()
            .filter(|unit| {
                unit.health > 0 && !unit.abilities_disabled && !unit.classifications.invisible
            })
            .flat_map(|unit| {
                unit.effective_passive_effects()
                    .filter_map(move |effect| {
                        let PassiveUnitEffect::SpeedAura(profile) = effect else {
                            return None;
                        };
                        Some((profile, unit.id, unit.team, unit.position, false))
                    })
                    .chain(unit.status.native_grants.iter().filter_map(move |grant| {
                        let crate::NativeAbilityGrantEffect::Speed {
                            attack,
                            movement,
                            buff_rawcode,
                        } = grant.effect
                        else {
                            return None;
                        };
                        Some((
                            crate::SpeedAuraProfile {
                                ability: grant.ability,
                                buff_rawcode,
                                radius: 0,
                                targets: AttackTargetMask::ALL,
                                hostile: false,
                                organic_only: false,
                                affects_invulnerable: false,
                                movement_percent_delta: movement,
                                attack_speed_percent_delta: attack,
                            },
                            unit.id,
                            unit.team,
                            unit.position,
                            true,
                        ))
                    }))
            })
            .collect();
        sources.sort_unstable_by_key(|(profile, id, _, _, _)| {
            (profile.buff_rawcode, profile.ability, *id)
        });
        for target in units.iter_mut().filter(|target| target.health > 0) {
            for family in
                sources.chunk_by(|left, right| left.0.buff_rawcode == right.0.buff_rawcode)
            {
                let eligible = family
                    .iter()
                    .filter(|(profile, id, team, position, self_only)| {
                        (!self_only || target.id == *id)
                            && (target.team != *team) == profile.hostile
                            && (!profile.organic_only || !target.mechanical)
                            && (!profile.hostile || !target.classifications.spell_immune)
                            && (profile.affects_invulnerable
                                || !target.classifications.invulnerable)
                            && profile
                                .targets
                                .can_target_unit(target.target_movement_class())
                            && position.distance_sq(target.position) <= square_i32(profile.radius)
                    });
                let Some((representative, _, _, _, _)) = eligible.clone().next() else {
                    continue;
                };
                let movement = eligible
                    .clone()
                    .map(|source| source.0.movement_percent_delta)
                    .max_by_key(|delta| (delta.unsigned_abs(), *delta))
                    .unwrap_or(0);
                let attack = eligible
                    .map(|source| source.0.attack_speed_percent_delta)
                    .max_by_key(|delta| (delta.unsigned_abs(), *delta))
                    .unwrap_or(0);
                let id = ModifierId(representative.ability.0);
                let expiry = self
                    .next_tick
                    .checked_add(1)
                    .expect("speed aura expiry overflow");
                apply_timed_armor_modifier(
                    &mut target.status,
                    TimedArmorModifier {
                        aura_membership: true,
                        id,
                        native_buff: Some(crate::NativeBuffIdentity {
                            rawcode: representative.buff_rawcode,
                            positive: !representative.hostile,
                            stealable: false,
                            organic_only: representative.organic_only,
                        }),
                        expires_tick: expiry,
                        ..Default::default()
                    },
                );
                apply_timed_movement_modifier(&mut target.status, id, movement, expiry);
                apply_timed_attack_speed_modifier(&mut target.status, id, attack, expiry);
            }
        }
    }
}
