//! Native missile splash keeps a directed primary hit and a separate collateral mask.
use super::*;

pub(super) struct HomingSplashImpact {
    pub projectile: GuaranteedHitProjectile,
    pub primary: TargetIndex,
    pub center: SimPoint,
    pub tick: u64,
}

impl Simulation {
    pub(super) fn resolve_homing_splash(
        &mut self,
        impact: HomingSplashImpact,
        grid: &SpatialGrid,
        state: DamageTargetState<'_>,
    ) -> usize {
        let HomingSplashImpact {
            projectile,
            primary,
            center,
            tick,
        } = impact;
        let Some(profile) = projectile.on_hit.splash_falloff else {
            return 0;
        };
        let primary_id = target_sim_id(primary, state.units, state.buildings);
        let mut targets = Vec::new();
        let radius_sq = square_i32(profile.outer_radius);
        let enemy_team = 1_u8
            .checked_sub(projectile.source_team.0)
            .expect("two-team splash");
        grid.for_each_candidate(
            SpatialPartition::global(enemy_team),
            center,
            profile.outer_radius,
            |index| {
                let unit = &state.units[index];
                let distance = center.distance_sq(state.unit_positions[index]);
                if unit.id != primary_id
                    && unit.team != projectile.source_team
                    && state.unit_health[index] > 0
                    && profile
                        .targets
                        .can_target_unit(unit.target_movement_class())
                    && distance <= radius_sq
                {
                    targets.push((TargetIndex::Unit(index), distance));
                }
            },
        );
        for (index, building) in state.buildings.iter().enumerate() {
            let distance = point_to_footprint_distance_sq(
                center,
                building.footprint,
                state.navigation_cell_size,
            );
            if building.id != primary_id
                && building.team != projectile.source_team
                && state.building_health[index] > 0
                && profile.targets.can_target_buildings()
                && distance <= radius_sq
            {
                targets.push((TargetIndex::Building(index), distance));
            }
        }
        targets.sort_unstable_by_key(|(target, _)| {
            target_sim_id(*target, state.units, state.buildings)
        });
        let mut effects = 0;
        for (target, distance) in targets {
            let factor = if distance <= square_i32(profile.full_radius) {
                10_000
            } else if distance <= square_i32(profile.medium_radius) {
                i32::from(profile.medium_damage_per_10k)
            } else {
                i32::from(profile.outer_damage_per_10k)
            };
            let scaled = i32::try_from(i64::from(projectile.damage) * i64::from(factor) / 10_000)
                .expect("native splash damage overflow");
            let damage = ranged_projectile_damage_after_defend(target, scaled, tick, state.units);
            effects += usize::from(
                self.apply_damage_to_target(
                    target,
                    WeaponDamageEvent {
                        kind: WeaponDamageKind::Ranged,
                        source_id: projectile.source,
                        damage,
                        damage_type: projectile.damage_type,
                        completed_tick: tick,
                    },
                    self.debug_buildings_invulnerable,
                    DamageTargetState {
                        match_seed: state.match_seed,
                        damage_rules: state.damage_rules,
                        units: state.units,
                        buildings: state.buildings,
                        unit_positions: state.unit_positions,
                        unit_health: state.unit_health,
                        building_health: state.building_health,
                        attackers_this_tick: state.attackers_this_tick,
                        next_defense_alerts: state.next_defense_alerts,
                        navigation_cell_size: state.navigation_cell_size,
                    },
                )
                .is_some(),
            );
            // Native attack modifiers belong to the primary directed hit. Collateral
            // damage does not duplicate poison, Bash, Feedback or triggered spells.
        }
        effects
    }
}
