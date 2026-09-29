//! Relay's control panel: everything that configures Relay itself.
//!
//! Layout rules that keep it readable in a 420×640 popover: one column, every
//! text node either truncates or wraps, and no element may set its own width from
//! content (that is what overflowed the Codex tab before).

use leptos::prelude::*;

use crate::components::controls::icon;
use crate::components::notice::notice_footer;
use crate::dom;
use crate::state::{PanelStore, PanelTab};
use crate::views::{agents, codex, policy, runtimes, status};

pub fn panel_app() -> AnyView {
    let store = expect_context::<PanelStore>();
    let t = store.translator();

    if store.daemon_down() {
        return view! {
            <div class="panel-shell panel-down">
                <span>{t.t("panel.daemonDown")}</span>
            </div>
        }
        .into_any();
    }

    store.reload();

    let codex_label = move || {
        if store.net_codex_configured() {
            t.t("menu.codexConnected")
        } else {
            t.t("menu.codexMissing")
        }
    };

    view! {
        <div class="panel-shell">
            <header class="panel-header">
                <span class="mark" role="img" aria-label="Relay"></span>
                <span class="panel-title">"Relay"</span>
                <span class=move || {
                    if store.net_codex_configured() { "badge badge-secondary" } else { "badge badge-destructive" }
                }>{codex_label}</span>
                <span class="panel-header-actions">
                    <button
                        class="btn-icon"
                        title=move || t.t("panel.openInspector")
                        on:click=move |_| store.open_inspector()
                    >
                        {icon(icondata::LuExternalLink, "icon icon-xs")}
                    </button>
                    <button class="btn-icon" title=move || t.t("common.refresh") on:click=move |_| store.reload()>
                        {icon(icondata::LuRefreshCw, "icon icon-xs")}
                    </button>
                    <button class="btn-icon" title=move || t.t("panel.close") on:click=move |_| dom::close_window()>
                        {icon(icondata::LuX, "icon icon-xs")}
                    </button>
                </span>
            </header>

            <div class="panel-tabs">
                {PanelTab::ORDER
                    .into_iter()
                    .map(|tab| {
                        view! {
                            <button
                                type="button"
                                class=move || if store.tab.get() == tab { "tab tab-active" } else { "tab" }
                                on:click=move |_| {
                                    store.tab.set(tab);
                                    store.consume_intent();
                                }
                            >
                                <span class="truncate">{t.t(tab.key())}</span>
                            </button>
                        }
                    })
                    .collect_view()}
            </div>

            {move || {
                let warnings = store
                    .config
                    .with(|config| config.as_ref().map(|value| value.warnings.clone()).unwrap_or_default());
                (!warnings.is_empty())
                    .then(|| {
                        view! {
                            <div class="panel-warnings">
                                {warnings
                                    .into_iter()
                                    .map(|warning| {
                                        view! {
                                            <p class="panel-warning">
                                                {icon(icondata::LuTriangleAlert, "icon icon-xs")}
                                                <span class="wrap">{warning}</span>
                                            </p>
                                        }
                                    })
                                    .collect_view()}
                            </div>
                        }
                    })
            }}

            <div class="panel-body">
                {move || store.error.get().map(|message| view! { <p class="panel-error wrap">{message}</p> })}
                // Every arm below is a branch that is disposed the moment
                // store.tab changes, so nothing created inside one may be
                // written from an async continuation. Cross-await state — model
                // options, the adapter catalogue, probe results, daemon health —
                // lives in PanelStore, which is created once in App and outlives
                // every branch; see PanelStore in state.rs.
                {move || match store.tab.get() {
                    PanelTab::Sessions => ().into_any(),
                    PanelTab::Agents => agents::agents_view(store).into_any(),
                    PanelTab::Runtimes => runtimes::runtimes_view(store).into_any(),
                    PanelTab::Policy => policy::policy_view(store).into_any(),
                    PanelTab::Codex => codex::codex_view(store).into_any(),
                    PanelTab::Status => status::status_view(store).into_any(),
                }}
            </div>

            {notice_footer(move || store.notice.get())}
        </div>
    }
    .into_any()
}
