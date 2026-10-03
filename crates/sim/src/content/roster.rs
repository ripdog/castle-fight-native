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
);

roster!(CastleFightTowerKind, CastleFightBuildingId;
    WatchTower = 0x2100_0001 => b"h006",
    PoofTower = 0x2100_0002 => b"h07P",
    Artillery = 0x2100_0003 => b"h001",
    Gjallarhorn = 0x2100_0004 => b"h010",
    VesselOfPurity = 0x2100_0005 => b"h07U",
    HeroicShrine = 0x2100_0006 => b"h05G",
    TreasureBox = 0x2100_0009 => b"h008",
    TinyWatchTower = 0x2100_0007 => b"h081",
    TinyMultishotTower = 0x2100_0008 => b"h082",
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
