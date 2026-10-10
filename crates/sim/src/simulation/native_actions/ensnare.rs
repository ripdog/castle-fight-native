use super::*;
use crate::components::{EnsnareMissileState, EnsnareProfile, EnsnareState};

impl Simulation {
    pub(in crate::simulation) fn cast_ensnare(
        &mut self,
        source: AbilitySourceSnapshot,
        selected: usize,
        profile: EnsnareProfile,
        radius: i32,
        maximum: u8,
        units: &[UnitSnapshot],
    ) -> usize {
        let AbilitySourceOrigin::Unit(origin) = source.origin else {
            unreachable!("unit net proxy needs a unit origin")
        };
        let center = units[selected].position;
        let mut targets = units
            .iter()
            .filter(|target| {
                if radius == 0 {
                    target.id == units[selected].id
                } else {
                    target.health > 0
                        && target.classifications.combat_sapper
                        && target.team != source.team
                        && target.target_movement_class() == MovementClass::Air
                        && center.distance_sq(target.position) <= square_i32(radius)
                }
            })
            .collect::<Vec<_>>();
        targets.sort_unstable_by_key(|target| target.id);
        let mut launched = 0;
        // Script counts attempted dummy casts, including native range/target failures.
        for target in targets.into_iter().take(usize::from(maximum)) {
            if target.health <= 0
                || target.team == source.team
                || target.classifications.invulnerable
                || target.target_movement_class() != MovementClass::Air
                || origin.distance_sq(target.position) > square_i32(profile.range)
            {
                continue;
            }
            let id = self.allocate_id();
            self.world.spawn((
                id,
                NativeAction::Ensnare(EnsnareMissileState {
                    source: source.id,
                    team: source.team,
                    target: target.id,
                    profile,
                    launch_position: origin,
                    launch_tick: self.next_tick,
                    position: origin,
                    position_tick: self.next_tick,
                    impact_tick: self.next_tick
                        + projectile_travel_ticks(
                            origin.distance_sq(target.position),
                            profile.speed_per_tick,
                        ),
                }),
            ));
            launched += 1;
        }
        launched
    }

    pub(super) fn advance_ensnare(
        &mut self,
        entity: Entity,
        mut state: EnsnareMissileState,
        units: &mut [UnitSnapshot],
    ) {
        let Some(index) = find_unit_index(units, state.target).filter(|&i| units[i].health > 0)
        else {
            self.world.despawn(entity);
            return;
        };
        let target = &mut units[index];
        let distance = state.position.distance_sq(target.position);
        state.position = state
            .position
            .step_towards(target.position, state.profile.speed_per_tick);
        state.position_tick = self.next_tick;
        if distance > square_i32(state.profile.speed_per_tick) {
            state.impact_tick = self.next_tick
                + projectile_travel_ticks(
                    state.position.distance_sq(target.position),
                    state.profile.speed_per_tick,
                );
            self.world
                .entity_mut(entity)
                .insert(NativeAction::Ensnare(state));
            return;
        }
        self.world.despawn(entity);
        // The committed native missile survives its proxy/caster. Ensnare is physical
        // and permits magic immunity; native invulnerability still prevents impact.
        if target.team == state.team || target.classifications.invulnerable {
            return;
        }
        let duration = if target.classifications.uses_hero_spell_rules() {
            state.profile.hero_duration_ticks
        } else {
            state.profile.duration_ticks
        };
        target.status.ensnare = Some(EnsnareState {
            profile: state.profile,
            applied_tick: self.next_tick,
            expires_tick: self.next_tick + u64::from(duration),
        });
        target.attack = ensnared_attack(target.attack, target.movement_class, target.status);
        self.last_ability_casts.push(AbilityCastEvent {
            source: state.source,
            ability: state.profile.ability,
            target: AbilityCastTarget::Unit(state.target),
            target_position: Some(target.position),
            effect: AbilityEffect::Ensnare {
                profile: state.profile,
                search_radius: 0,
                maximum_targets: 1,
            },
        });
    }
}

pub(in crate::simulation) fn ensnared_attack(
    mut attack: AttackProfile,
    movement_class: MovementClass,
    status: StatusState,
) -> AttackProfile {
    if movement_class == MovementClass::Air
        && matches!(attack.delivery, AttackDelivery::Melee)
        && let Some(net) = status.ensnare
    {
        attack.range = net.profile.melee_range;
    }
    attack
}
