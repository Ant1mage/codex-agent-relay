//! Relay's control panel: a frameless window anchored under the menu bar icon.
//!
//! Configuration needs real controls (text fields, selects, sliders, switches),
//! which a native NSMenu cannot carry — so the menu keeps status and quick
//! actions and this panel carries the forms. It is served by relayd, which makes
//! the panel same-origin with the API: no `file://` module restrictions and no
//! special case in the daemon's Origin guard.
//!
//! Ported from `apps/menu-bar/src/panel.ts` and `panel-target.ts`.

use std::sync::Mutex;

use tauri::{
    AppHandle, Manager, PhysicalPosition, Url, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use crate::menu_model::{PanelIntent, PanelTab};

pub const PANEL_LABEL: &str = "panel";
/// The panel's fixed geometry: a single-column popover.
pub const PANEL_WIDTH: f64 = 420.0;
pub const PANEL_HEIGHT: f64 = 640.0;
/// Gap between the tray icon and the panel, and the margin kept to the screen edge.
const TRAY_GAP: i32 = 6;
const SCREEN_MARGIN: i32 = 8;

/// Which daemon generation the open panel is talking to. A restart may reuse the
/// port but always rotates the token, so the URL has to be rebuilt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelTarget {
    /// Daemon base URL, e.g. `http://127.0.0.1:7352`.
    pub base: String,
    pub token: String,
    pub lang: String,
    pub tab: Option<PanelTab>,
    pub intent: Option<PanelIntent>,
    pub profile_id: Option<String>,
}

/// The URL the panel window loads. The token rides the fragment: it never
/// reaches the server or a referrer.
pub fn panel_url(target: &PanelTarget) -> String {
    panel_url_with_dev(target, std::env::var("RELAY_PANEL_DEV_URL").ok().as_deref())
}

pub fn panel_url_with_dev(target: &PanelTarget, dev: Option<&str>) -> String {
    let dev = dev.filter(|value| !value.is_empty());
    let base = match dev {
        Some(dev) => dev.to_string(),
        // A hand-edited `server.json` may carry a trailing slash; the daemon's own
        // base URL never does.
        None => format!("{}/panel/", target.base.trim_end_matches('/')),
    };
    let mut url = Url::parse(&base).expect("panel URL is absolute");
    {
        let mut query = url.query_pairs_mut();
        // A development server does not know the daemon, so it is told.
        if dev.is_some() {
            query.append_pair("base", &target.base);
        }
        query.append_pair("lang", &target.lang);
        if let Some(tab) = target.tab {
            query.append_pair("tab", &wire(tab));
        }
        if let Some(intent) = target.intent {
            query.append_pair("intent", &wire(intent));
        }
        if let Some(profile_id) = target.profile_id.as_deref() {
            query.append_pair("profileId", profile_id);
        }
    }
    url.set_fragment(Some(&format!("t={}", target.token)));
    url.to_string()
}

/// A daemon restart may reuse its port but always rotates its token.
pub fn panel_connection_changed(current: Option<&PanelTarget>, next: &PanelTarget) -> bool {
    match current {
        Some(current) => current.base != next.base || current.token != next.token,
        None => true,
    }
}

fn wire<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// What the shell remembers about the open panel.
#[derive(Default)]
pub struct PanelState {
    target: Mutex<Option<PanelTarget>>,
}

impl PanelState {
    pub fn target(&self) -> Option<PanelTarget> {
        self.target.lock().ok().and_then(|target| target.clone())
    }

    fn set_target(&self, next: Option<PanelTarget>) {
        if let Ok(mut target) = self.target.lock() {
            *target = next;
        }
    }
}

/// Opens the panel, or focuses the one already open.
///
/// An open panel navigates in place when the daemon did not change: same window,
/// same state, no reload. A rotated token or a moved port means the renderer is
/// holding a dead credential, so the window is hidden and reloaded.
pub fn open(app: &AppHandle, target: PanelTarget) -> Result<(), String> {
    let state = app.state::<PanelState>();
    if let Some(window) = app.get_webview_window(PANEL_LABEL) {
        let changed = panel_connection_changed(state.target().as_ref(), &target);
        state.set_target(Some(target.clone()));
        if changed {
            let url = Url::parse(&panel_url(&target)).map_err(|error| error.to_string())?;
            // Hidden first: showing a renderer that still holds the old token
            // would flash an unauthenticated page before the reload lands.
            let _ = window.hide();
            window.navigate(url).map_err(|error| error.to_string())?;
        } else {
            let _ = window.eval(navigation_event_script(&target));
            place_under_tray(app, &window);
            window.show().map_err(|error| error.to_string())?;
            window.set_focus().map_err(|error| error.to_string())?;
        }
        return Ok(());
    }

    let url = Url::parse(&panel_url(&target)).map_err(|error| error.to_string())?;
    state.set_target(Some(target));
    WebviewWindowBuilder::new(app, PANEL_LABEL, WebviewUrl::External(url))
        .title("Relay")
        .inner_size(PANEL_WIDTH, PANEL_HEIGHT)
        .decorations(false)
        .resizable(true)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .visible_on_all_workspaces(true)
        .visible(false)
        .focused(true)
        // Shown once the page is actually there: an empty window flashing over the
        // desktop would be worse than a short delay. This also re-places the
        // window after a reload moved it.
        .on_page_load(|window, _payload| {
            place_under_tray(window.app_handle(), &window);
            let _ = window.show();
            let _ = window.set_focus();
        })
        .build()
        .map_err(|error| error.to_string())?;
    Ok(())
}

/// Hides the panel; called when it loses focus.
pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(PANEL_LABEL) {
        let _ = window.hide();
    }
}

/// Where the panel goes: under the tray icon, clamped into the work area of the
/// screen the icon is on.
fn place_under_tray(app: &AppHandle, window: &WebviewWindow) {
    let Some(tray) = app.tray_by_id(crate::tray::TRAY_ID) else {
        return;
    };
    let Ok(Some(rect)) = tray.rect() else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    let tray_position = rect.position.to_physical::<i32>(1.0);
    let tray_size = rect.size.to_physical::<u32>(1.0);
    let monitor = app
        .monitor_from_point(tray_position.x as f64, tray_position.y as f64)
        .ok()
        .flatten();
    let (work_x, work_y, work_width, work_height) = match monitor.as_ref() {
        Some(monitor) => {
            let area = monitor.work_area();
            (
                area.position.x,
                area.position.y,
                area.size.width as i32,
                area.size.height as i32,
            )
        }
        None => (0, 0, i32::MAX / 2, i32::MAX / 2),
    };
    let width = size.width as i32;
    let height = size.height as i32;
    let centered = tray_position.x + (tray_size.width as i32) / 2 - width / 2;
    let right_edge = (work_x + work_width - width - SCREEN_MARGIN).max(work_x + SCREEN_MARGIN);
    let bottom_edge = (work_y + work_height - height - SCREEN_MARGIN).max(work_y + SCREEN_MARGIN);
    let x = centered.clamp(work_x + SCREEN_MARGIN, right_edge);
    let y = (tray_position.y + tray_size.height as i32 + TRAY_GAP)
        .clamp(work_y + SCREEN_MARGIN, bottom_edge);
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

/// The front-end has no Tauri IPC — it is served by relayd and built without any
/// npm packages — so the "already open, same daemon" navigation arrives as a DOM
/// event carrying the navigation payload.
pub fn navigation_event_script(target: &PanelTarget) -> String {
    let mut payload = serde_json::Map::new();
    if let Some(tab) = target.tab {
        payload.insert("tab".into(), serde_json::Value::String(wire(tab)));
    }
    if let Some(intent) = target.intent {
        payload.insert("intent".into(), serde_json::Value::String(wire(intent)));
    }
    if let Some(profile_id) = target.profile_id.as_deref() {
        payload.insert(
            "profileId".into(),
            serde_json::Value::String(profile_id.to_string()),
        );
    }
    // Double-encoded: the JSON is embedded as a string literal, so the payload
    // cannot break out of the script no matter what an id contains.
    let payload =
        serde_json::to_string(&serde_json::Value::Object(payload)).unwrap_or_else(|_| "{}".into());
    let literal = serde_json::to_string(&payload).unwrap_or_else(|_| "\"{}\"".into());
    "window.dispatchEvent(new CustomEvent('relay:panel', { detail: JSON.parse(__RELAY_PANEL_EVENT__) }));"
        .replace("__RELAY_PANEL_EVENT__", &literal)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(token: &str, base: &str) -> PanelTarget {
        PanelTarget {
            base: base.to_string(),
            token: token.to_string(),
            lang: "zh-CN".to_string(),
            tab: Some(PanelTab::Runtime),
            intent: None,
            profile_id: None,
        }
    }

    #[test]
    fn the_token_never_leaves_the_fragment() {
        let url = Url::parse(&panel_url_with_dev(
            &target("fresh-token", "http://127.0.0.1:7352"),
            None,
        ))
        .unwrap();
        assert_eq!(url.fragment(), Some("t=fresh-token"));
        assert!(!url.query_pairs().any(|(key, _)| key == "token"));
        assert!(!url.as_str().contains("token=fresh-token"));
        assert_eq!(url.path(), "/panel/");
    }

    #[test]
    fn every_panel_parameter_is_present_and_ordered() {
        let full = PanelTarget {
            base: "http://127.0.0.1:7352".into(),
            token: "tok".into(),
            lang: "en".into(),
            tab: Some(PanelTab::Codex),
            intent: Some(PanelIntent::CodexActions),
            profile_id: Some("agent-1".into()),
        };
        assert_eq!(
            panel_url_with_dev(&full, None),
            "http://127.0.0.1:7352/panel/?lang=en&tab=codex&intent=codex-actions&profileId=agent-1#t=tok"
        );

        // Only `lang` is mandatory.
        let minimal = PanelTarget {
            base: "http://127.0.0.1:7352".into(),
            token: "tok".into(),
            lang: "en".into(),
            tab: None,
            intent: None,
            profile_id: None,
        };
        assert_eq!(
            panel_url_with_dev(&minimal, None),
            "http://127.0.0.1:7352/panel/?lang=en#t=tok"
        );

        // A trailing slash on the base URL does not double up.
        let mut trailing = minimal;
        trailing.base = "http://127.0.0.1:7352/".into();
        assert_eq!(
            panel_url_with_dev(&trailing, None),
            "http://127.0.0.1:7352/panel/?lang=en#t=tok"
        );
    }

    #[test]
    fn a_development_server_is_told_which_daemon_to_use() {
        let url = panel_url_with_dev(
            &target("tok", "http://127.0.0.1:7353"),
            Some("http://localhost:5173/panel/"),
        );
        assert_eq!(
            url,
            "http://localhost:5173/panel/?base=http%3A%2F%2F127.0.0.1%3A7353&lang=zh-CN&tab=runtime#t=tok"
        );
    }

    #[test]
    fn a_rotated_token_or_moved_port_requires_a_reload() {
        assert!(panel_connection_changed(
            None,
            &target("new", "http://127.0.0.1:7352")
        ));
        assert!(panel_connection_changed(
            Some(&target("old", "http://127.0.0.1:7352")),
            &target("new", "http://127.0.0.1:7352")
        ));
        assert!(panel_connection_changed(
            Some(&target("same", "http://127.0.0.1:7352")),
            &target("same", "http://127.0.0.1:7353")
        ));
        // The same daemon generation: navigate in place instead.
        assert!(!panel_connection_changed(
            Some(&target("same", "http://127.0.0.1:7352")),
            &target("same", "http://127.0.0.1:7352")
        ));
    }

    #[test]
    fn the_navigation_event_carries_the_requested_view() {
        let script = navigation_event_script(&PanelTarget {
            base: "http://127.0.0.1:7352".into(),
            token: "tok".into(),
            lang: "en".into(),
            tab: Some(PanelTab::Agents),
            intent: Some(PanelIntent::EditAgent),
            profile_id: Some("a'b".into()),
        });
        // The payload is a JSON string literal, so nothing an id contains can
        // break out of the statement and the front-end receives a real object.
        let payload = serde_json::json!({
            "intent": "edit-agent",
            "profileId": "a'b",
            "tab": "agents",
        });
        let inner = serde_json::to_string(&payload).unwrap();
        let literal = serde_json::to_string(&inner).unwrap();
        let expected = format!(
            "window.dispatchEvent(new CustomEvent('relay:panel', {{ detail: JSON.parse({literal}) }}));"
        );
        assert_eq!(script, expected);
        assert!(!script.contains("JSON.parse({"));
        assert!(script.contains(r#"\"tab\":\"agents\""#));

        // An empty selection still dispatches, with an empty object.
        let bare = navigation_event_script(&PanelTarget {
            base: "http://127.0.0.1:7352".into(),
            token: "tok".into(),
            lang: "en".into(),
            tab: None,
            intent: None,
            profile_id: None,
        });
        assert!(bare.contains(r#"JSON.parse("{}")"#));
    }
}
