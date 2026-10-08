use super::*;
use crate::{
    ActionAnimationState, building_mechanics::BuildingAuraProfile, components::AuraCarrierState,
};

#[derive(Clone, Copy)]
struct Recipient {
    team: Team,
    position: SimPoint,
    health: i32,
    movement: MovementClass,
    mechanical: bool,
    invulnerable: bool,
}

// Regeneration accounts for the interval that closes at this timer tick. Live membership
// remains exclusive at removal, so this does not leave a trailing recipient buff.
fn eligible(state: AuraCarrierState, recipient: Recipient, tick: u64, for_timers: bool) -> bool {
    (if for_timers {
        tick > state.created_tick && tick <= state.expires_tick
    } else {
        tick < state.expires_tick
    }) && recipient.health > 0
        && recipient.team == state.team
        && (!state.profile.organic_only || !recipient.mechanical)
        && (state.profile.affects_invulnerable || !recipient.invulnerable)
        && state.profile.targets.can_target_unit(recipient.movement)
        && state.position.distance_sq(recipient.position) <= square_i32(state.profile.radius)
}

fn refresh_status(
    status: &mut StatusState,
    recipient: Recipient,
    sources: &[AuraCarrierState],
    tick: u64,
    for_timers: bool,
) {
    // Zero native duration means membership is resampled, without a trailing heal after exit.
    // Keep the carrier deadline on the live modifier so inspection reports its real lifetime.
    let old_count = usize::from(status.armor_modifier_count);
    let mut kept = 0;
    for index in 0..old_count {
        let active = status.armor_modifiers[index];
        if sources
            .iter()
            .any(|source| active.id.0 == source.profile.ability.0)
        {
            continue;
        }
        status.armor_modifiers[kept] = active;
        kept += 1;
    }
    status.armor_modifiers[kept..old_count].fill(TimedArmorModifier::default());
    status.armor_modifier_count = u8::try_from(kept).expect("bounded modifier count");
    // Ascending strength ensures one strongest payload per native buff family. Distinct native
    // buff identities remain independent. Equal strength prefers the later expiry,
    // then stable source identities.
    for &state in sources {
        if !eligible(state, recipient, tick, for_timers) {
            continue;
        }
        apply_timed_armor_modifier(
            status,
            TimedArmorModifier {
                id: ModifierId(state.profile.ability.0),
                native_buff: Some(crate::NativeBuffIdentity {
                    rawcode: state.profile.buff_rawcode,
                    positive: true,
                    stealable: false,
                    organic_only: state.profile.organic_only,
                }),
                regeneration_per_second_per_10k: state.profile.regeneration_per_second_per_10k,
                expires_tick: state.expires_tick + u64::from(for_timers),
                ..Default::default()
            },
        );
    }
}

impl Simulation {
    fn carrier_aura_sources(&self) -> Vec<AuraCarrierState> {
        let mut sources: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                let NativeAction::AuraCarrier(state) = entity.get::<NativeAction>()? else {
                    return None;
                };
                Some(*state)
            })
            .collect();
        sources.sort_unstable_by_key(|state| {
            (
                state.profile.regeneration_per_second_per_10k,
                state.expires_tick,
                state.profile.ability,
                state.source,
            )
        });
        sources
    }

    pub(in crate::simulation) fn refresh_carrier_auras_for_timers(&mut self) {
        let sources = self.carrier_aura_sources();
        if sources.is_empty() {
            return;
        }
        let mut targets = self.world.query_filtered::<(
            &Team,
            &Position,
            &Health,
            &MovementClass,
            Option<&MechanicalUnit>,
            Option<&UnitClassifications>,
            &mut StatusState,
        ), bevy_ecs::prelude::With<ResurrectionProfile>>();
        for (team, position, health, movement, mechanical, classifications, mut status) in
            targets.iter_mut(&mut self.world)
        {
            refresh_status(
                &mut status,
                Recipient {
                    team: *team,
                    position: position.0,
                    health: health.current,
                    movement: *movement,
                    mechanical: mechanical.is_some(),
                    invulnerable: classifications.is_some_and(|flags| flags.invulnerable),
                },
                &sources,
                self.next_tick,
                true,
            );
        }
    }

    pub(super) fn refresh_carrier_auras(&self, units: &mut [UnitSnapshot]) {
        let sources = self.carrier_aura_sources();
        if sources.is_empty() {
            return;
        }
        for unit in units {
            let recipient = Recipient {
                team: unit.team,
                position: unit.position,
                health: unit.health,
                movement: unit.target_movement_class(),
                mechanical: unit.mechanical,
                invulnerable: unit.classifications.invulnerable,
            };
            refresh_status(&mut unit.status, recipient, &sources, self.next_tick, false);
        }
    }

    pub(in crate::simulation) fn start_aura_carrier(
        &mut self,
        source: AbilitySourceSnapshot,
        profile: BuildingAuraProfile,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        let position = match source.origin {
            AbilitySourceOrigin::Unit(point) => point,
            AbilitySourceOrigin::Building(footprint) => {
                footprint_center_point(footprint, self.config.navigation_cell_size)
            }
        };
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::AuraCarrier(AuraCarrierState {
                source: source.id,
                team: source.team,
                position,
                profile,
                created_tick: self.next_tick,
                expires_tick: self.next_tick + u64::from(profile.lifetime_ticks),
            }),
        ));
        self.refresh_carrier_auras(units);
        if let Some(building) = buildings
            .iter_mut()
            .find(|building| building.id == source.id)
            && let Some(status) = building.status.as_mut()
        {
            status.action_animation = Some(ActionAnimationState {
                kind: ActionAnimationKind::Cast,
                started_tick: self.next_tick,
                until_tick: self.next_tick + u64::from(profile.stand_ticks),
            });
        }
        self.last_ability_casts.push(AbilityCastEvent {
            source: source.id,
            ability: profile.ability,
            target: AbilityCastTarget::Point(position),
            target_position: Some(position),
            effect: AbilityEffect::BuildingAura(profile),
        });
    }
}
