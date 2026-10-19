//! Bounds fixed-step catch-up for offline matches.
//!
//! When a rendered frame is slow, Bevy runs several fixed simulation steps to catch up, and
//! each extra step makes the following frame slower. Offline matches cap that catch-up so a
//! machine that cannot keep up slows game time instead of spiraling. Networked matches keep
//! Bevy's default virtual-clock clamp.

use std::time::Duration;

use bevy::{prelude::*, time::Virtual};

/// Most fixed simulation steps one rendered frame may run in an offline match.
pub(crate) const OFFLINE_MAX_FIXED_STEPS_PER_FRAME: u32 = 2;

/// Clamps the virtual clock's per-frame delta to `OFFLINE_MAX_FIXED_STEPS_PER_FRAME` timesteps.
///
/// The fixed accumulator is always below one timestep between frames, so a delta of at most
/// two timesteps yields at most two fixed steps in any frame. Call again whenever the fixed
/// timestep changes.
pub(crate) fn limit_offline_catch_up(virtual_time: &mut Time<Virtual>, fixed_timestep: Duration) {
    virtual_time.set_max_delta(fixed_timestep * OFFLINE_MAX_FIXED_STEPS_PER_FRAME);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use bevy::{
        MinimalPlugins,
        ecs::resource::Resource,
        prelude::*,
        time::{Fixed, TimeUpdateStrategy, Virtual},
    };
    use castle_fight_sim::CASTLE_FIGHT_SIMULATION_HZ;

    use super::limit_offline_catch_up;

    #[derive(Resource, Default)]
    struct FixedRuns(u32);

    /// Fixed steps run in each of `frames` consecutive updates that each advance by `frame`.
    fn fixed_steps_per_frame(capped: bool, frame: Duration, frames: usize) -> Vec<u32> {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(Time::<Fixed>::from_hz(f64::from(
                CASTLE_FIGHT_SIMULATION_HZ,
            )))
            .insert_resource(TimeUpdateStrategy::ManualDuration(frame))
            .init_resource::<FixedRuns>()
            .add_systems(FixedUpdate, |mut runs: ResMut<FixedRuns>| runs.0 += 1);
        if capped {
            let timestep = app.world().resource::<Time<Fixed>>().timestep();
            limit_offline_catch_up(
                app.world_mut().resource_mut::<Time<Virtual>>().as_mut(),
                timestep,
            );
        }
        (0..frames)
            .map(|_| {
                let before = app.world().resource::<FixedRuns>().0;
                app.update();
                app.world().resource::<FixedRuns>().0 - before
            })
            .collect()
    }

    #[test]
    fn offline_catch_up_runs_at_most_two_fixed_steps_per_slow_frame() {
        let steps = fixed_steps_per_frame(true, Duration::from_millis(500), 6);
        assert!(
            steps.iter().all(|steps| *steps <= 2),
            "capped catch-up ran {steps:?}"
        );
    }

    #[test]
    fn default_virtual_clamp_allows_more_than_two_fixed_steps_per_slow_frame() {
        let steps = fixed_steps_per_frame(false, Duration::from_millis(500), 6);
        assert!(
            steps.iter().any(|steps| *steps > 2),
            "uncapped catch-up ran {steps:?}"
        );
    }
}
