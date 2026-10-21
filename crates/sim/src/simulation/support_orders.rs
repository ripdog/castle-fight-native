use super::*;
use crate::components::{
    GobboRepairProfile, NativeAction, SupportOrderQueue, SupportOrderState, SupportTargetCache,
};

fn profile(unit: &UnitSnapshot) -> Option<crate::WindWalkProfile> {
    match unit.spellcasting?.ability.effect {
        AbilityEffect::WindWalk(p) => Some(p),
        _ => None,
    }
}

fn gobbo_profile(unit: &UnitSnapshot) -> Option<GobboRepairProfile> {
    unit.effective_passive_effects()
        .find_map(|effect| match effect {
            PassiveUnitEffect::GobboRepair(profile) => Some(profile),
            _ => None,
        })
}

/// The bodies that share the source's one support task queue.
fn queue_rules(unit: &UnitSnapshot) -> Option<crate::SupportOrderRules> {
    profile(unit)
        .map(|p| p.rules)
        .or_else(|| gobbo_profile(unit).map(|p| p.rules))
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
    pub(super) fn resolve_support_orders(
        &mut self,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
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
            let Some(rules) = queue_rules(u) else {
                continue;
            };
            if u.health <= 0 || u.status.support_order.is_some() {
                continue;
            }
            let q = queue.get_or_insert_with(|| SupportOrderQueue {
                rules,
                next_tick: self.next_tick.div_ceil(u64::from(rules.interval_ticks))
                    * u64::from(rules.interval_ticks),
                actors: Default::default(),
                caches: Vec::new(),
            });
            assert_eq!(
                q.rules, rules,
                "shared support queue rules must have one source revision"
            );
            q.actors.push_back(u.id);
            // A Gobbo waits where it appeared until its first order.
            u.status.support_order = Some(SupportOrderState {
                move_point: gobbo_profile(u).map(|_| u.position),
                ..Default::default()
            });
        }
        let Some(mut q) = queue else {
            return;
        };
        self.advance_gobbo_repairs(units, buildings);
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
                    .filter(|&i| units[i].health > 0 && queue_rules(&units[i]).is_some())
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
                if let Some(gobbo) = gobbo_profile(&units[i]) {
                    self.issue_gobbo_order(i, gobbo, units, buildings);
                } else {
                    self.issue_support_order(i, &mut q, units);
                }
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

impl Simulation {
    fn building_max_health(&self, building: &BuildingSnapshot) -> i32 {
        self.world
            .get::<Health>(building.entity)
            .map_or(building.health, |health| health.max)
    }

    /// `isGobboRepairableTarget` is a mechanical unit or structure missing more than one point of life.
    fn gobbo_repairable(mechanical_or_structure: bool, health: i32, maximum: i32) -> bool {
        mechanical_or_structure && health > 0 && maximum - health > 1
    }

    /// `orderGoboW` on a task-queue pass: keep a living, wounded ally; otherwise draw a new one
    /// uniformly from the allied wounded mechanical bodies and structures on the battlefield, or
    /// failing that from any allied body or structure there. A drawn body is followed (and repaired
    /// while it is repairable); an empty draw leaves the Gobbo idle.
    fn issue_gobbo_order(
        &mut self,
        i: usize,
        profile: GobboRepairProfile,
        units: &mut [UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) {
        if units[i].abilities_disabled
            || units[i].orders_suspended
            || self.next_tick < units[i].status.stunned_until_tick
        {
            return;
        }
        let mut state = units[i].status.support_order.unwrap_or_default();
        let team = units[i].team;
        let kept = state.selected_target.is_some_and(|id| {
            if let Some(j) = find_unit_index(units, id) {
                units[j].health > 0
                    && units[j].team == team
                    && units[j].health < units[j].health_max
            } else {
                buildings.iter().any(|building| {
                    building.id == id
                        && building.health > 0
                        && building.team == team
                        && building.health < self.building_max_health(building)
                })
            }
        });
        if kept {
            units[i].status.support_order = Some(state);
            return;
        }
        let rect = profile.rules.battlefield;
        let inside = |p: SimPoint| {
            p.x >= rect[0].x && p.y >= rect[0].y && p.x <= rect[1].x && p.y <= rect[1].y
        };
        let mut wounded = Vec::new();
        let mut allies = Vec::new();
        for unit in units.iter() {
            if unit.team == team && unit.health > 0 && inside(unit.position) {
                allies.push(unit.id);
                if Self::gobbo_repairable(unit.mechanical, unit.health, unit.health_max) {
                    wounded.push(unit.id);
                }
            }
        }
        for building in buildings {
            let center =
                footprint_center_point(building.footprint, self.config.navigation_cell_size);
            if building.team == team && building.health > 0 && inside(center) {
                allies.push(building.id);
                if Self::gobbo_repairable(true, building.health, self.building_max_health(building))
                {
                    wounded.push(building.id);
                }
            }
        }
        let gobbo = units[i].id;
        let rank = |id: &SimId| {
            (
                deterministic_ability_target_rank(
                    self.config.match_seed,
                    gobbo,
                    profile.ability,
                    state.sequence,
                    *id,
                ),
                *id,
            )
        };
        let group = if wounded.is_empty() {
            &allies
        } else {
            &wounded
        };
        let selected = group.iter().min_by_key(|id| rank(id)).copied();
        state.sequence += 1;
        state.iterations = 0;
        state.repair_remainder = 0;
        state.selected_target = selected;
        state.move_point = Some(units[i].position);
        units[i].status.support_order = Some(state);
    }

    /// The running `repair`/`smart` order: follow the selected body and, within the native repair
    /// range, restore its life at the retained fraction of its repair time. Completing a repair
    /// ends the order.
    fn advance_gobbo_repairs(
        &mut self,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        for i in 0..units.len() {
            let Some(profile) = gobbo_profile(&units[i]).filter(|_| units[i].health > 0) else {
                continue;
            };
            let Some(mut state) = units[i].status.support_order else {
                continue;
            };
            let position = units[i].position;
            let blocked = units[i].abilities_disabled
                || units[i].orders_suspended
                || self.next_tick < units[i].status.stunned_until_tick;
            let team = units[i].team;
            let Some(target) = state.selected_target else {
                state.move_point = Some(position);
                units[i].status.support_order = Some(state);
                continue;
            };
            let range_sq = square_i32(profile.repair_range);
            // (position, within range, repairable, maximum life, repair time, entity) of the body.
            enum Body {
                Unit(usize),
                Building(usize),
            }
            let body = find_unit_index(units, target)
                .filter(|&j| units[j].health > 0 && units[j].team == team)
                .map(Body::Unit)
                .or_else(|| {
                    buildings
                        .iter()
                        .position(|b| b.id == target && b.health > 0 && b.team == team)
                        .map(Body::Building)
                });
            let Some(body) = body else {
                state.selected_target = None;
                state.repair_remainder = 0;
                state.move_point = Some(position);
                units[i].status.support_order = Some(state);
                continue;
            };
            let (target_position, distance_sq, repairable, health, maximum, entity) = match body {
                Body::Unit(j) => (
                    units[j].position,
                    position.distance_sq(units[j].position),
                    Self::gobbo_repairable(
                        units[j].mechanical,
                        units[j].health,
                        units[j].health_max,
                    ),
                    units[j].health,
                    units[j].health_max,
                    units[j].entity,
                ),
                Body::Building(b) => (
                    footprint_center_point(
                        buildings[b].footprint,
                        self.config.navigation_cell_size,
                    ),
                    point_to_footprint_distance_sq(
                        position,
                        buildings[b].footprint,
                        self.config.navigation_cell_size,
                    ),
                    Self::gobbo_repairable(
                        true,
                        buildings[b].health,
                        self.building_max_health(&buildings[b]),
                    ),
                    buildings[b].health,
                    self.building_max_health(&buildings[b]),
                    buildings[b].entity,
                ),
            };
            state.move_point = Some(target_position);
            if repairable && !blocked && distance_sq <= range_sq {
                let repair_time = self
                    .world
                    .get::<RepairTimeTicks>(entity)
                    .map_or(0, |ticks| u64::from(ticks.0));
                let duration = (repair_time * u64::from(profile.repair_time_ratio_numerator))
                    .div_ceil(u64::from(profile.repair_time_ratio_denominator))
                    .max(1);
                let accumulated = u64::from(state.repair_remainder)
                    + u64::try_from(maximum).expect("repair target maximum life");
                let repaired = i32::try_from(accumulated / duration).unwrap_or(i32::MAX);
                state.repair_remainder = u32::try_from(accumulated % duration)
                    .expect("repair remainder fits the order state");
                let restored = health.saturating_add(repaired).min(maximum);
                match body {
                    Body::Unit(j) => units[j].health = restored,
                    Body::Building(b) => buildings[b].health = restored,
                }
                if restored >= maximum {
                    state.selected_target = None;
                    state.repair_remainder = 0;
                    state.move_point = Some(position);
                }
            }
            units[i].status.support_order = Some(state);
        }
    }
}

#[cfg(test)]
mod tests;
