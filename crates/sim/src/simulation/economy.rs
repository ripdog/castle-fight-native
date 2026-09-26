use super::*;

impl Simulation {
    #[must_use]
    pub fn player_resources_for(&self, player: PlayerId) -> Option<PlayerResources> {
        self.player_state(player).map(|state| state.resources)
    }

    /// Compatibility lookup for maps/tests with exactly one player on a team. Multi-player teams
    /// are intentionally ambiguous and return `None`; authoritative gameplay should use PlayerId.
    #[must_use]
    pub fn player_resources(&self, team: Team) -> Option<PlayerResources> {
        let player = self.unique_player_for_team(team)?;
        self.player_resources_for(player)
    }

    /// Adds resources directly to one player's authoritative economy state for developer tooling.
    pub fn debug_grant_player_resources_for(
        &mut self,
        player: PlayerId,
        gold: u32,
        lumber: u32,
    ) -> bool {
        let Some(state) = self.player_state_mut(player) else {
            return false;
        };
        state.resources.gold = state.resources.gold.saturating_add(gold);
        state.resources.lumber = state.resources.lumber.saturating_add(lumber);
        true
    }

    /// Compatibility helper for the current one-player-per-team fixtures.
    pub fn debug_grant_player_resources(&mut self, team: Team, gold: u32, lumber: u32) -> bool {
        let Some(player) = self.unique_player_for_team(team) else {
            return false;
        };
        self.debug_grant_player_resources_for(player, gold, lumber)
    }

    #[must_use]
    pub fn player_income_for(&self, player: PlayerId) -> Option<u32> {
        self.player_state(player)?;
        Some(taxed_income_from_fixed(
            self.raw_player_income_per_10k(player),
            self.config.economy.income_tax_bracket_per_10k,
        ))
    }

    #[must_use]
    pub fn player_income(&self, team: Team) -> Option<u32> {
        self.player_income_for(self.unique_player_for_team(team)?)
    }

    #[must_use]
    pub fn player_economy_for(&self, player: PlayerId) -> Option<PlayerEconomyView> {
        let resources = self.player_resources_for(player)?;
        let interval = self.config.economy.income_interval_ticks;
        let (progress, ticks_until_income) = if interval == 0 {
            (0, 0)
        } else {
            let phase = u32::try_from(self.next_tick % u64::from(interval))
                .expect("income phase fits interval width");
            let progress =
                u16::try_from(u64::from(phase) * RESOURCE_FIXED_SCALE / u64::from(interval))
                    .expect("income progress is at most 10,000");
            let remaining = if phase == 0 {
                interval
            } else {
                interval - phase
            };
            (progress, remaining)
        };
        Some(PlayerEconomyView {
            resources,
            income: self.player_income_for(player).expect("validated player id"),
            income_interval_ticks: interval,
            income_progress_per_10k: progress,
            ticks_until_income,
        })
    }

    #[must_use]
    pub fn player_economy(&self, team: Team) -> Option<PlayerEconomyView> {
        self.player_economy_for(self.unique_player_for_team(team)?)
    }

    #[must_use]
    pub fn can_afford_building_for_player(
        &self,
        player: PlayerId,
        economy: BuildingEconomyProfile,
    ) -> bool {
        self.player_resources_for(player).is_some_and(|resources| {
            resources.gold >= economy.gold_cost
                && resources.lumber >= economy.lumber_cost
                && resources.legendary_points_available() >= economy.legendary_points_cost
        })
    }

    #[must_use]
    pub fn can_afford_building(&self, team: Team, economy: BuildingEconomyProfile) -> bool {
        self.unique_player_for_team(team)
            .is_some_and(|player| self.can_afford_building_for_player(player, economy))
    }

    fn raw_player_income_per_10k(&self, player: PlayerId) -> u64 {
        let treasure_box_rawcode = u32::from_be_bytes(*b"h008");
        let mut treasure_boxes = 0u32;
        let raw = self
            .world
            .iter_entities()
            .filter(|entity| entity.get::<Owner>() == Some(&Owner(player)))
            .filter_map(|entity| {
                let economy = entity.get::<BuildingEconomyProfile>()?;
                if entity
                    .get::<ContentIdentity>()
                    .is_some_and(|content| content.rawcode == treasure_box_rawcode)
                {
                    treasure_boxes = treasure_boxes
                        .checked_add(1)
                        .expect("Treasure Box count overflow");
                }
                Some(economy)
            })
            .fold(self.config.economy.base_income_per_10k, |total, economy| {
                total
                    .checked_add(economy.income_per_10k)
                    .expect("player income overflow")
            });
        let multiplier_per_10k = match treasure_boxes {
            0 => 10_000u64,
            1 => 12_500,
            2 => 14_625,
            3 => 16_425,
            4 => 17_950,
            5 => 19_250,
            6 => 20_350,
            7 => 21_300,
            8 => 22_100,
            9 => 22_775,
            count => 22_775u64
                .checked_add(625u64 * u64::from(count - 9))
                .expect("Treasure Box multiplier overflow"),
        };
        raw.checked_mul(multiplier_per_10k)
            .expect("Treasure Box adjusted income overflow")
            / RESOURCE_FIXED_SCALE
    }

    pub(super) fn advance_economy_income(&mut self) {
        let interval = self.config.economy.income_interval_ticks;
        if interval == 0 {
            return;
        }
        let elapsed_after_tick = self
            .next_tick
            .checked_add(1)
            .expect("income tick counter overflow");
        if !elapsed_after_tick.is_multiple_of(u64::from(interval)) {
            return;
        }

        let payouts: Vec<_> = self
            .players
            .iter()
            .map(|player| {
                (
                    player.id,
                    taxed_income_from_fixed(
                        self.raw_player_income_per_10k(player.id),
                        self.config.economy.income_tax_bracket_per_10k,
                    ),
                )
            })
            .collect();
        for (player, income) in payouts {
            let resources = &mut self
                .player_state_mut(player)
                .expect("income player must exist")
                .resources;
            resources.gold = resources
                .gold
                .checked_add(income)
                .expect("player gold overflow");
        }
    }
}
