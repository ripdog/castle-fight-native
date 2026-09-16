use super::*;

impl Simulation {
    pub(super) fn resolve_burning_oil_zones(
        &mut self,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
        completed_tick: u64,
    ) {
        let mut query = self.world.query::<(Entity, &SimId, &BurningOilZone)>();
        let mut zones: Vec<_> = query
            .iter(&self.world)
            .map(|(entity, id, zone)| (entity, *id, *zone))
            .collect();
        zones.sort_unstable_by_key(|(_, id, _)| *id);

        let mut expired = Vec::new();
        let mut updates = Vec::new();
        for (entity, _, mut zone) in zones {
            let expires_tick = zone
                .created_tick
                .checked_add(ceil_millis_to_ticks(u64::from(
                    zone.profile.total_duration_millis,
                )))
                .expect("Burning Oil expiry tick overflow");
            if completed_tick >= expires_tick {
                expired.push(entity);
                continue;
            }

            while let Some((offset_millis, damage)) =
                burning_oil_pulse(zone.profile, zone.pulse_index)
            {
                let due_tick = zone
                    .created_tick
                    .checked_add(ceil_millis_to_ticks(u64::from(offset_millis)))
                    .expect("Burning Oil pulse tick overflow");
                if due_tick > completed_tick {
                    break;
                }
                let radius_sq = square_i32(zone.profile.radius);
                if zone.profile.target_ground_units {
                    for unit in units.iter_mut() {
                        if unit.health <= 0
                            || unit.team == zone.source_team
                            || unit.movement_class != MovementClass::Ground
                            || zone.center.distance_sq(unit.position) > radius_sq
                        {
                            continue;
                        }
                        let adjusted = self
                            .combat_rules
                            .damage_rules
                            .apply_spell(damage, unit.armor.armor_type);
                        let adjusted = spell_damage_after_defend(*unit, adjusted, completed_tick);
                        unit.health = unit
                            .health
                            .checked_sub(adjusted)
                            .expect("Burning Oil unit damage overflow");
                    }
                }
                if zone.profile.target_buildings {
                    for building in buildings.iter_mut() {
                        if building.health <= 0 || building.team == zone.source_team {
                            continue;
                        }
                        if point_to_footprint_distance_sq(
                            zone.center,
                            building.footprint,
                            self.config.navigation_cell_size,
                        ) > radius_sq
                        {
                            continue;
                        }
                        let adjusted = self
                            .combat_rules
                            .damage_rules
                            .apply_spell(damage, building.armor.armor_type);
                        building.health = building
                            .health
                            .checked_sub(adjusted)
                            .expect("Burning Oil building damage overflow");
                    }
                }
                zone.pulse_index = zone
                    .pulse_index
                    .checked_add(1)
                    .expect("Burning Oil pulse index overflow");
            }
            updates.push((entity, zone));
        }

        for entity in expired {
            self.world.despawn(entity);
        }
        for (entity, zone) in updates {
            if let Some(mut stored) = self.world.entity_mut(entity).get_mut::<BurningOilZone>() {
                *stored = zone;
            }
        }
    }

    pub(super) fn snapshot_due_projectiles(&mut self) -> Vec<ProjectileSnapshot> {
        let mut query = self
            .world
            .query::<(Entity, &SimId, &GuaranteedHitProjectile)>();
        let mut projectiles: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, projectile)| projectile.impact_tick <= self.next_tick)
            .map(|(entity, id, projectile)| ProjectileSnapshot {
                entity,
                id: *id,
                projectile: *projectile,
            })
            .collect();
        projectiles.sort_unstable_by_key(|projectile| projectile.id);
        projectiles
    }

    pub(super) fn snapshot_due_reflected_projectiles(
        &mut self,
    ) -> Vec<ReflectedProjectileSnapshot> {
        let mut query = self.world.query::<(Entity, &SimId, &ReflectedProjectile)>();
        let mut projectiles: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, projectile)| projectile.impact_tick <= self.next_tick)
            .map(|(entity, id, projectile)| ReflectedProjectileSnapshot {
                entity,
                id: *id,
                projectile: *projectile,
            })
            .collect();
        projectiles.sort_unstable_by_key(|projectile| projectile.id);
        projectiles
    }

    pub(super) fn snapshot_due_bounce_projectiles(&mut self) -> Vec<BounceProjectileSnapshot> {
        let mut query = self.world.query::<(Entity, &SimId, &BounceProjectile)>();
        let mut projectiles: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, projectile)| projectile.impact_tick <= self.next_tick)
            .map(|(entity, id, projectile)| BounceProjectileSnapshot {
                entity,
                id: *id,
                projectile: *projectile,
            })
            .collect();
        projectiles.sort_unstable_by_key(|projectile| projectile.id);
        projectiles
    }

    pub(super) fn snapshot_due_ballistic_projectiles(
        &mut self,
    ) -> Vec<BallisticProjectileSnapshot> {
        let mut query = self.world.query::<(Entity, &SimId, &BallisticProjectile)>();
        let mut projectiles: Vec<_> = query
            .iter(&self.world)
            .filter(|(_, _, projectile)| projectile.impact_tick <= self.next_tick)
            .map(|(entity, id, projectile)| BallisticProjectileSnapshot {
                entity,
                id: *id,
                projectile: *projectile,
            })
            .collect();
        projectiles.sort_unstable_by_key(|projectile| projectile.id);
        projectiles
    }

    pub(super) fn select_bounce_target(
        &self,
        projectile_id: SimId,
        projectile: &BounceProjectile,
        impact_position: SimPoint,
        context: &BounceSearchContext<'_>,
        candidate_checks: &mut usize,
    ) -> Option<usize> {
        let enemy_team = 1u8
            .checked_sub(projectile.source_team.0)
            .expect("verification slice supports teams 0 and 1 only");
        let range_sq = square_i32(projectile.bounce_range);
        let hit_count = usize::from(projectile.hit_count);
        let hit_targets = &projectile.hit_targets[..hit_count];
        let next_bounce_index = u32::from(projectile.bounce_index) + 1;
        let mut best: Option<(u64, SimId, usize)> = None;
        context.grid.for_each_candidate(
            SpatialPartition::global(enemy_team),
            impact_position,
            projectile.bounce_range,
            |unit_index| {
                *candidate_checks += 1;
                if context.unit_health[unit_index] <= 0 {
                    return;
                }
                let candidate = &context.units[unit_index];
                if !projectile
                    .target_mask
                    .can_target_unit(candidate.movement_class)
                    || candidate.id == projectile.target
                    || impact_position.distance_sq(candidate.position) > range_sq
                {
                    return;
                }
                if !projectile.allow_repeat_targets && hit_targets.contains(&candidate.id) {
                    return;
                }
                let rank = deterministic_random(
                    self.config.match_seed,
                    context.completed_tick,
                    projectile_id,
                    RANDOM_PURPOSE_BOUNCE_TARGET ^ candidate.id.0,
                    u64::from(next_bounce_index),
                );
                let key = (rank, candidate.id, unit_index);
                if best.is_none_or(|current| key < current) {
                    best = Some(key);
                }
            },
        );
        best.map(|(_, _, unit_index)| unit_index)
    }
}
