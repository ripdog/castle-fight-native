//! Native summon-body splitting; script converts each child into a separately resolved unit.
use super::*;
use crate::components::{LavaSpawnProfile, LavaSpawnState};

pub(super) fn initial_state(effects: PassiveUnitEffects) -> Option<LavaSpawnState> {
    effects.iter().find_map(|effect| match effect {
        PassiveUnitEffect::LavaSpawn(profile) => Some(LavaSpawnState {
            profile,
            landed_attacks: 0,
            splits: 0,
            split_tick: None,
        }),
        _ => None,
    })
}

pub(super) fn observe_landed_attack(
    source: SimId,
    target: TargetIndex,
    tick: u64,
    units: &mut [UnitSnapshot],
    buildings: &[BuildingSnapshot],
) {
    let Some(index) = units.binary_search_by_key(&source, |unit| unit.id).ok() else {
        return;
    };
    let team = units[index].team;
    if units[index].classifications.illusion {
        return;
    }
    let eligible = match target {
        TargetIndex::Unit(victim) => {
            units[victim].team != team
                && units[index].status.lava_spawn.is_some_and(|state| {
                    state
                        .profile
                        .targets
                        .can_target_unit(units[victim].target_movement_class())
                })
        }
        TargetIndex::Building(victim) => {
            buildings[victim].team != team
                && units[index]
                    .status
                    .lava_spawn
                    .is_some_and(|state| state.profile.targets.can_target_buildings())
        }
    };
    if !eligible {
        return;
    }
    let Some(state) = units[index].status.lava_spawn.as_mut() else {
        return;
    };
    if state.splits >= state.profile.maximum_splits || state.split_tick.is_some() {
        return;
    }
    state.landed_attacks = state.landed_attacks.saturating_add(1);
    if state.landed_attacks >= state.profile.attacks_to_split {
        state.split_tick = Some(tick + u64::from(state.profile.split_delay_ticks));
    }
}

impl Simulation {
    pub(super) fn advance_lava_spawns(&mut self) -> usize {
        let tick = self.next_tick;
        let mut refresh = Vec::new();
        let mut splits = Vec::new();
        for entity in self.world.iter_entities() {
            let Some(status) = entity.get::<StatusState>() else {
                continue;
            };
            if status
                .summon_order_refresh_tick
                .is_some_and(|due| tick >= due)
            {
                refresh.push(entity.id());
            }
            if let Some(state) = status.lava_spawn
                && state.split_tick.is_some_and(|due| tick >= due)
                && entity
                    .get::<Health>()
                    .is_some_and(|health| health.current > 0)
            {
                splits.push((*entity.get::<SimId>().expect("unit ID"), entity.id(), state));
            }
        }
        for entity in refresh {
            // Reissue the strategic attack order without suspending the initial native order.
            self.world
                .entity_mut(entity)
                .insert((NavigationState::default(), TargetState::default()));
            self.world
                .get_mut::<StatusState>(entity)
                .unwrap()
                .summon_order_refresh_tick = None;
        }
        splits.sort_unstable_by_key(|(id, _, _)| *id);
        let mut spawned = 0;
        for (_, entity, mut state) in splits {
            state.splits += 1;
            state.landed_attacks = 0;
            state.split_tick = None;
            let profile: LavaSpawnProfile = state.profile;
            let source = self.world.entity(entity);
            let owner = source.get::<Owner>().expect("unit owner").0;
            let team = *source.get::<Team>().unwrap();
            let position = source.get::<Position>().unwrap().0;
            let health = *source.get::<Health>().unwrap();
            let provenance = source.get::<StatusState>().unwrap().spawn_building;
            let healed = health
                .current
                .saturating_add(
                    i32::try_from(
                        i64::from(health.max) * i64::from(profile.healing_per_10k) / 10_000,
                    )
                    .unwrap(),
                )
                .min(health.max);
            self.world.get_mut::<Health>(entity).unwrap().current = healed;
            self.world
                .get_mut::<StatusState>(entity)
                .unwrap()
                .lava_spawn = Some(state);
            let content = *self
                .world
                .get::<ContentIdentity>(entity)
                .expect("native split source identity");
            let kind = crate::CastleFightUnitKind::from_rawcode_for_version(
                profile.child_rawcode,
                content.map_version,
            )
            .expect("split content version")
            .expect("promoted split child");
            let definition = kind
                .definition_for_version(content.map_version)
                .unwrap()
                .resolved();
            // Blizzard's replacement method 2 selects the new unit's default life/mana,
            // independently of both the damaged parent and the temporary native child.
            let id = self.spawn_resolved_unit_unchecked(
                Some(owner),
                UnitSpawn::from_template(team, position, definition.template),
                definition,
            );
            let child = self
                .world
                .iter_entities()
                .find(|entity| entity.get::<SimId>() == Some(&id))
                .unwrap()
                .id();
            let mut status = self.world.get_mut::<StatusState>(child).unwrap();
            status.spawn_building = provenance;
            status.summon_order_refresh_tick = Some(tick + u64::from(profile.order_refresh_ticks));
            self.last_ability_casts.push(AbilityCastEvent {
                source: id,
                ability: profile.ability,
                target: AbilityCastTarget::Unit(id),
                target_position: Some(position),
                effect: AbilityEffect::Damage { amount: 0 },
            });
            spawned += 1;
        }
        spawned
    }
}
