use super::*;

/// Native dead-unit handles retain installed abilities for already released damage.
/// Only active Forge grants are needed once current life is zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RetiredDamageSource {
    pub team: Team,
    pub hits_ground: bool,
    pub hits_air: bool,
    pub grants: crate::NativeAbilityGrants,
}

impl RetiredDamageSource {
    pub(super) fn capture(unit: &UnitSnapshot) -> Option<Self> {
        let mut grants = crate::NativeAbilityGrants::default();
        for grant in unit.status.native_grants.iter() {
            if let crate::NativeAbilityGrantEffect::SourceDamageSplash { dispatcher, .. } = grant.effect
                && (unit.status.native_grants.contains(dispatcher)
                    || unit.passive_effects.iter().any(|p| matches!(p, PassiveUnitEffect::SourceDamageDispatcher(id) if id == dispatcher))) {
                grants.insert(grant);
            }
        }
        grants.iter().next()?;
        Some(Self {
            team: unit.team,
            hits_ground: unit.attack_targets.can_target_unit(MovementClass::Ground)
                || unit
                    .secondary_attack
                    .is_some_and(|p| p.targets.can_target_unit(MovementClass::Ground)),
            hits_air: unit.attack_targets.can_target_unit(MovementClass::Air)
                || unit
                    .secondary_attack
                    .is_some_and(|p| p.targets.can_target_unit(MovementClass::Air)),
            grants,
        })
    }
}

pub(super) struct SourceDamageState<'a> {
    pub units: &'a mut [UnitSnapshot],
    pub health: &'a mut [i32],
    pub rules: DamageRules,
    pub tick: u64,
}

struct DamageStorage<'a> {
    units: &'a mut [UnitSnapshot],
    health: Option<&'a mut [i32]>,
    rules: DamageRules,
    tick: u64,
}
impl DamageStorage<'_> {
    fn health(&self, index: usize) -> i32 {
        self.health
            .as_ref()
            .map_or(self.units[index].health, |health| health[index])
    }
    fn damage(&mut self, index: usize, amount: i32) {
        let after = self
            .health(index)
            .checked_sub(amount)
            .expect("source damage health overflow");
        if let Some(health) = self.health.as_mut() {
            health[index] = after;
        } else {
            self.units[index].health = after;
        }
    }
}

/// hhb's source callback precedes the victim callback and the parent life write.
pub(super) fn dispatch(
    source: SimId,
    victim: usize,
    event_damage: i32,
    state: SourceDamageState<'_>,
    retired: &BTreeMap<SimId, RetiredDamageSource>,
) -> bool {
    dispatch_inner(
        source,
        victim,
        event_damage,
        retired,
        &mut DamageStorage {
            units: state.units,
            health: Some(state.health),
            rules: state.rules,
            tick: state.tick,
        },
    )
}
pub(super) fn dispatch_inline(
    source: SimId,
    victim: usize,
    event_damage: i32,
    units: &mut [UnitSnapshot],
    rules: DamageRules,
    tick: u64,
    retired: &BTreeMap<SimId, RetiredDamageSource>,
) {
    dispatch_inner(
        source,
        victim,
        event_damage,
        retired,
        &mut DamageStorage {
            units,
            health: None,
            rules,
            tick,
        },
    );
}
fn dispatch_inner(
    source: SimId,
    victim: usize,
    event_damage: i32,
    retired: &BTreeMap<SimId, RetiredDamageSource>,
    state: &mut DamageStorage<'_>,
) -> bool {
    if !state.units[victim].classifications.combat_sapper || state.health(victim) <= 0 {
        return false;
    }
    let caster = match find_unit_index(state.units, source) {
        Some(index) => RetiredDamageSource::capture(&state.units[index]),
        None => retired.get(&source).copied(),
    };
    if let Some(caster) = caster {
        for grant in caster.grants.iter() {
            let crate::NativeAbilityGrantEffect::SourceDamageSplash {
                damage_per_10k,
                radius,
                ..
            } = grant.effect
            else {
                continue;
            };
            let center = state.units[victim].position;
            let raw = scale_damage_per_10k(event_damage.max(0), damage_per_10k);
            for index in 0..state.units.len() {
                let target = state.units[index];
                if state.health(index) <= 0
                    || target.team == caster.team
                    || !target.classifications.combat_sapper
                    || target.classifications.invulnerable
                    || center.distance_sq(target.position) > square_i32(radius)
                    || !(if target.movement_class == MovementClass::Air {
                        caster.hits_air
                    } else {
                        caster.hits_ground
                    })
                {
                    continue;
                }
                // e008 dummy: Chaos/Unknown physical damage permits spell immunity and
                // retains numeric armor. Its own marker inventory cannot recurse into Forge.
                let amount = state.rules.apply_attack_with_armor_per_100(
                    raw,
                    DamageType::Chaos,
                    target.armor.armor_type,
                    effective_armor_points_per_100(&target),
                );
                let amount = scale_damage_per_10k(
                    native_kill_effects::damage_after_native_incoming(&target, amount, state.tick),
                    target.snow_damage_taken_per_10k,
                );
                let health = state.health(index);
                damage_thresholds::observe_damage_event(&mut state.units[index], health, amount);
                state.damage(index, amount);
            }
        }
    }
    let dummy_killed = state.health(victim) <= 0;
    aftershock_inner(source, victim, state);
    dummy_killed
}

#[cfg(test)]
pub(super) fn apply_aftershock(source: SimId, victim: usize, state: SourceDamageState<'_>) {
    aftershock_inner(
        source,
        victim,
        &mut DamageStorage {
            units: state.units,
            health: Some(state.health),
            rules: state.rules,
            tick: state.tick,
        },
    );
}

fn aftershock_inner(source: SimId, victim: usize, state: &mut DamageStorage<'_>) {
    if !state.units[victim].classifications.combat_sapper || state.health(victim) <= 0 {
        return;
    }
    let Some(source_index) = find_unit_index(state.units, source) else {
        return;
    };
    let caster = state.units[source_index];
    let profile = caster
        .passive_effects
        .iter()
        .find_map(|p| {
            if let PassiveUnitEffect::HealthScaledAftershock(p) = p {
                Some(p)
            } else {
                None
            }
        })
        .or_else(|| {
            caster.status.native_grants.iter().find_map(|g| {
                if let crate::NativeAbilityGrantEffect::SourceDamageDispatcher { aftershock } =
                    g.effect
                {
                    aftershock
                } else {
                    None
                }
            })
        });
    let Some(profile) = profile else { return };
    let amount = i32::try_from(
        i64::from(profile.maximum_damage) * i64::from(state.health(source_index).max(0))
            / i64::from(caster.health_max.max(1)),
    )
    .expect("aftershock damage fits validated health bounds");
    let target = state.units[victim];
    let amount = state.rules.apply_attack_with_armor_per_100(
        amount,
        DamageType::Siege,
        target.armor.armor_type,
        0,
    );
    let amount = scale_damage_per_10k(
        native_kill_effects::damage_after_native_incoming(&target, amount, state.tick),
        target.snow_damage_taken_per_10k,
    );
    let health = state.health(victim);
    // The nested Aftershock hit disables hhb but leaves independent low-life listeners.
    damage_thresholds::observe_damage(&mut state.units[victim], health);
    state.damage(victim, amount);
    if state.health(victim) <= 0 && state.health(source_index) > 0 {
        native_kill_effects::activate_kill_berserk(&mut state.units[source_index], state.tick);
    }
}
