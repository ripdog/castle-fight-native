//! Local client preferences that survive restarts (never part of the match or the network).
use std::{fs, path::PathBuf};

use bevy::prelude::Resource;
use serde::{Deserialize, Serialize};

use crate::{ui_icons::ConsoleTheme, view_state::config_directory};

pub(crate) const DEFAULT_JOIN_ADDRESS: &str = "127.0.0.1";

#[derive(Resource, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct ClientSettings {
    /// The server address last joined successfully, offered again next time.
    pub(crate) last_join_address: String,
    /// TCP port Host Game listens on.
    pub(crate) host_port: u16,
    /// Display name sent when joining. Empty means "use the account name".
    pub(crate) player_name: String,
    /// Master volume for all sound, 0.0 to 1.0.
    pub(crate) master_volume: f32,
    /// Scroll the camera when the pointer touches the window edge.
    pub(crate) edge_scroll: bool,
    /// Multiplier on the camera pan speed.
    pub(crate) pan_speed: f32,
    /// Multiplier on mouse-wheel zoom.
    pub(crate) zoom_speed: f32,
    /// Use an orange/blue pair that stays distinguishable with red-green colour blindness.
    pub(crate) colour_blind_friendly: bool,
    /// Warcraft III race whose console art frames the HUD.
    pub(crate) ui_theme: ConsoleTheme,
}

pub(crate) const MIN_SPEED_MULTIPLIER: f32 = 0.5;
pub(crate) const MAX_SPEED_MULTIPLIER: f32 = 2.0;

impl Default for ClientSettings {
    fn default() -> Self {
        Self {
            last_join_address: DEFAULT_JOIN_ADDRESS.to_owned(),
            host_port: castle_fight_server::DEFAULT_GAME_PORT,
            player_name: String::new(),
            master_volume: 1.0,
            edge_scroll: true,
            pan_speed: 1.0,
            zoom_speed: 1.0,
            colour_blind_friendly: false,
            ui_theme: ConsoleTheme::default(),
        }
    }
}

impl ClientSettings {
    fn path() -> PathBuf {
        config_directory().join("settings.json")
    }

    /// Pulls hand-edited or corrupt values back into their supported ranges.
    fn sanitized(mut self) -> Self {
        let clamp = |value: f32, min: f32, max: f32| {
            if value.is_finite() {
                value.clamp(min, max)
            } else {
                min
            }
        };
        self.master_volume = clamp(self.master_volume, 0.0, 1.0);
        self.pan_speed = clamp(self.pan_speed, MIN_SPEED_MULTIPLIER, MAX_SPEED_MULTIPLIER);
        self.zoom_speed = clamp(self.zoom_speed, MIN_SPEED_MULTIPLIER, MAX_SPEED_MULTIPLIER);
        self.player_name = castle_fight_protocol::sanitize_player_name(&self.player_name);
        self
    }

    /// Missing or unreadable settings fall back to the defaults rather than blocking the menu.
    pub(crate) fn load() -> Self {
        let path = Self::path();
        match fs::read(&path) {
            Ok(bytes) => serde_json::from_slice::<Self>(&bytes)
                .map(Self::sanitized)
                .unwrap_or_else(|error| {
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
