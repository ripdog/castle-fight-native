//! Change-detection-friendly writes for UI components refreshed every frame.
//!
//! Text measurement and UI layout only redo work for components marked changed, so per-frame UI
//! systems write through these helpers, which leave a component untouched when its value is
//! already current.

use bevy::prelude::*;

pub(crate) trait SetFieldIfNeq<T> {
    /// Sets the field `field` selects to `value`, marking the component changed only if the field
    /// held a different value.
    fn set_field_if_neq<V: PartialEq>(&mut self, field: impl FnOnce(&mut T) -> &mut V, value: V);
}

impl<T> SetFieldIfNeq<T> for Mut<'_, T> {
    fn set_field_if_neq<V: PartialEq>(&mut self, field: impl FnOnce(&mut T) -> &mut V, value: V) {
        let slot = field(self.bypass_change_detection());
        if *slot != value {
            *slot = value;
            self.set_changed();
        }
    }
}

/// Shows `image` on an image node, or nothing, touching the node only when the image differs.
pub(crate) fn set_ui_image(node: &mut Mut<ImageNode>, image: Option<Handle<Image>>) {
    let image = image.unwrap_or_else(|| ImageNode::default().image);
    node.set_field_if_neq(|node| &mut node.image, image);
}
