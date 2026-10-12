use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingAttackProxyLevel {
    pub freeze_ticks: u16,
    pub hero_freeze_ticks: u16,
    pub poison: crate::PoisonProfile,
}

/// The native trigger, script selector, timed actor and two weapon passives are independent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct BuildingAttackProxyProfile {
    pub map_version: MapVersion,
    pub parent: crate::NativeParasiteProfile,
    pub trigger_targets: crate::AttackTargetMask,
    pub trigger_mechanical_only: bool,
    pub trigger_spell_immune: bool,
    pub dummy_rawcode: u32,
    pub freeze_ability: AbilityId,
    pub levels: [BuildingAttackProxyLevel; 4],
    pub level_after_minutes: [u16; 3],
    pub excluded_rawcodes: [u32; 4],
    pub excluded_ability: AbilityId,
    pub damage: i32,
    pub range: i32,
    pub speed_per_tick: i32,
    pub dummy_ticks: u16,
    pub vision_radius: i32,
    pub vision_ticks: u16,
}

impl BuildingAttackProxyProfile {
    pub(crate) fn level(self, round_ticks: u64) -> u8 {
        let minutes = round_ticks / (CASTLE_FIGHT_SIMULATION_HZ as u64 * 60);
        self.level_after_minutes
            .partition_point(|&at| minutes >= u64::from(at)) as u8
    }

    pub(crate) fn visit_canonical_words(self, mut visit: impl FnMut(u64)) {
        for word in self.parent.canonical_words() {
            visit(word);
        }
        for word in [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.trigger_targets.bits()),
            u64::from(self.trigger_mechanical_only),
            u64::from(self.trigger_spell_immune),
            u64::from(self.dummy_rawcode),
            u64::from(self.freeze_ability.0),
            u64::from(self.excluded_ability.0),
            self.damage as u64,
            self.range as u64,
            self.speed_per_tick as u64,
            u64::from(self.dummy_ticks),
            self.vision_radius as u64,
            u64::from(self.vision_ticks),
        ] {
            visit(word);
        }
        for at in self.level_after_minutes {
            visit(u64::from(at));
        }
        for excluded in self.excluded_rawcodes {
            visit(u64::from(excluded));
        }
        for level in self.levels {
            visit(u64::from(level.freeze_ticks));
            visit(u64::from(level.hero_freeze_ticks));
            let p = level.poison;
            for word in [
                u64::from(p.ability.0),
                u64::from(p.buff_rawcode),
                p.damage_per_second as u64,
                p.movement_percent_delta as u64,
                p.attack_speed_percent_delta as u64,
                u64::from(p.duration_ticks),
                u64::from(p.hero_duration_ticks),
                u64::from(p.targets.bits()),
                u64::from(p.stack_damage),
                u64::from(p.can_kill),
            ] {
                visit(word);
            }
        }
    }
}

pub(crate) fn attack_proxy_spellcasting_for_version(version: MapVersion) -> SpellcastingProfile {
    assert_eq!(version, MapVersion::CASTLE_FIGHT_9_27);
    static PROFILE: OnceLock<SpellcastingProfile> = OnceLock::new();
    *PROFILE.get_or_init(|| {
        serde_json::from_value(evidence(version)["attack_proxy_spellcasting"].clone())
            .expect("retained native attack proxy projection")
    })
}

pub(crate) fn attack_proxy_bindings(
    version: MapVersion,
) -> [crate::ResolvedNativeEffectBinding; 3] {
    let spell = attack_proxy_spellcasting_for_version(version);
    let AbilityEffect::BuildingAttackProxy(p) = spell.ability.effect else {
        unreachable!()
    };
    [
        spell.ability.id,
        p.freeze_ability,
        p.levels[0].poison.ability,
    ]
    .map(|ability| crate::ResolvedNativeEffectBinding {
        source: crate::NativeEffectSource::new(
            crate::NativeEffectSourceKind::UnitAbility,
            ability.0,
        ),
        implementation: crate::NativeEffectImplementationId::WarcraftBuildingAttackProxyV1,
    })
}

pub(crate) fn attack_proxy_source(version: MapVersion, source: crate::NativeEffectSource) -> bool {
    version == MapVersion::CASTLE_FIGHT_9_27
        && attack_proxy_bindings(version)
            .iter()
            .any(|binding| binding.source == source)
}
