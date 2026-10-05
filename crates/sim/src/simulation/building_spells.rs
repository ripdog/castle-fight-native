//! Targeted negative building spells: native Hex and script-authored shield/control state.
use super::*;
use crate::{
    MapVersion,
    building_mechanics::{
        HexEffectProfile, HexFormProfile, markers_for_version, shield_armor_for_version,
    },
};
use bevy_ecs::prelude::Component;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HexState {
    pub expires_tick: u64,
    pub form: HexFormProfile,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum ControlAction {
    Attack,
    DefenderDefend,
    DefenderAttack,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct ControlCallback {
    pub due_tick: u64,
    pub action: ControlAction,
    pub profile: HexEffectProfile,
}

/// A dedicated autonomous entity (canonical tag 12), not a replacement for the logical unit.
#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct BuildingSpellTargetState {
    pub target: SimId,
    pub version: MapVersion,
    pub shield_level: u8,
    pub shield_expires_tick: Option<u64>,
    pub anti_negative: bool,
    pub selector_excluded: bool,
    pub overheat_level: u8,
    pub hex: Option<HexState>,
    pub defend_disabled: bool,
    pub orders_suspended: bool,
    pub callbacks: Vec<ControlCallback>,
}

/// Rebuildable cache. Only BuildingSpellTargetState is canonical.
#[derive(Component, Debug, Clone, Copy, Default)]
pub(super) struct BuildingSpellControl {
    pub version: Option<MapVersion>,
    pub hex: Option<HexState>,
    pub shield_level: u8,
    pub overheat_level: u8,
    pub defend_disabled: bool,
    pub orders_suspended: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingSpellVisualKind {
    ShieldConsumed,
    ShieldBlocked,
    OverheatIncreased,
    HexTransform,
    HexRestore,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BuildingSpellVisualEvent {
    pub target: SimId,
    pub position: SimPoint,
    pub kind: BuildingSpellVisualKind,
}

impl Simulation {
    fn building_spell_target_version(&self, entity: Entity) -> MapVersion {
        self.world
            .entity(entity)
            .get::<ContentIdentity>()
            .map_or(crate::CASTLE_FIGHT_DEFAULT_MAP_VERSION, |content| {
                content.map_version
            })
    }

    fn building_spell_state_entity(
        &mut self,
        target: SimId,
        unit_entity: Entity,
        version: MapVersion,
    ) -> Entity {
        if let Some(content) = self.world.entity(unit_entity).get::<ContentIdentity>() {
            assert_eq!(
                content.map_version, version,
                "building spell and target content versions must agree"
            );
        }
        if let Some(entity) = self.world.iter_entities().find_map(|entity| {
            entity
                .get::<BuildingSpellTargetState>()
                .filter(|state| state.target == target)
                .map(|state| {
                    assert_eq!(
                        state.version, version,
                        "building spell control cannot change version"
                    );
                    entity.id()
                })
        }) {
            return entity;
        }
        let code = self
            .world
            .entity(unit_entity)
            .get::<ContentIdentity>()
            .map_or(0, |content| content.rawcode);
        let (anti_negative, selector_excluded) = markers_for_version(code, version);
        let id = self.allocate_id();
        self.world
            .spawn((
                id,
                BuildingSpellTargetState {
                    target,
                    version,
                    shield_level: 0,
                    shield_expires_tick: None,
                    anti_negative,
                    selector_excluded,
                    overheat_level: if code
                        == crate::building_mechanics::overheat_shield_for_version(version).rawcode
                    {
                        crate::building_mechanics::overheat_shield_for_version(version)
                            .initial_level
                    } else {
                        0
                    },
                    hex: None,
                    defend_disabled: false,
                    orders_suspended: false,
                    callbacks: Vec::new(),
                },
            ))
            .id()
    }

    /// Engine setup/effect API for the source's A09L levels; expiration is exclusive.
    /// A level-two interception heals to maximum life and leaves level one.
    pub fn set_negative_building_shield(
        &mut self,
        target: SimId,
        level: u8,
        expires_tick: Option<u64>,
    ) -> bool {
        assert!(level <= 2);
        let Some(unit_entity) = self.world.iter_entities().find_map(|e| {
            (e.get::<SimId>() == Some(&target) && e.contains::<MovementProfile>()).then_some(e.id())
        }) else {
            return false;
        };
        let version = self.building_spell_target_version(unit_entity);
        let entity = self.building_spell_state_entity(target, unit_entity, version);
        let mut state = self.world.entity_mut(entity);
        let mut state = state.get_mut::<BuildingSpellTargetState>().unwrap();
        state.shield_level = level;
        state.shield_expires_tick = expires_tick;
        refresh_building_spell_controls(&mut self.world, self.next_tick);
        true
    }

    /// Remove the active A09L shield without triggering interception or its heal.
    pub fn dispel_negative_building_shield(&mut self, target: SimId) -> bool {
        let Some(entity) = self.world.iter_entities().find_map(|e| {
            e.get::<BuildingSpellTargetState>()
                .filter(|s| s.target == target && s.shield_level > 0)
                .map(|_| e.id())
        }) else {
            return false;
        };
        let mut entity = self.world.entity_mut(entity);
        let mut state = entity.get_mut::<BuildingSpellTargetState>().unwrap();
        state.shield_level = 0;
        state.shield_expires_tick = None;
        refresh_building_spell_controls(&mut self.world, self.next_tick);
        true
    }

    /// Explicit marker API for synthetic/content producers with hidden A070/A08H-like flags.
    pub fn set_negative_building_markers(
        &mut self,
        target: SimId,
        anti_negative: bool,
        selector_excluded: bool,
    ) -> bool {
        let Some(unit_entity) = self.world.iter_entities().find_map(|e| {
            (e.get::<SimId>() == Some(&target) && e.contains::<MovementProfile>()).then_some(e.id())
        }) else {
            return false;
        };
        let version = self.building_spell_target_version(unit_entity);
        let entity = self.building_spell_state_entity(target, unit_entity, version);
        let mut entity = self.world.entity_mut(entity);
        let mut state = entity.get_mut::<BuildingSpellTargetState>().unwrap();
        state.anti_negative = anti_negative;
        state.selector_excluded = selector_excluded;
        true
    }

    /// Dispelling native Hex does not cancel independent script callbacks.
    pub fn dispel_building_hex(&mut self, target: SimId) -> bool {
        let Some(entity) = self.world.iter_entities().find_map(|e| {
            e.get::<BuildingSpellTargetState>()
                .filter(|s| s.target == target && s.hex.is_some())
                .map(|_| e.id())
        }) else {
            return false;
        };
        self.world
            .entity_mut(entity)
            .get_mut::<BuildingSpellTargetState>()
            .unwrap()
            .hex = None;
        refresh_building_spell_controls(&mut self.world, self.next_tick);
        true
    }

    /// Native buff removal and the retained ability-removal list do not cancel
    /// script callbacks, order recovery, Defend restoration, or unrelated grants.
    pub(super) fn cleanse_building_spell_controls(
        &mut self,
        target: SimId,
        position: SimPoint,
        removed_abilities: &[AbilityId],
    ) {
        let Some(entity) = self.world.iter_entities().find_map(|entity| {
            entity
                .get::<BuildingSpellTargetState>()
                .filter(|state| state.target == target)
                .map(|_| entity.id())
        }) else {
            return;
        };
        let (hex_removed, shield_removed) = {
            let mut state = self
                .world
                .get_mut::<BuildingSpellTargetState>(entity)
                .unwrap();
            let hex_removed = state.hex.take().is_some();
            let remove_shield =
                removed_abilities.contains(&AbilityId(u32::from_be_bytes(*b"A09L")));
            let shield_removed =
                remove_shield && (state.shield_level > 0 || state.shield_expires_tick.is_some());
            if remove_shield {
                state.shield_level = 0;
                state.shield_expires_tick = None;
            }
            (hex_removed, shield_removed)
        };
        if hex_removed {
            self.last_building_spell_visuals
                .push(BuildingSpellVisualEvent {
                    target,
                    position,
                    kind: BuildingSpellVisualKind::HexRestore,
                });
        }
        if hex_removed || shield_removed {
            refresh_building_spell_controls(&mut self.world, self.next_tick);
        }
    }

    pub(super) fn hex_trigger_eligible(&self, unit: &UnitSnapshot, version: MapVersion) -> bool {
        if !unit.classifications.combat_sapper || unit.classifications.invulnerable {
            return false;
        }
        let code = self
            .world
            .entity(unit.entity)
            .get::<ContentIdentity>()
            .map_or(0, |c| c.rawcode);
        if markers_for_version(code, version).1 {
            return false;
        }
        !self.world.iter_entities().any(|e| {
            e.get::<BuildingSpellTargetState>()
                .is_some_and(|s| s.target == unit.id && s.selector_excluded)
        })
    }

    pub(super) fn resolve_building_hex(
        &mut self,
        target: &mut UnitSnapshot,
        profile: HexEffectProfile,
        caster: SimId,
        cast_sequence: u64,
    ) -> bool {
        let entity =
            self.building_spell_state_entity(target.id, target.entity, profile.map_version);
        let mut state = self
            .world
            .entity(entity)
            .get::<BuildingSpellTargetState>()
            .unwrap()
            .clone();
        let blocked;
        if state.shield_level > 0 {
            if state.shield_level == 2 {
                target.health = target.health_max;
            }
            state.shield_level -= 1;
            blocked = true;
            self.last_building_spell_visuals
                .push(BuildingSpellVisualEvent {
                    target: target.id,
                    position: target.position,
                    kind: BuildingSpellVisualKind::ShieldConsumed,
                });
        } else if state.overheat_level > 0 {
            // Source precedence: active shield, then Shredder's roll, then A070.
            // A failed Shredder roll returns false; it does NOT fall through to A070.
            let overheat = crate::building_mechanics::overheat_shield_for_version(state.version);
            let roll = deterministic_random(
                self.config.match_seed,
                self.next_tick,
                caster,
                u64::from(overheat.attack_speed_ability.0),
                cast_sequence ^ target.id.0,
            ) % 100;
            blocked = roll < u64::from(overheat.chance_percent);
            if blocked && state.overheat_level < overheat.maximum_level {
                state.overheat_level += 1;
                self.last_building_spell_visuals
                    .push(BuildingSpellVisualEvent {
                        target: target.id,
                        position: target.position,
                        kind: BuildingSpellVisualKind::OverheatIncreased,
                    });
            }
        } else {
            blocked = state.anti_negative;
        }
        if blocked {
            self.last_building_spell_visuals
                .push(BuildingSpellVisualEvent {
                    target: target.id,
                    position: target.position,
                    kind: BuildingSpellVisualKind::ShieldBlocked,
                });
        } else {
            // Native immunity failure is separate from script selection/shield interception.
            // The recovered level-one target mask has no organic restriction: mechanical units
            // are not rejected just because the unmodified AOhx base mask includes organic.
            if !target.classifications.spell_immune {
                let duration = if target.classifications.hero {
                    profile.hero_duration_ticks
                } else {
                    profile.duration_ticks
                };
                let form = if target.movement_class == MovementClass::Air {
                    profile.air
                } else {
                    profile.ground
                };
                state.hex = Some(HexState {
                    expires_tick: self.next_tick + u64::from(duration),
                    form,
                });
                state.orders_suspended = true;
                // Native morph suspends the toggled Defend ability. An attack order alone
                // is NOT an undefend order, particularly if the native Hex failed.
                if self
                    .world
                    .entity(target.entity)
                    .get::<ContentIdentity>()
                    .is_some_and(|c| c.rawcode == profile.defender_rawcode)
                {
                    state.defend_disabled = true;
                }
                self.last_building_spell_visuals
                    .push(BuildingSpellVisualEvent {
                        target: target.id,
                        position: target.position,
                        kind: BuildingSpellVisualKind::HexTransform,
                    });
            }
            state.callbacks.push(ControlCallback {
                due_tick: self.next_tick + u64::from(profile.initial_reengage_ticks),
                action: ControlAction::Attack,
                profile,
            });
            state.callbacks.sort_by_key(|c| c.due_tick);
        }
        self.world.entity_mut(entity).insert(state);
        refresh_building_spell_controls(&mut self.world, self.next_tick);
        self.project_building_spell_control(target);
        !blocked && !target.classifications.spell_immune
    }

    pub(super) fn advance_building_spell_controls(&mut self) {
        let units: BTreeMap<_, _> = self
            .world
            .iter_entities()
            .filter_map(|e| Some((*e.get::<SimId>()?, (e.id(), e.get::<Health>()?.current))))
            .collect();
        let mut states: Vec<_> = self
            .world
            .iter_entities()
            .filter_map(|e| {
                Some((
                    *e.get::<SimId>()?,
                    e.id(),
                    e.get::<BuildingSpellTargetState>()?.clone(),
                ))
            })
            .collect();
        states.sort_by_key(|(id, _, _)| *id);
        for (_, entity, mut state) in states {
            let Some(&(unit_entity, health)) = units.get(&state.target) else {
                self.world.despawn(entity);
                continue;
            };
            if health <= 0 {
                self.world.despawn(entity);
                continue;
            }
            if state
                .shield_expires_tick
                .is_some_and(|t| self.next_tick >= t)
            {
                state.shield_level = 0;
                state.shield_expires_tick = None;
            }
            if state.hex.is_some_and(|h| self.next_tick >= h.expires_tick) {
                state.hex = None;
                if let Some(position) = self.world.entity(unit_entity).get::<Position>() {
                    self.last_building_spell_visuals
                        .push(BuildingSpellVisualEvent {
                            target: state.target,
                            position: position.0,
                            kind: BuildingSpellVisualKind::HexRestore,
                        });
                }
            }
            let pending = std::mem::take(&mut state.callbacks);
            for callback in pending {
                if callback.due_tick > self.next_tick {
                    state.callbacks.push(callback);
                    continue;
                }
                match callback.action {
                    ControlAction::Attack => {
                        state.orders_suspended = false;
                        self.world
                            .entity_mut(unit_entity)
                            .insert(TargetState::default());
                        // Logical native type identity remains original through Hex. Do not test
                        // the visual critter rawcode, or Defender's branch would disappear.
                        if self
                            .world
                            .entity(unit_entity)
                            .get::<ContentIdentity>()
                            .is_some_and(|c| c.rawcode == callback.profile.defender_rawcode)
                        {
                            state.callbacks.push(ControlCallback {
                                due_tick: callback.due_tick
                                    + u64::from(callback.profile.defender_restore_ticks),
                                action: ControlAction::DefenderDefend,
                                ..callback
                            });
                        }
                    }
                    ControlAction::DefenderDefend => {
                        // An immediate Defend order cannot enable an ability while Hex
                        // still disables it (e.g. overlapping casts).
                        if state.hex.is_none() {
                            state.defend_disabled = false;
                        }
                        state.orders_suspended = true;
                        state.callbacks.push(ControlCallback {
                            due_tick: callback.due_tick
                                + u64::from(callback.profile.defender_resume_ticks),
                            action: ControlAction::DefenderAttack,
                            ..callback
                        });
                    }
                    ControlAction::DefenderAttack => {
                        state.orders_suspended = false;
                        self.world
                            .entity_mut(unit_entity)
                            .insert(TargetState::default());
                    }
                }
            }
            state.callbacks.sort_by_key(|c| c.due_tick);
            self.world.entity_mut(entity).insert(state);
        }
        refresh_building_spell_controls(&mut self.world, self.next_tick);
    }

    pub(super) fn project_building_spell_control(&self, unit: &mut UnitSnapshot) {
        let entity = self.world.entity(unit.entity);
        // Read immutable baselines, never capture a previously morphed projection.
        unit.armor = *entity.get::<ArmorProfile>().unwrap();
        unit.movement = *entity.get::<MovementProfile>().unwrap();
        unit.passive_effects = *entity.get::<PassiveUnitEffects>().unwrap();
        unit.collision_radius_override = entity.get::<CollisionRadius>().map(|r| r.0);
        unit.collision_radius = unit
            .collision_radius_override
            .unwrap_or_else(|| self.default_collision_radius());
        unit.attacks_disabled = false;
        unit.abilities_disabled = false;
        unit.orders_suspended = self.next_tick < unit.status.order_recovery_until_tick;
        let Some(control) = entity.get::<BuildingSpellControl>() else {
            return;
        };
        let version = control
            .version
            .expect("projected building spell control retains its version");
        unit.orders_suspended |= control.orders_suspended;
        if control.overheat_level > 0 {
            let profile = crate::building_mechanics::overheat_shield_for_version(version);
            apply_timed_attack_speed_modifier(
                &mut unit.status,
                ModifierId(profile.attack_speed_ability.0),
                profile.attack_speed_percent[usize::from(control.overheat_level - 1)],
                u64::MAX,
            );
        }
        unit.armor.armor_points += shield_armor_for_version(control.shield_level, version);
        if control.defend_disabled {
            unit.passive_effects = unit.passive_effects.without_defend();
        }
        if let Some(hex) = control.hex {
            unit.armor = hex.form.armor;
            unit.movement.speed_per_tick = hex.form.speed_per_tick;
            unit.collision_radius = hex.form.collision_radius;
            unit.collision_radius_override = Some(hex.form.collision_radius);
            unit.passive_effects = PassiveUnitEffects::EMPTY;
            unit.attacks_disabled = true;
            unit.abilities_disabled = true;
            unit.target = None;
            unit.direct_retaliation_lock = false;
            unit.ally_defense_lock = false;
        }
    }
}

impl Simulation {
    /// Native ground/structure death splash from the source shield's retained overheat level.
    /// Every dead source resolves once; cascading explosions are bounded by the unit count.
    pub(super) fn resolve_overheat_deaths(
        &self,
        units: &[UnitSnapshot],
        buildings: &[BuildingSnapshot],
        positions: &[SimPoint],
        unit_health: &mut [i32],
        building_health: &mut [i32],
    ) {
        let mut resolved = vec![false; units.len()];
        loop {
            let mut changed = false;
            for (index, unit) in units.iter().enumerate() {
                if resolved[index] || unit_health[index] > 0 {
                    continue;
                }
                resolved[index] = true;
                let Some(control) = self.world.entity(unit.entity).get::<BuildingSpellControl>()
                else {
                    continue;
                };
                let level = control.overheat_level;
                if level == 0 || unit.abilities_disabled {
                    continue;
                }
                changed = true;
                let version = control
                    .version
                    .expect("projected overheat control retains its version");
                let profile = crate::building_mechanics::overheat_shield_for_version(version)
                    .explosions[usize::from(level - 1)];
                let damage_at = |distance: u64| {
                    if distance <= square_i32(profile.full_radius) {
                        profile.full_damage
                    } else if distance <= square_i32(profile.partial_radius) {
                        profile.partial_damage
                    } else {
                        0
                    }
                };
                for (target_index, target) in units.iter().enumerate() {
                    if unit_health[target_index] <= 0
                        || target.team == unit.team
                        || target.movement_class != MovementClass::Ground
                        || target.classifications.spell_immune
                    {
                        continue;
                    }
                    let damage = self.combat_rules.damage_rules.apply_spell(
                        damage_at(positions[index].distance_sq(positions[target_index])),
                        target.armor.armor_type,
                    );
                    let damage = spell_damage_after_defend(*target, damage, self.next_tick);
                    unit_health[target_index] -= damage;
                }
                for (target_index, target) in buildings.iter().enumerate() {
                    if building_health[target_index] <= 0 || target.team == unit.team {
                        continue;
                    }
                    let distance = point_to_footprint_distance_sq(
                        positions[index],
                        target.footprint,
                        self.config.navigation_cell_size,
                    );
                    building_health[target_index] -= self
                        .combat_rules
                        .damage_rules
                        .apply_spell(damage_at(distance), target.armor.armor_type);
                }
            }
            if !changed {
                break;
            }
        }
    }
}

pub(super) fn refresh_building_spell_controls(world: &mut World, tick: u64) {
    let mut controls: BTreeMap<SimId, BuildingSpellControl> = world
        .iter_entities()
        .filter_map(|e| {
            let state = e.get::<BuildingSpellTargetState>()?;
            Some((
                state.target,
                BuildingSpellControl {
                    version: Some(state.version),
                    hex: state.hex.filter(|h| tick < h.expires_tick),
                    shield_level: if state.shield_expires_tick.is_some_and(|t| tick >= t) {
                        0
                    } else {
                        state.shield_level
                    },
                    overheat_level: state.overheat_level,
                    defend_disabled: state.defend_disabled,
                    orders_suspended: state.orders_suspended,
                },
            ))
        })
        .collect();
    let units: Vec<_> = world
        .iter_entities()
        .filter_map(|e| {
            e.get::<SimId>()
                .filter(|_| e.contains::<MovementProfile>())
                .map(|id| (*id, e.id()))
        })
        .collect();
    for (id, entity) in units {
        if let Some(control) = controls.remove(&id) {
            world.entity_mut(entity).insert(control);
        } else {
            world.entity_mut(entity).remove::<BuildingSpellControl>();
        }
    }
}
