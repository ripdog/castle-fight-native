//! Fine-grained wall-clock buckets for the sub-steps of `Simulation::step`.
//!
//! Span timings are observational only. Simulation logic never reads them and they never feed
//! the checksum, so measuring them cannot change gameplay results. Recording is a fixed-size
//! array write per sub-step, so it is always on and allocation free.

use std::time::Duration;

/// Declares every span once, in `step()` order, with its kebab-case name and the top-level
/// `TickTimings` phase it belongs to. `ALL`, `name`, and `phase` are generated from this table.
macro_rules! define_spans {
    ($( $variant:ident => ($name:literal, $phase:literal) ),* $(,)?) => {
        /// One timed sub-step of `Simulation::step`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Span {
            $($variant),*
        }

        impl Span {
            /// Number of spans; the length of [`SpanTimings`].
            pub const COUNT: usize = [$(Span::$variant),*].len();

            /// Every span in declaration (`step()`) order.
            pub const ALL: [Span; Self::COUNT] = [$(Span::$variant),*];

            /// Stable kebab-case identifier used in benchmark output.
            pub const fn name(self) -> &'static str {
                match self {
                    $(Span::$variant => $name),*
                }
            }

            /// The top-level `TickTimings` phase this span is attributed to.
            pub const fn phase(self) -> &'static str {
                match self {
                    $(Span::$variant => $phase),*
                }
            }
        }
    };
}

define_spans! {
    // timers
    ManaImprovementLinks => ("mana-improvement-links", "timers"),
    CarrierAurasTimers => ("carrier-auras-timers", "timers"),
    NativeManaAurasTimers => ("native-mana-auras-timers", "timers"),
    NativeBuildingAurasTimers => ("native-building-auras-timers", "timers"),
    AdvanceCooldowns => ("cooldowns", "timers"),
    RescueStrikes => ("rescue-strikes", "timers"),
    ShrineRevivals => ("shrine-revivals", "timers"),
    BuildingSpellControls => ("building-spell-controls", "timers"),
    MirrorImagesAndLava => ("mirror-images-and-lava", "timers"),
    ExpireCorpses => ("expire-corpses", "timers"),
    PauseFrozenBuildingActivities => ("pause-frozen-buildings", "timers"),
    AdvanceBuilders => ("advance-builders", "timers"),
    BuildingConstruction => ("building-construction", "timers"),
    // other (top-level phases without a dedicated span group)
    TopologyRefresh => ("topology-refresh", "other"),
    Production => ("production", "other"),
    // spatial
    RefreshVision => ("refresh-vision", "spatial"),
    SnapshotUnits => ("snapshot-units", "spatial"),
    SnapshotBuildings => ("snapshot-buildings", "spatial"),
    SnowProtection => ("snow-protection", "spatial"),
    SupportRefreshInitial => ("support-refresh-initial", "spatial"),
    PeriodicUnitStatuses => ("periodic-unit-statuses", "spatial"),
    BuildingDamageOverTime => ("building-damage-over-time", "spatial"),
    SupportRefreshAfterDamage => ("support-refresh-after-damage", "spatial"),
    BurningOilZones => ("burning-oil-zones", "spatial"),
    SpatialGridBuild => ("spatial-grid-build", "spatial"),
    AllyDefenseVictims => ("ally-defense-victims", "spatial"),
    DefenseGridBuild => ("defense-grid-build", "spatial"),
    // ability
    SupportOrders => ("support-orders", "ability"),
    WorldFreezer => ("world-freezer", "ability"),
    NativeActions => ("native-actions", "ability"),
    PermanentImmolation => ("permanent-immolation", "ability"),
    PassiveAuras => ("passive-auras", "ability"),
    SpeedAuras => ("speed-auras", "ability"),
    HostileArmorAuras => ("hostile-armor-auras", "ability"),
    DamageCharge => ("damage-charge", "ability"),
    NativeBuildingAuras => ("native-building-auras", "ability"),
    AutomaticAbilities => ("automatic-abilities", "ability"),
    NagaCharmGridRebuild => ("naga-charm-grid-rebuild", "ability"),
    // target
    SelectTargets => ("select-targets", "target"),
    SelectBuildingTargets => ("select-building-targets", "target"),
    // combat
    CombatPrep => ("combat-prep", "combat"),
    ChainLightning => ("chain-lightning", "combat"),
    NativeCarriers => ("native-carriers", "combat"),
    TargetProjectiles => ("target-projectiles", "combat"),
    Attacks => ("attacks", "combat"),
    // intent
    Movement => ("movement", "intent"),
    // impact
    SnowProtectionAt => ("snow-protection-at", "impact"),
    BallisticImpacts => ("ballistic-impacts", "impact"),
    HailstoneImpacts => ("hailstone-impacts", "impact"),
    AttackProxyImpacts => ("attack-proxy-impacts", "impact"),
    LineProjectiles => ("line-projectiles", "impact"),
    // commit
    CommitProjectileChanges => ("commit-projectile-changes", "commit"),
    OverheatDeaths => ("overheat-deaths", "commit"),
    ReleaseSpheres => ("release-spheres", "commit"),
    DeathObservers => ("death-observers", "commit"),
    UnitDeathLoop => ("unit-death-loop", "commit"),
    CorpseSpawns => ("corpse-spawns", "commit"),
    BuildingDeathLoop => ("building-death-loop", "commit"),
    BuildingDespawn => ("building-despawn", "commit"),
    PostCommitCleanup => ("post-commit-cleanup", "commit"),
    ObjectiveOutcome => ("objective-outcome", "commit"),
    EconomyIncome => ("economy-income", "commit"),
    FogAndVision => ("fog-and-vision", "commit"),
    // hash
    Checksum => ("checksum", "hash"),
}

/// Accumulated time per [`Span`], indexed by the span's declaration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpanTimings([Duration; Span::COUNT]);

impl Default for SpanTimings {
    fn default() -> Self {
        Self([Duration::ZERO; Span::COUNT])
    }
}

impl SpanTimings {
    #[inline]
    pub fn get(&self, span: Span) -> Duration {
        self.0[span as usize]
    }

    #[inline]
    pub fn add(&mut self, span: Span, duration: Duration) {
        let slot = &mut self.0[span as usize];
        *slot = slot.saturating_add(duration);
    }

    pub fn accumulate(&mut self, other: &SpanTimings) {
        for (slot, duration) in self.0.iter_mut().zip(other.0.iter()) {
            *slot = slot.saturating_add(*duration);
        }
    }
}
