use std::{fs, path::PathBuf};

use bevy::{
    prelude::*,
    window::{
        CursorGrabMode, CursorOptions, MonitorSelection, PrimaryWindow, WindowFocused, WindowMode,
        WindowPosition,
    },
};
use serde::{Deserialize, Serialize};

const DEFAULT_WIDTH: u32 = 1440;
const DEFAULT_HEIGHT: u32 = 900;

/// Client-only display preferences; fullscreen never replaces the saved windowed geometry.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct ViewState {
    fullscreen: bool,
    pub(crate) width: u32,
    pub(crate) height: u32,
    position: Option<[i32; 2]>,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            fullscreen: true,
            width: DEFAULT_WIDTH,
            height: DEFAULT_HEIGHT,
            position: None,
        }
    }
}

impl ViewState {
    pub(crate) fn load() -> Self {
        let path = config_path();
        match fs::read(&path) {
            Ok(bytes) => match serde_json::from_slice::<Self>(&bytes) {
                Ok(state) if state.valid() => state,
                Ok(_) => {
                    eprintln!("invalid view state in {}; using defaults", path.display());
                    Self::default()
                }
                Err(error) => {
                    eprintln!("cannot parse view state in {}: {error}", path.display());
                    Self::default()
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => {
                eprintln!("cannot read view state in {}: {error}", path.display());
                Self::default()
            }
        }
    }

    fn valid(self) -> bool {
        (1..=16384).contains(&self.width) && (1..=16384).contains(&self.height)
    }

    pub(crate) fn window_mode(self) -> WindowMode {
        if self.fullscreen {
            WindowMode::BorderlessFullscreen(MonitorSelection::Current)
        } else {
            WindowMode::Windowed
        }
    }

    pub(crate) fn window_position(self) -> WindowPosition {
        self.position.map_or(WindowPosition::Automatic, |[x, y]| {
            WindowPosition::At(IVec2::new(x, y))
        })
    }

    fn observe_window(&mut self, window: &Window) {
        self.fullscreen = window.mode != WindowMode::Windowed;
        if !self.fullscreen {
            let width = window.resolution.physical_width();
            let height = window.resolution.physical_height();
            if width > 0 && height > 0 {
                self.width = width;
                self.height = height;
            }
            if let WindowPosition::At(position) = window.position {
                self.position = Some([position.x, position.y]);
            }
        }
    }
}

#[derive(Resource)]
pub(crate) struct ViewStatePersistence {
    state: ViewState,
    saved: ViewState,
    path: PathBuf,
}

impl ViewStatePersistence {
    pub(crate) fn new(state: ViewState) -> Self {
        Self {
            state,
            saved: state,
            path: config_path(),
        }
    }

    fn save_if_changed(&mut self) {
        if self.state == self.saved {
            return;
        }
        let result = (|| -> Result<(), Box<dyn std::error::Error>> {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&self.path, serde_json::to_vec_pretty(&self.state)?)?;
            Ok(())
        })();
        match result {
            Ok(()) => self.saved = self.state,
            Err(error) => eprintln!("cannot save view state in {}: {error}", self.path.display()),
        }
    }
}

fn config_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("APPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")));
    base.unwrap_or_else(|| PathBuf::from("."))
        .join("castle-fight-native")
        .join("view-state.json")
}

pub(crate) fn toggle_fullscreen(
    keys: Res<ButtonInput<KeyCode>>,
    mut window: Single<&mut Window, With<PrimaryWindow>>,
    mut persistence: ResMut<ViewStatePersistence>,
) {
    let alt = keys.pressed(KeyCode::AltLeft) || keys.pressed(KeyCode::AltRight);
    if !alt || !(keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter)) {
        return;
    }
    if window.mode == WindowMode::Windowed {
        persistence.state.observe_window(&window);
        window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
    } else {
        window.mode = WindowMode::Windowed;
        window
            .resolution
            .set_physical_resolution(persistence.state.width, persistence.state.height);
        window.position = persistence.state.window_position();
    }
}

/// Keep the pointer at the screen edge for camera scrolling while fullscreen.
pub(crate) fn sync_cursor_grab(
    mut focus_events: MessageReader<WindowFocused>,
    window: Single<(Entity, &Window, &mut CursorOptions), With<PrimaryWindow>>,
) {
    let (entity, window, mut cursor) = window.into_inner();
    let desired = if window.mode == WindowMode::Windowed {
        CursorGrabMode::None
    } else {
        CursorGrabMode::Confined
    };
    let refocused = focus_events
        .read()
        .filter(|event| event.window == entity)
        .last()
        .is_some_and(|event| event.focused);
    if cursor.grab_mode != desired || (refocused && desired == CursorGrabMode::Confined) {
        cursor.grab_mode = desired;
    }
}

pub(crate) fn persist_view_state(
    window: Single<&Window, With<PrimaryWindow>>,
    mut persistence: ResMut<ViewStatePersistence>,
) {
    persistence.state.observe_window(&window);
    persistence.save_if_changed();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullscreen_keeps_windowed_geometry() {
        let mut state = ViewState::default();
        let mut window = Window {
            mode: WindowMode::Windowed,
            resolution: (1280, 720).into(),
            position: WindowPosition::At(IVec2::new(32, 48)),
            ..default()
        };
        state.observe_window(&window);
        window.mode = WindowMode::BorderlessFullscreen(MonitorSelection::Current);
        window.resolution.set(2560.0, 1440.0);
        state.observe_window(&window);
        assert!(state.fullscreen);
        assert_eq!((state.width, state.height), (1280, 720));
        assert_eq!(state.position, Some([32, 48]));
    }

    #[test]
    fn rejects_invalid_saved_dimensions() {
        let state = ViewState {
            width: 0,
            ..ViewState::default()
        };
        assert!(!state.valid());
    }

    #[test]
    fn saved_view_state_restores_mode_and_geometry() {
        let state = ViewState {
            fullscreen: false,
            width: 1600,
            height: 1000,
            position: Some([120, 80]),
        };
        let encoded = serde_json::to_vec(&state).unwrap();
        let restored: ViewState = serde_json::from_slice(&encoded).unwrap();
        assert!(restored.valid());
        assert_eq!(restored.window_mode(), WindowMode::Windowed);
        assert_eq!(
            restored.window_position(),
            WindowPosition::At(IVec2::new(120, 80))
        );
        assert_eq!((restored.width, restored.height), (1600, 1000));
    }

    #[test]
    fn alt_enter_toggles_fullscreen_and_restores_windowed_size() {
        let mut app = App::new();
        app.insert_resource(ButtonInput::<KeyCode>::default())
            .insert_resource(ViewStatePersistence::new(ViewState::default()))
            .add_message::<WindowFocused>()
            .add_systems(Update, (toggle_fullscreen, sync_cursor_grab).chain());
        let window_entity = app
            .world_mut()
            .spawn((
                Window {
                    mode: WindowMode::BorderlessFullscreen(MonitorSelection::Current),
                    resolution: (2560, 1440).into(),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.update();
        assert_eq!(
            app.world()
                .get::<CursorOptions>(window_entity)
                .unwrap()
                .grab_mode,
            CursorGrabMode::Confined
        );
        let mut keys = app.world_mut().resource_mut::<ButtonInput<KeyCode>>();
        keys.press(KeyCode::AltLeft);
        keys.press(KeyCode::Enter);
        app.update();
        let window = app.world().get::<Window>(window_entity).unwrap();
        assert_eq!(window.mode, WindowMode::Windowed);
        assert_eq!(window.resolution.physical_width(), DEFAULT_WIDTH);
        assert_eq!(window.resolution.physical_height(), DEFAULT_HEIGHT);
        assert_eq!(
            app.world()
                .get::<CursorOptions>(window_entity)
                .unwrap()
                .grab_mode,
            CursorGrabMode::None
        );
    }

    #[test]
    fn fullscreen_reapplies_cursor_grab_after_focus_returns() {
        let mut app = App::new();
        app.add_message::<WindowFocused>()
            .add_systems(Update, sync_cursor_grab);
        let window_entity = app
            .world_mut()
            .spawn((
                Window {
                    mode: WindowMode::BorderlessFullscreen(MonitorSelection::Current),
                    ..default()
                },
                PrimaryWindow,
            ))
            .id();
        app.update();
        let grab_changed = app
            .world()
            .entity(window_entity)
            .get_change_ticks::<CursorOptions>()
            .unwrap()
            .changed;

        app.world_mut().write_message(WindowFocused {
            window: window_entity,
            focused: false,
        });
        app.update();
        assert_eq!(
            app.world()
                .entity(window_entity)
                .get_change_ticks::<CursorOptions>()
                .unwrap()
                .changed,
            grab_changed
        );

        app.world_mut().write_message(WindowFocused {
            window: window_entity,
            focused: true,
        });
        app.update();
        let window = app.world().entity(window_entity);
        assert_eq!(
            window.get::<CursorOptions>().unwrap().grab_mode,
            CursorGrabMode::Confined
        );
        assert_ne!(
            window.get_change_ticks::<CursorOptions>().unwrap().changed,
            grab_changed
        );
    }
}
