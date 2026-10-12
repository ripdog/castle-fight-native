use super::*;
use crate::components::{IncinerateExplosion, NativeAction};

pub(super) fn eligible(u: &UnitSnapshot, team: Team) -> bool {
    u.team != team
        && !u.mechanical
        && !u.classifications.ancient
        && !u.classifications.locust
        && !u.classifications.invulnerable
        && !u.classifications.spell_immune
}

pub(super) fn burning(u: &UnitSnapshot, tick: u64) -> bool {
    u.status.incinerate.is_some_and(|p| {
        tick < p.expires_tick
            && u.status.armor_modifiers[..usize::from(u.status.armor_modifier_count)]
                .iter()
                .any(|m| {
                    tick < m.expires_tick
                        && m.native_buff
                            .is_some_and(|b| b.rawcode == p.profile.buff_rawcode)
                })
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "landed payload operates on existing split combat state"
)]
pub(super) fn landed_hit(
    index: usize,
    profile: crate::IncinerateProfile,
    source: PendingAttackEffectSource,
    tick: u64,
    units: &mut [UnitSnapshot],
    health: &mut [i32],
    rules: DamageRules,
    retired: &BTreeMap<SimId, source_damage_runtime::RetiredDamageSource>,
) {
    if !eligible(&units[index], source.team) {
        return;
    }
    let stacks = units[index]
        .status
        .incinerate
        .filter(|p| burning(&units[index], tick) && p.profile.buff_rawcode == profile.buff_rawcode)
        .map_or(1, |p| p.stacks.saturating_add(1))
        .min(profile.maximum_stacks);
    let duration = if units[index].classifications.uses_hero_spell_rules() {
        profile.hero_duration_ticks
    } else {
        profile.duration_ticks
    };
    if duration == 0 {
        return;
    }
    let expires_tick = tick + u64::from(duration);
    units[index].status.incinerate = Some(crate::IncinerateStatus {
        source: source.id,
        team: source.team,
        profile,
        stacks,
        expires_tick,
    });
    apply_timed_armor_modifier(
        &mut units[index].status,
        TimedArmorModifier {
            id: ModifierId(profile.ability.0),
            native_buff: Some(crate::NativeBuffIdentity {
                rawcode: profile.buff_rawcode,
                positive: false,
                stealable: false,
                organic_only: true,
            }),
            expires_tick,
            ..Default::default()
        },
    );
    // Stamp even a fatal landed weapon hit. The native death effect does not require its owner
    // to land the killing blow; removed units still bypass ordinary death callbacks.
    if health[index] <= 0 || profile.bonus_per_hit == 0 {
        return;
    }
    let damage = rules.apply_spell(
        profile
            .bonus_per_hit
            .checked_mul(i32::try_from(stacks).expect("Incinerate stack count fits damage bounds"))
            .expect("Incinerate bonus overflow"),
        units[index].armor.armor_type,
    );
    let damage = spell_damage_after_defend(units[index], damage, tick);
    source_damage_runtime::dispatch(
        source.id,
        index,
        damage,
        source_damage_runtime::SourceDamageState {
            units,
            health,
            rules,
            tick,
        },
        retired,
    );
    damage_thresholds::observe_damage_event(&mut units[index], health[index], damage);
    let after = health[index]
        .checked_sub(damage)
        .expect("Incinerate health overflow");
    death_healing::commit_split_health_from(units, health, None, index, after, Some(source.id));
}

impl Simulation {
    pub(super) fn observe_incinerate_deaths(
        &mut self,
        units: &[UnitSnapshot],
        health: &[i32],
        positions: &[SimPoint],
    ) {
        for (i, u) in units.iter().enumerate() {
            if health[i] > 0 || u.removed_without_death || !burning(u, self.next_tick) {
                continue;
            }
            let p = u.status.incinerate.expect("burning requires status");
            let id = self.allocate_id();
            self.world.spawn((
                id,
                NativeAction::IncinerateExplosion(IncinerateExplosion {
                    source: p.source,
                    team: p.team,
                    profile: p.profile,
                    position: positions[i],
                    due_tick: self.next_tick + u64::from(p.profile.delay_ticks),
                }),
            ));
        }
    }
    pub(super) fn advance_incinerate_explosion(
        &mut self,
        entity: Entity,
        state: IncinerateExplosion,
        units: &mut [UnitSnapshot],
    ) {
        if self.next_tick < state.due_tick {
            return;
        }
        for i in 0..units.len() {
            let target = units[i];
            let distance = target.position.distance_sq(state.position);
            if target.health <= 0
                || !eligible(&target, state.team)
                || distance > square_i32(state.profile.half_radius)
            {
                continue;
            }
            let damage = if distance <= square_i32(state.profile.full_radius) {
                state.profile.full_damage
            } else {
                state.profile.half_damage
            };
            apply_ability_effect_to_unit(
                &mut units[i],
                AbilityEffect::Damage { amount: damage },
                state.team,
                self.next_tick,
                self.combat_rules.damage_rules,
            );
            death_healing::observe_inline_death_from(units, i, target.health, Some(state.source));
        }
        self.last_ability_casts.push(AbilityCastEvent {
            source: state.source,
            ability: state.profile.ability,
            target: AbilityCastTarget::Point(state.position),
            target_position: Some(state.position),
            effect: AbilityEffect::Damage {
                amount: state.profile.full_damage,
            },
        });
        self.world.despawn(entity);
    }
}
