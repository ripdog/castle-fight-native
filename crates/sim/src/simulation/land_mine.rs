//! Mine Layer: a Parasite-released script plants a hidden land mine around the caster. The mine is
//! an independent authoritative action: once armed, an enemy ground unit in range detonates it and
//! its native death explosion follows after the retained post-death delay.
use super::*;
use crate::components::{
    AbilityEffect, DeathExplosionState, LandMineProfile, LandMineState, NativeAction,
    ParasiteChild, SpellcastingProfile,
};

const RANDOM_PURPOSE_LAND_MINE: u64 = 0x4c41_4e44_4d49_4e01;
const RANDOM_PURPOSE_TRAINED_MANA: u64 = 0x4c41_4e44_4d49_4e02;
/// Rejection sampling of a direction: candidates live in a square of this half-extent.
const DIRECTION_EXTENT: i64 = 1 << 20;
const DIRECTION_ATTEMPTS: u64 = 32;

impl Simulation {
    /// `mineLayerSpell`: a uniformly random angle and distance from the caster, then the mine
    /// appears after the carrier's travel time. Nothing is retained about the carrier itself.
    pub(super) fn plant_land_mine(
        &mut self,
        source: AbilitySourceSnapshot,
        owner: PlayerId,
        origin: SimPoint,
        profile: LandMineProfile,
    ) {
        let sequence = source.ability_state.map_or(0, |state| state.cast_sequence);
        let span = u64::try_from(profile.maximum_distance - profile.minimum_distance + 1)
            .expect("land mine distance range");
        let distance = i64::from(profile.minimum_distance)
            + i64::try_from(
                deterministic_random(
                    self.config.match_seed,
                    self.next_tick,
                    source.id,
                    RANDOM_PURPOSE_LAND_MINE,
                    sequence << 8,
                ) % span,
            )
            .expect("land mine distance");
        let (dx, dy) = self.land_mine_direction(source.id, sequence, distance);
        let position = SimPoint::new(
            origin.x + i32::try_from(dx).expect("land mine offset"),
            origin.y + i32::try_from(dy).expect("land mine offset"),
        );
        // `doAfter(distance / carrier speed)`: travel time, rounded up to whole ticks.
        let speed = i64::from(profile.carrier_speed_per_second);
        let travel = u64::try_from(
            (distance * i64::from(CASTLE_FIGHT_SIMULATION_HZ) + speed - 1) / speed,
        )
        .expect("land mine travel ticks");
        let placed_tick = self.next_tick + travel;
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::LandMine(LandMineState {
                profile,
                source: source.id,
                owner,
                team: source.team,
                position,
                placed_tick,
                armed_tick: placed_tick + u64::from(profile.activation_ticks),
            }),
        ));
    }

    /// A direction uniform in angle without trigonometry: sample the square until a point falls
    /// inside the inscribed circle, then scale it to the requested length.
    fn land_mine_direction(&self, source: SimId, sequence: u64, distance: i64) -> (i64, i64) {
        for attempt in 0..DIRECTION_ATTEMPTS {
            let axis = |salt: u64| {
                let roll = deterministic_random(
                    self.config.match_seed,
                    self.next_tick,
                    source,
                    RANDOM_PURPOSE_LAND_MINE,
                    sequence << 8 | (attempt * 2 + salt + 1),
                ) % u64::try_from(2 * DIRECTION_EXTENT + 1).expect("direction extent");
                i64::try_from(roll).expect("direction roll") - DIRECTION_EXTENT
            };
            let (x, y) = (axis(0), axis(1));
            let length_sq = x * x + y * y;
            if length_sq != 0 && length_sq <= DIRECTION_EXTENT * DIRECTION_EXTENT {
                let length = length_sq.isqrt();
                return (x * distance / length, y * distance / length);
            }
        }
        (distance, 0)
    }

    pub(super) fn advance_land_mine(
        &mut self,
        entity: Entity,
        state: LandMineState,
        units: &[UnitSnapshot],
    ) {
        let tick = self.next_tick;
        let profile = state.profile;
        // The mine does not exist until the carrier arrives, and ignores units until armed.
        if tick < state.armed_tick {
            return;
        }
        let radius_sq = square_i32(profile.detection_radius);
        let triggered = units.iter().any(|unit| {
            unit.health > 0
                && unit.team != state.team
                && !unit.classifications.invulnerable
                && profile
                    .trigger_targets
                    .can_target_unit(unit.target_movement_class())
                && state.position.distance_sq(unit.position) <= radius_sq
        });
        if !triggered {
            return;
        }
        self.world.despawn(entity);
        // The mine dies and its native death explosion lands after the retained delay.
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::DeathExplosion(DeathExplosionState {
                source: state.source,
                owner: state.owner,
                team: state.team,
                profile: profile.explosion,
                position: state.position,
                due_tick: tick + u64::from(profile.explosion.delay_ticks),
            }),
        ));
    }

    /// `onUnitTrained`: a freshly trained Mine Layer's mana is uniform in the retained range.
    pub(super) fn trained_land_mine_mana(
        &self,
        spellcasting: &SpellcastingProfile,
        id: SimId,
    ) -> Option<ManaState> {
        let AbilityEffect::ParasiteScript(script) = spellcasting.ability.effect else {
            return None;
        };
        let ParasiteChild::LandMine(profile) = script.child else {
            return None;
        };
        let (minimum, maximum) = (
            u64::from(profile.initial_mana_minimum_10k),
            u64::from(profile.initial_mana_maximum_10k),
        );
        let roll = deterministic_random(
            self.config.match_seed,
            self.next_tick,
            id,
            RANDOM_PURPOSE_TRAINED_MANA,
            0,
        );
        let mana = (minimum + roll % (maximum - minimum + 1))
            .min(u64::try_from(spellcasting.mana.maximum).expect("mana maximum") * 10_000);
        Some(ManaState {
            current: i32::try_from(mana / 10_000).expect("trained mana"),
            regen_remainder_per_10k: u16::try_from(mana % 10_000).expect("trained mana remainder"),
        })
    }
}
