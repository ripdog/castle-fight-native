//! Source-owned array timers and swap-removal order; deterministic fixed-point motion.
use super::*;
use crate::components::{
    MasterFan, MasterMinion, MasterMover, MasterMoverProfile, MasterParasite,
    MasterProjectileProfile, MasterProjectileState, NativeAction,
};

const DIRECTION_SCALE: i64 = 1_000_000;
const RANDOM_BRANCH: u64 = 0x6d61_7374_6572_6272;
const RANDOM_FAN: u64 = 0x6d61_7374_6572_6661;

fn normalize(p: SimPoint) -> SimPoint {
    let x = i64::from(p.x);
    let y = i64::from(p.y);
    let length = ((i128::from(x) * i128::from(x) + i128::from(y) * i128::from(y)) as u128)
        .isqrt()
        .max(1) as i64;
    if x == 0 && y == 0 {
        return SimPoint::new(DIRECTION_SCALE as i32, 0);
    }
    SimPoint::new(
        (x * DIRECTION_SCALE / length) as i32,
        (y * DIRECTION_SCALE / length) as i32,
    )
}
/// Integer CORDIC: millidegree fan RNG never introduces platform floating point.
fn rotate(p: SimPoint, angle: i32) -> SimPoint {
    const ATAN: [i32; 20] = [
        45_000_000, 26_565_051, 14_036_243, 7_125_016, 3_576_334, 1_789_911, 895_174, 447_614,
        223_811, 111_906, 55_953, 27_976, 13_988, 6_994, 3_497, 1_749, 874, 437, 219, 109,
    ];
    let mut z = angle.rem_euclid(360_000) * 1000;
    if z > 180_000_000 {
        z -= 360_000_000;
    }
    let mut sign = 1_i64;
    if z > 90_000_000 {
        z -= 180_000_000;
        sign = -1;
    } else if z < -90_000_000 {
        z += 180_000_000;
        sign = -1;
    }
    let mut x = 607_252_935_i64;
    let mut y = 0_i64;
    for (index, atan) in ATAN.into_iter().enumerate() {
        let direction = if z >= 0 { 1 } else { -1 };
        let next_x = x - direction * (y >> index);
        y += direction * (x >> index);
        x = next_x;
        z -= direction as i32 * atan;
    }
    normalize(SimPoint::new(
        ((i64::from(p.x) * x - i64::from(p.y) * y) * sign / 1_000_000_000) as i32,
        ((i64::from(p.x) * y + i64::from(p.y) * x) * sign / 1_000_000_000) as i32,
    ))
}
fn offset(origin: SimPoint, direction: SimPoint, distance: i32) -> SimPoint {
    SimPoint::new(
        origin.x + (i64::from(direction.x) * i64::from(distance) / DIRECTION_SCALE) as i32,
        origin.y + (i64::from(direction.y) * i64::from(distance) / DIRECTION_SCALE) as i32,
    )
}
fn mover(
    source: &MasterFan,
    id: SimId,
    position: SimPoint,
    direction: SimPoint,
    budget: i32,
    splits: u16,
    tick: u64,
) -> MasterMover {
    MasterMover {
        id,
        source: source.source,
        owner: source.owner,
        team: source.team,
        position_fractional: [
            i64::from(position.x) * DIRECTION_SCALE,
            i64::from(position.y) * DIRECTION_SCALE,
        ],
        direction,
        damage: source.damage,
        budget,
        splits,
        stage: 0,
        last_hit: None,
        last_original_lane: false,
        born_tick: tick,
    }
}
fn advance(m: &mut MasterMover, p: MasterMoverProfile, bounds: [i32; 4]) -> bool {
    m.position_fractional[0] += i64::from(m.direction.x) * i64::from(p.step);
    m.position_fractional[1] += i64::from(m.direction.y) * i64::from(p.step);
    let point = m.position();
    point.x >= bounds[0] && point.y >= bounds[1] && point.x <= bounds[2] && point.y <= bounds[3]
}
fn direct_damage(
    units: &mut [UnitSnapshot],
    index: usize,
    damage: i32,
    rules: DamageRules,
    tick: u64,
) {
    let unit = &mut units[index];
    if unit.classifications.invulnerable || unit.classifications.spell_immune {
        return;
    }
    let damage = spell_damage_after_defend(
        *unit,
        rules.apply_spell(damage, unit.armor.armor_type),
        tick,
    );
    damage_thresholds::observe_damage_event(unit, unit.health, damage);
    let after = unit.health.saturating_sub(damage);
    death_healing::commit_health_from(units, index, after, None);
}

impl Simulation {
    fn master_state(&self) -> Option<(Entity, MasterProjectileState)> {
        self.world
            .iter_entities()
            .find_map(|entity| match entity.get::<NativeAction>()? {
                NativeAction::MasterProjectiles(state) => Some((entity.id(), state.clone())),
                _ => None,
            })
    }
    pub(super) fn start_master_projectiles(
        &mut self,
        source: AbilitySourceSnapshot,
        profile: MasterProjectileProfile,
        target_index: usize,
        cast_sequence: u64,
        units: &mut [UnitSnapshot],
        buildings: &[BuildingSnapshot],
    ) {
        let AbilitySourceIndex::Unit(index) = source.source else {
            unreachable!("Master requires a combat body");
        };
        let version = source.map_version.expect("versioned Master profile");
        let owner = units[index].owner;
        let origin = units[index].position;
        let direction = normalize(units[index].status.facing_direction);
        let mut counts = [0_u16; 5];
        for building in buildings.iter().filter(|b| b.health > 0) {
            if self.world.get::<Owner>(building.entity) != Some(&Owner(owner)) {
                continue;
            }
            let content = if let Some(construction) =
                self.world.get::<BuildingConstruction>(building.entity)
            {
                let Some(previous) = construction.upgrade_from else {
                    continue;
                };
                previous.properties.content
            } else {
                self.world.get::<ContentIdentity>(building.entity).copied()
            };
            let Some(content) = content.filter(|c| c.map_version == version) else {
                continue;
            };
            for (i, bucket) in profile.buildings.iter().enumerate() {
                if bucket.contains(&content.rawcode) {
                    counts[i] = counts[i].saturating_add(1);
                }
            }
        }
        let stored = self.master_state();
        let (entity, mut state) = stored.unwrap_or_else(|| {
            let id = self.allocate_id();
            let entity = self.world.spawn((id,)).id();
            (
                entity,
                MasterProjectileState {
                    profile: Box::new(profile),
                    map_version: version,
                    lightning: Vec::new(),
                    frost: Vec::new(),
                    fans: Vec::new(),
                    parasites: Vec::new(),
                    minions: Vec::new(),
                    lightning_due_millis: 0,
                    frost_due_millis: 0,
                },
            )
        });
        assert_eq!(
            *state.profile, profile,
            "one registered content version per native projectile system"
        );
        // Primary native Parasite is independent of its SPELL_EFFECT script handler.
        if !units[target_index].classifications.hero
            && !units[target_index].classifications.invulnerable
            && !units[target_index].classifications.spell_immune
        {
            state.parasites.push(MasterParasite {
                source: source.id,
                owner,
                team: source.team,
                target: units[target_index].id,
                position: origin,
                buff_until_tick: None,
            });
        }
        let now = self.next_tick * 1000 / CASTLE_FIGHT_SIMULATION_HZ as u64;
        let lightning = deterministic_random(
            self.config.match_seed,
            self.next_tick,
            source.id,
            RANDOM_BRANCH,
            cast_sequence,
        ) % u64::from(profile.branch_roll_size)
            < u64::from(profile.lightning_roll_below);
        if let Some(action) = units[index].status.action_animation.as_mut() {
            action.kind = if lightning {
                ActionAnimationKind::CastSlam
            } else {
                ActionAnimationKind::CastThrow
            };
            action.started_tick = self.next_tick;
        }
        let mut fan = MasterFan {
            source: source.id,
            owner,
            team: source.team,
            direction,
            damage: profile.damage
                * i32::from((1 + counts[usize::from(!lightning)]).min(profile.damage_count_cap)),
            remaining: 0,
            sequence: cast_sequence,
            due_millis: now,
        };
        if lightning {
            let count =
                profile.lightning_initial_count + counts[2] / profile.lightning_count_divisor;
            let splits = profile.split_initial_count + counts[4] / profile.split_count_divisor;
            let perpendicular = SimPoint::new(-direction.y, direction.x);
            let base = offset(origin, direction, profile.forward_offset);
            let mut lateral = -profile.lane_spacing * i32::from(count / 2)
                + profile.lane_spacing * i32::from((count + 1) % 2) / 2;
            if state.lightning.is_empty() {
                state.lightning_due_millis = now + u64::from(profile.lightning.interval_millis);
            }
            for lane in 0..count {
                let id = self.allocate_id();
                let mut m = mover(
                    &fan,
                    id,
                    offset(base, perpendicular, lateral),
                    direction,
                    profile.lightning.budget,
                    splits,
                    self.next_tick,
                );
                m.last_original_lane = lane + 1 == count;
                state.lightning.push(m);
                lateral += profile.lane_spacing;
            }
        } else {
            fan.remaining = u32::from(profile.frost_initial_count)
                + u32::from(counts[3]) * u32::from(profile.frost_count_factor);
            self.spawn_master_fan_step(&mut state, fan, origin, now);
        }
        self.world
            .entity_mut(entity)
            .insert(NativeAction::MasterProjectiles(state));
    }
    fn spawn_master_fan_step(
        &mut self,
        state: &mut MasterProjectileState,
        mut fan: MasterFan,
        origin: SimPoint,
        now: u64,
    ) {
        if fan.remaining == 0 {
            return;
        }
        let p = *state.profile;
        let spread = p.fan_spread_millidegrees;
        let random = deterministic_random(
            self.config.match_seed,
            fan.due_millis,
            fan.source,
            RANDOM_FAN,
            fan.sequence,
        );
        let angle = (random % (2 * spread as u64 + 1)) as i32 - spread;
        let direction = rotate(fan.direction, angle);
        if state.frost.is_empty() {
            state.frost_due_millis = now + u64::from(p.frost.interval_millis);
        }
        let id = self.allocate_id();
        state.frost.push(mover(
            &fan,
            id,
            offset(origin, direction, -p.frost.step),
            direction,
            p.frost.budget,
            0,
            self.next_tick,
        ));
        fan.remaining -= 1;
        if fan.remaining > 0 {
            fan.sequence += 1;
            fan.due_millis = now + u64::from(p.fan_interval_millis);
            state.fans.push(fan);
        }
    }
    pub(super) fn advance_master_projectiles(
        &mut self,
        entity: Entity,
        mut state: MasterProjectileState,
        units: &mut [UnitSnapshot],
    ) {
        let now = self.next_tick * 1000 / CASTLE_FIGHT_SIMULATION_HZ as u64;
        let bounds =
            crate::building_mechanics::snowveil_for_version(state.map_version).battlefield_bounds;
        loop {
            let fan_due = state
                .fans
                .iter()
                .map(|f| f.due_millis)
                .min()
                .unwrap_or(u64::MAX);
            let lightning_due = if state.lightning.is_empty() {
                u64::MAX
            } else {
                state.lightning_due_millis
            };
            let frost_due = if state.frost.is_empty() {
                u64::MAX
            } else {
                state.frost_due_millis
            };
            let due = fan_due.min(lightning_due).min(frost_due);
            if due > now {
                break;
            }
            if due == fan_due {
                let index = state.fans.iter().position(|f| f.due_millis == due).unwrap();
                let fan = state.fans.remove(index);
                if let Some(index) =
                    find_unit_index(units, fan.source).filter(|&i| units[i].health > 0)
                {
                    self.spawn_master_fan_step(&mut state, fan, units[index].position, due);
                }
            } else if due == lightning_due {
                self.advance_master_lightning(&mut state, units, bounds);
                state.lightning_due_millis += u64::from(state.profile.lightning.interval_millis);
            } else {
                self.advance_master_frost(&mut state, units, bounds);
                state.frost_due_millis += u64::from(state.profile.frost.interval_millis);
            }
        }
        state.minions.retain(|m| self.next_tick < m.expires_tick);
        state.parasites.retain_mut(|p| {
            let Some(index) = find_unit_index(units, p.target) else {
                return false;
            };
            if let Some(until) = p.buff_until_tick {
                return self.next_tick < until;
            }
            if units[index].health <= 0 {
                return false;
            }
            let destination = units[index].position;
            let distance = p.position.distance_sq(destination);
            p.position = p
                .position
                .step_towards(destination, state.profile.parasite_speed);
            if distance <= square_i32(state.profile.parasite_speed) {
                if units[index].classifications.spell_immune
                    || units[index].classifications.invulnerable
                {
                    return false;
                }
                p.buff_until_tick =
                    Some(self.next_tick + u64::from(state.profile.parasite_duration_ticks));
            }
            true
        });
        self.world
            .entity_mut(entity)
            .insert(NativeAction::MasterProjectiles(state));
    }
    fn advance_master_lightning(
        &mut self,
        state: &mut MasterProjectileState,
        units: &mut [UnitSnapshot],
        bounds: [i32; 4],
    ) {
        let p = *state.profile;
        let mut index = 0;
        while index < state.lightning.len() {
            let m = &mut state.lightning[index];
            if !advance(m, p.lightning, bounds) {
                state.lightning.swap_remove(index);
                continue;
            }
            if let Some(target) = units.iter().position(|u| {
                u.health > 0
                    && u.classifications.combat_sapper
                    && u.team != m.team
                    && Some(u.id) != m.last_hit
                    && m.position().distance_sq(u.position) <= square_i32(p.lightning.radius)
            }) {
                direct_damage(
                    units,
                    target,
                    m.damage,
                    self.combat_rules.damage_rules,
                    self.next_tick,
                );
                m.last_hit = Some(units[target].id);
                self.last_ability_casts.push(AbilityCastEvent {
                    source: m.source,
                    ability: AbilityId(p.lightning.rawcode),
                    target: AbilityCastTarget::Unit(units[target].id),
                    target_position: Some(units[target].position),
                    effect: AbilityEffect::Damage { amount: m.damage },
                });
            }
            m.budget -= p.lightning.decrement;
            if m.budget > 0 {
                index += 1;
                continue;
            }
            if m.stage == 0 {
                if m.splits == 0 {
                    state.lightning.swap_remove(index);
                    continue;
                }
                m.splits -= 1;
                m.stage = 1;
                m.budget = p.stub_budget;
                let original = m.direction;
                m.direction = rotate(original, -p.split_angle_millidegrees);
                let mut child = m.clone();
                child.id = self.allocate_id();
                child.direction = rotate(original, p.split_angle_millidegrees);
                child.stage = -1;
                child.last_hit = None;
                child.last_original_lane = false;
                child.born_tick = self.next_tick;
                state.lightning.push(child);
                index += 1;
            } else if m.stage > 0 && !m.last_original_lane {
                state.lightning.swap_remove(index);
            } else {
                m.direction = rotate(m.direction, i32::from(m.stage) * p.split_angle_millidegrees);
                m.stage = 0;
                m.budget = p.lightning.budget;
                index += 1;
            }
        }
    }
    fn advance_master_frost(
        &mut self,
        state: &mut MasterProjectileState,
        units: &mut [UnitSnapshot],
        bounds: [i32; 4],
    ) {
        let p = *state.profile;
        let mut index = 0;
        while index < state.frost.len() {
            let m = &mut state.frost[index];
            if !advance(m, p.frost, bounds) {
                state.frost.swap_remove(index);
                continue;
            }
            let hit = units.iter().position(|u| {
                u.health > 0
                    && u.classifications.combat_sapper
                    && u.team != m.team
                    && m.position().distance_sq(u.position) <= square_i32(p.frost.radius)
            });
            m.budget -= p.frost.decrement;
            if let Some(target) = hit {
                direct_damage(
                    units,
                    target,
                    m.damage,
                    self.combat_rules.damage_rules,
                    self.next_tick,
                );
                // Dummy order has its own live target/native immunity gate. The object has only
                // one level, so SetUnitAbilityLevel(1..3) cannot strengthen this secondary Nova.
                if units[target].health > 0
                    && !units[target].classifications.invulnerable
                    && !units[target].classifications.spell_immune
                {
                    let center = units[target].position;
                    let id = units[target].id;
                    self.reveal_area(
                        m.team,
                        center,
                        p.nova_reveal_radius,
                        u64::from(p.nova_reveal_ticks),
                        false,
                    );
                    for i in 0..units.len() {
                        let u = &mut units[i];
                        if u.health <= 0
                            || u.team == m.team
                            || center.distance_sq(u.position) > square_i32(p.nova.radius)
                        {
                            continue;
                        }
                        let before = u.health;
                        apply_ability_effect_to_unit(
                            u,
                            p.nova.effect(u.id == id),
                            m.team,
                            self.next_tick,
                            self.combat_rules.damage_rules,
                        );
                        death_healing::observe_inline_death_from(units, i, before, None);
                    }
                    self.last_ability_casts.push(AbilityCastEvent {
                        source: m.source,
                        ability: p.nova.ability,
                        target: AbilityCastTarget::Unit(id),
                        target_position: Some(center),
                        effect: p.nova.effect(true),
                    });
                }
            }
            if hit.is_some() || m.budget <= 0 {
                state.frost.swap_remove(index);
            } else {
                index += 1;
            }
        }
    }
    pub(super) fn observe_master_parasite_deaths(
        &mut self,
        units: &[UnitSnapshot],
        health: &[i32],
        positions: &[SimPoint],
    ) {
        let Some((entity, mut state)) = self.master_state() else {
            return;
        };
        let mut survivors = Vec::with_capacity(state.parasites.len());
        for p in std::mem::take(&mut state.parasites) {
            if let Some(index) = find_unit_index(units, p.target)
                && health[index] <= 0
                && p.buff_until_tick
                    .is_some_and(|until| self.next_tick < until)
            {
                let id = self.allocate_id();
                state.minions.push(MasterMinion {
                    id,
                    source: p.source,
                    owner: p.owner,
                    team: p.team,
                    position: positions[index],
                    expires_tick: self.next_tick
                        + u64::from(state.profile.parasite_child_duration_ticks),
                });
            } else {
                survivors.push(p);
            }
        }
        state.parasites = survivors;
        self.world
            .entity_mut(entity)
            .insert(NativeAction::MasterProjectiles(state));
    }
}

fn facing_degrees(p: SimPoint) -> i16 {
    const ATAN: [i32; 20] = [
        45_000_000, 26_565_051, 14_036_243, 7_125_016, 3_576_334, 1_789_911, 895_174, 447_614,
        223_811, 111_906, 55_953, 27_976, 13_988, 6_994, 3_497, 1_749, 874, 437, 219, 109,
    ];
    let mut x = i64::from(p.x) * 1024;
    let mut y = i64::from(p.y) * 1024;
    let mut angle = 0;
    if x < 0 {
        x = -x;
        y = -y;
        angle = 180_000_000;
    }
    for (index, atan) in ATAN.into_iter().enumerate() {
        let direction = if y > 0 { 1 } else { -1 };
        let next_x = x + direction * (y >> index);
        y -= direction * (x >> index);
        x = next_x;
        angle += direction as i32 * atan;
    }
    (angle / 1_000_000).rem_euclid(360) as i16
}
impl Simulation {
    pub(super) fn append_master_projectile_views(&self, views: &mut Vec<ProjectileView>) {
        for entity in self.world.iter_entities() {
            let Some(NativeAction::MasterProjectiles(state)) = entity.get::<NativeAction>() else {
                continue;
            };
            for (movers, p) in [
                (&state.lightning, state.profile.lightning),
                (&state.frost, state.profile.frost),
            ] {
                views.extend(movers.iter().map(|m| ProjectileView {
                    id: m.id,
                    source: m.source,
                    ability: Some(state.profile.ability),
                    launch_position: m.position(),
                    launch_tick: self.next_tick.saturating_sub(1),
                    impact_tick: self.next_tick,
                    kind: ProjectileViewKind::NativeMover {
                        rawcode: p.rawcode,
                        owner: m.owner,
                        facing_degrees: facing_degrees(m.direction),
                        height_world_units: p.height,
                    },
                }));
            }
            views.extend(state.minions.iter().map(|m| ProjectileView {
                id: m.id,
                source: m.source,
                ability: Some(state.profile.ability),
                launch_position: m.position,
                launch_tick: self.next_tick.saturating_sub(1),
                impact_tick: m.expires_tick,
                kind: ProjectileViewKind::NativeMover {
                    rawcode: state.profile.parasite_child_rawcode,
                    owner: m.owner,
                    facing_degrees: 0,
                    height_world_units: 0,
                },
            }));
        }
    }
}
