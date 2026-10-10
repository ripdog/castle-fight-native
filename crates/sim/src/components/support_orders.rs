use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SupportOrderRules {
    pub map_version: crate::MapVersion,
    pub interval_ticks: u16,
    pub budget: u16,
    pub cache_ticks: u16,
    pub retention_limit: u8,
    pub preferred_mana_above: i32,
    pub fallback_health_below: [i32; 2],
    pub local_rectangles: [[SimPoint; 2]; 2],
    pub battlefield: [SimPoint; 2],
    pub excluded_ability: u32,
    pub excluded_buffs: [u32; 2],
}
impl SupportOrderRules {
    pub(crate) fn visit_canonical_words(self, mut write: impl FnMut(u64)) {
        for word in [
            u64::from(self.map_version.major),
            u64::from(self.map_version.minor),
            u64::from(self.interval_ticks),
            u64::from(self.budget),
            u64::from(self.cache_ticks),
            u64::from(self.retention_limit),
            self.preferred_mana_above as u64,
            self.fallback_health_below[0] as u64,
            self.fallback_health_below[1] as u64,
            u64::from(self.excluded_ability),
            u64::from(self.excluded_buffs[0]),
            u64::from(self.excluded_buffs[1]),
        ] {
            write(word);
        }
        for p in self
            .local_rectangles
            .into_iter()
            .flatten()
            .chain(self.battlefield)
        {
            write(p.x as u64);
            write(p.y as u64);
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WindWalkProfile {
    pub ability: AbilityId,
    pub rules: SupportOrderRules,
    pub fade_ticks: u16,
    pub duration_ticks: u16,
    pub movement_percent_delta: i16,
    pub backstab_damage: i32,
    pub fallback_health_below: i32,
}
impl WindWalkProfile {
    pub(crate) fn visit_canonical_words(self, mut write: impl FnMut(u64)) {
        self.rules.visit_canonical_words(&mut write);
        for word in [
            u64::from(self.ability.0),
            u64::from(self.fade_ticks),
            u64::from(self.duration_ticks),
            self.movement_percent_delta as u64,
            self.backstab_damage as u64,
            self.fallback_health_below as u64,
        ] {
            write(word);
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct WindWalkState {
    pub ability: AbilityId,
    pub cloaked_tick: u64,
    pub expires_tick: u64,
    pub movement_percent_delta: i16,
    pub backstab_damage: i32,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SupportOrderState {
    pub selected_target: Option<SimId>,
    pub order_target: Option<SimId>,
    pub move_point: Option<SimPoint>,
    pub iterations: u8,
    pub sequence: u64,
    pub backstab_pending: i32,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SupportTargetCache {
    pub owner: PlayerId,
    pub expires_tick: u64,
    /// Local then global: mana targets, ordinary fallback, royal fallback.
    pub groups: [[Vec<SimId>; 3]; 2],
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SupportOrderQueue {
    pub rules: SupportOrderRules,
    pub next_tick: u64,
    pub actors: std::collections::VecDeque<SimId>,
    pub caches: Vec<SupportTargetCache>,
}
impl SupportOrderQueue {
    pub(crate) fn visit_canonical_words(&self, mut write: impl FnMut(u64)) {
        self.rules.visit_canonical_words(&mut write);
        write(self.next_tick);
        write(self.actors.len() as u64);
        for actor in &self.actors {
            write(actor.0);
        }
        write(self.caches.len() as u64);
        for cache in &self.caches {
            write(u64::from(cache.owner.0));
            write(cache.expires_tick);
            for group in cache.groups.iter().flatten() {
                write(group.len() as u64);
                for id in group {
                    write(id.0);
                }
            }
        }
    }
}
