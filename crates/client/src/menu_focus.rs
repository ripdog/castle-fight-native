//! Keyboard navigation and consistent hover/focus/pressed feedback for the full-screen menus.
//!
//! Every menu overlay root carries a [`MenuOverlay`]; the one with the highest priority owns the
//! keyboard. `Tab`/`Shift+Tab` and the arrow keys move a focus ring between its buttons and
//! `Enter`/`Space` press the focused one by setting its [`Interaction`], which the menus already
//! react to for mouse clicks.
use bevy::{
    input::{
        ButtonState,
        keyboard::{Key, KeyboardInput},
    },
    prelude::*,
    ui::UiSystems,
};

const FOCUS_RING: Color = Color::srgb(0.45, 0.79, 0.89);
const PRESSED: Color = Color::srgb(0.30, 0.42, 0.52);

pub(crate) struct MenuFocusPlugin;

impl Plugin for MenuFocusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuFocus>()
            .add_message::<KeyboardInput>()
            .add_systems(PreUpdate, navigate_menu_buttons.after(UiSystems::Focus))
            .add_systems(Update, (track_menu_buttons, style_menu_buttons).chain());
    }
}

/// Marks the root node of a menu overlay. A higher priority sits on top and owns the keyboard.
#[derive(Component, Clone, Copy)]
pub(crate) struct MenuOverlay(pub(crate) u8);

/// A button that currently cannot be used; it never takes focus or shows press feedback.
#[derive(Component)]
pub(crate) struct ButtonDisabled;

/// The button's own border colour, restored when the focus ring leaves it.
#[derive(Component)]
struct RestingBorder(BorderColor);

#[derive(Resource, Default)]
pub(crate) struct MenuFocus {
    focused: Option<Entity>,
}

impl MenuFocus {
    /// Whether a button has keyboard focus (so `Enter` should press it, not confirm a text box).
    pub(crate) fn has_focus(&self) -> bool {
        self.focused.is_some()
    }
}

fn top_overlay(overlays: &Query<(Entity, &MenuOverlay)>) -> Option<Entity> {
    overlays
        .iter()
        .max_by_key(|(_, overlay)| overlay.0)
        .map(|(entity, _)| entity)
}

/// Buttons under `root` in layout order.
fn collect_buttons(
    entity: Entity,
    children: &Query<&Children>,
    buttons: &Query<(), (With<Button>, Without<ButtonDisabled>)>,
    out: &mut Vec<Entity>,
) {
    if buttons.contains(entity) {
        out.push(entity);
    }
    if let Ok(kids) = children.get(entity) {
        for kid in kids.iter() {
            collect_buttons(kid, children, buttons, out);
        }
    }
}

fn navigate_menu_buttons(
    mut keyboard: MessageReader<KeyboardInput>,
    keys: Option<Res<ButtonInput<KeyCode>>>,
    overlays: Query<(Entity, &MenuOverlay)>,
    children: Query<&Children>,
    buttons: Query<(), (With<Button>, Without<ButtonDisabled>)>,
    mut interactions: Query<&mut Interaction>,
    mut focus: ResMut<MenuFocus>,
) {
    let Some(root) = top_overlay(&overlays) else {
        focus.focused = None;
        keyboard.clear();
        return;
    };
    let mut order = Vec::new();
    collect_buttons(root, &children, &buttons, &mut order);
    if focus
        .focused
        .is_some_and(|focused| !order.contains(&focused))
    {
        focus.focused = None;
    }
    let shift = keys
        .is_some_and(|keys| keys.pressed(KeyCode::ShiftLeft) || keys.pressed(KeyCode::ShiftRight));
    for event in keyboard.read() {
        if event.state != ButtonState::Pressed || order.is_empty() {
            continue;
        }
        let position = focus
            .focused
            .and_then(|focused| order.iter().position(|entity| *entity == focused));
        let step = |forward: bool| match (position, forward) {
            (None, true) => 0,
            (None, false) => order.len() - 1,
            (Some(index), true) => (index + 1) % order.len(),
            (Some(index), false) => (index + order.len() - 1) % order.len(),
        };
        match &event.logical_key {
            Key::Tab => focus.focused = Some(order[step(!shift)]),
            Key::ArrowDown | Key::ArrowRight => focus.focused = Some(order[step(true)]),
            Key::ArrowUp | Key::ArrowLeft => focus.focused = Some(order[step(false)]),
            Key::Enter | Key::Space => {
                if let Some(focused) = focus.focused
                    && let Ok(mut interaction) = interactions.get_mut(focused)
                {
                    *interaction = Interaction::Pressed;
                }
            }
            _ => {}
        }
    }
}

/// Remembers each overlay button's resting border so the focus ring can be taken off again.
fn track_menu_buttons(
    mut commands: Commands,
    overlays: Query<Entity, With<MenuOverlay>>,
    children: Query<&Children>,
    buttons: Query<&BorderColor, (With<Button>, Without<RestingBorder>)>,
) {
    for root in &overlays {
        for entity in children.iter_descendants(root) {
            if let Ok(border) = buttons.get(entity) {
                commands.entity(entity).insert(RestingBorder(*border));
            }
        }
    }
}

type StyledButton<'a> = (
    Entity,
    &'a Interaction,
    &'a mut BorderColor,
    &'a mut BackgroundColor,
    &'a RestingBorder,
    Has<ButtonDisabled>,
);

fn style_menu_buttons(focus: Res<MenuFocus>, mut buttons: Query<StyledButton, With<Button>>) {
    for (entity, interaction, mut border, mut background, resting, disabled) in &mut buttons {
        let ring = !disabled
            && (focus.focused == Some(entity)
                || matches!(interaction, Interaction::Hovered | Interaction::Pressed));
        let wanted = if ring {
            BorderColor::all(FOCUS_RING)
        } else {
            resting.0
        };
        if *border != wanted {
            *border = wanted;
        }
        // Menu handlers repaint on hover changes; a held press is shown here.
        if !disabled && *interaction == Interaction::Pressed && background.0 != PRESSED {
            background.0 = PRESSED;
        }
    }
}
