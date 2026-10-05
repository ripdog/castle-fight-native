use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShrineRevivalEvent {
    pub unit: SimId,
    pub position: SimPoint,
    pub model_path: &'static str,
}

/// Phase-local projection of the actual supporting buildings, never a default-version lookup.
#[derive(Debug, Clone, Copy)]
pub(super) struct ShrineSupport {
    map_version: crate::MapVersion,
    chance: u32,
}

fn shrine_definition(version: crate::MapVersion) -> &'static crate::GoldenShrineDefinition {
    crate::golden_shrine_definition_for_version(version).expect("registered shrine system version")
}

fn is_golden_shrine(content: ContentIdentity) -> bool {
    crate::CastleFightTowerKind::from_rawcode_for_version(content.rawcode, content.map_version)
        .expect("registered shrine building version")
        == Some(crate::CastleFightTowerKind::GoldenShrineOfJustice)
}

impl Simulation {
    /// Recomputed from canonical building state: completed construction, removal, upgrades and
    /// ownership/team changes cannot leave stale script counters behind.
    #[must_use]
    pub fn golden_shrine_revive_chance(&self, team: Team) -> u32 {
        self.golden_shrine_support(team)
            .map_or(0, |support| support.chance)
    }

    pub(super) fn golden_shrine_support(&self, team: Team) -> Option<ShrineSupport> {
        let mut version = None;
        let mut count = 0_u32;
        for entity in self.world.iter_entities().filter(|entity| {
            entity.get::<BuildingFootprint>().is_some()
                && entity.get::<BuildingConstruction>().is_none()
                && entity
                    .get::<Health>()
                    .is_some_and(|health| health.current > 0)
                && entity
                    .get::<Owner>()
                    .and_then(|owner| self.player_state(owner.0))
                    .map(|player| player.team)
                    .or_else(|| entity.get::<Team>().copied())
                    == Some(team)
        }) {
            let Some(content) = entity
                .get::<ContentIdentity>()
                .copied()
                .filter(|content| is_golden_shrine(*content))
            else {
                continue;
            };
            if let Some(version) = version {
                assert_eq!(
                    version, content.map_version,
                    "supporting shrines must share a content version"
                );
            }
            version = Some(content.map_version);
            count = count.checked_add(1).expect("shrine count overflow");
        }
        version.map(|map_version| {
            let p = &shrine_definition(map_version).parameters;
            ShrineSupport {
                map_version,
                chance: count
                    .saturating_mul(p.chance_percent_per_shrine)
                    .min(p.maximum_effective_chance_percent),
            }
        })
    }

    /// Source owner-change event for this script-managed building. The shrine contribution follows
    /// the new owner's team immediately, including while construction is still in progress.
    pub fn transfer_golden_shrine_owner(&mut self, shrine: SimId, owner: PlayerId) -> bool {
        let Some(team) = self.player_state(owner).map(|player| player.team) else {
            return false;
        };
        let Some(entity) = self
            .world
            .iter_entities()
            .find(|entity| {
                entity.get::<SimId>() == Some(&shrine)
                    && entity.get::<BuildingFootprint>().is_some()
                    && entity
                        .get::<ContentIdentity>()
                        .is_some_and(|content| is_golden_shrine(*content))
            })
            .map(|entity| entity.id())
        else {
            return false;
        };
        let old_owner = self.world.get::<Owner>(entity).map(|owner| owner.0);
        if old_owner == Some(owner) {
            return true;
        }
        let points = self
            .world
            .get::<BuildingConstruction>(entity)
            .and_then(|construction| construction.properties.economy)
            .or_else(|| self.world.get::<BuildingEconomyProfile>(entity).copied())
            .map_or(0, |economy| economy.legendary_points_cost);
        if let Some(old_owner) = old_owner {
            let resources = &mut self
                .player_state_mut(old_owner)
                .expect("canonical building owner")
                .resources;
            resources.legendary_points_used = resources
                .legendary_points_used
                .checked_sub(points)
                .expect("allocated building legendary points");
        }
        let resources = &mut self
            .player_state_mut(owner)
            .expect("validated owner")
            .resources;
        resources.legendary_points_used = resources
            .legendary_points_used
            .checked_add(points)
            .expect("legendary allocation overflow");
        let mut building = self.world.entity_mut(entity);
        building.insert((Owner(owner), team));
        if let Some(mut construction) = building.get_mut::<BuildingConstruction>() {
            construction.building.team = team;
        }
        true
    }

    /// Scripted removals/kills suppress exactly one death attempt, not the resurrection baseline.
    pub fn suppress_next_golden_shrine_revival(&mut self, unit: SimId) -> bool {
        let Some(entity) = self
            .world
            .iter_entities()
            .find(|entity| {
                entity.get::<SimId>() == Some(&unit) && entity.get::<MovementProfile>().is_some()
            })
            .map(|entity| entity.id())
        else {
            return false;
        };
        let mut state = self
            .world
            .get::<ShrineRevivalState>(entity)
            .copied()
            .unwrap_or_default();
        state.suppress_next_death = true;
        self.world.entity_mut(entity).insert(state);
        true
    }

    /// The Lua A7 generation advances on the end-of-round signal, NOT on each unit death.
    /// Round controllers call this boundary hook; ordinary deaths never invalidate each other.
    pub fn invalidate_pending_golden_shrine_revivals(&mut self) {
        self.shrine_death_generation = self
            .shrine_death_generation
            .checked_add(1)
            .expect("shrine generation exhausted");
    }

    #[must_use]
    pub fn shrine_revivals_last_tick(&self) -> &[ShrineRevivalEvent] {
        &self.last_shrine_revivals
    }

    /// Called only for actual fatalities in sourced combat resolution. No-killer developer deaths
    /// deliberately bypass this: fJ's shrine branch is nested inside the non-nil killer branch.
    pub(super) fn schedule_shrine_revival(
        &mut self,
        entity: Entity,
        owner: PlayerId,
        team: Team,
        position: SimPoint,
        final_health: i32,
        support: Option<ShrineSupport>,
    ) -> ShrineRevivalState {
        let mut state = self
            .world
            .get::<ShrineRevivalState>(entity)
            .copied()
            .unwrap_or_default();
        let flags = self
            .world
            .get::<UnitClassifications>(entity)
            .copied()
            .unwrap_or_default();
        if final_health > 0
            || !flags.combat_sapper
            || flags.legendary
            || flags.summoned_marker
            || flags.summoned
            || flags.illusion
        {
            return state;
        }
        let Some(support) = support.filter(|support| support.chance > 0) else {
            return state;
        };
        let definition = self
            .world
            .get::<ResurrectionProfile>(entity)
            .expect("cold combat unit baseline")
            .0;
        for content in [
            self.world.get::<ContentIdentity>(entity).copied(),
            definition.properties.content,
        ]
        .into_iter()
        .flatten()
        {
            assert_eq!(
                content.map_version, support.map_version,
                "revival source, live unit and original definition must share a content version"
            );
        }
        // eJ consumes the one-shot flag only inside the eligible team's shrine branch.
        let suppressed = state.suppress_next_death;
        state.suppress_next_death = false;
        if suppressed || state.revived {
            return state;
        }
        let source_unit = state.death_identity.unwrap_or_else(|| {
            *self
                .world
                .get::<SimId>(entity)
                .expect("dying combat unit identity")
        });
        let p = &shrine_definition(support.map_version).parameters;
        let roll = p.chance_roll_min as u64
            + deterministic_random(
                self.config.match_seed,
                self.next_tick,
                source_unit,
                0x474f_4c44_5245_5649,
                0,
            ) % u64::from(p.chance_roll_max - p.chance_roll_min + 1);
        if roll >= u64::from(support.chance) {
            return state;
        }
        let pending = DelayedShrineRevival {
            map_version: support.map_version,
            source_unit,
            owner,
            team,
            position,
            definition,
            due_tick: self
                .next_tick
                .checked_add(p.revive_delay_seconds * CASTLE_FIGHT_SIMULATION_HZ as u64)
                .expect("revival tick overflow"),
            death_generation: self.shrine_death_generation,
        };
        let id = self.allocate_id();
        self.world.spawn((id, pending));
        state
    }

    pub(super) fn resolve_shrine_revivals(&mut self) {
        let mut due: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|entity| {
                let pending = *entity.get::<DelayedShrineRevival>()?;
                (pending.due_tick <= self.next_tick).then_some((
                    *entity.get::<SimId>()?,
                    entity.id(),
                    pending,
                ))
            })
            .collect();
        due.sort_unstable_by_key(|(id, ..)| *id);
        for (_, entity, pending) in due {
            self.world.despawn(entity);
            if pending.death_generation != self.shrine_death_generation {
                continue;
            }
            // Callback is unconditional on corpse presence/raise eligibility. Native resurrection
            // may have restored the same original handle before this callback removes it.
            let mut originals: Vec<_> = self
                .world
                .iter_entities()
                .filter(|entity| {
                    entity.get::<Corpse>().is_some_and(|corpse| {
                        corpse.source_unit == pending.source_unit
                            || corpse.shrine_state.death_identity == Some(pending.source_unit)
                    }) || entity
                        .get::<ShrineRevivalState>()
                        .is_some_and(|state| state.death_identity == Some(pending.source_unit))
                })
                .map(|entity| {
                    (
                        *entity.get::<SimId>().expect("authoritative original"),
                        entity.id(),
                    )
                })
                .collect();
            originals.sort_unstable_by_key(|(id, _)| *id);
            for (_, original) in originals {
                self.world.despawn(original);
            }
            let unit = self.spawn_resolved_unit_unchecked(
                Some(pending.owner),
                UnitSpawn::from_template(
                    pending.team,
                    pending.position,
                    pending.definition.template,
                ),
                pending.definition,
            );
            let entity = self
                .world
                .iter_entities()
                .find(|entity| entity.get::<SimId>() == Some(&unit))
                .expect("fresh replacement")
                .id();
            self.world.entity_mut(entity).insert(ShrineRevivalState {
                revived: true,
                ..ShrineRevivalState::default()
            });
            self.last_shrine_revivals.push(ShrineRevivalEvent {
                unit,
                position: pending.position,
                model_path: &shrine_definition(pending.map_version).resurrection_model,
            });
        }
    }
}

#[cfg(test)]
mod tests;
