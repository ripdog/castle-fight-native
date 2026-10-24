//! Ancient of Gale: a stationary Tornado body whose damage and slow auras are its whole effect.
use super::*;
use crate::{
    MapVersion,
    components::{NativeAction, TornadoDamage, TornadoProfile, TornadoState},
};

impl Simulation {
    /// `galeSpell`: a random living hostile combat body (no shield check) receives a dummy-cast
    /// Summon Tornado at its own position.
    pub(super) fn cast_tornado(
        &mut self,
        source: AbilitySourceSnapshot,
        ability: AbilityId,
        sequence: u64,
        (options, excluded_ability, excluded_type): ([TornadoProfile; 2], AbilityId, u32),
        units: &[UnitSnapshot],
    ) -> usize {
        let version = source.map_version.unwrap_or(MapVersion::CASTLE_FIGHT_9_27);
        let Some(target) = units
            .iter()
            .filter(|unit| {
                unit.team != source.team
                    && unit.health > 0
                    && unit.classifications.combat_sapper
                    && !unit.classifications.invulnerable
                    && unit.rawcode != excluded_type
                    && !crate::content::unit_has_ability_for_version(
                        unit.rawcode,
                        excluded_ability,
                        version,
                    )
            })
            .min_by_key(|unit| {
                (
                    deterministic_ability_target_rank(
                        self.config.match_seed,
                        source.id,
                        ability,
                        sequence,
                        unit.id,
                    ),
                    unit.id,
                )
            })
        else {
            return 0;
        };
        let profile = options[usize::from(!self.config.match_rules.artillery_enabled)];
        let id = self.allocate_id();
        self.world.spawn((
            id,
            NativeAction::Tornado(TornadoState {
                source: source.id,
                team: source.team,
                center: target.position,
                profile,
                // The auras strike one period after the summon.
                next_pulse_tick: self.next_tick + u64::from(profile.pulse_ticks),
                expires_tick: self.next_tick + u64::from(profile.duration_ticks),
            }),
        ));
        1
    }

    pub(super) fn advance_tornado(
        &mut self,
        entity: Entity,
        mut state: TornadoState,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        let tick = self.next_tick;
        if tick >= state.expires_tick {
            self.world.despawn(entity);
            return;
        }
        self.apply_tornado_slow(&state, units);
        if tick >= state.next_pulse_tick {
            let profile = state.profile;
            for aura in [Some(profile.unit_damage), profile.structure_damage]
                .into_iter()
                .flatten()
            {
                self.apply_tornado_damage(&state, aura, units, buildings);
            }
            death_healing::refresh_support(units, buildings, None);
            state.next_pulse_tick += u64::from(profile.pulse_ticks);
        }
        if let Some(mut action) = self.world.get_mut::<NativeAction>(entity) {
            *action = NativeAction::Tornado(state);
        }
    }

    /// `Aasl`: hostile combat bodies inside the radius are slowed for as long as they stay in it.
    fn apply_tornado_slow(&self, state: &TornadoState, units: &mut [UnitSnapshot]) {
        let slow = state.profile.slow;
        super::wisp::apply_slow_aura(
            self.next_tick,
            state.center,
            state.team,
            (
                slow.ability,
                slow.buff,
                slow.radius,
                slow.movement_percent_delta,
                slow.attack_speed_percent_delta,
                slow.targets,
                slow.affects_invulnerable,
            ),
            units,
        );
    }

    /// `Atdg`: one second of damage to every hostile body (and structure) the aura selects. The
    /// auras pierce spell immunity, so only the invulnerable marker can protect a target.
    fn apply_tornado_damage(
        &mut self,
        state: &TornadoState,
        aura: TornadoDamage,
        units: &mut [UnitSnapshot],
        buildings: &mut [BuildingSnapshot],
    ) {
        let radius_sq = square_i32(aura.radius);
        let amount = aura.damage_per_second;
        for index in 0..units.len() {
            let unit = &units[index];
            if unit.team == state.team
                || unit.health <= 0
                || unit.classifications.locust
                || (unit.classifications.invulnerable && !aura.affects_invulnerable)
                || !((unit.classifications.combat_sapper
                    && aura
                        .combat_bodies
                        .can_target_unit(unit.target_movement_class()))
                    || (aura.mechanical_units && unit.mechanical))
                || state.center.distance_sq(unit.position) > radius_sq
            {
                continue;
            }
            self.apply_source_ability_to_unit(
                state.source,
                state.team,
                index,
                AbilityEffect::Damage { amount },
                units,
            );
        }
        if !aura.structures || self.debug_buildings_invulnerable {
            return;
        }
        for building in buildings.iter_mut() {
            if building.team == state.team
                || building.health <= 0
                || (building.classifications.invulnerable && !aura.affects_invulnerable)
                || state.center.distance_sq(footprint_center_point(
                    building.footprint,
                    self.config.navigation_cell_size,
                )) > radius_sq
            {
                continue;
            }
            let damage = self
                .combat_rules
                .damage_rules
                .apply_spell(amount, building.armor.armor_type);
            building.health = building.health.saturating_sub(scale_damage_per_10k(
                damage,
                building.snow_damage_taken_per_10k,
            ));
        }
    }
}
