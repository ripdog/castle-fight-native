//! Forgotten One: the scripted tentacle ring behind the native Parasite building trigger.
use super::*;
use crate::building_mechanics::ForgottenOneProfile;
use crate::components::NativeAction;

/// Offset of ring member `index` around the centre for the authored 60-degree step.
fn ring_offset(index: u8, step_degrees: u16, radius: i32) -> SimPoint {
    assert_eq!(
        step_degrees, 60,
        "ring geometry is derived for a 60-degree step"
    );
    let half = radius / 2;
    let height = ((3 * i64::from(radius) * i64::from(radius)) / 4).isqrt() as i32;
    match index % 6 {
        0 => SimPoint::new(radius, 0),
        1 => SimPoint::new(half, height),
        2 => SimPoint::new(-half, height),
        3 => SimPoint::new(-radius, 0),
        4 => SimPoint::new(-half, -height),
        _ => SimPoint::new(half, -height),
    }
}

impl Simulation {
    pub(super) fn cast_forgotten_one(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AbilityId,
        trigger: SimId,
        p: ForgottenOneProfile,
        sequence: u64,
        units: &[UnitSnapshot],
    ) -> bool {
        let owner = self
            .world
            .iter_entities()
            .find(|e| e.get::<SimId>() == Some(&source.id))
            .and_then(|e| e.get::<Owner>())
            .map_or(PlayerId(source.team.0), |o| o.0);
        self.launch_native_parasite(source, owner, trigger, p.parent);
        let bounds =
            crate::building_mechanics::snowveil_for_version(p.map_version).battlefield_bounds;
        let Some(center) = units
            .iter()
            .filter(|u| {
                u.health > 0
                    && u.team != source.team
                    && u.classifications.combat_sapper
                    && u.target_movement_class() == MovementClass::Ground
                    && !p
                        .selector_exclusions
                        .into_iter()
                        .any(|a| self.native_ability_installed(u, a))
                    && u.position.x >= bounds[0]
                    && u.position.y >= bounds[1]
                    && u.position.x <= bounds[2]
                    && u.position.y <= bounds[3]
            })
            .min_by_key(|u| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        ability,
                        sequence,
                        u.id,
                    ),
                    u.id,
                )
            })
            .map(|u| u.position)
        else {
            return false;
        };
        let rawcode = if self.config.match_rules.artillery_enabled {
            p.artillery_body
        } else {
            p.plain_body
        };
        let definition =
            crate::CastleFightUnitKind::from_rawcode_for_version(rawcode, p.map_version)
                .expect("supported tentacle version")
                .expect("complete tentacle body")
                .definition_for_version(p.map_version)
                .unwrap()
                .resolved();
        for index in 0..p.count {
            let offset = ring_offset(index, p.angle_step_degrees, p.ring_radius);
            let child = self.spawn_resolved_unit_unchecked(
                Some(owner),
                UnitSpawn::from_template(
                    source.team,
                    SimPoint::new(center.x + offset.x, center.y + offset.y),
                    definition.template,
                ),
                definition,
            );
            let entity = self
                .world
                .iter_entities()
                .find(|e| e.get::<SimId>() == Some(&child))
                .unwrap()
                .id();
            // The source faces each body at 360 degrees minus its ring angle.
            self.world
                .get_mut::<StatusState>(entity)
                .unwrap()
                .facing_direction = SimPoint::new(offset.x, -offset.y);
            let id = self.allocate_id();
            self.world.spawn((
                id,
                NativeAction::TimedLife {
                    child,
                    expires_tick: self.next_tick + u64::from(p.lifetime_ticks),
                },
            ));
        }
        true
    }
}

#[cfg(test)]
mod tests;
