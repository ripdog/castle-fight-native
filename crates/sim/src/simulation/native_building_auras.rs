//! Intrinsic, zero-linger structure armor and stacking native unit regeneration.
use super::*;
use crate::building_mechanics::NativeBuildingAuraProfile;
#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
struct AuraSource {
    version: crate::MapVersion,
    team: Team,
    position: SimPoint,
    alive: bool,
}
#[derive(Clone, Copy)]
struct Recipient {
    team: Team,
    position: SimPoint,
    alive: bool,
    building: bool,
    movement: MovementClass,
    invulnerable: bool,
}
fn refresh(
    status: &mut StatusState,
    r: Recipient,
    p: NativeBuildingAuraProfile,
    sources: &[AuraSource],
    tick: u64,
) {
    native_carriers::retain_modifiers(
        &mut status.armor_modifiers,
        &mut status.armor_modifier_count,
        |m| m.id.0 != p.armor_ability.0 && m.id.0 != p.regeneration_ability.0,
    );
    if !r.alive {
        return;
    }
    let matching = |radius| {
        sources.iter().filter(move |s| {
            s.alive
                && s.version == p.map_version
                && s.team == r.team
                && s.position.distance_sq(r.position) <= square_i32(radius)
        })
    };
    let (ability, armor, regen, buff) = if r.building {
        if !p.armor_targets.can_target_buildings()
            || (r.invulnerable && !p.armor_invulnerable)
            || matching(p.armor_radius).next().is_none()
        {
            return;
        }
        (
            p.armor_ability,
            p.armor_bonus_per_100,
            0,
            Some(crate::NativeBuffIdentity {
                rawcode: p.armor_buff,
                positive: true,
                stealable: false,
                organic_only: false,
            }),
        )
    } else {
        if !p.regeneration_targets.can_target_unit(r.movement)
            || (r.invulnerable && !p.regeneration_invulnerable)
        {
            return;
        }
        let regen = matching(p.regeneration_radius).fold(0u32, |rate, _| {
            rate.saturating_add(p.regeneration_per_second_per_10k)
        });
        if regen == 0 {
            return;
        }
        (p.regeneration_ability, 0, regen, None)
    };
    apply_timed_armor_modifier(
        status,
        TimedArmorModifier {
            id: ModifierId(ability.0),
            armor_bonus_per_100: armor,
            regeneration_per_second_per_10k: regen,
            native_buff: buff,
            aura_membership: true,
            undispellable: true,
            expires_tick: tick + 1,
            ..Default::default()
        },
    );
}
impl Simulation {
    pub(super) fn refresh_native_building_auras_for_timers(&mut self) {
        // Aura matching only tests existence and sums saturating rates, so visit order is moot.
        let sources: Vec<_> = self
            .world
            .try_query_filtered::<
                (&ContentIdentity, &Team, &BuildingFootprint, &Health),
                bevy_ecs::query::Without<BuildingConstruction>,
            >()
            .map(|mut query| {
                query
                    .iter(&self.world)
                    .filter_map(|(content, team, footprint, health)| {
                        let p =
                            crate::building_mechanics::coral_auras_for_version(content.map_version)?;
                        (content.rawcode == p.source_rawcode).then_some(AuraSource {
                            version: p.map_version,
                            team: *team,
                            position: footprint_center_point(
                                *footprint,
                                self.config.navigation_cell_size,
                            ),
                            alive: health.current > 0,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let mut q = self.world.query::<(
            &Team,
            &Position,
            &Health,
            &MovementClass,
            Option<&UnitClassifications>,
            Option<&ContentIdentity>,
            &mut StatusState,
        )>();
        for (team, position, health, movement, flags, content, mut status) in
            q.iter_mut(&mut self.world)
        {
            let Some(p) = crate::building_mechanics::coral_auras_for_version(
                content.map_or(crate::CASTLE_FIGHT_DEFAULT_MAP_VERSION, |c| c.map_version),
            ) else {
                continue;
            };
            refresh(
                &mut status,
                Recipient {
                    team: *team,
                    position: position.0,
                    alive: health.current > 0 && !flags.is_some_and(|f| f.locust),
                    building: false,
                    movement: *movement,
                    invulnerable: flags.is_some_and(|f| f.invulnerable),
                },
                p,
                &sources,
                self.next_tick,
            );
        }
    }
    pub(super) fn refresh_native_building_auras(
        &self,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        let sources: Vec<_> = buildings
            .iter()
            .filter_map(|b| {
                let p = crate::building_mechanics::coral_auras_for_version(
                    b.map_version
                        .unwrap_or(crate::CASTLE_FIGHT_DEFAULT_MAP_VERSION),
                )?;
                (self.world.get::<ContentIdentity>(b.entity)?.rawcode == p.source_rawcode
                    && self.world.get::<BuildingConstruction>(b.entity).is_none())
                .then_some(AuraSource {
                    version: p.map_version,
                    team: b.team,
                    position: footprint_center_point(b.footprint, self.config.navigation_cell_size),
                    alive: b.health > 0,
                })
            })
            .collect();
        for u in units {
            let Some(p) = crate::building_mechanics::coral_auras_for_version(
                u.map_version
                    .unwrap_or(crate::CASTLE_FIGHT_DEFAULT_MAP_VERSION),
            ) else {
                continue;
            };
            let movement = u.target_movement_class();
            refresh(
                &mut u.status,
                Recipient {
                    team: u.team,
                    position: u.position,
                    alive: u.health > 0 && !u.classifications.locust,
                    building: false,
                    movement,
                    invulnerable: u.classifications.invulnerable,
                },
                p,
                &sources,
                self.next_tick,
            );
        }
        for b in buildings {
            let Some(p) = crate::building_mechanics::coral_auras_for_version(
                b.map_version
                    .unwrap_or(crate::CASTLE_FIGHT_DEFAULT_MAP_VERSION),
            ) else {
                continue;
            };
            let mut status = b.status.unwrap_or_default();
            refresh(
                &mut status,
                Recipient {
                    team: b.team,
                    position: footprint_center_point(b.footprint, self.config.navigation_cell_size),
                    alive: b.health > 0,
                    building: true,
                    movement: MovementClass::Ground,
                    invulnerable: b.classifications.invulnerable,
                },
                p,
                &sources,
                self.next_tick,
            );
            if b.status.is_some() || status.armor_modifier_count > 0 {
                b.status = Some(status);
            }
        }
    }
}
