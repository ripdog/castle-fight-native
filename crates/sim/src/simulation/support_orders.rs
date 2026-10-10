use super::*;
use crate::components::{NativeAction, SupportOrderQueue, SupportTargetCache};

fn profile(unit: &UnitSnapshot) -> Option<crate::WindWalkProfile> {
    match unit.spellcasting?.ability.effect {
        AbilityEffect::WindWalk(p) => Some(p),
        _ => None,
    }
}
fn within(p: SimPoint, rect: [SimPoint; 2]) -> bool {
    p.x >= rect[0].x && p.y >= rect[0].y && p.x <= rect[1].x && p.y <= rect[1].y
}
fn target_valid(
    target: &UnitSnapshot,
    team: Team,
    rules: crate::SupportOrderRules,
    tick: u64,
) -> bool {
    target.health > 0
        && target.team != team
        && target.classifications.combat_sapper
        && !target.classifications.invulnerable
        && target.movement_class != MovementClass::Air
        && !target.status.wind_walk_active(tick)
        && !crate::content::unit_has_ability_for_version(
            target.rawcode,
            AbilityId(rules.excluded_ability),
            rules.map_version,
        )
        && !target.status.armor_modifiers[..usize::from(target.status.armor_modifier_count)]
            .iter()
            .any(|m| {
                tick < m.expires_tick
                    && m.native_buff
                        .is_some_and(|b| rules.excluded_buffs.contains(&b.rawcode))
            })
}
impl Simulation {
    pub(super) fn resolve_support_orders(&mut self, units: &mut [UnitSnapshot]) {
        let (existing, mut queue) = self
            .world
            .iter_entities()
            .find_map(|e| match e.get::<NativeAction>()? {
                NativeAction::SupportOrders(q) => Some((Some(e.id()), Some(q.clone()))),
                _ => None,
            })
            .unwrap_or((None, None));
        for u in units.iter_mut() {
            if u.status
                .wind_walk
                .is_some_and(|w| self.next_tick >= w.expires_tick)
            {
                u.status.wind_walk = None;
            }
            let Some(p) = profile(u) else {
                continue;
            };
            if u.health <= 0 || u.status.support_order.is_some() {
                continue;
            }
            let q = queue.get_or_insert_with(|| SupportOrderQueue {
                rules: p.rules,
                next_tick: self.next_tick.div_ceil(u64::from(p.rules.interval_ticks))
                    * u64::from(p.rules.interval_ticks),
                actors: Default::default(),
                caches: Vec::new(),
            });
            assert_eq!(
                q.rules, p.rules,
                "shared support queue rules must have one source revision"
            );
            q.actors.push_back(u.id);
            u.status.support_order = Some(Default::default());
        }
        let Some(mut q) = queue else {
            return;
        };
        if self.next_tick >= q.next_tick {
            let original_count = q.actors.len();
            let interval = if q.actors.len() > usize::from(q.rules.budget) * 4 {
                q.rules.interval_ticks / 2
            } else {
                q.rules.interval_ticks
            };
            q.next_tick = self.next_tick + u64::from(interval);
            let mut serviced = 0;
            let mut popped = 0;
            while popped < original_count && serviced < usize::from(q.rules.budget) {
                let id = q.actors.pop_front().expect("queue count");
                popped += 1;
                let Some(i) = find_unit_index(units, id)
                    .filter(|&i| units[i].health > 0 && profile(&units[i]).is_some())
                else {
                    // Deindexed/deleted jobs are replaced within the same native budget.
                    // A dead actor whose original handle/corpse remains consumes one slot.
                    let handle_exists = self.world.iter_entities().any(|e| {
                        e.get::<SimId>() == Some(&id)
                            || e.get::<Corpse>().is_some_and(|c| c.source_unit == id)
                    });
                    serviced += usize::from(handle_exists);
                    continue;
                };
                serviced += 1;
                self.issue_support_order(i, &mut q, units);
                q.actors.push_back(id);
            }
        }
        if let Some(entity) = existing {
            *self
                .world
                .get_mut::<NativeAction>(entity)
                .expect("support queue") = NativeAction::SupportOrders(q);
        } else {
            let id = self.allocate_id();
            self.world.spawn((id, NativeAction::SupportOrders(q)));
        }
    }
    fn issue_support_order(
        &mut self,
        i: usize,
        q: &mut SupportOrderQueue,
        units: &mut [UnitSnapshot],
    ) {
        let p = profile(&units[i]).expect("support actor");
        let mut state = units[i].status.support_order.unwrap();
        let blocked = units[i].abilities_disabled
            || units[i].orders_suspended
            || self.next_tick < units[i].status.stunned_until_tick;
        if let Some(target) = state
            .selected_target
            .and_then(|id| find_unit_index(units, id))
            .filter(|&j| target_valid(&units[j], units[i].team, q.rules, self.next_tick))
        {
            state.iterations += 1;
            if state.iterations > q.rules.retention_limit {
                state.selected_target = None;
                state.iterations = 0;
            } else if blocked {
                // Native attack/move orders fail, but the source queue still advances its counter.
            } else if units[target].visible_to(units[i].team, self.next_tick) {
                state.order_target = Some(units[target].id);
                state.move_point = None;
            } else {
                state.order_target = None;
                state.move_point = Some(units[target].position);
            }
        } else {
            let casting = units[i].spellcasting.unwrap();
            let ready = units[i].ability_state.unwrap();
            let can_cast = !blocked
                && units[i].mana_current.unwrap() >= casting.ability.mana_cost
                && self.next_tick >= ready.ready_tick;
            if units[i].status.wind_walk_active(self.next_tick) || can_cast {
                if !units[i].status.wind_walk_active(self.next_tick) {
                    units[i].mana_current =
                        Some(units[i].mana_current.unwrap() - casting.ability.mana_cost);
                    units[i].ability_state.as_mut().unwrap().ready_tick =
                        self.next_tick + u64::from(casting.ability.cooldown_ticks);
                    units[i].ability_state.as_mut().unwrap().cast_sequence += 1;
                    units[i].status.wind_walk = Some(crate::WindWalkState {
                        ability: p.ability,
                        cloaked_tick: self.next_tick + u64::from(p.fade_ticks),
                        expires_tick: self.next_tick + u64::from(p.duration_ticks),
                        movement_percent_delta: p.movement_percent_delta,
                        backstab_damage: p.backstab_damage,
                    });
                    units[i].status.pending_attack = None;
                    self.last_ability_casts.push(AbilityCastEvent {
                        source: units[i].id,
                        ability: p.ability,
                        target: AbilityCastTarget::Unit(units[i].id),
                        target_position: Some(units[i].position),
                        effect: AbilityEffect::WindWalk(p),
                    });
                }
                let owner = units[i].owner;
                let cache_index = q.caches.iter().position(|c| c.owner == owner);
                if cache_index.is_none_or(|c| self.next_tick >= q.caches[c].expires_tick) {
                    let mut groups: [[Vec<SimId>; 3]; 2] = Default::default();
                    for target in units
                        .iter()
                        .filter(|u| target_valid(u, units[i].team, q.rules, self.next_tick))
                    {
                        let maximum_mana = target
                            .map_version
                            .and_then(|v| {
                                crate::content::native_mana_maximum_for_version(v, target.rawcode)
                            })
                            .unwrap_or_else(|| target.spellcasting.map_or(0, |s| s.mana.maximum));
                        for (area, rect) in [
                            q.rules.local_rectangles[usize::from(units[i].team.0)],
                            q.rules.battlefield,
                        ]
                        .into_iter()
                        .enumerate()
                        {
                            if !within(target.position, rect) {
                                continue;
                            }
                            if maximum_mana > q.rules.preferred_mana_above {
                                groups[area][0].push(target.id);
                            }
                            // Both fallback classes remain cached, matching source per-owner groups.
                            let ordinary = q.rules.fallback_health_below[0];
                            let royal = q.rules.fallback_health_below[1];
                            if target.health < ordinary {
                                groups[area][1].push(target.id);
                            }
                            if target.health < royal {
                                groups[area][2].push(target.id);
                            }
                        }
                    }
                    let cache = SupportTargetCache {
                        owner,
                        expires_tick: self.next_tick + u64::from(q.rules.cache_ticks),
                        groups,
                    };
                    if let Some(c) = cache_index {
                        q.caches[c] = cache;
                    } else {
                        q.caches.push(cache);
                        q.caches.sort_unstable_by_key(|c| c.owner);
                    }
                }
                let cache = q.caches.iter().find(|c| c.owner == owner).unwrap();
                let royal = q.rules.fallback_health_below[1] == p.fallback_health_below;
                let fallback = if royal { 2 } else { 1 };
                let selected = [
                    &cache.groups[0][0],
                    &cache.groups[0][fallback],
                    &cache.groups[1][0],
                    &cache.groups[1][fallback],
                ]
                .into_iter()
                .find(|g| !g.is_empty())
                .and_then(|g| {
                    let roll = deterministic_random(
                        self.config.match_seed,
                        self.next_tick,
                        units[i].id,
                        RANDOM_PURPOSE_ABILITY_TARGET ^ u64::from(p.ability.0),
                        state.sequence,
                    );
                    find_unit_index(units, g[(roll % g.len() as u64) as usize])
                });
                state.iterations = 0;
                state.sequence += 1;
                state.selected_target = selected.map(|j| units[j].id);
                if !blocked {
                    state.order_target = None;
                    state.move_point =
                        Some(selected.map_or(units[i].position, |j| units[j].position));
                }
            } else if !blocked {
                state.selected_target = None;
                state.order_target = None;
                state.move_point = None;
            }
        }
        units[i].status.support_order = Some(state);
    }
}

#[cfg(test)]
mod tests;
