use super::*;

impl Simulation {
    pub(super) fn observe_damage_retaliation(
        &mut self,
        attacker: SimId,
        victim_index: usize,
        tick: u64,
        units: &mut [UnitSnapshot],
        health: &[i32],
        positions: &[SimPoint],
    ) {
        let Some(attacker_index) = find_unit_index(units, attacker) else {
            return;
        };
        if health[victim_index] <= 0 || !units[attacker_index].classifications.melee_attacker {
            return;
        }
        let Some(profile) = units[victim_index]
            .effective_passive_effects()
            .find_map(|effect| {
                if let PassiveUnitEffect::DamageRetaliation(profile) = effect {
                    Some(profile)
                } else {
                    None
                }
            })
        else {
            return;
        };
        let sequence = units[victim_index].status.retaliation_sequence;
        units[victim_index].status.retaliation_sequence = sequence
            .checked_add(1)
            .expect("damage response sequence overflow");
        // The script rolls before issuing the independent native dummy order.
        let roll = deterministic_random(
            self.config.match_seed,
            tick,
            units[victim_index].id,
            RANDOM_PURPOSE_ATTACK_PROC ^ u64::from(profile.marker.0) ^ attacker.0.rotate_left(13),
            sequence,
        ) % 100;
        if roll * 100 >= u64::from(profile.chance_per_10k) {
            return;
        }
        let source = &units[victim_index];
        let target = &units[attacker_index];
        if health[attacker_index] <= 0
            || source.team == target.team
            || target.classifications.invulnerable
            || target.classifications.spell_immune
            || (profile.organic_only && target.mechanical)
            || !profile
                .projectile
                .targets
                .can_target_unit(target.target_movement_class())
            || positions[victim_index].distance_sq(positions[attacker_index])
                > square_i32(profile.range)
            || !target.visible_to(source.team, tick)
        {
            return;
        }
        self.launch_dummy_bolt(
            source.id,
            source.team,
            positions[victim_index],
            attacker,
            positions[attacker_index],
            profile.projectile,
        );
    }
}
