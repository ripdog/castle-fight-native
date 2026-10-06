use std::collections::{BTreeMap, BTreeSet};

use bevy::prelude::Resource;
use castle_fight_sim::{
    AbilityCastEvent, AbilityId, ArmorProfile, AttackDelivery, AttackEvent, AttackProfile,
    BuilderLocomotion, BuildingFootprint, ChainLightningEvent, ContentIdentity, CorpseView,
    DamageRules, DamageType, MovementClass, PlayerEconomyView, PlayerId, PlayerView,
    ProjectileView, SecondaryAttackProfile, SimId, SimPoint, Simulation, StatusState, Team,
};

mod network_timeline;
use network_timeline::NetworkTimeline;

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
            (
                AttackDelivery::RangedInstant
                | AttackDelivery::RangedGuaranteedHit { .. }
                | AttackDelivery::Line { .. },
                false,
            ) => Self::Ranged,
            (AttackDelivery::RangedBallistic { .. }, false) => Self::Ballistic,
            (AttackDelivery::Bounce { .. }, false) => Self::Bounce,
            (AttackDelivery::Melee, true) => Self::MeleeCaster,
            (
                AttackDelivery::RangedInstant
                | AttackDelivery::RangedGuaranteedHit { .. }
                | AttackDelivery::Line { .. },
                true,
            ) => Self::RangedCaster,
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
    pub invisible: bool,
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
    pub hex: Option<castle_fight_sim::HexState>,
    pub negative_building_shield_level: u8,
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
    pub remembered: bool,
    pub construction_observed_tick: Option<u64>,
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
    pub status: StatusState,
    pub visual_kind: BuildingVisualKind,
}

#[derive(Debug, Clone)]
pub struct PresentationSnapshot {
    pub projectile_positions: BTreeMap<SimId, SimPoint>,
    pub navigation_cell_size: i32,
    pub fog: Option<castle_fight_sim::FogOfWar>,
    pub observer: Option<Team>,
    pub hidden_entities: BTreeSet<SimId>,
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
    pub shrine_revivals: Vec<castle_fight_sim::ShrineRevivalEvent>,
    pub chain_lightnings: Vec<ChainLightningEvent>,
    pub building_spell_visuals: Vec<castle_fight_sim::BuildingSpellVisualEvent>,
}

impl PresentationSnapshot {
    #[must_use]
    pub fn capture(simulation: &Simulation) -> Self {
        let units: BTreeMap<_, _> = simulation
            .units()
            .into_iter()
            .map(|unit| {
                (
                    unit.id,
                    UnitSample {
                        invisible: unit.classifications.invisible,
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
                        hex: unit.hex,
                        negative_building_shield_level: unit.negative_building_shield_level,
                    },
                )
            })
            .collect();
        let builders: BTreeMap<_, _> = simulation
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
        let buildings: BTreeMap<_, _> = simulation
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
                        remembered: false,
                        construction_observed_tick: None,
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
                        status: building.status,
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
        let projectiles: BTreeMap<_, _> = simulation
            .projectiles()
            .into_iter()
            .map(|projectile| (projectile.id, projectile))
            .collect();

        let projectile_positions = projectiles
            .values()
            .map(|projectile| {
                let target = match projectile.kind {
                    castle_fight_sim::ProjectileViewKind::GuaranteedHit { target }
                    | castle_fight_sim::ProjectileViewKind::Reflected { target, .. }
                    | castle_fight_sim::ProjectileViewKind::NativeCarrierBolt { target, .. }
                    | castle_fight_sim::ProjectileViewKind::Bounce { target, .. } => Some(target),
                    castle_fight_sim::ProjectileViewKind::Line {
                        primary_target,
                        spill_origin: None,
                        ..
                    } => Some(primary_target),
                    _ => None,
                };
                let end = target
                    .and_then(|id| {
                        units
                            .get(&id)
                            .map(|u: &UnitSample| u.position)
                            .or_else(|| builders.get(&id).map(|b: &BuilderSample| b.position))
                            .or_else(|| {
                                buildings.get(&id).map(|b: &BuildingSample| {
                                    structure_center(
                                        b.footprint,
                                        simulation.config().navigation_cell_size,
                                    )
                                })
                            })
                    })
                    .or(match projectile.kind {
                        castle_fight_sim::ProjectileViewKind::Line { destination, .. }
                        | castle_fight_sim::ProjectileViewKind::Ballistic { destination, .. } => {
                            Some(destination)
                        }
                        _ => None,
                    })
                    .unwrap_or(projectile.launch_position);
                let (start, launch) = match projectile.kind {
                    castle_fight_sim::ProjectileViewKind::Line {
                        spill_origin: Some(origin),
                        primary_impact_tick,
                        ..
                    } => (origin, primary_impact_tick),
                    _ => (projectile.launch_position, projectile.launch_tick),
                };
                let duration = projectile.impact_tick.saturating_sub(launch).max(1);
                let elapsed = simulation
                    .tick()
                    .saturating_sub(1)
                    .saturating_sub(launch)
                    .min(duration);
                let axis = |start: i32, end: i32| {
                    start
                        + ((i64::from(end) - i64::from(start)) * elapsed as i64 / duration as i64)
                            as i32
                };
                (
                    projectile.id,
                    SimPoint::new(axis(start.x, end.x), axis(start.y, end.y)),
                )
            })
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
            projectile_positions,
            navigation_cell_size: simulation.config().navigation_cell_size,
            fog: simulation.fog_of_war(),
            observer: None,
            hidden_entities: BTreeSet::new(),
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
            shrine_revivals: simulation.shrine_revivals_last_tick().to_vec(),
            chain_lightnings: simulation.chain_lightnings_last_tick().to_vec(),
            building_spell_visuals: simulation.building_spell_visuals_last_tick().to_vec(),
        }
    }
}

#[derive(Resource, Debug, Clone)]
pub struct PresentationSamples {
    observer: Option<Team>,
    pub previous: PresentationSnapshot,
    pub current: PresentationSnapshot,
    network: Option<NetworkTimeline>,
    revision: u64,
    tick_advanced: bool,
}

impl PresentationSamples {
    #[must_use]
    pub fn new(initial: PresentationSnapshot) -> Self {
        Self {
            observer: None,
            previous: initial.clone(),
            current: initial,
            network: None,
            revision: 0,
            tick_advanced: false,
        }
    }

    pub fn with_observer(mut self, team: Option<Team>) -> Self {
        self.observer = team;
        if let Some(team) = team {
            self.current.restrict_to_team(team);
            self.previous = self.current.clone();
        }
        self
    }

    pub fn publish(&mut self, mut next: PresentationSnapshot) {
        if let Some(team) = self.observer {
            next.restrict_to_team(team);
        }
        self.tick_advanced = next.tick > self.current.tick;
        self.previous = std::mem::replace(&mut self.current, next);
        self.revision += 1;
    }

    pub(crate) fn revision(&self) -> u64 {
        self.revision
    }

    pub(crate) fn tick_advanced(&self) -> bool {
        self.tick_advanced
    }

    pub(crate) fn reset(&mut self, mut snapshot: PresentationSnapshot) {
        if let Some(team) = self.observer {
            snapshot.restrict_to_team(team);
        }
        snapshot.clear_events();
        if self.network.is_some() {
            self.network = Some(NetworkTimeline::default());
        }
        self.previous = snapshot.clone();
        self.current = snapshot;
        self.tick_advanced = false;
        self.revision += 1;
    }
}

impl PresentationSnapshot {
    fn restrict_to_team(&mut self, team: Team) {
        if self.observer == Some(team) {
            return;
        }
        let Some(fog) = &self.fog else {
            return;
        };
        self.observer = Some(team);
        self.units.retain(|id, unit| {
            let visible = unit.team == team
                || unit.status.is_revealed_to(team, self.tick)
                || fog.detects_invisible(team, unit.position)
                || (!unit.invisible && fog.is_visible(team, unit.position));
            if !visible {
                self.hidden_entities.insert(*id);
            }
            visible
        });
        self.builders.retain(|id, builder| {
            let visible = builder.team == team || fog.is_visible(team, builder.position);
            if !visible {
                self.hidden_entities.insert(*id);
            }
            visible
        });
        // Structure memory carries only the last observed silhouette, never live health, production,
        // mana, target, construction, buffs or upgrades from behind fog.
        let memory = &fog.remembered_structures[usize::from(team.0)];
        self.buildings.retain(|id, building| {
            let visible = building.team == team
                || memory.iter().any(|s| s.id == *id)
                    && fog.is_visible(
                        team,
                        structure_center(building.footprint, self.navigation_cell_size),
                    );
            if !visible {
                self.hidden_entities.insert(*id);
            }
            visible
        });
        for structure in memory {
            if self.buildings.contains_key(&structure.id) {
                continue;
            }
            self.buildings.insert(
                structure.id,
                BuildingSample {
                    remembered: true,
                    construction_observed_tick: structure.construction.map(|c| c.observed_tick),
                    id: structure.id,
                    content: structure.content,
                    owner: structure.owner,
                    team: Team(1 - team.0),
                    footprint: structure.footprint,
                    health: 1,
                    health_max: 1,
                    construction_started_tick: structure.construction.map(|c| c.started_tick),
                    construction_complete_tick: structure.construction.map(|c| c.complete_tick),
                    attack: None,
                    damage_type: None,
                    armor: ArmorProfile::default(),
                    target: None,
                    next_spawn_tick: None,
                    production_queue: None,
                    production_interval_ticks: None,
                    cooldown_remaining: None,
                    mana_current: None,
                    mana_maximum: None,
                    ability_ready_tick: None,
                    ability_autocast_enabled: None,
                    stunned_until_tick: None,
                    status: StatusState::default(),
                    visual_kind: BuildingVisualKind::Structure,
                },
            );
        }
        self.corpses.retain(|_, corpse| {
            let visible = fog.is_visible(team, corpse.position);
            if !visible {
                self.hidden_entities.insert(corpse.source_unit);
            }
            visible
        });
        self.projectiles.retain(|id, _| {
            let visible = self
                .projectile_positions
                .get(id)
                .is_some_and(|p| fog.is_visible(team, *p));
            if !visible {
                self.hidden_entities.insert(*id);
            }
            visible
        });
        self.projectile_positions
            .retain(|id, _| self.projectiles.contains_key(id));
        let seen = |id: &SimId| {
            self.units.contains_key(id)
                || self.builders.contains_key(id)
                || self
                    .buildings
                    .get(id)
                    .is_some_and(|building| !building.remembered)
        };
        self.attacks.retain(|event| {
            seen(&event.source)
                && fog.is_visible(team, event.source_position)
                && fog.is_visible(team, event.target_position)
        });
        self.ability_casts.retain(|event| {
            (seen(&event.source)
                || match event.target {
                    castle_fight_sim::AbilityCastTarget::Unit(target) => seen(&target),
                    castle_fight_sim::AbilityCastTarget::Point(position) => {
                        fog.is_visible(team, position)
                    }
                    _ => false,
                })
                && event
                    .target_position
                    .is_none_or(|p| fog.is_visible(team, p))
        });
        self.shrine_revivals
            .retain(|event| fog.is_visible(team, event.position));
        self.chain_lightnings.retain(|event| {
            seen(&event.source) && event.points().iter().all(|p| fog.is_visible(team, *p))
        });
        self.building_spell_visuals
            .retain(|event| seen(&event.target));
        let seen_ids: BTreeSet<_> = self
            .units
            .keys()
            .chain(self.builders.keys())
            .chain(
                self.buildings
                    .iter()
                    .filter_map(|(id, b)| (!b.remembered).then_some(id)),
            )
            .copied()
            .collect();
        for unit in self.units.values_mut() {
            unit.target = unit.target.filter(|id| seen_ids.contains(id));
        }
        for building in self.buildings.values_mut() {
            building.target = building.target.filter(|id| seen_ids.contains(id));
        }
    }

    fn clear_events(&mut self) {
        self.attacks.clear();
        self.ability_casts.clear();
        self.shrine_revivals.clear();
        self.chain_lightnings.clear();
        self.building_spell_visuals.clear();
    }

    fn prepend_events(&mut self, earlier: &mut Self) {
        // A low frame rate can cross several display ticks. Retain every event in
        // chronological order, while publishing only the final state for this frame.
        macro_rules! prepend {
            ($field:ident) => {
                std::mem::swap(&mut self.$field, &mut earlier.$field);
                self.$field.append(&mut earlier.$field);
            };
        }
        prepend!(attacks);
        prepend!(ability_casts);
        prepend!(shrine_revivals);
        prepend!(chain_lightnings);
        prepend!(building_spell_visuals);
    }
}

fn structure_center(footprint: BuildingFootprint, cell_size: i32) -> SimPoint {
    SimPoint::new(
        footprint.min_x * cell_size + i32::from(footprint.width) * cell_size / 2,
        footprint.min_y * cell_size + i32::from(footprint.height) * cell_size / 2,
    )
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
    fn observer_hides_enemies_and_retains_only_last_seen_structure_silhouettes() {
        use castle_fight_sim::{
            BuildingSpawn, FogRules, NavCell, SightProfile, UnitClassifications,
        };
        let scale = SUBUNITS_PER_WORLD_UNIT;
        let mut sim = Simulation::new(
            SimulationConfig {
                navigation_min: NavCell::new(-10, -10),
                navigation_max: NavCell::new(40, 10),
                fog: Some(FogRules {
                    cell_size: scale,
                    fallback_sight: SightProfile {
                        day: 4 * scale,
                        night: 4 * scale,
                    },
                    initially_explored: true,
                    night: false,
                    clock: None,
                    attack_reveal: None,
                    permanent_rectangles: [Vec::new(), Vec::new()],
                    sight_blockers: Vec::new(),
                }),
                ..SimulationConfig::default()
            },
            1,
        );
        let spawn = |team, x| UnitSpawn {
            team,
            position: SimPoint::new(x * scale, scale / 2),
            health: 100,
            attack: AttackProfile {
                damage: 0,
                range: 0,
                acquisition_range: 0,
                cooldown_ticks: 1,
                delivery: AttackDelivery::Melee,
            },
            movement: MovementProfile { speed_per_tick: 0 },
        };
        let own = sim.spawn_unit(spawn(Team(0), 0));
        let visible = sim.spawn_unit(spawn(Team(1), 2));
        let hidden = sim.spawn_unit(spawn(Team(1), 20));
        let invisible = sim.spawn_unit_with_properties(
            spawn(Team(1), 2),
            castle_fight_sim::UnitGameplayProperties {
                classifications: UnitClassifications {
                    invisible: true,
                    ..Default::default()
                },
                ..Default::default()
            },
        );
        let structure = sim.spawn_building(BuildingSpawn {
            team: Team(1),
            footprint: BuildingFootprint::new(3, 1, 1, 1),
            health: 100,
            attack: None,
            production: None,
            spellcasting: None,
        });
        let mut snapshot = PresentationSnapshot::capture(&sim);
        for target in [visible, invisible] {
            snapshot.ability_casts.push(AbilityCastEvent {
                source: hidden,
                ability: AbilityId(900),
                target: castle_fight_sim::AbilityCastTarget::Unit(target),
                target_position: Some(SimPoint::new(2 * scale, scale / 2)),
                effect: castle_fight_sim::AbilityEffect::Damage { amount: 1 },
            });
        }
        let mut samples = PresentationSamples::new(snapshot.clone()).with_observer(Some(Team(0)));
        assert_eq!(
            samples.current.ability_casts.len(),
            1,
            "visible impacts survive a concealed caster without exposing an invisible target"
        );
        assert!(samples.current.units.contains_key(&own));
        assert!(samples.current.units.contains_key(&visible));
        assert!(!samples.current.units.contains_key(&hidden));
        assert!(!samples.current.units.contains_key(&invisible));
        assert!(!samples.current.buildings[&structure].remembered);
        let mut veiled = snapshot;
        veiled.fog.as_mut().unwrap().visible[0].fill(0);
        veiled.tick += 1;
        samples.publish(veiled.clone());
        assert!(samples.current.units.contains_key(&own));
        assert!(!samples.current.units.contains_key(&visible));
        assert!(samples.current.hidden_entities.contains(&visible));
        let ghost = &samples.current.buildings[&structure];
        assert!(ghost.remembered);
        assert!(
            ghost.target.is_none()
                && ghost.next_spawn_tick.is_none()
                && ghost.mana_current.is_none()
        );
        assert_eq!(ghost.status, StatusState::default());
        let remembered_footprint = ghost.footprint;
        veiled.buildings.remove(&structure);
        samples.reset(veiled.clone());
        assert!(samples.current.buildings[&structure].remembered);
        assert_eq!(
            samples.previous.buildings[&structure].footprint,
            remembered_footprint
        );
        samples.enqueue_network_boundary(veiled);
        samples.advance_network_timeline(0.0, 1.0, true, true);
        assert!(!samples.current.units.contains_key(&visible));
        assert_eq!(samples.current.observer, Some(Team(0)));
    }

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
    fn capture_preserves_passive_structure_status_through_wire_and_expiry_without_mutation() {
        use castle_fight_sim::{
            AbilityEffect, AbilityTargetPolicy, AttackTargetMask, AutomaticAbilityProfile,
            BuildingSpawn, ManaProfile, NativeBoltProfile, SpellcastingProfile,
        };
        let mut original = Simulation::new(SimulationConfig::default(), 1);
        original.spawn_unit_with_spellcasting(
            UnitSpawn {
                team: Team(0),
                position: SimPoint::new(0, 0),
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
            SpellcastingProfile {
                mana: ManaProfile::per_second(0, 0, 0),
                ability: AutomaticAbilityProfile {
                    id: AbilityId(2),
                    mana_cost: 0,
                    cooldown_ticks: 500,
                    range: 10 * SUBUNITS_PER_WORLD_UNIT,
                    target_policy: AbilityTargetPolicy::RandomEnemyUnitOrBuilding,
                    effect: AbilityEffect::PhoenixFire(NativeBoltProfile {
                        ability: AbilityId(3),
                        damage: 1,
                        stun_ticks: 0,
                        hero_stun_ticks: 0,
                        damage_per_second: 1,
                        duration_ticks: 90,
                        speed_per_tick: SUBUNITS_PER_WORLD_UNIT,
                        cleanse: false,
                        targets: AttackTargetMask::ALL,
                    }),
                },
            },
        );
        let target = original.spawn_building(BuildingSpawn {
            team: Team(1),
            footprint: BuildingFootprint::new(5, 0, 1, 1),
            health: 100,
            production: None,
            attack: None,
            spellcasting: None,
        });
        assert_eq!(
            PresentationSnapshot::capture(&original).buildings[&target].status,
            StatusState::default()
        );
        original.step();
        let impact = original.projectiles()[0].impact_tick;
        while original.tick() <= impact {
            original.step();
        }
        let status = original.building(target).unwrap().status;
        assert_eq!(status.damage_over_time_count, 1);
        let content = castle_fight_sim::castle_fight_content_bundle(
            castle_fight_sim::CASTLE_FIGHT_DEFAULT_MAP_VERSION,
        )
        .unwrap();
        let wire = original.capture_snapshot().encode_wire().unwrap();
        let decoded = castle_fight_sim::SimulationSnapshot::decode_wire(&wire, content).unwrap();
        let mut restored = Simulation::new(SimulationConfig::default(), 4);
        restored.restore_snapshot(&decoded).unwrap();
        let expiry = status.damage_over_time[0].expires_tick;
        while original.tick() <= expiry {
            let before = original.checksum();
            let sample = PresentationSnapshot::capture(&original);
            assert_eq!(
                sample.buildings[&target].status,
                original.building(target).unwrap().status
            );
            assert_eq!(
                sample.buildings[&target].status,
                PresentationSnapshot::capture(&restored).buildings[&target].status
            );
            assert_eq!(original.checksum(), before);
            assert_eq!(original.step().checksum, restored.step().checksum);
        }
        assert_eq!(
            PresentationSnapshot::capture(&original).buildings[&target]
                .status
                .damage_over_time_count,
            0
        );
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
