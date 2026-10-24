//! Local client preferences that survive restarts (never part of the match or the network).
use std::{fs, path::PathBuf};

use serde::{Deserialize, Serialize};

use crate::view_state::config_directory;

pub(crate) const DEFAULT_JOIN_ADDRESS: &str = "127.0.0.1";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ClientSettings {
    /// The server address last joined successfully, offered again next time.
    pub(crate) last_join_address: String,
    /// TCP port Host Game listens on.
    pub(crate) host_port: u16,
    /// Display name sent when joining. Empty means "use the account name".
    pub(crate) player_name: String,
}

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            last_join_address: DEFAULT_JOIN_ADDRESS.to_owned(),
            host_port: castle_fight_server::DEFAULT_GAME_PORT,
            player_name: String::new(),
        }
    }
}

impl ClientSettings {
    fn path() -> PathBuf {
        config_directory().join("settings.json")
    }

    /// Missing or unreadable settings fall back to the defaults rather than blocking the menu.
    pub(crate) fn load() -> Self {
        let path = Self::path();
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|error| {
                eprintln!("cannot parse settings in {}: {error}", path.display());
                Self::default()
            }),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                eprintln!("cannot read settings in {}: {error}", path.display());
                Self::default()
            }
        }
    }

    pub(crate) fn save(&self) {
        let path = Self::path();
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&path, serde_json::to_vec_pretty(self)?)?;
            Ok(())
        })();
        if let Err(error) = result {
            eprintln!("cannot save settings in {}: {error}", path.display());
        }
    }
}
