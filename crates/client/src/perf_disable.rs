//! Runtime kill switches for default-on performance optimisations that could change visuals.
//!
//! `--perf-disable offscreen-animation,frozen-bounds,...` turns the named optimisations off so a
//! visual difference can be bisected against the optimised default without rebuilding.

use bevy::prelude::*;

/// Which default-on optimisations are disabled for this run.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct PerfDisable {
    /// Keep evaluating animation poses for players whose meshes are offscreen.
    pub(crate) offscreen_animation: bool,
    /// Keep recomputing skinned mesh bounds from joints every frame.
    pub(crate) frozen_bounds: bool,
    /// Queue zero-alpha and off-frustum particles instead of culling them.
    pub(crate) particle_cull: bool,
    /// Use one depth-sorted phase item per particle instead of per-emitter batches.
    pub(crate) emitter_batching: bool,
    /// Keep evaluating animation targets that no clip of their graph animates.
    pub(crate) unanimated_targets: bool,
}

impl PerfDisable {
    pub(crate) const NAMES: &'static str =
        "offscreen-animation|frozen-bounds|particle-cull|emitter-batching|unanimated-targets|all";

    pub(crate) fn parse(list: &str) -> Result<Self, String> {
        let mut disable = Self::default();
        for name in list.split(',').filter(|name| !name.is_empty()) {
            match name {
                "offscreen-animation" => disable.offscreen_animation = true,
                "frozen-bounds" => disable.frozen_bounds = true,
                "particle-cull" => disable.particle_cull = true,
                "emitter-batching" => disable.emitter_batching = true,
                "unanimated-targets" => disable.unanimated_targets = true,
                "all" => {
                    disable = Self {
                        offscreen_animation: true,
                        frozen_bounds: true,
                        particle_cull: true,
                        emitter_batching: true,
                        unanimated_targets: true,
                    }
                }
                other => {
                    return Err(format!(
                        "unknown --perf-disable name {other:?}; expected a comma list of {}",
                        Self::NAMES
                    ));
                }
            }
        }
        Ok(disable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_comma_lists_and_rejects_unknown_names() {
        let parsed = PerfDisable::parse("frozen-bounds,particle-cull").unwrap();
        assert!(parsed.frozen_bounds && parsed.particle_cull);
        assert!(!parsed.offscreen_animation && !parsed.emitter_batching);
        assert_eq!(PerfDisable::parse("").unwrap(), PerfDisable::default());
        assert!(PerfDisable::parse("frozen-bounds,nope").is_err());
    }
}
