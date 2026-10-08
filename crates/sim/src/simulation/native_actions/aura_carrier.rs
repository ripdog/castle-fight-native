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
    (if for_timers && state.profile.regeneration_per_second_per_10k > 0 {
        tick > state.created_tick && tick <= state.expires_tick
    } else {
        tick >= state.created_tick && tick < state.expires_tick
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
    use crate::simulation::native_carriers::retain_modifiers;
    // A representative ability can outlive the carrier that first supplied that identity.
    // Clear the complete old native family, including its paired speed modifiers, before
    // selecting this tick's contributors. This also removes membership without extra linger.
    let mut removed_ids = [ModifierId(0); MAX_TIMED_ARMOR_MODIFIERS];
    let mut removed_count = 0;
    for modifier in &status.armor_modifiers[..usize::from(status.armor_modifier_count)] {
        if sources.iter().any(|source| {
            modifier.id.0 == source.profile.ability.0
                || modifier
                    .native_buff
                    .is_some_and(|buff| buff.rawcode == source.profile.buff_rawcode)
        }) {
            removed_ids[removed_count] = modifier.id;
            removed_count += 1;
        }
    }
    let keep = |id: ModifierId| {
        !removed_ids[..removed_count].contains(&id)
            && !sources
                .iter()
                .any(|source| source.profile.ability.0 == id.0)
    };
    retain_modifiers(
        &mut status.movement_modifiers,
        &mut status.movement_modifier_count,
        |modifier| keep(modifier.id),
    );
    retain_modifiers(
        &mut status.attack_speed_modifiers,
        &mut status.attack_speed_modifier_count,
        |modifier| keep(modifier.id),
    );
    retain_modifiers(
        &mut status.armor_modifiers,
        &mut status.armor_modifier_count,
        |modifier| keep(modifier.id),
    );

    for family in
        sources.chunk_by(|left, right| left.profile.buff_rawcode == right.profile.buff_rawcode)
    {
        let eligible = family
            .iter()
            .copied()
            .filter(|state| eligible(*state, recipient, tick, for_timers));
        let Some(representative) = eligible.clone().next() else {
            continue;
        };
        let regeneration = eligible
            .clone()
            .filter(|state| state.profile.regeneration_per_second_per_10k > 0)
            .max_by_key(|state| {
                (
                    state.profile.regeneration_per_second_per_10k,
                    state.expires_tick,
                )
            });
        let movement = eligible
            .clone()
            .filter(|state| state.profile.movement_percent_delta != 0)
            .max_by_key(|state| {
                (
                    state.profile.movement_percent_delta.unsigned_abs(),
                    state.profile.movement_percent_delta,
                    state.expires_tick,
                )
            });
        let attack_speed = eligible
            .clone()
            .filter(|state| state.profile.attack_speed_percent_delta != 0)
            .max_by_key(|state| {
                (
                    state.profile.attack_speed_percent_delta.unsigned_abs(),
                    state.profile.attack_speed_percent_delta,
                    state.expires_tick,
                )
            });
        let id = ModifierId(representative.profile.ability.0);
        let regeneration_expiry =
            regeneration.map_or(0, |state| state.expires_tick + u64::from(for_timers));
        let expires_tick = regeneration_expiry
            .max(movement.map_or(0, |state| state.expires_tick))
            .max(attack_speed.map_or(0, |state| state.expires_tick));
        if expires_tick == 0 {
            continue;
        }
        apply_timed_armor_modifier(
            status,
            TimedArmorModifier {
                id,
                native_buff: Some(crate::NativeBuffIdentity {
                    rawcode: representative.profile.buff_rawcode,
                    positive: true,
                    stealable: false,
                    organic_only: eligible.clone().all(|state| state.profile.organic_only),
                }),
                regeneration_per_second_per_10k: regeneration
                    .map_or(0, |state| state.profile.regeneration_per_second_per_10k),
                expires_tick,
                ..Default::default()
            },
        );
        if let Some(state) = movement {
            apply_timed_movement_modifier(
                status,
                id,
                state.profile.movement_percent_delta,
                state.expires_tick,
            );
        }
        if let Some(state) = attack_speed {
            apply_timed_attack_speed_modifier(
                status,
                id,
                state.profile.attack_speed_percent_delta,
                state.expires_tick,
            );
        }
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
                state.profile.buff_rawcode,
                state.profile.ability,
                state.source,
                state.expires_tick,
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

/// Presentation of the carrier's own native aura art; it does not expose a combat handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuraCarrierView {
    pub id: SimId,
    pub source: SimId,
    pub team: Team,
    pub position: SimPoint,
    pub ability: AbilityId,
    pub created_tick: u64,
    pub expires_tick: u64,
}

impl Simulation {
    #[must_use]
    pub fn aura_carriers(&self) -> Vec<AuraCarrierView> {
        let mut views: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                let NativeAction::AuraCarrier(state) = entity.get::<NativeAction>()? else {
                    return None;
                };
                (self.next_tick < state.expires_tick).then(|| AuraCarrierView {
                    id: *entity.get::<SimId>().expect("native action identity"),
                    source: state.source,
                    team: state.team,
                    position: state.position,
                    ability: state.profile.ability,
                    created_tick: state.created_tick,
                    expires_tick: state.expires_tick,
                })
            })
            .collect();
        views.sort_unstable_by_key(|view| view.id);
        views
    }
}
