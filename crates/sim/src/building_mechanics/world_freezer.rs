//! Source-owned delayed movers, native aura and free child orders.
use super::*;
use crate::{AttackTargetMask, EntanglingRootsEffectProfile, NativeBoltProfile};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct WorldFreezerProfile {
    pub map_version: MapVersion,
    pub parent: AbilityId,
    pub dummy_rawcode: u32,
    pub trigger_targets: AttackTargetMask,
    pub trigger_invulnerable: bool,
    pub trigger_spell_immune: bool,
    pub delay_ticks: u16,
    pub angle_offsets: [i16; 3],
    pub interval_millis: u16,
    pub step: i32,
    pub bounce_step: i32,
    pub horizontal_bounds: [i32; 2],
    pub vertical_bounds: [i32; 2],
    pub counter_limit: u16,
    pub target_radius: i32,
    pub aura: AbilityId,
    pub aura_radius: i32,
    pub aura_targets: AttackTargetMask,
    pub movement_percent_delta: i16,
    pub attack_speed_percent_delta: i16,
    pub fire_buff: u32,
    pub air_buff: u32,
    pub ground_buff: u32,
    pub fire: NativeBoltProfile,
    pub alternate_fire: NativeBoltProfile,
    pub fire_radius: i32,
    pub fire_cooldown_millis: u16,
    pub air: NativeBoltProfile,
    pub ground: NativeBoltProfile,
    pub roots: EntanglingRootsEffectProfile,
    pub vision_radius: i32,
    pub vision_ticks: u16,
}

impl WorldFreezerProfile {
    /// Fixed-size numeric encoding, without per-tick allocation or unordered data.
    pub(crate) fn canonical_words(self) -> impl Iterator<Item = u64> {
        let n = |v: i32| u64::from(v as u32);
        let bolt = |p: NativeBoltProfile| {
            [
                u64::from(p.ability.0),
                n(p.damage),
                u64::from(p.stun_ticks),
                u64::from(p.hero_stun_ticks),
                n(p.damage_per_second),
                u64::from(p.duration_ticks),
                n(p.speed_per_tick),
                u64::from(p.cleanse),
                u64::from(p.targets.bits()),
            ]
        };
        [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.parent.0),
            u64::from(self.dummy_rawcode),
            u64::from(self.trigger_targets.bits()),
            u64::from(self.trigger_invulnerable),
            u64::from(self.trigger_spell_immune),
            u64::from(self.delay_ticks),
            n(i32::from(self.angle_offsets[0])),
            n(i32::from(self.angle_offsets[1])),
            n(i32::from(self.angle_offsets[2])),
            u64::from(self.interval_millis),
            n(self.step),
            n(self.bounce_step),
            n(self.horizontal_bounds[0]),
            n(self.horizontal_bounds[1]),
            n(self.vertical_bounds[0]),
            n(self.vertical_bounds[1]),
            u64::from(self.counter_limit),
            n(self.target_radius),
            u64::from(self.aura.0),
            n(self.aura_radius),
            u64::from(self.aura_targets.bits()),
            n(i32::from(self.movement_percent_delta)),
            n(i32::from(self.attack_speed_percent_delta)),
            u64::from(self.fire_buff),
            u64::from(self.air_buff),
            u64::from(self.ground_buff),
            n(self.fire_radius),
            u64::from(self.fire_cooldown_millis),
            u64::from(self.roots.ability.0),
            n(self.roots.damage_per_second),
            u64::from(self.roots.duration_ticks),
            u64::from(self.roots.hero_duration_ticks),
            u64::from(self.roots.nonhero_only),
            u64::from(self.roots.targets.bits()),
            n(self.vision_radius),
            u64::from(self.vision_ticks),
        ]
        .into_iter()
        .chain(bolt(self.fire))
        .chain(bolt(self.alternate_fire))
        .chain(bolt(self.air))
        .chain(bolt(self.ground))
    }
}

pub(crate) fn world_freezer_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    let data = evidence(version);
    let row = &data["freezer"];
    let params: Value = serde_json::from_str(row["parameters_json"].as_str().unwrap()).unwrap();
    let number = |v: &Value| {
        v.as_f64()
            .unwrap_or_else(|| v.as_str().unwrap().parse().unwrap())
    };
    let ticks = |v: &Value| (number(v) * f64::from(CASTLE_FIGHT_SIMULATION_HZ)).ceil() as u16;
    let world = |v: &Value| (number(v) * f64::from(SUBUNITS_PER_WORLD_UNIT)).round() as i32;
    let ability = |key: &str| AbilityId(u32::try_from(params[key].as_u64().unwrap()).unwrap());
    let object = |id: u32| -> &Value {
        let bytes = id.to_be_bytes();
        &data["fields"][std::str::from_utf8(&bytes).unwrap()]
    };
    let parent = AbilityId(u32::from_be_bytes(
        row["ability_rawcode"]
            .as_str()
            .unwrap()
            .as_bytes()
            .try_into()
            .unwrap(),
    ));
    let protected = |id: AbilityId, field: &str| {
        let bytes = id.0.to_be_bytes();
        data["protected"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["rawcode"] == std::str::from_utf8(&bytes).unwrap() && r["field"] == field)
            .unwrap()["runtime_value"]
            .clone()
    };
    let bolt = |id: AbilityId, fire: bool| {
        let f = object(id.0);
        if !fire {
            assert_eq!(number(&protected(id, "mana_cost")), 0.0);
            assert_eq!(number(&protected(id, "cooldown")), 0.0);
        }
        NativeBoltProfile {
            ability: id,
            damage: number(&f[if fire { "pxf1:1" } else { "Htb1:1" }]) as i32,
            stun_ticks: if fire { 0 } else { ticks(&f["adur:1"]) },
            hero_stun_ticks: if fire { 0 } else { ticks(&f["ahdu:1"]) },
            damage_per_second: if fire { number(&f["pxf2:1"]) as i32 } else { 0 },
            duration_ticks: if fire { ticks(&f["adur:1"]) } else { 0 },
            speed_per_tick: world(&f["amsp:0"]) / CASTLE_FIGHT_SIMULATION_HZ,
            cleanse: false,
            targets: source_mask(f["atar:1"].as_str().unwrap()),
        }
    };
    let aura = ability("ambient_slow_ability_id");
    let af = object(aura.0);
    let roots_id = ability("ground_damage_ability_id");
    let rf = object(roots_id.0);
    assert_eq!(number(&protected(roots_id, "mana_cost")), 0.0);
    assert_eq!(number(&protected(roots_id, "cooldown")), 0.0);
    let fire = ability("ambient_damage_ability_id");
    let ff = object(fire.0);
    let parent_fields = object(parent.0);
    let mask = parent_fields["atar:1"].as_str().unwrap();
    let building = &data["fields"][row["building_rawcode"].as_str().unwrap()];
    let bounds = &data["terrain_grid"]["battlefield_bounds"];
    SpellcastingProfile {
        mana: ManaProfile::per_second(
            number(&building["umpm:0"]) as i32,
            0,
            (number(&building["umpr:0"]) * 10_000.0) as u32,
        ),
        ability: AutomaticAbilityProfile {
            id: parent,
            mana_cost: number(&protected(parent, "mana_cost")) as i32,
            cooldown_ticks: ticks(&protected(parent, "cooldown")),
            range: world(&parent_fields["aran:1"]),
            target_policy: AbilityTargetPolicy::NativeBuildingSpellTrigger,
            effect: AbilityEffect::WorldFreezer(WorldFreezerProfile {
                map_version: version,
                parent,
                dummy_rawcode: params["orb_unit_id"].as_u64().unwrap() as u32,
                trigger_targets: source_mask(mask),
                trigger_invulnerable: mask.split(',').any(|v| v == "invulnerable"),
                trigger_spell_immune: mask.split(',').any(|v| v == "magicimmune"),
                delay_ticks: ticks(&params["spawn_delay_seconds"]),
                angle_offsets: std::array::from_fn(|i| {
                    params["angle_offsets_degrees"][i].as_i64().unwrap() as i16
                }),
                interval_millis: (number(&params["movement_tick_seconds"]) * 1000.0).round() as u16,
                step: world(&params["movement_step_world_units"]),
                bounce_step: world(&data["freezer_bounce_step"]),
                horizontal_bounds: [world(&bounds[0]), world(&bounds[2])],
                vertical_bounds: std::array::from_fn(|i| world(&params["vertical_bounds"][i])),
                counter_limit: params["target_check_after_ticks_gt"].as_u64().unwrap() as u16,
                target_radius: world(&params["target_radius"]),
                aura,
                aura_radius: world(&af["aare:1"]),
                aura_targets: source_mask(af["atar:1"].as_str().unwrap()),
                movement_percent_delta: (number(&af["Oae1:1"]) * 100.0).round() as i16,
                attack_speed_percent_delta: (number(&af["Oae2:1"]) * 100.0).round() as i16,
                fire_buff: u32::from_be_bytes(
                    ff["abuf:1"]
                        .as_str()
                        .unwrap()
                        .as_bytes()
                        .try_into()
                        .unwrap(),
                ),
                air_buff: u32::from_be_bytes(
                    object(ability("flying_target_ability_id").0)["abuf:1"]
                        .as_str()
                        .unwrap()
                        .as_bytes()
                        .try_into()
                        .unwrap(),
                ),
                ground_buff: u32::from_be_bytes(
                    object(ability("ground_stun_ability_id").0)["abuf:1"]
                        .as_str()
                        .unwrap()
                        .as_bytes()
                        .try_into()
                        .unwrap(),
                ),
                fire: bolt(fire, true),
                alternate_fire: bolt(ability("alternate_mode_damage_ability_id"), true),
                fire_radius: world(&ff["aare:1"]),
                fire_cooldown_millis: (number(&ff["acdn:1"]) * 1000.0).round() as u16,
                air: bolt(ability("flying_target_ability_id"), false),
                ground: bolt(ability("ground_stun_ability_id"), false),
                roots: EntanglingRootsEffectProfile {
                    ability: roots_id,
                    damage_per_second: number(&rf["Eer1:1"]) as i32,
                    duration_ticks: ticks(&rf["adur:1"]),
                    hero_duration_ticks: ticks(&rf["ahdu:1"]),
                    nonhero_only: rf["atar:1"]
                        .as_str()
                        .unwrap()
                        .split(',')
                        .any(|v| v == "nonhero"),
                    targets: source_mask(rf["atar:1"].as_str().unwrap()),
                },
                vision_radius: world(&data["target_vision"]["radius"]),
                vision_ticks: ticks(&data["target_vision"]["duration"]),
            }),
        },
    }
}

pub(crate) fn world_freezer_bindings_for_version(
    version: MapVersion,
) -> [crate::ResolvedNativeEffectBinding; 7] {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    [
        (crate::NativeEffectSourceKind::UnitAbility, *b"A04I"),
        (crate::NativeEffectSourceKind::AbilityEffect, *b"A081"),
        (crate::NativeEffectSourceKind::AbilityEffect, *b"A080"),
        (crate::NativeEffectSourceKind::AbilityEffect, *b"A084"),
        (crate::NativeEffectSourceKind::AbilityEffect, *b"A082"),
        (crate::NativeEffectSourceKind::AbilityEffect, *b"A083"),
        (crate::NativeEffectSourceKind::AbilityEffect, *b"A04H"),
    ]
    .map(|(kind, code)| crate::ResolvedNativeEffectBinding {
        source: crate::NativeEffectSource::new(kind, u32::from_be_bytes(code)),
        implementation: crate::NativeEffectImplementationId::WarcraftWorldFreezerV1,
    })
}
pub(crate) fn world_freezer_source_for_version(
    source: crate::NativeEffectSource,
    version: MapVersion,
) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && world_freezer_bindings_for_version(version)
            .iter()
            .any(|b| b.source == source)
}

/// Source animation deadlines, independent of gameplay and exported clip duration.
#[derive(Debug, Clone, Copy)]
pub struct WorldFreezerVisualTiming {
    pub building_rawcode: u32,
    pub cooldown_ticks: u16,
    pub death_ticks: u16,
    pub orb_height_world_units: i32,
}

pub fn world_freezer_visual_timing_for_version(version: MapVersion) -> WorldFreezerVisualTiming {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static TIMING: std::sync::LazyLock<WorldFreezerVisualTiming> = std::sync::LazyLock::new(|| {
        let version = MapVersion::CASTLE_FIGHT_9_27;
        let data = evidence(version);
        let casting = world_freezer_spellcasting_for_version(version);
        let AbilityEffect::WorldFreezer(profile) = casting.ability.effect else {
            unreachable!()
        };
        WorldFreezerVisualTiming {
            building_rawcode: u32::from_be_bytes(
                data["freezer"]["building_rawcode"]
                    .as_str()
                    .unwrap()
                    .as_bytes()
                    .try_into()
                    .unwrap(),
            ),
            cooldown_ticks: casting.ability.cooldown_ticks,
            death_ticks: profile.delay_ticks + ticks(&data["freezer_animation_seconds"]),
            orb_height_world_units: data["fields"]
                [std::str::from_utf8(&profile.dummy_rawcode.to_be_bytes()).unwrap()]["umvh:0"]
                .as_i64()
                .unwrap() as i32,
        }
    });
    *TIMING
}
