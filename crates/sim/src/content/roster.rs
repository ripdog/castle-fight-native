//! Native promotion registry: identities only, never map tuning or production relationships.
//!
//! Adding an ordinary entity requires one row here and complete native behavior coverage.
//! Names, stats, weapons, corpse capabilities and production links come from retained extraction.

use super::{CastleFightBuildingId, CastleFightUnitId};

macro_rules! roster {
    ($kind:ident, $id:ident; $($variant:ident = $stable:literal => $rawcode:literal),+ $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $kind { $($variant),+ }

        impl $kind {
            pub const ALL: [Self; roster!(@count $($variant),+)] = [$(Self::$variant),+];

            #[must_use]
            pub const fn stable_id(self) -> $id {
                $id(match self { $(Self::$variant => $stable),+ })
            }

            // Rawcodes are source identities scoped to this retained map, not portable IDs.
            pub(super) const fn rawcode_9_27(self) -> u32 {
                match self { $(Self::$variant => u32::from_be_bytes(*$rawcode)),+ }
            }

            pub(super) fn from_retained_rawcode_9_27(rawcode: u32) -> Option<Self> {
                Self::ALL.into_iter().find(|kind| kind.rawcode_9_27() == rawcode)
            }
        }
    };
    (@count $($variant:ident),+) => { <[()]>::len(&[$(roster!(@one $variant)),+]) };
    (@one $variant:ident) => { () };
}

roster!(CastleFightUnitKind, CastleFightUnitId;
    Footman = 0x1000_0001 => b"hfoo",
    Defender = 0x1000_0002 => b"h03A",
    Sniper = 0x1000_0007 => b"hrif",
    Mortar = 0x1000_0008 => b"hmtm",
    HeavyGunner = 0x1000_0009 => b"h0A2",
    Marksman = 0x1000_000a => b"h05C",
    Ranger = 0x1000_0003 => b"e003",
    Catapult = 0x1000_0004 => b"o001",
    IceTrollShadowPriest = 0x1000_0005 => b"n015",
    GryphonRider = 0x1000_0006 => b"h016",
    Crusader = 0x1000_000b => b"h03B",
    Paladin = 0x1000_000c => b"h03C",
    HolyWarrior = 0x1000_000d => b"h074",
    Warlock = 0x1000_000e => b"n005",
    Archer = 0x1000_000f => b"n022",
    MasterArcher = 0x1000_0010 => b"n023",
    Blademaster = 0x1000_0011 => b"n006",
    ElderBlademaster = 0x1000_0012 => b"n00Y",
    Bloodthirster = 0x1000_0013 => b"h00U",
    Ballista = 0x1000_0014 => b"e005",
    DragonhawkRider = 0x1000_0015 => b"n01Y",
    Sorceress = 0x1000_0016 => b"h07B",
    Wizard = 0x1000_0017 => b"h00W",
    FrostWolf = 0x1000_0018 => b"n017",
    PolarBear = 0x1000_0019 => b"n018",
    Magnataur = 0x1000_001a => b"n016",
    Hrimthrusa = 0x1000_001b => b"n011",
    IceQueen = 0x1000_0021 => b"n014",
    AzureDrake = 0x1000_0020 => b"n010",
    AncientWandigoo = 0x1000_001f => b"n013",
    Wandigoo = 0x1000_001e => b"n012",
    IceTrollWitchDoctor = 0x1000_001d => b"o00A",
    AngryHrimthrusa = 0x1000_001c => b"n02I",
    Grunt = 0x1000_0022 => b"o002",
);

roster!(CastleFightProductionKind, CastleFightBuildingId;
    Barracks = 0x2000_0001 => b"h000",
    Stronghold = 0x2000_0002 => b"h039",
    SniperNest = 0x2000_0007 => b"h003",
    WeaponLab = 0x2000_0008 => b"h004",
    GunnersHall = 0x2000_0009 => b"h05D",
    MarksmensEncampment = 0x2000_000a => b"h0A1",
    RangersHall = 0x2000_0003 => b"h03D",
    OrcishSiegeFactory = 0x2000_0004 => b"h02I",
    IceTrollHut = 0x2000_0005 => b"h03K",
    GryphonRock = 0x2000_0006 => b"h015",
    Chapel = 0x2000_000b => b"h037",
    Church = 0x2000_000c => b"h038",
    HolyAltar = 0x2000_000d => b"h072",
    Hjordhejmen = 0x2000_000e => b"h00K",
    ArcheryRange = 0x2000_000f => b"h08X",
    ArcheryTower = 0x2000_0010 => b"h08Y",
    HallOfHonor = 0x2000_0011 => b"h00T",
    HallOfTheEldest = 0x2000_0012 => b"h03F",
    BloodelfWarAcademy = 0x2000_0013 => b"h00V",
    HighelfSiegeFactory = 0x2000_0014 => b"h070",
    DragonhawkPortal = 0x2000_0015 => b"h06Y",
    SchoolOfWizardry = 0x2000_0016 => b"h09X",
    TowerOfSupremeMagic = 0x2000_0017 => b"h00X",
    SnowyRocks = 0x2000_0018 => b"h049",
    IcyRocks = 0x2000_0019 => b"h04F",
    Glacier = 0x2000_001a => b"h03W",
    Igloo = 0x2000_001b => b"h03U",
    CrystalPalace = 0x2000_0021 => b"h03S",
    AzureNest = 0x2000_0020 => b"h03I",
    FrostClaws = 0x2000_001f => b"h043",
    IceClaws = 0x2000_001e => b"h03T",
    IceTrollVoodooLounge = 0x2000_001d => b"h03J",
    ModernIgloo = 0x2000_001c => b"h06L",
    FightersHall = 0x2000_0022 => b"h029",
);

roster!(CastleFightTowerKind, CastleFightBuildingId;
    WorldFreezer = 0x2100_000f => b"h03O",
    IcyTower = 0x2100_000e => b"h03Q",
    ChillingMushroom = 0x2100_000d => b"h047",
    FrostLauncher = 0x2100_000b => b"h048",
    GreaterFrostLauncher = 0x2100_000c => b"h03L",
    SnowveilFountain = 0x2100_000a => b"h07W",
    WatchTower = 0x2100_0001 => b"h006",
    PoofTower = 0x2100_0002 => b"h07P",
    Artillery = 0x2100_0003 => b"h001",
    Gjallarhorn = 0x2100_0004 => b"h010",
    VesselOfPurity = 0x2100_0005 => b"h07U",
    HeroicShrine = 0x2100_0006 => b"h05G",
    TreasureBox = 0x2100_0009 => b"h008",
    GoldenShrineOfJustice = 0x3000_000d => b"h059",
    TinyWatchTower = 0x2100_0007 => b"h081",
    TinyMultishotTower = 0x2100_0008 => b"h082",
    CityOfMagic = 0x3000_000a => b"h00Z",
    ArcaneTower = 0x3000_000b => b"h014",
    ObeliskOfLight = 0x3000_000c => b"h005",
);

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn promoted_identities_are_unique_and_rawcode_lookup_round_trips() {
        let mut ids = BTreeSet::new();
        let mut rawcodes = BTreeSet::new();
        for kind in CastleFightUnitKind::ALL {
            assert!(ids.insert(kind.stable_id().0));
            assert!(rawcodes.insert(kind.rawcode_9_27()));
            assert_eq!(
                CastleFightUnitKind::from_retained_rawcode_9_27(kind.rawcode_9_27()),
                Some(kind)
            );
        }
        for kind in CastleFightProductionKind::ALL {
            assert!(ids.insert(kind.stable_id().0));
            assert!(rawcodes.insert(kind.rawcode_9_27()));
            assert_eq!(
                CastleFightProductionKind::from_retained_rawcode_9_27(kind.rawcode_9_27()),
                Some(kind)
            );
        }
        for kind in CastleFightTowerKind::ALL {
            assert!(ids.insert(kind.stable_id().0));
            assert!(rawcodes.insert(kind.rawcode_9_27()));
            assert_eq!(
                CastleFightTowerKind::from_retained_rawcode_9_27(kind.rawcode_9_27()),
                Some(kind)
            );
        }
        assert!(CastleFightUnitKind::from_retained_rawcode_9_27(0).is_none());
        assert!(CastleFightProductionKind::from_retained_rawcode_9_27(0).is_none());
        assert!(CastleFightTowerKind::from_retained_rawcode_9_27(0).is_none());
    }
}
