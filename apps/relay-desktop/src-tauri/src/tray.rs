//! The menu bar item.
//!
//! Ported from the tray half of `apps/menu-bar/src/main.ts`: the model from
//! `menu_model.rs` becomes native menu objects here, and every actionable entry
//! carries its `MenuBarAction` in the item id, so one menu event handler can
//! dispatch the whole menu without a closure per entry.

use tauri::image::Image;
use tauri::menu::{
    CheckMenuItemBuilder, IsMenuItem, Menu, MenuBuilder, MenuItemBuilder, PredefinedMenuItem,
    SubmenuBuilder,
};
use tauri::tray::{TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Wry};

use crate::daemon;
use crate::menu_model::{MenuBarAction, MenuBarItem, MenuItemKind};

pub const TRAY_ID: &str = "relay";

/// Prefix that marks an id as ours: `relay:<n>:<action json>`.
const ID_PREFIX: &str = "relay:";

/// Builds the tray icon.
///
/// Electron attached a 16 pt image plus a 32 px representation to one NSImage.
/// Tauri's tray takes a single image and scales it to the menu bar height, so the
/// 2× asset is the one that carries the Retina detail — it is the same
/// `relay-icon-32.png` the Electron image carried as its 2× representation. The
/// 16 pt asset is the fallback when only it is present.
pub fn tray_image() -> Result<Image<'static>, String> {
    let mut last_error: Option<String> = None;
    for relative in [
        "appicon/png/light/relay-icon-32.png",
        "appicon/png/light/relay-icon-16.png",
    ] {
        let Some(path) = daemon::asset_path(relative) else {
            continue;
        };
        match Image::from_path(&path) {
            Ok(image) => return Ok(image),
            Err(error) => last_error = Some(format!("{}: {error}", path.display())),
        }
    }
    Err(last_error.unwrap_or_else(|| {
        format!(
            "menu bar icon is missing; looked under {}",
            daemon::resources_dir().display()
        )
    }))
}

/// Creates the tray icon. The menu is attached by the first refresh, which runs
/// immediately after startup.
pub fn build(app: &AppHandle) -> Result<(), String> {
    let icon = tray_image()?;
    TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Relay")
        .show_menu_on_left_click(true)
        .on_tray_icon_event(|tray, event| {
            // Electron opened the inspector on a double click; on macOS the menu
            // itself opens on a left click, so this only fires elsewhere.
            if let TrayIconEvent::DoubleClick { .. } = event {
                crate::shell::dispatch_double_click(tray.app_handle());
            }
        })
        .build(app)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Replaces the tray's menu and tooltip. Only called when the serialized model
/// actually changed, which is what keeps the open menu from flickering.
pub fn apply(app: &AppHandle, items: &[MenuBarItem], tooltip: &str) -> Result<(), String> {
    let menu = build_menu(app, items).map_err(|error| error.to_string())?;
    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| "menu bar item is missing".to_string())?;
    tray.set_menu(Some(menu))
        .map_err(|error| error.to_string())?;
    tray.set_tooltip(Some(tooltip))
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn build_menu(app: &AppHandle, items: &[MenuBarItem]) -> tauri::Result<Menu<Wry>> {
    let mut counter = 0usize;
    let built = build_items(app, items, &mut counter);
    let refs: Vec<&dyn IsMenuItem<Wry>> = built.iter().map(|item| &**item).collect();
    MenuBuilder::new(app).items(&refs).build()
}

/// The next item id, carrying the action the entry performs.
///
/// Native menus are rebuilt wholesale and two entries can share an action (the
/// control panel appears twice), so a counter keeps every id unique.
fn next_id(counter: &mut usize, action: Option<&MenuBarAction>) -> String {
    *counter += 1;
    let payload = action
        .and_then(|action| serde_json::to_string(action).ok())
        .unwrap_or_default();
    format!("{ID_PREFIX}{counter}:{payload}")
}

/// Recovers the action from a menu event id. `None` for headers and separators.
pub fn parse_action(id: &str) -> Option<MenuBarAction> {
    let rest = id.strip_prefix(ID_PREFIX)?;
    let (_, payload) = rest.split_once(':')?;
    if payload.is_empty() {
        return None;
    }
    serde_json::from_str(payload).ok()
}

/// Builds one native item. A single entry that cannot be built (an unsupported
/// accelerator, say) is skipped with a warning instead of taking the whole menu
/// down: the menu bar is the one surface the user has left.
fn build_items(
    app: &AppHandle,
    items: &[MenuBarItem],
    counter: &mut usize,
) -> Vec<Box<dyn IsMenuItem<Wry>>> {
    let mut built: Vec<Box<dyn IsMenuItem<Wry>>> = Vec::new();
    for item in items {
        match build_item(app, item, counter) {
            Ok(native) => built.push(native),
            Err(error) => eprintln!("[relay] skipping the {:?} menu entry: {error}", item.label),
        }
    }
    built
}

fn build_item(
    app: &AppHandle,
    item: &MenuBarItem,
    counter: &mut usize,
) -> tauri::Result<Box<dyn IsMenuItem<Wry>>> {
    match item.kind {
        MenuItemKind::Separator => Ok(Box::new(PredefinedMenuItem::separator(app)?)),
        MenuItemKind::Header => {
            // A header is a disabled entry: the menu shows it, nobody can pick it.
            let header = MenuItemBuilder::with_id(next_id(counter, None), &item.label)
                .enabled(false)
                .build(app)?;
            Ok(Box::new(header))
        }
        MenuItemKind::Normal | MenuItemKind::Checkbox => {
            if let Some(children) = item.submenu.as_ref() {
                let child_built = build_items(app, children, counter);
                let child_refs: Vec<&dyn IsMenuItem<Wry>> =
                    child_built.iter().map(|child| &**child).collect();
                let submenu = SubmenuBuilder::new(app, &item.label)
                    .items(&child_refs)
                    .build()?;
                return Ok(Box::new(submenu));
            }
            let enabled = item.enabled.unwrap_or(true);
            let id = next_id(counter, item.action.as_ref());
            if item.kind == MenuItemKind::Checkbox {
                let checkbox = CheckMenuItemBuilder::with_id(id, &item.label)
                    .checked(item.checked.unwrap_or(false))
                    .enabled(enabled)
                    .build(app)?;
                return Ok(Box::new(checkbox));
            }
            let mut builder = MenuItemBuilder::with_id(id, &item.label).enabled(enabled);
            if let Some(accelerator) = item.accelerator.as_deref() {
                builder = builder.accelerator(accelerator);
            }
            Ok(Box::new(builder.build(app)?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu_model::PanelTab;

    #[test]
    fn menu_ids_are_unique_and_round_trip_the_action() {
        let mut counter = 0;
        let first = next_id(&mut counter, Some(&MenuBarAction::Refresh));
        let second = next_id(&mut counter, Some(&MenuBarAction::Refresh));
        assert_ne!(first, second);
        assert_eq!(parse_action(&first), Some(MenuBarAction::Refresh));
        assert_eq!(parse_action(&second), Some(MenuBarAction::Refresh));

        // Session ids contain colons; only the counter separator may be consumed.
        let action = MenuBarAction::CancelWorker {
            worker_session_id: "codex:s 1".into(),
        };
        let id = next_id(&mut counter, Some(&action));
        assert_eq!(parse_action(&id), Some(action));

        let panel = next_id(
            &mut counter,
            Some(&MenuBarAction::OpenPanel {
                tab: PanelTab::Runtime,
                intent: None,
                profile_id: Some("p:1".into()),
            }),
        );
        assert_eq!(
            parse_action(&panel),
            Some(MenuBarAction::OpenPanel {
                tab: PanelTab::Runtime,
                intent: None,
                profile_id: Some("p:1".into())
            })
        );

        // Headers, separators and foreign ids do nothing.
        let header = next_id(&mut counter, None);
        assert_eq!(parse_action(&header), None);
        assert_eq!(parse_action("some-other-menu-item"), None);
        assert_eq!(parse_action("relay:1:"), None);
    }
}
