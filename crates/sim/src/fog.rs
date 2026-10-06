//! Integer, team-shared Warcraft-style visibility. Smoothing belongs exclusively to the client.
use serde::{Deserialize, Serialize};

use crate::{BuildingFootprint, ContentIdentity, PlayerId, SimId, SimPoint, Team};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SightProfile {
    pub day: i32,
    pub night: i32,
}

impl SightProfile {
    pub const fn radius(self, night: bool) -> i32 {
        if night { self.night } else { self.day }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FogRules {
    pub cell_size: i32,
    /// Synthetic/contentless fixtures only. Map entities use the versioned sight catalog.
    pub fallback_sight: SightProfile,
    pub initially_explored: bool,
    pub night: bool,
    pub clock: Option<FogClock>,
    pub attack_reveal: Option<FogAttackReveal>,
    pub permanent_rectangles: [Vec<(SimPoint, SimPoint)>; 2],
    pub sight_blockers: Vec<(SimPoint, SimPoint)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FogAttackReveal {
    pub radius: i32,
    pub duration_ticks: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub struct FogClock {
    pub cycle_ticks: u64,
    pub dawn_phase_ticks: u64,
    pub dusk_phase_ticks: u64,
    pub initial_phase_ticks: u64,
}

impl FogRules {
    pub fn is_night(&self, tick: u64) -> bool {
        self.clock.map_or(self.night, |clock| {
            let phase = (tick % clock.cycle_ticks + clock.initial_phase_ticks) % clock.cycle_ticks;
            phase < clock.dawn_phase_ticks || phase >= clock.dusk_phase_ticks
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct FogReveal {
    pub team: Team,
    pub position: SimPoint,
    pub radius: i32,
    pub detects_invisible: bool,
    /// Exclusive expiry, in simulation ticks.
    pub expires_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RememberedConstruction {
    pub started_tick: u64,
    pub complete_tick: u64,
    pub observed_tick: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RememberedStructure {
    pub id: SimId,
    pub content: Option<ContentIdentity>,
    pub owner: Option<PlayerId>,
    pub footprint: BuildingFootprint,
    pub construction: Option<RememberedConstruction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FogOfWar {
    pub origin: SimPoint,
    pub cell_size: i32,
    pub width: usize,
    pub height: usize,
    /// 0/1 cells, south to north; live visibility is rebuilt from authoritative entities.
    pub visible: [Vec<u8>; 2],
    pub explored: [Vec<u8>; 2],
    pub reveals: Vec<FogReveal>,
    pub remembered_structures: [Vec<RememberedStructure>; 2],
}

impl FogOfWar {
    pub(crate) fn new(origin: SimPoint, maximum: SimPoint, rules: &FogRules) -> Self {
        assert!(
            rules.cell_size > 0 && rules.fallback_sight.day >= 0 && rules.fallback_sight.night >= 0
        );
        if let Some(clock) = rules.clock {
            assert!(
                clock.cycle_ticks > 0
                    && clock.dawn_phase_ticks < clock.dusk_phase_ticks
                    && clock.dusk_phase_ticks < clock.cycle_ticks
                    && clock.initial_phase_ticks < clock.cycle_ticks,
                "invalid day/night vision clock"
            );
        }
        let width = usize::try_from(
            u32::try_from(maximum.x - origin.x)
                .unwrap()
                .div_ceil(rules.cell_size as u32),
        )
        .unwrap();
        let height = usize::try_from(
            u32::try_from(maximum.y - origin.y)
                .unwrap()
                .div_ceil(rules.cell_size as u32),
        )
        .unwrap();
        let len = width.checked_mul(height).expect("fog grid overflow");
        assert!(len > 0 && len <= 1_048_576, "fog grid is too large");
        Self {
            origin,
            cell_size: rules.cell_size,
            width,
            height,
            visible: [vec![0; len], vec![0; len]],
            explored: [
                vec![u8::from(rules.initially_explored); len],
                vec![u8::from(rules.initially_explored); len],
            ],
            reveals: Vec::new(),
            remembered_structures: [Vec::new(), Vec::new()],
        }
    }

    pub fn index(&self, point: SimPoint) -> Option<usize> {
        let x =
            (i64::from(point.x) - i64::from(self.origin.x)).div_euclid(i64::from(self.cell_size));
        let y =
            (i64::from(point.y) - i64::from(self.origin.y)).div_euclid(i64::from(self.cell_size));
        (x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height)
            .then(|| y as usize * self.width + x as usize)
    }

    pub fn is_visible(&self, team: Team, point: SimPoint) -> bool {
        self.index(point).is_some_and(|index| {
            self.visible
                .get(usize::from(team.0))
                .is_some_and(|cells| cells[index] != 0)
        })
    }

    pub fn is_explored(&self, team: Team, point: SimPoint) -> bool {
        self.index(point).is_some_and(|index| {
            self.explored
                .get(usize::from(team.0))
                .is_some_and(|cells| cells[index] != 0)
        })
    }

    pub fn detects_invisible(&self, team: Team, point: SimPoint) -> bool {
        self.reveals.iter().any(|reveal| {
            reveal.team == team
                && reveal.detects_invisible
                && reveal.position.distance_sq(point) <= i64::from(reveal.radius).pow(2) as u64
        })
    }

    pub(crate) fn center(&self, x: i32, y: i32) -> SimPoint {
        SimPoint::new(
            self.origin.x + x * self.cell_size + self.cell_size / 2,
            self.origin.y + y * self.cell_size + self.cell_size / 2,
        )
    }

    pub(crate) fn unveil_circle(
        &mut self,
        team: Team,
        position: SimPoint,
        radius: i32,
        mut can_see: impl FnMut(SimPoint) -> bool,
    ) {
        if radius <= 0 {
            return;
        }
        let min_x = (position.x.saturating_sub(radius) - self.origin.x)
            .div_euclid(self.cell_size)
            .max(0);
        let min_y = (position.y.saturating_sub(radius) - self.origin.y)
            .div_euclid(self.cell_size)
            .max(0);
        let max_x = (position.x.saturating_add(radius) - self.origin.x)
            .div_euclid(self.cell_size)
            .min(self.width as i32 - 1);
        let max_y = (position.y.saturating_add(radius) - self.origin.y)
            .div_euclid(self.cell_size)
            .min(self.height as i32 - 1);
        let radius_sq = i64::from(radius).pow(2) as u64;
        if min_x > max_x || min_y > max_y {
            return;
        }
        for y in min_y..=max_y {
            let start = y as usize * self.width + min_x as usize;
            let end = y as usize * self.width + max_x as usize + 1;
            if !self.visible[usize::from(team.0)][start..end].contains(&0) {
                continue;
            }
            for x in min_x..=max_x {
                let index = y as usize * self.width + x as usize;
                if self.visible[usize::from(team.0)][index] != 0 {
                    continue;
                }
                let point = self.center(x, y);
                if position.distance_sq(point) <= radius_sq && can_see(point) {
                    self.visible[usize::from(team.0)][index] = 1;
                }
            }
        }
        // A positive-radius source always occupies a visible cell, even below cell resolution.
        if let Some(index) = self.index(position) {
            self.visible[usize::from(team.0)][index] = 1;
        }
    }

    pub(crate) fn unveil_rectangle(&mut self, team: Team, min: SimPoint, max: SimPoint) {
        for y in 0..self.height as i32 {
            for x in 0..self.width as i32 {
                let p = self.center(x, y);
                if p.x >= min.x && p.x < max.x && p.y >= min.y && p.y < max.y {
                    self.visible[usize::from(team.0)][y as usize * self.width + x as usize] = 1;
                }
            }
        }
    }

    pub(crate) fn valid_shape(&self, expected: &Self) -> bool {
        self.origin == expected.origin
            && self.cell_size == expected.cell_size
            && self.width == expected.width
            && self.height == expected.height
            && self.visible.iter().chain(&self.explored).all(|cells| {
                cells.len() == self.width * self.height && cells.iter().all(|cell| *cell <= 1)
            })
            && self.reveals.iter().all(|r| r.team.0 < 2 && r.radius > 0)
            && self
                .remembered_structures
                .iter()
                .all(|structures| structures.windows(2).all(|p| p[0].id < p[1].id))
    }
}
