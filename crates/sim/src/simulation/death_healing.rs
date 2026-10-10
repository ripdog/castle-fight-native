//! Death callbacks commit before the next damage event, using canonical building ownership.
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeathHealVisualEvent {
    pub target: SimId,
    pub position: SimPoint,
    pub map_version: crate::MapVersion,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum DeathHealContribution {
    Linker,
    Production(PlayerId),
}

#[derive(Debug, Clone, Copy)]
pub(super) struct DeathHealSupport {
    pub version: crate::MapVersion,
    pub active: bool,
    pub healing: i32,
    pub radius: i32,
    pub other_type_per_10k: u16,
}

pub(super) fn building_contribution(
    entity: &bevy_ecs::world::EntityRef<'_>,
) -> Option<DeathHealContribution> {
    let content = if let Some(construction) = entity.get::<BuildingConstruction>() {
        // Native construction registers once; upgrade start retains the old registration.
        construction.upgrade_from?.properties.content?
    } else {
        *entity.get::<ContentIdentity>()?
    };
    let definition = crate::building_mechanics::death_heal_for_version(content.map_version)?;
    if content.rawcode == definition.source_rawcode {
        Some(DeathHealContribution::Linker)
    } else if definition.production_rawcodes.contains(&content.rawcode) {
        Some(DeathHealContribution::Production(entity.get::<Owner>()?.0))
    } else {
        None
    }
}

/// Derived support is refreshed at phase boundaries and whenever a contributing building dies.
pub(super) fn refresh_support(
    units: &mut [UnitSnapshot],
    buildings: &[BuildingSnapshot],
    health: Option<&[i32]>,
) {
    if !units.iter().any(|u| u.death_heal.is_some()) {
        return;
    }
    let mut presence = [false; 2];
    let mut counts = BTreeMap::<PlayerId, i32>::new();
    for (i, b) in buildings.iter().enumerate() {
        if health.map_or(b.health, |h| h[i]) <= 0 {
            continue;
        }
        match b.death_heal_contribution {
            Some(DeathHealContribution::Linker) => presence[usize::from(b.team.0)] = true,
            Some(DeathHealContribution::Production(owner)) => {
                *counts.entry(owner).or_default() += 1
            }
            None => {}
        }
    }
    for u in units {
        if let Some(support) = &mut u.death_heal {
            let p = crate::building_mechanics::death_heal_for_version(support.version).unwrap();
            support.active = presence[usize::from(u.team.0)];
            support.healing = if presence[usize::from(u.team.0)] {
                counts
                    .get(&u.owner)
                    .copied()
                    .unwrap_or(0)
                    .saturating_mul(p.heal_per_building)
                    .min(p.maximum_heal)
            } else {
                0
            };
        }
    }
}

pub(super) fn observe_inline_death(units: &mut [UnitSnapshot], index: usize, before: i32) {
    if before > 0 && units[index].health <= 0 {
        heal(units, None, None, index);
    }
}

pub(super) fn commit_health(units: &mut [UnitSnapshot], index: usize, after: i32) {
    let before = units[index].health;
    units[index].health = after;
    if before > 0 && after <= 0 {
        heal(units, None, None, index);
    }
}

pub(super) fn commit_split_health(
    units: &mut [UnitSnapshot],
    health: &mut [i32],
    positions: Option<&[SimPoint]>,
    index: usize,
    after: i32,
) {
    let before = health[index];
    health[index] = after;
    if before > 0 && after <= 0 {
        heal(units, Some(health), positions, index);
    }
}

fn heal(
    units: &mut [UnitSnapshot],
    mut health: Option<&mut [i32]>,
    positions: Option<&[SimPoint]>,
    index: usize,
) {
    let dead = units[index];
    let Some(p) = dead.death_heal else {
        return;
    };
    if !p.active
        || !dead.classifications.combat_sapper
        || dead.classifications.legendary
        || dead.classifications.summoned_marker
        || dead.classifications.summoned
        || dead.classifications.illusion
    {
        return;
    }
    let origin = positions.map_or(dead.death_position, |v| v[index]);
    for (i, target) in units.iter_mut().enumerate() {
        let current = health.as_ref().map_or(target.health, |v| v[i]);
        let point = positions.map_or(target.death_position, |v| v[i]);
        if current <= 0
            || target.team != dead.team
            || !target.classifications.combat_sapper
            || origin.distance_sq(point) > square_i32(p.radius)
        {
            continue;
        }
        let amount = if target.rawcode == dead.rawcode {
            p.healing
        } else {
            i32::try_from(i64::from(p.healing) * i64::from(p.other_type_per_10k) / 10_000)
                .expect("native death healing is bounded")
        };
        let after = current.saturating_add(amount).min(target.health_max);
        if let Some(health) = &mut health {
            health[i] = after;
        } else {
            target.health = after;
        }
        // Even zero healing creates native recipient art. This field is presentation-only.
        target.death_heal_visuals = target.death_heal_visuals.saturating_add(1);
        target.death_heal_visual_version = Some(p.version);
    }
}

impl Simulation {
    pub(super) fn emit_death_heal_visuals(
        &mut self,
        units: &[UnitSnapshot],
        positions: &[SimPoint],
    ) {
        for (i, u) in units.iter().enumerate() {
            for _ in 0..u.death_heal_visuals {
                self.last_death_heals.push(DeathHealVisualEvent {
                    target: u.id,
                    position: positions[i],
                    map_version: u.death_heal_visual_version.unwrap(),
                });
            }
        }
    }
    pub(super) fn observe_world_death_heal(&mut self, dead: SimId) {
        let mut units = self.snapshot_units_with_dead(true);
        let Some(index) = find_unit_index(&units, dead) else {
            return;
        };
        if units[index].health > 0 {
            return;
        }
        let buildings = self.snapshot_buildings();
        refresh_support(&mut units, &buildings, None);
        heal(&mut units, None, None, index);
        let positions: Vec<_> = units.iter().map(|u| u.position).collect();
        self.emit_death_heal_visuals(&units, &positions);
        for u in units {
            self.world.get_mut::<Health>(u.entity).unwrap().current = u.health;
        }
    }
    #[must_use]
    pub fn death_heals_last_tick(&self) -> &[DeathHealVisualEvent] {
        &self.last_death_heals
    }
}

#[cfg(test)]
mod tests;
