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
    leptos::mount::mount_to_body(App);
}

#[component]
fn App() -> AnyView {
    let theme_mode = leptos_use::use_color_mode_with_options(
        leptos_use::UseColorModeOptions::default()
            .storage_key(dom::THEME_KEY)
            .emit_auto(true),
    );
    let dark = Signal::derive(move || theme_mode.state.get() == leptos_use::ColorMode::Dark);
    provide_context(dom::ThemeControl {
        set_mode: theme_mode.set_mode,
        dark,
    });
    let theme = RwSignal::new(thaw::Theme::light());
    Effect::new(move |_| {
        theme.set(if dark.get() {
            thaw::Theme::dark()
        } else {
            thaw::Theme::light()
        })
    });
    let pathname = dom::pathname();
    // The daemon (or the tray) hands the token over in the URL; it is stored and
    // taken back out of the address bar before anything renders.
    let token = dom::resolve_token();
    let base = dom::query("base").unwrap_or_else(dom::origin);
    let client = token.map(|token| Client::new(&base, &token));

    let locale = if dom::is_panel_path(&pathname) {
        dom::panel_locale()
    } else {
        dom::inspector_locale()
    };

    let tab = PanelTab::from_id(&dom::query("tab").unwrap_or_default());
    let intent = dom::query("intent").and_then(|value| PanelIntent::from_id(&value));
    let profile = dom::query("profileId");

    let panel_store = PanelStore::new(client.clone(), base, locale, tab, intent, profile);
    let store = Store::new(client, locale);

    dom::set_document_lang(locale);
    dom::sync_native_locale(locale);
    provide_context(panel_store);
    provide_context(store);

    Effect::new(move |_| {
        let mode = match theme_mode.mode.get() {
            leptos_use::ColorMode::Dark => dom::ThemeMode::Dark,
            leptos_use::ColorMode::Light => dom::ThemeMode::Light,
            _ => dom::ThemeMode::Auto,
        };
        store.theme.set(mode);
    });
    view! { <thaw::ConfigProvider theme=theme class="relay-theme">{views::unified::unified_app()}</thaw::ConfigProvider> }.into_any()
}
