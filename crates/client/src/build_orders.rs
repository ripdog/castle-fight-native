//! Responsive construction feedback. This projects submitted commands onto the latest
//! authoritative boundary without changing simulation state or canonical command ordering.
use std::collections::BTreeMap;

use castle_fight_sim::{
    BuildingEconomyProfile, BuildingFootprint, CastleFightContentBundle, CommandSubmission,
    ContentIdentity, PlayerCommand, PlayerId, PlayerResources, SimId, Simulation,
};

use crate::ClientCommandSubmission;

#[derive(Default)]
pub(crate) struct PendingBuildCommands(Vec<PendingBuildCommand>);

struct PendingBuildCommand {
    player: PlayerId,
    sequence: u64,
    command: PlayerCommand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BuildSitePreview {
    pub(crate) owner: PlayerId,
    pub(crate) content: ContentIdentity,
    pub(crate) footprint: BuildingFootprint,
    pub(crate) economy: BuildingEconomyProfile,
}

#[derive(Default)]
pub(crate) struct BuildOrderProjection {
    pub(crate) orders: BTreeMap<SimId, BuildSitePreview>,
    pub(crate) resources: BTreeMap<PlayerId, PlayerResources>,
}

fn command_builder(command: PlayerCommand) -> Option<SimId> {
    match command {
        PlayerCommand::PlaceBuilding { builder, .. }
        | PlayerCommand::MoveBuilder { builder, .. }
        | PlayerCommand::FollowWithBuilder { builder, .. }
        | PlayerCommand::StopBuilder { builder }
        | PlayerCommand::RepairWithBuilder { builder, .. }
        | PlayerCommand::BlinkBuilder { builder, .. } => Some(builder),
        _ => None,
    }
}

impl PendingBuildCommands {
    pub(crate) fn record(
        &mut self,
        player: PlayerId,
        command: PlayerCommand,
        submission: ClientCommandSubmission,
    ) {
        if command_builder(command).is_none() {
            return;
        }
        let sequence = match submission {
            ClientCommandSubmission::Local(CommandSubmission::Scheduled(scheduled)) => {
                scheduled.client_sequence.0
            }
            ClientCommandSubmission::Submitted { client_sequence } => client_sequence,
            _ => return,
        };
        self.0.push(PendingBuildCommand {
            player,
            sequence,
            command,
        });
    }

    pub(crate) fn resolve(&mut self, player: PlayerId, sequence: u64) {
        self.0
            .retain(|pending| pending.player != player || pending.sequence != sequence);
    }

    pub(crate) fn clear(&mut self) {
        self.0.clear();
    }

    pub(crate) fn project(
        &self,
        simulation: &Simulation,
        content: &CastleFightContentBundle,
    ) -> BuildOrderProjection {
        let players = simulation.players();
        // There is at most one builder per player. Look up those early-spawned actors instead
        // of scanning every military entity again for each command-card affordability check.
        let builders: BTreeMap<_, _> = players
            .iter()
            .filter_map(|player| simulation.builder_for_player(player.id))
            .map(|builder| (builder.id, builder))
            .collect();
        let mut projection = BuildOrderProjection {
            resources: players
                .into_iter()
                .map(|player| (player.id, player.resources))
                .collect(),
            ..Default::default()
        };
        for builder in builders.values() {
            if let (Some(identity), Some(footprint)) =
                (builder.build_content, builder.build_footprint)
                && let Some(economy) = content
                    .building_kind_for_rawcode(identity.rawcode)
                    .and_then(|kind| kind.economy(content))
            {
                projection.orders.insert(
                    builder.id,
                    BuildSitePreview {
                        owner: builder.owner,
                        content: identity,
                        footprint,
                        economy,
                    },
                );
            }
        }
        for pending in &self.0 {
            let Some(builder) = command_builder(pending.command).and_then(|id| builders.get(&id))
            else {
                continue;
            };
            let next = if let PlayerCommand::PlaceBuilding {
                building, position, ..
            } = pending.command
            {
                let Some(kind) = content.building_kind(building) else {
                    continue;
                };
                let Some(rawcode) = kind.rawcode(content) else {
                    continue;
                };
                let Some(identity) = content.content_identity_for_rawcode(rawcode) else {
                    continue;
                };
                let Some(economy) = kind.economy(content) else {
                    continue;
                };
                let Some(size) = kind.footprint_size_cells(content) else {
                    continue;
                };
                Some(BuildSitePreview {
                    owner: builder.owner,
                    content: identity,
                    footprint: BuildingFootprint::new(position.min_x, position.min_y, size, size),
                    economy,
                })
            } else {
                None
            };
            projection.replace_order(builder.id, builder.owner, next);
        }
        projection
    }
}

impl BuildOrderProjection {
    fn replace_order(&mut self, builder: SimId, owner: PlayerId, next: Option<BuildSitePreview>) {
        let Some(mut resources) = self.resources.get(&owner).copied() else {
            return;
        };
        if let Some(previous) = self.orders.get(&builder) {
            refund(&mut resources, previous.economy);
        }
        if let Some(next) = next {
            if !can_afford(resources, next.economy) {
                return;
            }
            resources.gold -= next.economy.gold_cost;
            resources.lumber -= next.economy.lumber_cost;
            resources.legendary_points_used += next.economy.legendary_points_cost;
            self.orders.insert(builder, next);
        } else {
            self.orders.remove(&builder);
        }
        self.resources.insert(owner, resources);
    }

    pub(crate) fn can_afford(
        &self,
        builder: SimId,
        owner: PlayerId,
        cost: BuildingEconomyProfile,
    ) -> bool {
        let Some(mut resources) = self.resources.get(&owner).copied() else {
            return false;
        };
        if let Some(order) = self.orders.get(&builder) {
            refund(&mut resources, order.economy);
        }
        can_afford(resources, cost)
    }
}

fn can_afford(resources: PlayerResources, cost: BuildingEconomyProfile) -> bool {
    resources.gold >= cost.gold_cost
        && resources.lumber >= cost.lumber_cost
        && resources.legendary_points_available() >= cost.legendary_points_cost
}

fn refund(resources: &mut PlayerResources, cost: BuildingEconomyProfile) {
    resources.gold = resources.gold.saturating_add(cost.gold_cost);
    resources.lumber = resources.lumber.saturating_add(cost.lumber_cost);
    resources.legendary_points_used = resources
        .legendary_points_used
        .saturating_sub(cost.legendary_points_cost);
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{
        AuthoritativeSimulation, advance_authoritative_simulation_once,
        bridge::{PresentationSamples, PresentationSnapshot},
        demo::create_demo_world,
    };
    use castle_fight_sim::{
        BuildPosition, CastleFightBuildingKind, CastleFightProductionKind, Team,
    };

    pub(crate) fn legal_position(
        simulation: &Simulation,
        content: &CastleFightContentBundle,
        team: Team,
        kind: CastleFightBuildingKind,
    ) -> BuildPosition {
        let size = kind.footprint_size_cells(content).unwrap();
        simulation
            .team_build_regions(team)
            .iter()
            .flat_map(|region| {
                (region.min_y..=region.max_y()).flat_map(move |y| {
                    (region.min_x..=region.max_x())
                        .map(move |x| BuildingFootprint::new(x, y, size, size))
                })
            })
            .find(|footprint| simulation.can_place_building_for_team(team, *footprint))
            .map(|footprint| BuildPosition::new(footprint.min_x, footprint.min_y))
            .expect("fixture needs a legal build site")
    }

    #[test]
    fn submitted_build_reserves_display_resources_without_mutating_simulation() {
        let demo = create_demo_world(1, None);
        let builder = demo.simulation.builder_for_player(PlayerId(0)).unwrap();
        let kind = castle_fight_sim::CastleFightBuildingKind::Production(
            CastleFightProductionKind::Barracks,
        );
        let cost = kind.economy(demo.content).unwrap();
        let before = demo.simulation.player_resources_for(builder.owner).unwrap();
        let checksum = demo.simulation.checksum();
        let mut pending = PendingBuildCommands::default();
        pending.record(
            builder.owner,
            PlayerCommand::PlaceBuilding {
                builder: builder.id,
                building: kind.stable_id(),
                position: legal_position(&demo.simulation, demo.content, builder.team, kind),
            },
            ClientCommandSubmission::Submitted { client_sequence: 7 },
        );
        let projected = pending.project(&demo.simulation, demo.content);
        assert_eq!(
            projected.resources[&builder.owner].gold,
            before.gold - cost.gold_cost
        );
        assert_eq!(
            projected.resources[&builder.owner].lumber,
            before.lumber - cost.lumber_cost
        );
        assert_eq!(
            projected.orders[&builder.id].content.rawcode,
            kind.rawcode(demo.content).unwrap()
        );
        assert_eq!(demo.simulation.checksum(), checksum);
        pending.resolve(builder.owner, 7);
        let rejected = pending.project(&demo.simulation, demo.content);
        assert_eq!(rejected.resources[&builder.owner], before);
        assert!(rejected.orders.is_empty());
    }

    #[test]
    fn replacement_and_cancellation_refund_all_reserved_currencies() {
        let owner = PlayerId(0);
        let builder = SimId(1);
        let initial = PlayerResources {
            gold: 200,
            lumber: 150,
            legendary_points_cap: 3,
            legendary_points_used: 0,
        };
        let mut projection = BuildOrderProjection {
            resources: BTreeMap::from([(owner, initial)]),
            ..Default::default()
        };
        let site = BuildSitePreview {
            owner,
            content: ContentIdentity {
                map_version: castle_fight_sim::MapVersion::new(1, 0),
                rawcode: 1,
                name: "Synthetic building",
            },
            footprint: BuildingFootprint::new(1, 1, 1, 1),
            economy: BuildingEconomyProfile {
                gold_cost: 120,
                lumber_cost: 90,
                legendary_points_cost: 2,
                ..Default::default()
            },
        };
        projection.replace_order(builder, owner, Some(site));
        assert_eq!(projection.resources[&owner].legendary_points_used, 2);
        let replacement = BuildSitePreview {
            footprint: BuildingFootprint::new(3, 1, 1, 1),
            ..site
        };
        assert!(projection.can_afford(builder, owner, replacement.economy));
        projection.replace_order(builder, owner, Some(replacement));
        assert_eq!(
            projection.resources[&owner].gold,
            initial.gold - site.economy.gold_cost
        );
        assert_eq!(projection.orders[&builder], replacement);
        let reserved = projection.resources[&owner];
        let unaffordable = BuildSitePreview {
            economy: BuildingEconomyProfile {
                gold_cost: initial.gold + 1,
                ..site.economy
            },
            ..site
        };
        projection.replace_order(builder, owner, Some(unaffordable));
        assert_eq!(projection.resources[&owner], reserved);
        assert_eq!(projection.orders[&builder], replacement);
        projection.replace_order(SimId(2), owner, Some(site));
        assert_eq!(projection.resources[&owner], reserved);
        assert!(!projection.orders.contains_key(&SimId(2)));
        projection.replace_order(builder, owner, None);
        assert_eq!(projection.resources[&owner], initial);
        assert!(projection.orders.is_empty());
    }

    #[test]
    fn accepted_build_handoff_never_charges_twice_and_stop_refunds() {
        let demo = create_demo_world(1, None);
        let builder = demo.simulation.builder_for_player(PlayerId(0)).unwrap();
        let kind = castle_fight_sim::CastleFightBuildingKind::Production(
            CastleFightProductionKind::Barracks,
        );
        let mut authoritative = AuthoritativeSimulation::new(demo.simulation, demo.content);
        let mut samples =
            PresentationSamples::new(PresentationSnapshot::capture(&authoritative.simulation));
        let before = authoritative
            .simulation
            .player_resources_for(builder.owner)
            .unwrap();
        authoritative.submit_local_command(
            builder.owner,
            PlayerCommand::PlaceBuilding {
                builder: builder.id,
                building: kind.stable_id(),
                position: legal_position(
                    &authoritative.simulation,
                    demo.content,
                    builder.team,
                    kind,
                ),
            },
        );
        let projected = authoritative
            .pending_build_commands
            .project(&authoritative.simulation, demo.content);
        assert!(projected.orders.contains_key(&builder.id));
        advance_authoritative_simulation_once(&mut authoritative, &mut samples).unwrap();
        assert_eq!(
            authoritative
                .simulation
                .builder(builder.id)
                .unwrap()
                .build_content,
            Some(projected.orders[&builder.id].content)
        );
        let accepted = authoritative
            .pending_build_commands
            .project(&authoritative.simulation, demo.content);
        assert_eq!(accepted.resources, projected.resources);
        assert_eq!(accepted.orders, projected.orders);
        authoritative.submit_local_command(
            builder.owner,
            PlayerCommand::StopBuilder {
                builder: builder.id,
            },
        );
        let cancelled = authoritative
            .pending_build_commands
            .project(&authoritative.simulation, demo.content);
        assert_eq!(cancelled.resources[&builder.owner], before);
        assert!(cancelled.orders.is_empty());
        advance_authoritative_simulation_once(&mut authoritative, &mut samples).unwrap();
        assert_eq!(
            authoritative
                .simulation
                .player_resources_for(builder.owner)
                .unwrap(),
            before
        );
        assert!(
            authoritative
                .pending_build_commands
                .project(&authoritative.simulation, demo.content)
                .orders
                .is_empty()
        );
    }

    #[test]
    fn execution_failure_removes_ghost_and_restores_resources() {
        let demo = create_demo_world(1, None);
        let builder = demo.simulation.builder_for_player(PlayerId(0)).unwrap();
        let definition = demo
            .content
            .production_building(CastleFightProductionKind::Barracks)
            .unwrap();
        let kind = demo
            .content
            .building_kind_for_rawcode(definition.rawcode)
            .unwrap();
        let position = legal_position(&demo.simulation, demo.content, builder.team, kind);
        let mut authoritative = AuthoritativeSimulation::new(demo.simulation, demo.content);
        let mut samples =
            PresentationSamples::new(PresentationSnapshot::capture(&authoritative.simulation));
        let before = authoritative
            .simulation
            .player_resources_for(builder.owner)
            .unwrap();
        authoritative.submit_local_command(
            builder.owner,
            PlayerCommand::PlaceBuilding {
                builder: builder.id,
                building: kind.stable_id(),
                position,
            },
        );
        assert!(
            authoritative
                .pending_build_commands
                .project(&authoritative.simulation, demo.content)
                .orders
                .contains_key(&builder.id)
        );
        // Another accepted action occupies the site before this command executes.
        authoritative.simulation.spawn_building_with_properties(
            definition.spawn(
                builder.team,
                BuildingFootprint::new(
                    position.min_x,
                    position.min_y,
                    definition.footprint_size_cells,
                    definition.footprint_size_cells,
                ),
            ),
            definition.gameplay_properties(),
        );
        let result =
            advance_authoritative_simulation_once(&mut authoritative, &mut samples).unwrap();
        assert!(matches!(
            result.executions[0].outcome,
            castle_fight_sim::CommandOutcome::Rejected(_)
        ));
        let rejected = authoritative
            .pending_build_commands
            .project(&authoritative.simulation, demo.content);
        assert!(rejected.orders.is_empty());
        assert_eq!(rejected.resources[&builder.owner], before);
    }
}
