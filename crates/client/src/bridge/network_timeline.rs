use std::collections::VecDeque;

use castle_fight_sim::CASTLE_FIGHT_SIMULATION_HZ;

use super::{PresentationSamples, PresentationSnapshot};

// Presentation policy, independent of versioned gameplay values. One additional
// interval absorbs arrival jitter; severe stalls rebase rather than growing forever.
const BUFFERED_TICKS: usize = 2;
const MAX_QUEUED_TICKS: usize = 64;

#[derive(Debug, Clone)]
pub(super) struct NetworkTimeline {
    queued: VecDeque<PresentationSnapshot>,
    boundary: Option<PresentationSnapshot>,
    rebase: Option<PresentationSnapshot>,
    alpha: f64,
    buffering: bool,
    buffered_ticks: f64,
}

impl Default for NetworkTimeline {
    fn default() -> Self {
        Self {
            queued: VecDeque::new(),
            boundary: None,
            rebase: None,
            alpha: 1.0,
            buffering: true,
            buffered_ticks: 0.0,
        }
    }
}

impl PresentationSamples {
    pub(crate) fn enable_network_timeline(&mut self) {
        self.network.get_or_insert_with(NetworkTimeline::default);
    }

    pub(crate) fn network_alpha(&self) -> Option<f32> {
        self.network.as_ref().map(|timeline| timeline.alpha as f32)
    }

    pub(crate) fn enqueue_network_tick(&mut self, mut snapshot: PresentationSnapshot) {
        if let Some(observer) = self.observer {
            snapshot.restrict_to_observer(observer);
        }
        self.enable_network_timeline();
        let timeline = self.network.as_mut().unwrap();
        if timeline.queued.len() == MAX_QUEUED_TICKS {
            // This is recovery from a stalled renderer, not normal interpolation.
            // Historical cosmetics must not all fire at the replacement position.
            snapshot.clear_events();
            timeline.queued.clear();
            timeline.boundary = None;
            timeline.rebase = Some(snapshot);
            bevy::log::warn!(
                "Network presentation backlog exceeded {MAX_QUEUED_TICKS} ticks; rebasing display"
            );
        } else {
            timeline.queued.push_back(snapshot);
        }
    }

    pub(crate) fn enqueue_network_boundary(&mut self, mut snapshot: PresentationSnapshot) {
        if let Some(observer) = self.observer {
            snapshot.restrict_to_observer(observer);
        }
        self.enable_network_timeline();
        let timeline = self.network.as_mut().unwrap();
        snapshot.clear_events();
        if let Some(tail) = timeline
            .queued
            .back_mut()
            .filter(|tail| tail.tick == snapshot.tick)
        {
            // The tick hasn't been displayed yet. Keep its original cosmetics,
            // replacing only persistent state with the subsequent boundary control.
            snapshot.prepend_events(tail);
            *tail = snapshot;
        } else if let Some(rebase) = timeline
            .rebase
            .as_mut()
            .filter(|state| state.tick == snapshot.tick)
        {
            *rebase = snapshot;
        } else {
            debug_assert_eq!(snapshot.tick, self.current.tick);
            timeline.boundary = Some(snapshot);
        }
    }

    pub(crate) fn freeze_network_timeline(&mut self) {
        if let Some(timeline) = &mut self.network {
            timeline.queued.clear();
            timeline.boundary = None;
            timeline.rebase = None;
            timeline.buffering = true;
            timeline.buffered_ticks = 0.0;
        }
    }

    /// Advances cosmetic time only. Callers deliberately mark the Bevy resource
    /// changed only when this publishes state, never for alpha/queue mutations.
    pub(crate) fn advance_network_timeline(
        &mut self,
        delta_seconds: f64,
        speed: f64,
        paused: bool,
        connected: bool,
    ) -> bool {
        let Some(mut timeline) = self.network.take() else {
            return false;
        };
        let revision = self.revision;
        if connected {
            if let Some(snapshot) = timeline.rebase.take() {
                self.reset(snapshot);
                timeline.alpha = 1.0;
                timeline.buffering = true;
                timeline.buffered_ticks = 0.0;
            }
            if let Some(snapshot) = timeline.boundary.take() {
                // Preserve the prior movement endpoint and current interpolation
                // progress. Boundary cosmetics were cleared when admitted.
                self.current = snapshot;
                self.tick_advanced = false;
                self.revision += 1;
            }
            // Leave ordinary one/two-tick arrival variation alone. Drain larger
            // bursts gradually so a render stall cannot leave permanent input lag.
            let recovery_rate = (1.0
                + timeline.queued.len().saturating_sub(BUFFERED_TICKS + 1) as f64 * 0.1)
                .min(2.0);
            let mut remaining =
                delta_seconds * f64::from(CASTLE_FIGHT_SIMULATION_HZ) * speed * recovery_rate;
            let mut published_tick = false;
            loop {
                if paused {
                    timeline.alpha = 1.0;
                } else {
                    let advance = remaining.min(1.0 - timeline.alpha);
                    timeline.alpha += advance;
                    remaining -= advance;
                    if timeline.alpha < 1.0 {
                        break;
                    }
                }
                if timeline.queued.is_empty() {
                    timeline.buffering = true;
                    timeline.buffered_ticks = 0.0;
                    break;
                }
                if timeline.buffering && timeline.queued.len() < BUFFERED_TICKS && !paused {
                    // A final match tick may have no successor. Release a lone
                    // confirmed sample after a bounded wait, without extrapolation.
                    timeline.buffered_ticks += remaining;
                    if timeline.buffered_ticks < BUFFERED_TICKS as f64 {
                        break;
                    }
                }
                timeline.buffering = false;
                timeline.buffered_ticks = 0.0;
                let mut next = timeline.queued.pop_front().unwrap();
                if published_tick {
                    next.prepend_events(&mut self.current);
                }
                self.publish(next);
                published_tick = true;
                timeline.alpha = 0.0;
                if !paused && remaining == 0.0 {
                    break;
                }
            }
        }
        self.network = Some(timeline);
        self.revision != revision
    }
}

#[cfg(test)]
mod tests {
    use castle_fight_sim::{AttackDelivery, AttackEvent, SimId, SimPoint, Simulation};

    use super::*;

    fn snapshot(tick: u64) -> PresentationSnapshot {
        let mut sample = PresentationSnapshot::capture(&Simulation::new(Default::default(), 1));
        sample.tick = tick;
        sample.attacks.push(AttackEvent {
            source: SimId(tick),
            target: SimId(100),
            source_position: SimPoint::new(tick as i32, 0),
            target_position: SimPoint::new(100, 0),
            delivery: AttackDelivery::Melee,
            missed: false,
            critical: false,
        });
        sample
    }

    fn samples() -> PresentationSamples {
        let mut initial = snapshot(0);
        initial.clear_events();
        let mut samples = PresentationSamples::new(initial);
        samples.enable_network_timeline();
        samples
    }

    fn render_tick(samples: &PresentationSamples) -> f64 {
        samples.previous.tick as f64
            + (samples.current.tick - samples.previous.tick) as f64
                * f64::from(samples.network_alpha().unwrap())
    }

    #[test]
    fn late_ticks_hold_and_rebuffer_without_reversing_display_time() {
        let mut samples = samples();
        samples.enqueue_network_tick(snapshot(1));
        assert!(!samples.advance_network_timeline(0.01, 1.0, false, true));
        assert_eq!(render_tick(&samples), 0.0);
        samples.enqueue_network_tick(snapshot(2));
        assert!(samples.advance_network_timeline(0.01, 1.0, false, true));
        let before = render_tick(&samples);
        let revision = samples.revision();
        assert!(!samples.advance_network_timeline(0.01, 1.0, false, true));
        assert!(render_tick(&samples) > before);
        assert_eq!(
            samples.revision(),
            revision,
            "alpha updates cannot replay events"
        );
        samples.advance_network_timeline(10.0, 1.0, false, true);
        assert_eq!(render_tick(&samples), 2.0);
        samples.advance_network_timeline(10.0, 1.0, false, true);
        assert_eq!(render_tick(&samples), 2.0);
        samples.enqueue_network_tick(snapshot(3));
        samples.advance_network_timeline(0.01, 1.0, false, true);
        assert_eq!(render_tick(&samples), 2.0);
        samples.enqueue_network_tick(snapshot(4));
        samples.advance_network_timeline(0.01, 1.0, false, true);
        assert!(render_tick(&samples) > 2.0);
        assert!(
            render_tick(&samples) < 3.0,
            "starvation time is not carried forward"
        );
    }

    #[test]
    fn burst_ticks_keep_every_crossed_event_in_order_once() {
        let mut samples = samples();
        for tick in 1..=8 {
            samples.enqueue_network_tick(snapshot(tick));
        }
        samples.advance_network_timeline(0.06, 1.0, false, true);
        assert_eq!(samples.current.tick, 3);
        assert_eq!(samples.previous.tick, 2);
        assert_eq!(
            samples
                .current
                .attacks
                .iter()
                .map(|event| event.source.0)
                .collect::<Vec<_>>(),
            [1, 2, 3]
        );
        let revision = samples.revision();
        samples.advance_network_timeline(0.001, 1.0, false, true);
        assert_eq!(samples.revision(), revision);
        samples.advance_network_timeline(0.02, 1.0, false, true);
        assert_eq!(
            samples
                .current
                .attacks
                .iter()
                .map(|event| event.source.0)
                .collect::<Vec<_>>(),
            [4]
        );
    }

    #[test]
    fn boundaries_preserve_interpolation_and_do_not_replay_tick_cosmetics() {
        let mut samples = samples();
        samples.enqueue_network_tick(snapshot(1));
        samples.enqueue_network_tick(snapshot(2));
        let mut boundary = snapshot(2);
        boundary
            .players
            .get_mut(&castle_fight_sim::PlayerId(0))
            .unwrap()
            .connection = castle_fight_sim::PlayerConnectionStatus::Disconnected;
        let expected_connection = boundary.players[&castle_fight_sim::PlayerId(0)].connection;
        samples.enqueue_network_boundary(boundary);
        samples.advance_network_timeline(0.04, 1.0, false, true);
        assert_eq!(samples.current.tick, 2);
        assert_eq!(samples.current.attacks.len(), 2);
        assert_eq!(
            samples.current.players[&castle_fight_sim::PlayerId(0)].connection,
            expected_connection
        );
        let time = render_tick(&samples);
        let previous_tick = samples.previous.tick;
        let mut boundary = snapshot(2);
        boundary
            .players
            .get_mut(&castle_fight_sim::PlayerId(0))
            .unwrap()
            .connection = expected_connection;
        samples.enqueue_network_boundary(boundary);
        assert!(samples.advance_network_timeline(0.0, 1.0, false, true));
        assert_eq!(render_tick(&samples), time);
        assert_eq!(samples.previous.tick, previous_tick);
        assert!(samples.current.attacks.is_empty());
        assert!(!samples.tick_advanced());
    }

    #[test]
    fn pause_steps_speed_changes_disconnect_and_reconnect_are_explicit() {
        let mut samples = samples();
        samples.enqueue_network_tick(snapshot(1));
        assert!(samples.advance_network_timeline(0.0, 1.0, true, true));
        assert_eq!(render_tick(&samples), 1.0);
        let revision = samples.revision();
        assert!(!samples.advance_network_timeline(1.0, 1.0, true, true));
        assert_eq!(samples.revision(), revision);
        samples.enqueue_network_tick(snapshot(2));
        samples.enqueue_network_tick(snapshot(3));
        samples.advance_network_timeline(0.005, 2.0, false, true);
        assert!((render_tick(&samples) - 1.3).abs() < 0.00001);
        samples.advance_network_timeline(0.005, 0.5, false, true);
        assert!((render_tick(&samples) - 1.375).abs() < 0.00001);
        samples.freeze_network_timeline();
        let frozen = render_tick(&samples);
        samples.advance_network_timeline(10.0, 1.0, false, false);
        assert_eq!(render_tick(&samples), frozen);
        samples.reset(snapshot(50));
        assert_eq!(render_tick(&samples), 50.0);
        assert!(samples.current.attacks.is_empty());
        samples.enqueue_network_tick(snapshot(51));
        samples.enqueue_network_tick(snapshot(52));
        samples.advance_network_timeline(0.01, 1.0, false, true);
        assert!(render_tick(&samples) > 50.0);
        assert_eq!(samples.current.attacks[0].source, SimId(51));
    }

    #[test]
    fn severe_backlog_is_bounded_and_rebases_without_historical_effects() {
        let mut samples = samples();
        for tick in 1..=MAX_QUEUED_TICKS as u64 + 1 {
            samples.enqueue_network_tick(snapshot(tick));
        }
        assert!(samples.network.as_ref().unwrap().queued.len() <= MAX_QUEUED_TICKS);
        samples.advance_network_timeline(0.01, 1.0, false, true);
        assert_eq!(samples.current.tick, MAX_QUEUED_TICKS as u64 + 1);
        assert_eq!(samples.previous.tick, samples.current.tick);
        assert!(samples.current.attacks.is_empty());
    }

    #[test]
    fn a_lone_final_tick_is_released_after_a_bounded_buffer_wait() {
        let mut samples = samples();
        samples.enqueue_network_tick(snapshot(1));
        assert!(!samples.advance_network_timeline(0.02, 1.0, false, true));
        assert!(!samples.advance_network_timeline(0.02, 1.0, false, true));
        samples.advance_network_timeline(0.04, 1.0, false, true);
        assert_eq!(render_tick(&samples), 1.0);
        assert_eq!(samples.current.attacks.len(), 1);
        let revision = samples.revision();
        samples.advance_network_timeline(1.0, 1.0, false, true);
        assert_eq!(samples.revision(), revision);
    }

    #[test]
    fn a_large_burst_recovers_while_new_ticks_continue_at_the_server_rate() {
        let mut samples = samples();
        for tick in 1..=20 {
            samples.enqueue_network_tick(snapshot(tick));
        }
        let mut last_display_time = 0.0;
        let mut seen = Vec::new();
        for tick in 21..=110 {
            samples.enqueue_network_tick(snapshot(tick));
            if samples.advance_network_timeline(
                1.0 / f64::from(CASTLE_FIGHT_SIMULATION_HZ),
                1.0,
                false,
                true,
            ) {
                seen.extend(samples.current.attacks.iter().map(|event| event.source.0));
            }
            let displayed = render_tick(&samples);
            assert!(displayed >= last_display_time);
            last_display_time = displayed;
        }
        assert!(
            110.0 - last_display_time < 4.0,
            "burst delay must drain rather than remain permanently"
        );
        assert_eq!(seen, (1..=samples.current.tick).collect::<Vec<_>>());
    }
}
