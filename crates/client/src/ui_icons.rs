use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path},
    str::FromStr,
};

use bevy::prelude::*;
use castle_fight_sim::{ArmorType, DamageType, MapVersion};
use serde::Deserialize;

use crate::terrain::client_asset_root;

const UI_ICON_MANIFEST: &str = "wc3/ui/manifest.json";
const UI_ICON_ASSET_PREFIX: &str = "wc3/ui";
const UI_ICON_MANIFEST_SCHEMA_VERSION: u32 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum UiObjectIconKind {
    Unit,
    Ability,
    Buff,
    Item,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum UiIconRole {
    Normal,
    Research,
    TurnOff,
    Buff,
    GameInterface,
    Interface,
    CasterUpgrade,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum UiCommandIcon {
    Move,
    Attack,
    BuildHuman,
    Cancel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum UiResourceIcon {
    Gold,
    Lumber,
    Supply,
    Upkeep,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum UiCursorTheme {
    Human,
    Orc,
    Undead,
    NightElf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum UiIconKey {
    Object {
        kind: UiObjectIconKind,
        rawcode: u32,
        role: UiIconRole,
    },
    Command(UiCommandIcon),
    Resource(UiResourceIcon),
    InfoDamage(DamageType),
    InfoArmor(ArmorType),
}

impl UiIconKey {
    #[must_use]
    pub(crate) const fn unit_game_interface(rawcode: u32) -> Self {
        Self::Object {
            kind: UiObjectIconKind::Unit,
            rawcode,
            role: UiIconRole::GameInterface,
        }
    }

    #[must_use]
    pub(crate) const fn ability(rawcode: u32, role: UiIconRole) -> Self {
        Self::Object {
            kind: UiObjectIconKind::Ability,
            rawcode,
            role,
        }
    }
}

/// Versioned semantic bindings for command-card presentation.
///
/// These keys are intentionally separate from the gameplay content hash: changing art should not
/// invalidate deterministic replays, but selecting a different Castle Fight map version must still
/// select that version's authored presentation bindings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CastleFightPresentationCatalog {
    pub(crate) map_version: MapVersion,
    pub(crate) move_command: UiIconKey,
    pub(crate) attack_command: UiIconKey,
    pub(crate) build_command: UiIconKey,
    pub(crate) cancel_command: UiIconKey,
    pub(crate) repair_command: UiIconKey,
    pub(crate) repair_turn_off_command: UiIconKey,
    pub(crate) blink_command: UiIconKey,
    pub(crate) cursor_theme: UiCursorTheme,
}

impl CastleFightPresentationCatalog {
    #[must_use]
    pub(crate) const fn for_version(version: MapVersion) -> Option<Self> {
        if version.major != 9 || version.minor != 27 {
            return None;
        }
        Some(Self {
            map_version: version,
            move_command: UiIconKey::Command(UiCommandIcon::Move),
            attack_command: UiIconKey::Command(UiCommandIcon::Attack),
            build_command: UiIconKey::Command(UiCommandIcon::BuildHuman),
            cancel_command: UiIconKey::Command(UiCommandIcon::Cancel),
            repair_command: UiIconKey::ability(u32::from_be_bytes(*b"Ahrp"), UiIconRole::Normal),
            repair_turn_off_command: UiIconKey::ability(
                u32::from_be_bytes(*b"Ahrp"),
                UiIconRole::TurnOff,
            ),
            blink_command: UiIconKey::ability(u32::from_be_bytes(*b"A0-1"), UiIconRole::Normal),
            cursor_theme: UiCursorTheme::Human,
        })
    }
}

#[derive(Resource, Default)]
pub(crate) struct UiIconAssets {
    paths: BTreeMap<UiIconKey, String>,
    handles: BTreeMap<UiIconKey, Handle<Image>>,
    cursor_paths: BTreeMap<UiCursorTheme, String>,
    cursor_handles: BTreeMap<UiCursorTheme, Handle<Image>>,
}

impl UiIconAssets {
    #[must_use]
    pub(crate) fn load_for_version(map_version: MapVersion) -> Self {
        let asset_root = client_asset_root();
        let manifest_path = asset_root.join(UI_ICON_MANIFEST);
        if !manifest_path.is_file() {
            return Self::default();
        }

        match load_manifest_entries(&manifest_path, UI_ICON_ASSET_PREFIX) {
            Ok(resolved) if resolved.map_version == map_version => {
                println!(
                    "Loaded {} generated WC3 UI icon binding(s) and {} cursor atlas binding(s) for Castle Fight {}",
                    resolved.paths.len(),
                    resolved.cursor_paths.len(),
                    resolved.map_version
                );
                Self {
                    paths: resolved.paths,
                    handles: BTreeMap::new(),
                    cursor_paths: resolved.cursor_paths,
                    cursor_handles: BTreeMap::new(),
                }
            }
            Ok(resolved) => {
                eprintln!(
                    "warning: ignoring generated WC3 UI icons for Castle Fight {}; selected match is {}",
                    resolved.map_version, map_version
                );
                Self::default()
            }
            Err(error) => {
                eprintln!("warning: ignoring generated WC3 UI icons: {error}");
                Self::default()
            }
        }
    }

    pub(crate) fn image(
        &mut self,
        key: UiIconKey,
        asset_server: &AssetServer,
    ) -> Option<Handle<Image>> {
        if let Some(handle) = self.handles.get(&key) {
            return Some(handle.clone());
        }
        let path = self.paths.get(&key)?.clone();
        let handle = asset_server.load(path);
        self.handles.insert(key, handle.clone());
        Some(handle)
    }

    pub(crate) fn cursor_atlas(
        &mut self,
        theme: UiCursorTheme,
        asset_server: &AssetServer,
    ) -> Option<Handle<Image>> {
        if let Some(handle) = self.cursor_handles.get(&theme) {
            return Some(handle.clone());
        }
        let path = self.cursor_paths.get(&theme)?.clone();
        let handle = asset_server.load(path);
        self.cursor_handles.insert(theme, handle.clone());
        Some(handle)
    }
}

#[derive(Debug, Deserialize)]
struct UiAssetManifest {
    schema_version: u32,
    castle_fight_catalog_version: String,
    assets: Vec<UiBindingManifest>,
}

#[derive(Debug, Deserialize)]
struct UiBindingManifest {
    owner_kind: String,
    owner_rawcode: String,
    role: String,
    png: Option<String>,
}

#[derive(Debug, PartialEq, Eq)]
struct ResolvedUiManifest {
    map_version: MapVersion,
    paths: BTreeMap<UiIconKey, String>,
    cursor_paths: BTreeMap<UiCursorTheme, String>,
}

fn load_manifest_entries(path: &Path, asset_prefix: &str) -> Result<ResolvedUiManifest, String> {
    let json = fs::read_to_string(path)
        .map_err(|error| format!("failed reading {}: {error}", path.display()))?;
    resolve_manifest_entries(&json, asset_prefix)
}

fn resolve_manifest_entries(json: &str, asset_prefix: &str) -> Result<ResolvedUiManifest, String> {
    let manifest: UiAssetManifest = serde_json::from_str(json)
        .map_err(|error| format!("invalid UI asset manifest: {error}"))?;
    if manifest.schema_version != UI_ICON_MANIFEST_SCHEMA_VERSION {
        return Err(format!(
            "unsupported UI asset manifest schema {}",
            manifest.schema_version
        ));
    }
    let map_version =
        MapVersion::from_str(&manifest.castle_fight_catalog_version).map_err(|_| {
            format!(
                "invalid Castle Fight UI catalog version {:?}",
                manifest.castle_fight_catalog_version
            )
        })?;

    let mut paths = BTreeMap::new();
    let mut cursor_paths = BTreeMap::new();
    for entry in manifest.assets {
        let Some(png) = entry.png.as_deref() else {
            continue;
        };
        let png = png.replace('\\', "/");
        validate_relative_asset_path(&png)?;
        let asset_path = format!("{}/{}", asset_prefix.trim_end_matches('/'), png);
        if entry.owner_kind == "cursors" {
            let theme = parse_cursor_theme(&entry)?;
            if let Some(previous) = cursor_paths.insert(theme, asset_path.clone()) {
                return Err(format!(
                    "duplicate UI cursor atlas binding {theme:?}: {previous:?} and {asset_path:?}"
                ));
            }
            continue;
        }

        let key = parse_icon_key(&entry)?;
        if let Some(previous) = paths.insert(key, asset_path.clone()) {
            return Err(format!(
                "duplicate UI icon binding {key:?}: {previous:?} and {asset_path:?}"
            ));
        }
    }

    Ok(ResolvedUiManifest {
        map_version,
        paths,
        cursor_paths,
    })
}

fn parse_cursor_theme(entry: &UiBindingManifest) -> Result<UiCursorTheme, String> {
    if entry.role != "atlas" {
        return Err(format!(
            "cursor UI asset {} has unexpected role {:?}",
            entry.owner_rawcode, entry.role
        ));
    }
    match entry.owner_rawcode.as_str() {
        "human" => Ok(UiCursorTheme::Human),
        "orc" => Ok(UiCursorTheme::Orc),
        "undead" => Ok(UiCursorTheme::Undead),
        "night_elf" => Ok(UiCursorTheme::NightElf),
        other => Err(format!("unknown UI cursor theme {other:?}")),
    }
}

fn parse_icon_key(entry: &UiBindingManifest) -> Result<UiIconKey, String> {
    match entry.owner_kind.as_str() {
        "units" | "abilities" | "buffs" | "items" => {
            let kind = match entry.owner_kind.as_str() {
                "units" => UiObjectIconKind::Unit,
                "abilities" => UiObjectIconKind::Ability,
                "buffs" => UiObjectIconKind::Buff,
                "items" => UiObjectIconKind::Item,
                _ => unreachable!(),
            };
            let role = parse_object_role(&entry.role)?;
            Ok(UiIconKey::Object {
                kind,
                rawcode: parse_rawcode(&entry.owner_rawcode)?,
                role,
            })
        }
        "commands" => {
            if entry.role != "command" {
                return Err(format!(
                    "command UI icon {} has unexpected role {:?}",
                    entry.owner_rawcode, entry.role
                ));
            }
            let command = match entry.owner_rawcode.as_str() {
                "move" => UiCommandIcon::Move,
                "attack" => UiCommandIcon::Attack,
                "build_human" => UiCommandIcon::BuildHuman,
                "cancel" => UiCommandIcon::Cancel,
                other => return Err(format!("unknown command UI icon {other:?}")),
            };
            Ok(UiIconKey::Command(command))
        }
        "resources" => {
            if entry.role != "bar" {
                return Err(format!(
                    "resource UI icon {} has unexpected role {:?}",
                    entry.owner_rawcode, entry.role
                ));
            }
            let resource = match entry.owner_rawcode.as_str() {
                "gold" => UiResourceIcon::Gold,
                "lumber" => UiResourceIcon::Lumber,
                "supply" => UiResourceIcon::Supply,
                "upkeep" => UiResourceIcon::Upkeep,
                other => return Err(format!("unknown resource UI icon {other:?}")),
            };
            Ok(UiIconKey::Resource(resource))
        }
        "info_panel" => {
            if entry.role != "icon" {
                return Err(format!(
                    "info-panel icon {} has unexpected role {:?}",
                    entry.owner_rawcode, entry.role
                ));
            }
            match entry.owner_rawcode.as_str() {
                "damage_normal" => Ok(UiIconKey::InfoDamage(DamageType::Normal)),
                "damage_pierce" => Ok(UiIconKey::InfoDamage(DamageType::Pierce)),
                "damage_siege" => Ok(UiIconKey::InfoDamage(DamageType::Siege)),
                "damage_magic" => Ok(UiIconKey::InfoDamage(DamageType::Magic)),
                "damage_chaos" => Ok(UiIconKey::InfoDamage(DamageType::Chaos)),
                "damage_spells" => Ok(UiIconKey::InfoDamage(DamageType::Spells)),
                "damage_hero" => Ok(UiIconKey::InfoDamage(DamageType::Hero)),
                "armor_small" => Ok(UiIconKey::InfoArmor(ArmorType::Small)),
                "armor_unarmored" => Ok(UiIconKey::InfoArmor(ArmorType::Unarmored)),
                "armor_medium" => Ok(UiIconKey::InfoArmor(ArmorType::Medium)),
                "armor_large" => Ok(UiIconKey::InfoArmor(ArmorType::Large)),
                "armor_hero" => Ok(UiIconKey::InfoArmor(ArmorType::Hero)),
                "armor_fortified" => Ok(UiIconKey::InfoArmor(ArmorType::Fortified)),
                "armor_divine" => Ok(UiIconKey::InfoArmor(ArmorType::Divine)),
                "armor_normal" => Ok(UiIconKey::InfoArmor(ArmorType::Normal)),
                other => Err(format!("unknown info-panel icon {other:?}")),
            }
        }
        other => Err(format!("unknown UI icon owner kind {other:?}")),
    }
}

fn parse_object_role(role: &str) -> Result<UiIconRole, String> {
    match role {
        "normal" => Ok(UiIconRole::Normal),
        "research" => Ok(UiIconRole::Research),
        "turn_off" => Ok(UiIconRole::TurnOff),
        "buff" => Ok(UiIconRole::Buff),
        "game_interface" => Ok(UiIconRole::GameInterface),
        "interface" => Ok(UiIconRole::Interface),
        "caster_upgrade" => Ok(UiIconRole::CasterUpgrade),
        other => Err(format!("unknown object UI icon role {other:?}")),
    }
}

fn parse_rawcode(rawcode: &str) -> Result<u32, String> {
    let bytes: [u8; 4] = rawcode
        .as_bytes()
        .try_into()
        .map_err(|_| format!("UI icon rawcode {rawcode:?} is not exactly four bytes"))?;
    Ok(u32::from_be_bytes(bytes))
}

fn validate_relative_asset_path(path: &str) -> Result<(), String> {
    let path = Path::new(path);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "UI icon path {} is not a safe relative asset path",
            path.display()
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_versioned_object_command_and_resource_icons() {
        let json = r#"{
            "schema_version": 2,
            "castle_fight_catalog_version": "9.27",
            "assets": [
                {
                    "owner_kind": "commands",
                    "owner_rawcode": "move",
                    "role": "command",
                    "png": "textures/move.png"
                },
                {
                    "owner_kind": "units",
                    "owner_rawcode": "h000",
                    "role": "game_interface",
                    "png": "textures/barracks.png"
                },
                {
                    "owner_kind": "abilities",
                    "owner_rawcode": "Ahrp",
                    "role": "normal",
                    "png": "textures/repair.png"
                },
                {
                    "owner_kind": "resources",
                    "owner_rawcode": "gold",
                    "role": "bar",
                    "png": "textures/gold.png"
                },
                {
                    "owner_kind": "info_panel",
                    "owner_rawcode": "damage_pierce",
                    "role": "icon",
                    "png": "textures/piercing.png"
                },
                {
                    "owner_kind": "info_panel",
                    "owner_rawcode": "armor_small",
                    "role": "icon",
                    "png": "textures/light_armor.png"
                },
                {
                    "owner_kind": "cursors",
                    "owner_rawcode": "human",
                    "role": "atlas",
                    "png": "textures/human_cursor.png"
                },
                {
                    "owner_kind": "buffs",
                    "owner_rawcode": "B005",
                    "role": "buff",
                    "png": null
                }
            ]
        }"#;

        let resolved = resolve_manifest_entries(json, "wc3/ui").expect("manifest resolves");
        assert_eq!(resolved.map_version, MapVersion::CASTLE_FIGHT_9_27);
        assert_eq!(
            resolved.paths[&UiIconKey::Command(UiCommandIcon::Move)],
            "wc3/ui/textures/move.png"
        );
        assert_eq!(
            resolved.paths[&UiIconKey::unit_game_interface(u32::from_be_bytes(*b"h000"))],
            "wc3/ui/textures/barracks.png"
        );
        assert_eq!(
            resolved.paths[&UiIconKey::ability(u32::from_be_bytes(*b"Ahrp"), UiIconRole::Normal,)],
            "wc3/ui/textures/repair.png"
        );
        assert_eq!(
            resolved.paths[&UiIconKey::Resource(UiResourceIcon::Gold)],
            "wc3/ui/textures/gold.png"
        );
        assert_eq!(
            resolved.paths[&UiIconKey::InfoDamage(DamageType::Pierce)],
            "wc3/ui/textures/piercing.png"
        );
        assert_eq!(
            resolved.paths[&UiIconKey::InfoArmor(ArmorType::Small)],
            "wc3/ui/textures/light_armor.png"
        );
        assert_eq!(
            resolved.cursor_paths[&UiCursorTheme::Human],
            "wc3/ui/textures/human_cursor.png"
        );
        assert!(!resolved.paths.contains_key(&UiIconKey::Object {
            kind: UiObjectIconKind::Buff,
            rawcode: u32::from_be_bytes(*b"B005"),
            role: UiIconRole::Buff,
        }));
    }

    #[test]
    fn presentation_catalog_keeps_927_ability_identity_out_of_ui_callers() {
        let catalog = CastleFightPresentationCatalog::for_version(MapVersion::CASTLE_FIGHT_9_27)
            .expect("9.27 presentation catalog exists");
        assert_eq!(
            catalog.blink_command,
            UiIconKey::ability(u32::from_be_bytes(*b"A0-1"), UiIconRole::Normal)
        );
        assert_eq!(
            catalog.repair_command,
            UiIconKey::ability(u32::from_be_bytes(*b"Ahrp"), UiIconRole::Normal)
        );
        assert_eq!(catalog.cursor_theme, UiCursorTheme::Human);
        assert!(CastleFightPresentationCatalog::for_version(MapVersion::new(9, 28)).is_none());
    }

    #[test]
    fn rejects_unsafe_ui_icon_paths() {
        let json = r#"{
            "schema_version": 2,
            "castle_fight_catalog_version": "9.27",
            "assets": [{
                "owner_kind": "commands",
                "owner_rawcode": "move",
                "role": "command",
                "png": "../outside.png"
            }]
        }"#;
        assert!(resolve_manifest_entries(json, "wc3/ui").is_err());
    }
}
