//! Relay's UI: one Trunk bundle, two surfaces.
//!
//! `/` (and `/s/<session>`, `/s/<session>/r/<run>`) is the read-only inspector;
//! `/panel` is the control panel. Routing is a pathname check — the two surfaces
//! share the API client, the contract types and the stylesheet, so they ship as
//! one wasm module and the daemon only has to serve one `dist/`.

mod api;
mod components;
mod dom;
mod format;
mod i18n;
mod state;
mod views;

use leptos::prelude::*;

use crate::api::Client;
use crate::state::{PanelIntent, PanelStore, PanelTab, Store};

fn main() {
    // Panics become readable console errors instead of `unreachable executed`.
    console_error_panic_hook::set_once();
    // The inspector has no theme switch: it follows the browser, like any log view.
    dom::watch_theme();
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> AnyView {
    let pathname = dom::pathname();
    // The daemon (or the tray) hands the token over in the URL; it is stored and
    // taken back out of the address bar before anything renders.
    let token = dom::resolve_token();
    let base = dom::query("base").unwrap_or_else(dom::origin);
    let client = token.map(|token| Client::new(&base, &token));

    if dom::is_panel_path(&pathname) {
        let locale = dom::panel_locale();
        let tab = PanelTab::from_id(&dom::query("tab").unwrap_or_default());
        let intent = dom::query("intent").and_then(|value| PanelIntent::from_id(&value));
        let profile = dom::query("profileId");
        let store = PanelStore::new(client, base, locale, tab, intent, profile);
        dom::set_document_lang(locale);
        provide_context(store);
        return views::panel::panel_app().into_any();
    }

    let locale = dom::inspector_locale();
    let store = Store::new(client, locale);
    dom::set_document_lang(locale);
    provide_context(store);
    views::inspector::inspector_app().into_any()
}
