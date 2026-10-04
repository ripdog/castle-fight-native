//! Native targeted missile-line delivery. The primary missile follows its target; after
//! impact, its spill is a fixed directed strip, swept at missile speed (not radial splash).
use super::*;

#[cfg(test)]
mod tests;

impl Simulation {
    pub(super) fn resolve_line_projectiles(
        &mut self,
        context: TargetProjectileContext<'_>,
    ) -> (usize, usize, usize) {
        let TargetProjectileContext {
            units,
            buildings,
            unit_health,
            building_health,
            positions,
            attackers_this_tick,
            next_defense_alerts,
            completed_tick,
            ..
        } = context;
        let mut query = self.world.query::<(Entity, &SimId, &LineProjectile)>();
        let mut projectiles: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, p)| p.primary_impact_tick <= completed_tick)
            .map(|(entity, id, p)| (entity, *id, p.clone()))
            .collect();
        projectiles.sort_unstable_by_key(|(_, id, _)| *id);
        let mut effects = 0;
        let mut impacts = 0;
        let mut invalidations = 0;
        for (entity, _, mut p) in projectiles {
            let AttackDelivery::Line {
                speed_per_tick,
                spill_distance,
                spill_radius,
                damage_retention_per_10k,
                spill_targets,
                ..
            } = p.delivery
            else {
                unreachable!("line projectile must retain line delivery");
            };
            let mut hits = Vec::new();
            if p.spill_origin.is_none() {
                impacts += 1;
                let Some(target) = find_target_index(units, buildings, p.target) else {
                    self.world.despawn(entity);
                    invalidations += 1;
                    continue;
                };
                let origin = match target {
                    TargetIndex::Unit(index) if unit_health[index] > 0 => positions[index],
                    TargetIndex::Building(index) if building_health[index] > 0 => {
                        footprint_center_point(
                            buildings[index].footprint,
                            self.config.navigation_cell_size,
                        )
                    }
                    _ => {
                        self.world.despawn(entity);
                        invalidations += 1;
                        continue;
                    }
                };
                p.spill_origin = Some(origin);
                p.destination = line_endpoint(p.launch_position, origin, spill_distance);
                hits.push((0, p.target, target));
            }
            let origin = p.spill_origin.expect("primary impact fixes spill origin");
            let elapsed = completed_tick.saturating_sub(p.primary_impact_tick);
            let end = i32::try_from((elapsed * speed_per_tick as u64).min(spill_distance as u64))
                .expect("spill travel within i32");
            let start = i32::try_from(
                (elapsed.saturating_sub(1) * speed_per_tick as u64).min(spill_distance as u64),
            )
            .expect("spill travel within i32");
            if elapsed > 0 {
                for (index, unit) in units.iter().enumerate() {
                    if unit.team != p.source_team
                        && unit_health[index] > 0
                        && spill_targets.can_target_unit(unit.movement_class)
                        && !p.hit_targets.contains(&unit.id)
                        && unit.id != p.target
                        && let Some(dot) = line_point_hit(
                            p.launch_position,
                            origin,
                            positions[index],
                            start,
                            end,
                            spill_radius,
                        )
                    {
                        hits.push((dot, unit.id, TargetIndex::Unit(index)));
                    }
                }
                for (index, building) in buildings.iter().enumerate() {
                    if building.team == p.source_team
                        || building_health[index] <= 0
                        || !spill_targets.can_target_buildings()
                        || building.id == p.target
                        || p.hit_targets.contains(&building.id)
                    {
                        continue;
                    }
                    if let Some(dot) = line_footprint_hit(
                        p.launch_position,
                        origin,
                        building.footprint,
                        self.config.navigation_cell_size,
                        start,
                        end,
                        spill_radius,
                    ) {
                        hits.push((dot, building.id, TargetIndex::Building(index)));
                    }
                }
            }
            // Damage loss is per collision, so spatial ordering matters; stable IDs break ties.
            hits.sort_unstable_by_key(|(dot, id, _)| (*dot, *id));
            for (_, id, target) in hits {
                let damage =
                    ranged_projectile_damage_after_defend(target, p.damage, completed_tick, units);
                if apply_damage_to_target(
                    target,
                    p.source,
                    damage,
                    p.damage_type,
                    completed_tick,
                    self.debug_buildings_invulnerable,
                    DamageTargetState {
                        damage_rules: self.combat_rules.damage_rules,
                        units,
                        buildings,
                        unit_positions: positions,
                        unit_health,
                        building_health,
                        attackers_this_tick,
                        next_defense_alerts,
                        navigation_cell_size: self.config.navigation_cell_size,
                    },
                )
                .is_some()
                {
                    effects += 1;
                }
                p.hit_targets.push(id);
                p.damage = i32::try_from(
                    i64::from(p.damage) * i64::from(damage_retention_per_10k) / 10_000,
                )
                .expect("line retained damage within i32");
            }
            if completed_tick >= p.impact_tick || p.damage == 0 || origin == p.launch_position {
                self.world.despawn(entity);
            } else {
                *self
                    .world
                    .entity_mut(entity)
                    .get_mut::<LineProjectile>()
                    .expect("line state") = p;
            }
        }
        (impacts, effects, invalidations)
    }
}

fn line_endpoint(source: SimPoint, origin: SimPoint, distance: i32) -> SimPoint {
    let dx = i64::from(origin.x) - i64::from(source.x);
    let dy = i64::from(origin.y) - i64::from(source.y);
    let length_sq = dx as i128 * dx as i128 + dy as i128 * dy as i128;
    // Subunit-truncated sqrt can stretch a short diagonal by sqrt(2). Normalize the
    // presentation endpoint with 24 fractional bits; i32-coordinate differences fit
    // comfortably in u128 after this shift. Gameplay intersections remain exact below.
    const SCALE: i128 = 1 << 24;
    let length = ((length_sq as u128) << 48).isqrt().max(1) as i128;
    SimPoint::new(
        i32::try_from(
            i128::from(origin.x) + i128::from(dx) * i128::from(distance) * SCALE / length,
        )
        .expect("line endpoint x"),
        i32::try_from(
            i128::from(origin.y) + i128::from(dy) * i128::from(distance) * SCALE / length,
        )
        .expect("line endpoint y"),
    )
}

/// Exact dot/cross comparisons avoid diagonal speed/radius errors from normalization.
fn line_point_hit(
    source: SimPoint,
    origin: SimPoint,
    point: SimPoint,
    start: i32,
    end: i32,
    radius: i32,
) -> Option<i128> {
    let dx = i128::from(origin.x) - i128::from(source.x);
    let dy = i128::from(origin.y) - i128::from(source.y);
    let px = i128::from(point.x) - i128::from(origin.x);
    let py = i128::from(point.y) - i128::from(origin.y);
    let length_sq = dx * dx + dy * dy;
    let dot = dx * px + dy * py;
    let cross = dx * py - dy * px;
    (length_sq > 0
        && dot >= 0
        && dot * dot >= i128::from(start).pow(2) * length_sq
        && dot * dot <= i128::from(end).pow(2) * length_sq
        && cross * cross <= i128::from(radius).pow(2) * length_sq)
        .then_some(dot)
}

/// SAT intersection of an axis-aligned footprint with the directed spill rectangle.
/// All projections use doubled coordinates; irrational direction lengths are compared
/// by squaring only nonnegative quantities, without rounding the gameplay ray.
fn line_footprint_hit(
    source: SimPoint,
    origin: SimPoint,
    footprint: BuildingFootprint,
    cell: i32,
    start: i32,
    end: i32,
    radius: i32,
) -> Option<i128> {
    let dx = i128::from(origin.x) - i128::from(source.x);
    let dy = i128::from(origin.y) - i128::from(source.y);
    let len = dx * dx + dy * dy;
    if len == 0 {
        return None;
    }
    let min_x = i128::from(footprint.min_x) * i128::from(cell);
    let min_y = i128::from(footprint.min_y) * i128::from(cell);
    let max_x = (i128::from(footprint.max_x()) + 1) * i128::from(cell);
    let max_y = (i128::from(footprint.max_y()) + 1) * i128::from(cell);
    let cx = min_x + max_x - 2 * i128::from(origin.x);
    let cy = min_y + max_y - 2 * i128::from(origin.y);
    let wx = max_x - min_x;
    let wy = max_y - min_y;
    let dot = dx * cx + dy * cy;
    let dot_extent = dx.abs() * wx + dy.abs() * wy;
    let cross = dx * cy - dy * cx;
    let cross_extent = dy.abs() * wx + dx.abs() * wy;
    let sum = i128::from(start) + i128::from(end);
    let span = i128::from(end) - i128::from(start);
    let radius = i128::from(radius);
    // Ray axis and perpendicular axis.
    if !root_le(2 * i128::from(start), dot + dot_extent, len)
        || !root_le(-(2 * i128::from(end)), -(dot - dot_extent), len)
        || !root_le(-2 * radius, -(cross.abs() - cross_extent), len)
    {
        return None;
    }
    // World X and Y axes. |c * sqrt(len) - direction * sum| <=
    // footprint_width * sqrt(len) + |direction| * span + perpendicular_width.
    for (c, width, direction, perpendicular) in [(cx, wx, dx, dy), (cy, wy, dy, dx)] {
        let extent = direction.abs() * span + 2 * perpendicular.abs() * radius;
        if !root_le(c - width, direction * sum + extent, len)
            || !root_le(-c - width, -direction * sum + extent, len)
        {
            return None;
        }
    }
    Some((dot - dot_extent).max(0) / 2)
}

/// a * sqrt(len) <= b, including signed operands.
fn root_le(a: i128, b: i128, len: i128) -> bool {
    if a >= 0 && b < 0 {
        return false;
    }
    if a <= 0 && b >= 0 {
        return true;
    }
    if a > 0 {
        a * a * len <= b * b
    } else {
        a * a * len >= b * b
    }
}
