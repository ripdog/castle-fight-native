use super::{MapVersion, evidence};

#[derive(Debug, Clone, Copy, serde::Deserialize)]
pub(crate) struct ProductionReplicationProfile {
    pub shrine_rawcode: u32,
    pub legendary_exclusion: u32,
    pub marker: u32,
    pub local_extra_copies: usize,
    pub extra_copies_per_shrine: usize,
    pub maximum_shrines: usize,
    pub chance_per_100: u64,
}

impl ProductionReplicationProfile {
    pub fn local_copies(self, marked: bool) -> usize {
        1 + if marked { self.local_extra_copies } else { 0 }
    }

    pub fn shrine_copies(self, marked: bool) -> usize {
        1 + if marked {
            self.extra_copies_per_shrine
        } else {
            0
        }
    }
}

pub(crate) fn production_replication_for_version(
    version: MapVersion,
) -> ProductionReplicationProfile {
    let data = evidence(version);
    static PROFILE: std::sync::OnceLock<ProductionReplicationProfile> = std::sync::OnceLock::new();
    *PROFILE.get_or_init(|| {
        serde_json::from_value(data["production_replication"].clone())
            .expect("retained production replication")
    })
}
