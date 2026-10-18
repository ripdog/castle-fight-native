//! Map-script children released by a native targeted Parasite.
use super::*;
use crate::components::{ParasiteChild, ParasiteScriptProfile, PendingParasiteScript};

const RANDOM_PURPOSE_PARASITE_SCRIPT: u64 = 0x5041_5241_5343_5201;

/// Parent eligibility: the native Parasite mask plus the script's sapper-body requirement.
pub(super) fn primary_eligible(
    u: &UnitSnapshot,
    team: Team,
    p: ParasiteScriptProfile,
    tick: u64,
) -> bool {
    u.health > 0
        && u.team != team
        && u.classifications.combat_sapper
        && p.parent_targets.can_target_unit(u.target_movement_class())
        && !(p.parent_organic_only && u.mechanical)
        && !u.classifications.invulnerable
        && !u.classifications.spell_immune
        && !(p.parent_nonhero_only && u.classifications.uses_hero_spell_rules())
        && u.visible_to(team, tick)
}

impl Simulation {
    pub(super) fn cast_parasite_script(
        &mut self,
        source: AbilitySourceSnapshot,
        target: usize,
        p: ParasiteScriptProfile,
        units: &mut [UnitSnapshot],
    ) -> usize {
        let caster = find_unit_index(units, source.id).expect("Parasite script caster");
        self.launch_native_parasite(source, units[caster].owner, units[target].id, p.parent);
        match p.child {
            ParasiteChild::SandCloud(profile) => {
                let center = units[target].position;
                let rawcode = units[caster].rawcode;
                self.launch_sand_cloud(source, units[caster].owner, rawcode, center, profile);
            }
            ParasiteChild::LocustSwarm(profile) => {
                // The dummy appears at the Parasite target's position at cast time.
                let center = units[target].position;
                self.launch_locust_swarm(source, units[caster].owner, center, profile);
            }
            ParasiteChild::LifeSteal {
                minimum_delay_ticks,
                maximum_delay_ticks,
                ..
            } => {
                // GetRandomReal(min, max) fires strictly after the lower bound and, on the
                // timer grid, at the first tick not before the drawn instant.
                let span = u64::from(maximum_delay_ticks.saturating_sub(minimum_delay_ticks)).max(1);
                let roll = deterministic_random(
                    self.config.match_seed,
                    self.next_tick,
                    source.id,
                    RANDOM_PURPOSE_PARASITE_SCRIPT,
                    source.ability_state.map_or(0, |state| state.cast_sequence),
                );
                let delay = u64::from(minimum_delay_ticks) + 1 + roll % span;
                units[caster].status.pending_parasite_script = Some(PendingParasiteScript {
                    ability: p.ability,
                    child: p.child,
                    target: units[target].id,
                    due_tick: self.next_tick + delay,
                });
            }
        }
        1
    }

    /// Fires due child callbacks in unit order. A callback does not recheck the parent's
    /// target mask: it reads the target's life when it fires.
    pub(super) fn resolve_pending_parasite_scripts(&self, units: &mut [UnitSnapshot]) -> usize {
        let mut effects = 0;
        for caster in 0..units.len() {
            let Some(pending) = units[caster].status.pending_parasite_script else {
                continue;
            };
            if pending.due_tick > self.next_tick {
                continue;
            }
            units[caster].status.pending_parasite_script = None;
            match pending.child {
                ParasiteChild::LocustSwarm(_) | ParasiteChild::SandCloud(_) => {}
                ParasiteChild::LifeSteal { maximum_life, .. } => {
                    let target = find_unit_index(units, pending.target);
                    let life = target.map_or(0, |index| units[index].health.max(0));
                    let stolen = maximum_life.min(life);
                    if let Some(target) = target
                        && stolen > 0
                    {
                        self.chaos_attack_damage(units[caster].id, target, stolen, units);
                        effects += 1;
                    }
                    // SetUnitState(life) on a dead caster has no effect; the heal is the
                    // requested amount, independent of what the target's defenses allowed.
                    if units[caster].health > 0 {
                        units[caster].health = units[caster]
                            .health
                            .saturating_add(stolen)
                            .min(units[caster].health_max);
                    }
                }
            }
        }
        effects
    }

    /// `UnitDamageTarget(attack=true, ranged=false, ATTACK_TYPE_CHAOS, DAMAGE_TYPE_NORMAL)`.
    fn chaos_attack_damage(
        &self,
        source: SimId,
        victim: usize,
        amount: i32,
        units: &mut [UnitSnapshot],
    ) {
        let rules = self.combat_rules.damage_rules;
        let target = units[victim];
        if target.health <= 0 || target.classifications.invulnerable {
            return;
        }
        let damage = rules.apply_attack_with_armor_per_100(
            amount,
            DamageType::Chaos,
            target.armor.armor_type,
            effective_armor_points_per_100(&target),
        );
        let damage = super::night_elf::hardened_skin_after_armor(
            &mut units[victim],
            damage,
            source,
            self.next_tick,
        );
        let damage = scale_damage_per_10k(
            native_kill_effects::damage_after_native_incoming(&target, damage, self.next_tick),
            target.snow_damage_taken_per_10k,
        );
        source_damage_runtime::dispatch_inline(
            source,
            victim,
            damage,
            units,
            rules,
            self.next_tick,
            &self.retired_damage_sources,
        );
        let current = units[victim].health;
        damage_thresholds::observe_damage_event(&mut units[victim], current, damage);
        death_healing::commit_health_from(
            units,
            victim,
            current.saturating_sub(damage),
            Some(source),
        );
    }
}
