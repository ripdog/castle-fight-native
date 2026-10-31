//! Saves consecutive rendered frames as PNGs for objective visual comparison.
//!
//! `--capture-frames <first_tick> <count> <dir>` starts when the presented simulation tick
//! reaches `first_tick`, then spawns one window screenshot per rendered frame until `count`
//! frames have been written, and exits once they are all on disk. Files are named
//! `frame_NNN_tickTTTTT.png`, so frame order and the tick shown are both visible.

use std::path::PathBuf;

use bevy::{
    prelude::*,
    render::view::screenshot::{Screenshot, ScreenshotCaptured, save_to_disk},
};

use crate::bridge::PresentationSamples;

#[derive(Debug, Clone)]
pub(crate) struct FrameCaptureOptions {
    pub(crate) first_tick: u64,
    pub(crate) count: usize,
    pub(crate) directory: PathBuf,
}

pub(crate) struct FrameCapturePlugin(pub(crate) FrameCaptureOptions);

impl Plugin for FrameCapturePlugin {
    fn build(&self, app: &mut App) {
        std::fs::create_dir_all(&self.0.directory)
            .unwrap_or_else(|error| panic!("cannot create capture directory: {error}"));
        app.insert_resource(FrameCapture {
            options: self.0.clone(),
            requested: 0,
            saved: 0,
        })
        .add_systems(Update, capture_frame);
    }
}

#[derive(Resource)]
struct FrameCapture {
    options: FrameCaptureOptions,
    requested: usize,
    saved: usize,
}

fn capture_frame(
    mut capture: ResMut<FrameCapture>,
    presentation: Res<PresentationSamples>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    if capture.saved >= capture.options.count {
        exit.write(AppExit::Success);
        return;
    }
    let tick = presentation.current.tick;
    if capture.requested >= capture.options.count || tick < capture.options.first_tick {
        return;
    }
    let path = capture
        .options
        .directory
        .join(format!("frame_{:03}_tick{tick:05}.png", capture.requested));
    capture.requested += 1;
    commands
        .spawn(Screenshot::primary_window())
        .observe(save_to_disk(path))
        .observe(
            |_: On<ScreenshotCaptured>, mut capture: ResMut<FrameCapture>| {
                capture.saved += 1;
            },
        );
}
