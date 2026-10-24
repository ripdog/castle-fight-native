//! Transient on-screen messages (command rejections, network events) and the persistent
//! connection-lost banner.
//!
//! The action panel's single status line is overwritten by the next message, so rejections such as
//! "Not enough gold" used to vanish immediately. Messages here stack for a few seconds instead.
use std::collections::VecDeque;

use bevy::prelude::*;
use castle_fight_sim::{
    BuilderBuildError, BuilderCommandError, BuildingCommandError, BuildingConstructionCancelError,
    BuildingPlacementError, BuildingUpgradeError, CommandRejectReason, MatchLifecycle,
    ResourcePurchaseError,
};

use crate::{
    AuthoritativeSimulation, AuthorityMode,
    console::{TOP_BAR_UNITS, wc3},
    game_menu::MatchSessionReset,
    lobby::LobbyState,
    main_menu::MainMenuState,
};

const TOAST_SECONDS: f32 = 5.0;
const MAX_TOASTS: usize = 5;
const TEXT: Color = Color::srgb(0.95, 0.95, 0.92);
const WARNING: Color = Color::srgb(1.0, 0.78, 0.35);
const ERROR: Color = Color::srgb(1.0, 0.45, 0.4);

pub(crate) struct NotificationsPlugin;

impl Plugin for NotificationsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Notifications>()
            .add_message::<MatchSessionReset>()
            .add_systems(Startup, setup_notifications)
            .add_systems(
                Update,
                (
                    clear_notifications_for_new_match,
                    expire_toasts,
                    redraw_toasts,
                    update_connection_banner,
                )
                    .chain(),
            );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Severity {
    Info,
    Warning,
}

struct Toast {
    text: String,
    severity: Severity,
    remaining: f32,
}

#[derive(Resource, Default)]
pub(crate) struct Notifications {
    toasts: VecDeque<Toast>,
    dirty: bool,
}

impl Notifications {
    /// Shows `text` for a few seconds. Repeating the newest message restarts its timer instead of
    /// stacking duplicates.
    pub(crate) fn push(&mut self, severity: Severity, text: impl Into<String>) {
        let text = text.into();
        self.dirty = true;
        if let Some(last) = self.toasts.back_mut()
            && last.text == text
        {
            last.remaining = TOAST_SECONDS;
            return;
        }
        if self.toasts.len() == MAX_TOASTS {
            self.toasts.pop_front();
        }
        self.toasts.push_back(Toast {
            text,
            severity,
            remaining: TOAST_SECONDS,
        });
    }
}

#[derive(Component)]
struct ToastRoot;

#[derive(Component)]
struct ConnectionBanner;

#[derive(Component)]
struct ConnectionBannerText;

fn setup_notifications(mut commands: Commands) {
    commands.spawn((
        ToastRoot,
        Node {
            position_type: PositionType::Absolute,
            top: wc3(TOP_BAR_UNITS),
            margin: UiRect::top(px(48.0 - 2.0)),
            left: percent(25.0),
            width: percent(50.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: px(4.0),
            ..default()
        },
        GlobalZIndex(200),
        Pickable::IGNORE,
    ));
    commands
        .spawn((
            ConnectionBanner,
            Node {
                position_type: PositionType::Absolute,
                top: wc3(TOP_BAR_UNITS),
                margin: UiRect::top(px(90.0 - 2.0)),
                left: percent(20.0),
                width: percent(60.0),
                padding: UiRect::axes(px(14.0), px(10.0)),
                border: UiRect::all(px(1.0)),
                justify_content: JustifyContent::Center,
                display: Display::None,
                ..default()
            },
            BackgroundColor(Color::srgba(0.28, 0.05, 0.04, 0.92)),
            BorderColor::all(ERROR),
            GlobalZIndex(210),
            Pickable::IGNORE,
        ))
        .with_child((
            ConnectionBannerText,
            Text::new(""),
            TextFont::from_font_size(18.0),
            TextColor(TEXT),
            TextLayout::justify(Justify::Center),
        ));
}

fn clear_notifications_for_new_match(
    mut resets: MessageReader<MatchSessionReset>,
    mut notifications: ResMut<Notifications>,
) {
    if resets.read().count() > 0 {
        notifications.toasts.clear();
        notifications.dirty = true;
    }
}

fn expire_toasts(time: Res<Time>, mut notifications: ResMut<Notifications>) {
    if notifications.toasts.is_empty() {
        return;
    }
    let delta = time.delta_secs();
    for toast in &mut notifications.toasts {
        toast.remaining -= delta;
    }
    let before = notifications.toasts.len();
    notifications.toasts.retain(|toast| toast.remaining > 0.0);
    if notifications.toasts.len() != before {
        notifications.dirty = true;
    }
}

fn redraw_toasts(
    mut commands: Commands,
    mut notifications: ResMut<Notifications>,
    root: Single<(Entity, Option<&Children>), With<ToastRoot>>,
) {
    if !std::mem::take(&mut notifications.dirty) {
        return;
    }
    let (root, children) = *root;
    for child in children.into_iter().flatten() {
        commands.entity(*child).despawn();
    }
    commands.entity(root).with_children(|list| {
        for toast in &notifications.toasts {
            let color = match toast.severity {
                Severity::Info => TEXT,
                Severity::Warning => WARNING,
            };
            list.spawn((
                Node {
                    padding: UiRect::axes(px(12.0), px(5.0)),
                    border: UiRect::all(px(1.0)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.04, 0.035, 0.03, 0.88)),
                BorderColor::all(color.with_alpha(0.55)),
                Pickable::IGNORE,
            ))
            .with_child((
                Text::new(toast.text.clone()),
                TextFont::from_font_size(16.0),
                TextColor(color),
            ));
        }
    });
}

/// A persistent warning while the connection to the server is down, with the reason.
fn update_connection_banner(
    authoritative: Res<AuthoritativeSimulation>,
    lobby: Option<Res<LobbyState>>,
    main_menu: Option<Res<MainMenuState>>,
    mut banner: Single<&mut Node, With<ConnectionBanner>>,
    mut text: Single<&mut Text, With<ConnectionBannerText>>,
) {
    let on_match_screen = !lobby.as_ref().is_some_and(|lobby| lobby.active())
        && !main_menu.as_ref().is_some_and(|menu| menu.active());
    let finished = matches!(
        authoritative.simulation.lifecycle(),
        MatchLifecycle::Finished { .. }
    );
    let down = matches!(
        authoritative.authority,
        AuthorityMode::Network {
            connected: false,
            ..
        }
    );
    if !(on_match_screen && down && !finished) {
        banner.display = Display::None;
        return;
    }
    banner.display = Display::Flex;
    let reason = authoritative
        .connection_issue
        .as_deref()
        .map(|reason| format!("\n{reason}"))
        .unwrap_or_default();
    text.0 =
        format!("CONNECTION LOST - trying to reconnect...{reason}\nPress F10 to leave the match.");
}

/// Plain-language explanation of why the simulation refused a command.
pub(crate) fn rejection_text(reason: CommandRejectReason) -> String {
    match reason {
        CommandRejectReason::Builder(error) => builder_error(error).to_owned(),
        CommandRejectReason::Build(error) => match error {
            BuilderBuildError::TypeLimitReached => {
                "You already have the maximum number of that building.".to_owned()
            }
            BuilderBuildError::Builder(error) => builder_error(error).to_owned(),
            BuilderBuildError::TeamMismatch => "That building belongs to another team.".to_owned(),
            BuilderBuildError::MissingBuildingIdentity
            | BuilderBuildError::MissingEconomyProfile
            | BuilderBuildError::BuildingNotInCatalog => {
                "Your builder cannot build that.".to_owned()
            }
            BuilderBuildError::Resources(error) => resource_error(error),
            BuilderBuildError::Placement(error) => placement_error(error).to_owned(),
        },
        CommandRejectReason::CancelConstruction(error) => match error {
            BuildingConstructionCancelError::ConstructionNotFound => {
                "That construction is already finished or gone.".to_owned()
            }
            BuildingConstructionCancelError::NotOwner => "You do not own that building.".to_owned(),
        },
        CommandRejectReason::Upgrade(error) => upgrade_error(error),
        CommandRejectReason::Building(error) => building_error(error).to_owned(),
        CommandRejectReason::UnknownBuildingDefinition(_)
        | CommandRejectReason::SourceDefinitionMismatch => {
            "That building is not available.".to_owned()
        }
        CommandRejectReason::InvalidBuildPosition(_) => {
            "That is not a valid building position.".to_owned()
        }
    }
}

fn builder_error(error: BuilderCommandError) -> &'static str {
    match error {
        BuilderCommandError::OrderQueueFull => "The builder's order queue is full.",
        BuilderCommandError::BuilderNotFound => "That builder no longer exists.",
        BuilderCommandError::NotAuthorized => "You cannot control that builder.",
        BuilderCommandError::OutsideBuildRegion => "That spot is outside your build area.",
        BuilderCommandError::BlinkOutOfRange => "Too far to blink there.",
        BuilderCommandError::FollowTargetNotFound => "The unit to follow is gone.",
        BuilderCommandError::RepairTargetNotFound => "The repair target is gone.",
        BuilderCommandError::NotFriendlyRepairTarget => "You can only repair friendly targets.",
        BuilderCommandError::RepairTargetNotRepairable => "That cannot be repaired.",
    }
}

fn resource_error(error: ResourcePurchaseError) -> String {
    match error {
        ResourcePurchaseError::InsufficientGold {
            available,
            required,
        } => format!("Not enough gold ({available}/{required})."),
        ResourcePurchaseError::InsufficientLumber {
            available,
            required,
        } => format!("Not enough lumber ({available}/{required})."),
        ResourcePurchaseError::InsufficientLegendaryPoints {
            available,
            required,
        } => format!("Not enough legendary points ({available}/{required})."),
    }
}

fn placement_error(error: BuildingPlacementError) -> &'static str {
    match error {
        BuildingPlacementError::OutsideNavigation => "That spot is off the map.",
        BuildingPlacementError::OutsideBuildRegion => "That spot is outside your build area.",
        BuildingPlacementError::StaticObstacle => "Something is in the way there.",
        BuildingPlacementError::BuildingOverlap => "That overlaps another building.",
        BuildingPlacementError::BuildingReserved => "Another builder is already building there.",
        BuildingPlacementError::UnitOccupied => "Units are standing there.",
    }
}

fn upgrade_error(error: BuildingUpgradeError) -> String {
    match error {
        BuildingUpgradeError::TypeLimitReached => {
            "You already have the maximum number of that building.".to_owned()
        }
        BuildingUpgradeError::SourceNotFound => "That building no longer exists.".to_owned(),
        BuildingUpgradeError::NotOwner => "You do not own that building.".to_owned(),
        BuildingUpgradeError::SourceUnderConstruction => {
            "Wait for the building to finish construction.".to_owned()
        }
        BuildingUpgradeError::SourceDisabled => "That building is disabled.".to_owned(),
        BuildingUpgradeError::ProductionQueueNotEmpty => {
            "Finish or cancel the production queue first.".to_owned()
        }
        BuildingUpgradeError::SourceDefinitionMismatch
        | BuildingUpgradeError::TeamMismatch
        | BuildingUpgradeError::FootprintMismatch
        | BuildingUpgradeError::MissingEconomyProfile => {
            "That upgrade is not available.".to_owned()
        }
        BuildingUpgradeError::Resources(error) => resource_error(error),
    }
}

fn building_error(error: BuildingCommandError) -> &'static str {
    match error {
        BuildingCommandError::SourceNotFound => "That building no longer exists.",
        BuildingCommandError::NotAuthorized => "You cannot control that building.",
        BuildingCommandError::SourceCannotAttack => "That building cannot attack.",
        BuildingCommandError::SourceCannotProduce => "That building cannot train units.",
        BuildingCommandError::SourceCannotCast => "That building cannot cast spells.",
        BuildingCommandError::InsufficientMana => "Not enough mana.",
        BuildingCommandError::AbilityOnCooldown => "That ability is not ready yet.",
        BuildingCommandError::ProductionQueueEmpty => "Nothing is queued.",
        BuildingCommandError::ProductionQueueFull => "The production queue is full.",
        BuildingCommandError::TargetNotFound => "The target is gone.",
        BuildingCommandError::FriendlyTarget => "That target is friendly.",
        BuildingCommandError::InvalidTargetType => "That cannot be targeted.",
        BuildingCommandError::TargetOutOfRange => "The target is out of range.",
    }
}
