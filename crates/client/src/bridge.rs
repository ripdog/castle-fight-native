use std::collections::BTreeMap;

use bevy::prelude::Resource;
use castle_fight_sim::{
    AbilityCastEvent, AbilityId, ArmorProfile, AttackDelivery, AttackEvent, AttackProfile,
    BuilderLocomotion, BuildingFootprint, ChainLightningEvent, ContentIdentity, CorpseView,
    DamageRules, DamageType, MovementClass, PlayerEconomyView, PlayerId, PlayerView,
    ProjectileView, SecondaryAttackProfile, SimId, SimPoint, Simulation, StatusState, Team,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnitVisualKind {
    Melee,
    Ranged,
    Ballistic,
    Bounce,
    MeleeCaster,
    RangedCaster,
    BallisticCaster,
    BounceCaster,
}

impl UnitVisualKind {
    fn from_delivery(delivery: AttackDelivery, spellcaster: bool) -> Self {
        match (delivery, spellcaster) {
            (AttackDelivery::Melee, false) => Self::Melee,
            (AttackDelivery::RangedInstant | AttackDelivery::RangedGuaranteedHit { .. }, false) => {
                Self::Ranged
            }
            (AttackDelivery::RangedBallistic { .. }, false) => Self::Ballistic,
            (AttackDelivery::Bounce { .. }, false) => Self::Bounce,
            (AttackDelivery::Melee, true) => Self::MeleeCaster,
            (AttackDelivery::RangedInstant | AttackDelivery::RangedGuaranteedHit { .. }, true) => {
                Self::RangedCaster
            }
            (AttackDelivery::RangedBallistic { .. }, true) => Self::BallisticCaster,
            (AttackDelivery::Bounce { .. }, true) => Self::BounceCaster,
        }
    }

    pub(crate) const fn is_caster(self) -> bool {
        matches!(
            self,
            Self::MeleeCaster | Self::RangedCaster | Self::BallisticCaster | Self::BounceCaster
        )
    }

    pub(crate) const fn weapon_kind(self) -> UnitVisualKind {
        match self {
            Self::MeleeCaster => Self::Melee,
            Self::RangedCaster => Self::Ranged,
            Self::BallisticCaster => Self::Ballistic,
            Self::BounceCaster => Self::Bounce,
            other => other,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingVisualKind {
    Structure,
    Production,
    Attack,
    Spellcaster,
    ProductionAttack,
    ProductionSpellcaster,
    AttackSpellcaster,
    ProductionAttackSpellcaster,
}

impl BuildingVisualKind {
    fn from_roles(production: bool, attack: bool, spellcaster: bool) -> Self {
        match (production, attack, spellcaster) {
            (false, false, false) => Self::Structure,
            (true, false, false) => Self::Production,
            (false, true, false) => Self::Attack,
            (false, false, true) => Self::Spellcaster,
            (true, true, false) => Self::ProductionAttack,
            (true, false, true) => Self::ProductionSpellcaster,
            (false, true, true) => Self::AttackSpellcaster,
            (true, true, true) => Self::ProductionAttackSpellcaster,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct UnitSample {
    pub id: SimId,
    pub content: Option<ContentIdentity>,
    pub owner: PlayerId,
    pub team: Team,
    pub position: SimPoint,
    pub collision_radius: i32,
    pub movement_class: MovementClass,
    pub mechanical: bool,
    pub health: i32,
    pub health_max: i32,
    pub attack: AttackProfile,
    pub secondary_attack: Option<SecondaryAttackProfile>,
    pub damage_type: DamageType,
    pub armor: ArmorProfile,
    pub target: Option<SimId>,
    pub direct_retaliation_lock: bool,
    pub ally_defense_lock: bool,
    pub cooldown_remaining: u16,
    pub stunned_until_tick: u64,
    pub status: StatusState,
    pub mana_current: Option<i32>,
    pub mana_maximum: Option<i32>,
    pub visual_kind: UnitVisualKind,
    pub active_defend_ability: Option<AbilityId>,
}

#[derive(Debug, Clone, Copy)]
pub struct BuilderSample {
    pub id: SimId,
    pub owner: PlayerId,
    pub team: Team,
    pub position: SimPoint,
    pub appearance: ContentIdentity,
    pub locomotion: BuilderLocomotion,
    pub destination: Option<SimPoint>,
    pub follow_target: Option<SimId>,
    pub repair_target: Option<SimId>,
    pub build_footprint: Option<BuildingFootprint>,
    pub repair_autocast_enabled: bool,
    pub blink_range: i32,
    pub build_catalog_len: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct BuildingSample {
    pub id: SimId,
    pub content: Option<ContentIdentity>,
    pub owner: Option<PlayerId>,
    pub team: Team,
    pub footprint: BuildingFootprint,
    pub health: i32,
    pub health_max: i32,
    pub construction_started_tick: Option<u64>,
    pub construction_complete_tick: Option<u64>,
    pub attack: Option<AttackProfile>,
    pub damage_type: Option<DamageType>,
    pub armor: ArmorProfile,
    pub target: Option<SimId>,
    pub next_spawn_tick: Option<u64>,
    pub production_queue: Option<u8>,
    pub production_interval_ticks: Option<u16>,
    pub cooldown_remaining: Option<u16>,
    pub mana_current: Option<i32>,
    pub mana_maximum: Option<i32>,
    pub ability_ready_tick: Option<u64>,
    pub ability_autocast_enabled: Option<bool>,
    pub stunned_until_tick: Option<u64>,
    pub visual_kind: BuildingVisualKind,
}

#[derive(Debug, Clone)]
pub struct PresentationSnapshot {
    pub tick: u64,
    pub damage_rules: DamageRules,
    pub players: BTreeMap<PlayerId, PlayerView>,
    pub player_economy: BTreeMap<PlayerId, PlayerEconomyView>,
    pub units: BTreeMap<SimId, UnitSample>,
    pub builders: BTreeMap<SimId, BuilderSample>,
    pub buildings: BTreeMap<SimId, BuildingSample>,
    pub corpses: BTreeMap<SimId, CorpseView>,
    pub projectiles: BTreeMap<SimId, ProjectileView>,
    pub attacks: Vec<AttackEvent>,
    pub ability_casts: Vec<AbilityCastEvent>,
    pub chain_lightnings: Vec<ChainLightningEvent>,
}

impl PresentationSnapshot {
    #[must_use]
    pub fn capture(simulation: &Simulation) -> Self {
        let units = simulation
            .units()
            .into_iter()
            .map(|unit| {
                (
                    unit.id,
                    UnitSample {
                        id: unit.id,
                        content: unit.content,
                        owner: unit.owner,
                        team: unit.team,
                        position: unit.position,
                        collision_radius: unit.collision_radius,
                        movement_class: unit.movement_class,
                        mechanical: unit.mechanical,
                        health: unit.health,
                        health_max: unit.health_max,
                        attack: unit.attack,
                        secondary_attack: unit.secondary_attack,
                        damage_type: unit.damage_type,
                        armor: unit.armor,
                        target: unit.target,
                        direct_retaliation_lock: unit.direct_retaliation_lock,
                        ally_defense_lock: unit.ally_defense_lock,
                        cooldown_remaining: unit.cooldown_remaining,
                        stunned_until_tick: unit.stunned_until_tick,
                        status: unit.status,
                        mana_current: unit.mana_current,
                        mana_maximum: unit.mana_maximum,
                        visual_kind: UnitVisualKind::from_delivery(
                            unit.attack_delivery,
                            unit.mana_maximum.is_some(),
                        ),
                        active_defend_ability: unit.active_defend_ability,
                    },
                )
            })
            .collect();
        let builders = simulation
            .builders()
            .into_iter()
            .map(|builder| {
                (
                    builder.id,
                    BuilderSample {
                        id: builder.id,
                        owner: builder.owner,
                        team: builder.team,
                        position: builder.position,
                        appearance: builder.configuration.appearance,
                        locomotion: builder.configuration.locomotion,
                        destination: builder.destination,
                        follow_target: builder.follow_target,
                        repair_target: builder.repair_target,
                        build_footprint: builder.build_footprint,
                        repair_autocast_enabled: builder.repair_autocast_enabled,
                        blink_range: builder.profile.blink_range,
                        build_catalog_len: builder.configuration.build_catalog.len(),
                    },
                )
            })
            .collect();
        let buildings = simulation
            .buildings()
            .into_iter()
            .map(|building| {
                let visual_kind = BuildingVisualKind::from_roles(
                    building.production.is_some(),
                    building.attack_delivery.is_some(),
                    building.mana_maximum.is_some(),
                );
                (
                    building.id,
                    BuildingSample {
                        id: building.id,
                        content: building.content,
                        owner: building.owner,
                        team: building.team,
                        footprint: building.footprint,
                        health: building.health,
                        health_max: building.health_max,
                        construction_started_tick: building.construction_started_tick,
                        construction_complete_tick: building.construction_complete_tick,
                        attack: building.attack,
                        damage_type: building.attack_delivery.map(|_| building.damage_type),
                        armor: building.armor,
                        target: building.target,
                        next_spawn_tick: building.next_spawn_tick,
                        production_queue: building.production_queue,
                        production_interval_ticks: building
                            .production
                            .map(|production| production.interval_ticks),
                        cooldown_remaining: building.cooldown_remaining,
                        mana_current: building.mana_current,
                        mana_maximum: building.mana_maximum,
                        ability_ready_tick: building.ability_ready_tick,
                        ability_autocast_enabled: building.ability_autocast_enabled,
                        stunned_until_tick: building.stunned_until_tick,
                        visual_kind,
                    },
                )
            })
            .collect();
        let corpses = simulation
            .corpses()
            .into_iter()
            .map(|corpse| (corpse.id, corpse))
            .collect();
        let projectiles = simulation
            .projectiles()
            .into_iter()
            .map(|projectile| (projectile.id, projectile))
            .collect();

        let players = simulation
            .players()
            .into_iter()
            .map(|player| (player.id, player))
            .collect::<BTreeMap<_, _>>();
        let player_economy = players
            .keys()
            .copied()
            .map(|player| {
                (
                    player,
                    simulation
                        .player_economy_for(player)
                        .expect("presentation player must have canonical economy state"),
                )
            })
            .collect();

        Self {
            tick: simulation.tick(),
            damage_rules: simulation.damage_rules(),
            players,
            player_economy,
            units,
            builders,
            buildings,
            corpses,
            projectiles,
            attacks: simulation.attacks_last_tick().to_vec(),
            ability_casts: simulation.ability_casts_last_tick().to_vec(),
            chain_lightnings: simulation.chain_lightnings_last_tick().to_vec(),
        }
    }
}

#[derive(Resource, Debug, Clone)]
pub struct PresentationSamples {
    pub previous: PresentationSnapshot,
    pub current: PresentationSnapshot,
}

impl PresentationSamples {
    #[must_use]
    pub fn new(initial: PresentationSnapshot) -> Self {
        Self {
            previous: initial.clone(),
            current: initial,
        }
    }

    pub fn publish(&mut self, next: PresentationSnapshot) {
        self.previous = std::mem::replace(&mut self.current, next);
    }
}

#[cfg(test)]
mod tests {
    use castle_fight_sim::{
        AttackProfile, BuildingFootprint, CastleFightBuilderRace, CastleFightProductionKind,
        CastleFightUnitKind, CorpseDefinitionId, CorpseProfile, MatchDriver, MovementProfile,
        PlayerCommand, SUBUNITS_PER_WORLD_UNIT, SimulationConfig, UnitSpawn,
    };

    use super::*;

    #[test]
    fn captures_authoritative_corpses_and_attack_events() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 1);
        simulation.spawn_unit(UnitSpawn {
            team: Team(0),
            position: SimPoint::new(0, 0),
            health: 100,
            attack: AttackProfile {
                delivery: AttackDelivery::Melee,
                damage: 100,
                range: 2 * SUBUNITS_PER_WORLD_UNIT,
                acquisition_range: 2 * SUBUNITS_PER_WORLD_UNIT,
                cooldown_ticks: 1,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        });
        let victim = simulation.spawn_unit_with_corpse(
            UnitSpawn {
                team: Team(1),
                position: SimPoint::new(SUBUNITS_PER_WORLD_UNIT, 0),
                health: 100,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 0,
                    range: 0,
                    acquisition_range: 0,
                    cooldown_ticks: 1,
                },
                movement: MovementProfile { speed_per_tick: 0 },
            },
            CorpseProfile {
                definition: CorpseDefinitionId(7),
                decay_start_ticks: 1,
                lifetime_ticks: Some(5),
            },
        );

        simulation.step();
        simulation.step();
        let snapshot = PresentationSnapshot::capture(&simulation);

        assert_eq!(snapshot.corpses.len(), 1);
        assert_eq!(
            snapshot.corpses.values().next().unwrap().source_unit,
            victim
        );
        assert!(
            snapshot
                .attacks
                .iter()
                .any(|attack| attack.target == victim)
        );
    }

    #[test]
    fn capture_includes_configured_builder_state() {
        let demo = crate::demo::create_demo_world(1, Some(0));
        let human = CastleFightBuilderRace::Human.definition();
        let mut simulation = demo.simulation;
        let builder_view = simulation
            .builder_for_player(PlayerId(0))
            .expect("western builder");
        let builder = builder_view.id;
        let expected_build_catalog_len = builder_view.configuration.build_catalog.len();
        let destination = SimPoint::new(
            builder_view.position.x + 20 * SUBUNITS_PER_WORLD_UNIT,
            builder_view.position.y,
        );
        let mut driver = MatchDriver::new(&simulation, demo.content);
        assert!(
            driver
                .submit_local_command(
                    &simulation,
                    PlayerId(0),
                    PlayerCommand::MoveBuilder {
                        builder,
                        destination,
                    },
                )
                .is_newly_scheduled()
        );
        driver.advance_local_tick(&mut simulation).unwrap();

        let snapshot = PresentationSnapshot::capture(&simulation);
        let sample = snapshot.builders.get(&builder).unwrap();
        assert_eq!(sample.appearance.rawcode, human.rawcode);
        assert_eq!(sample.locomotion, human.locomotion);
        assert_eq!(sample.destination, Some(destination));
        assert_eq!(sample.follow_target, None);
        assert_eq!(sample.repair_target, None);
        assert!(sample.repair_autocast_enabled);
        assert_eq!(sample.blink_range, human.profile.blink_range);
        assert_eq!(sample.build_catalog_len, expected_build_catalog_len);
    }

    #[test]
    fn capture_preserves_imported_content_names() {
        let mut simulation = Simulation::new(SimulationConfig::default(), 1);
        let footman = CastleFightUnitKind::Footman.definition();
        simulation.spawn_unit_with_properties(
            UnitSpawn::from_template(
                Team(0),
                SimPoint::new(40 * SUBUNITS_PER_WORLD_UNIT, 0),
                footman.template(),
            ),
            footman.gameplay_properties(),
        );
        let barracks = CastleFightProductionKind::Barracks.definition();
        simulation.spawn_building_with_properties(
            barracks.spawn(Team(0), BuildingFootprint::new(80, 0, 4, 4)),
            barracks.gameplay_properties(),
        );

        let snapshot = PresentationSnapshot::capture(&simulation);
        assert_eq!(
            snapshot
                .units
                .values()
                .next()
                .unwrap()
                .content
                .unwrap()
                .name,
            "Footman"
        );
        let building = snapshot.buildings.values().next().unwrap();
        assert_eq!(building.content.unwrap().name, "Barracks");
        assert_eq!(
            building.production_interval_ticks,
            Some(barracks.spawn_interval_ticks)
        );
        assert!(building.next_spawn_tick.is_some());
    }

    #[test]
    fn visual_kinds_cover_every_exposed_delivery_and_role_combination() {
        let deliveries = [
            AttackDelivery::Melee,
            AttackDelivery::RangedGuaranteedHit { speed_per_tick: 1 },
            AttackDelivery::RangedBallistic {
                speed_per_tick: 1,
                impact_radius: 2,
            },
            AttackDelivery::Bounce {
                speed_per_tick: 1,
                bounce_range: 2,
                max_bounces: 3,
                damage_percent_per_bounce: 50,
                allow_repeat_targets: false,
            },
        ];
        let mut kinds = Vec::new();
        for delivery in deliveries {
            kinds.push(UnitVisualKind::from_delivery(delivery, false));
            kinds.push(UnitVisualKind::from_delivery(delivery, true));
        }
        assert_eq!(
            kinds,
            [
                UnitVisualKind::Melee,
                UnitVisualKind::MeleeCaster,
                UnitVisualKind::Ranged,
                UnitVisualKind::RangedCaster,
                UnitVisualKind::Ballistic,
                UnitVisualKind::BallisticCaster,
                UnitVisualKind::Bounce,
                UnitVisualKind::BounceCaster,
            ]
        );

        let mut building_kinds = Vec::new();
        for production in [false, true] {
            for attack in [false, true] {
                for spellcaster in [false, true] {
                    building_kinds.push(BuildingVisualKind::from_roles(
                        production,
                        attack,
                        spellcaster,
                    ));
                }
            }
        }
        assert_eq!(building_kinds.len(), 8);
        assert!(building_kinds.contains(&BuildingVisualKind::ProductionAttackSpellcaster));
    }

    #[test]
    fn repeated_presentation_capture_does_not_change_authoritative_checksums() {
        fn fixture() -> Simulation {
            let mut simulation = Simulation::new(SimulationConfig::default(), 1);
            simulation.spawn_unit(UnitSpawn {
                team: Team(0),
                position: SimPoint::new(2 * SUBUNITS_PER_WORLD_UNIT, 0),
                health: 100,
                attack: AttackProfile {
                    delivery: AttackDelivery::Melee,
                    damage: 1,
                    range: SUBUNITS_PER_WORLD_UNIT,
                    acquisition_range: 4 * SUBUNITS_PER_WORLD_UNIT,
                    cooldown_ticks: 30,
                },
                movement: MovementProfile { speed_per_tick: 1 },
            });
            simulation
        }

        let mut headless = fixture();
        let mut presented = fixture();
        for _ in 0..120 {
            headless.step();
            presented.step();
            let snapshot = PresentationSnapshot::capture(&presented);

            assert_eq!(snapshot.tick, presented.tick());
            assert_eq!(snapshot.units.len(), 1);
            assert_eq!(headless.checksum(), presented.checksum());
        }
    }
}
