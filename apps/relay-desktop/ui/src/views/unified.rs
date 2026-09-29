//! The Unified Desktop Application view for Relay.
//!
//! Consolidates the popover panel and web inspector into a single modern desktop client
//! (1200x800, traffic light overlay, left navigation rail, continuous timeline stream).

use leptos::prelude::*;

use crate::api::Route;
use crate::components::controls::icon;
use crate::components::session_rail::session_rail_collapsible;
use crate::components::timeline_stream::TimelineStream;
use crate::dom;
use crate::i18n::Locale;
use crate::state::{self, preferred_run, PanelIntent, PanelStore, PanelTab, Store};
use crate::views::{agents, codex, policy, runtimes, status};

pub fn unified_app() -> AnyView {
    let store = expect_context::<Store>();
    let panel_store = expect_context::<PanelStore>();
    let t = store.translator();

    // No client/token: show token setup instruction card
    let Some(client) = store.client() else {
        return view! {
            <div class="center-screen">
                <div class="card token-card">
                    <h2 class="card-title">{t.t("inspector.tokenTitle")}</h2>
                    <p class="card-body">{t.t("inspector.tokenBody")}</p>
                </div>
            </div>
        }
        .into_any();
    };

    let sidebar_open = RwSignal::new(true);
    let route = RwSignal::new(dom::parse_path(&dom::pathname()));
    dom::on_popstate(move || route.set(dom::parse_path(&dom::pathname())));

    // Listen to Tauri tray / navigation custom events
    let panel_route = route;
    dom::on_panel_nav(move |detail| {
        if let Some(tab_name) = detail.tab {
            panel_store.tab.set(PanelTab::from_id(&tab_name));
        }
        if let Some(intent_name) = detail.intent {
            panel_store.intent.set(PanelIntent::from_id(&intent_name));
        }
        if let Some(profile_id) = detail.profile_id {
            panel_store.intent_profile.set(Some(profile_id));
        }
        if detail.session.is_some() || detail.run.is_some() {
            panel_store.tab.set(PanelTab::Sessions);
            panel_route.set(Route {
                session: detail.session,
                run: detail.run,
            });
        }
    });

    // Start background stream & panel reload
    state::start_stream(store, client.clone());
    panel_store.reload();

    /*
     * The URL decides what is selected in the inspector.
     */
    Effect::new(move |_| {
        let Some(snapshot) = store.snapshot.get() else {
            return;
        };
        if snapshot.sessions.is_empty() {
            return;
        }
        let requested = route.get();
        let session = requested
            .session
            .as_ref()
            .and_then(|id| snapshot.sessions.iter().find(|view| &view.session.id == id))
            .unwrap_or(&snapshot.sessions[0]);
        let run = requested
            .run
            .as_ref()
            .and_then(|id| session.runs.iter().find(|view| &view.run.id == id))
            .or_else(|| preferred_run(session));
        let current_step = store.step.get_untracked();
        let step_exists = current_step
            .as_ref()
            .map(|id| {
                run.map(|view| view.steps.iter().any(|step| &step.id == id))
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        let step = if step_exists {
            current_step
        } else {
            run.and_then(|view| view.steps.first().map(|step| step.id.clone()))
        };
        store.session.set(Some(session.session.id.clone()));
        store.run.set(run.map(|view| view.run.id.clone()));
        store.step.set(step);

        let canonical = dom::route_path(&Route {
            session: Some(session.session.id.clone()),
            run: run.map(|view| view.run.id.clone()),
        });
        if dom::pathname() != canonical && !dom::is_panel_path(&dom::pathname()) {
            dom::replace_url(&canonical);
        }
    });

    // History is pulled per run
    Effect::new(move |_| {
        let Some(run_id) = store.run.get() else {
            return;
        };
        let cached = store.events.with_untracked(|all| {
            all.get(&run_id)
                .map(|bucket| !bucket.is_empty())
                .unwrap_or(false)
        });
        if cached {
            return;
        }
        state::fetch_run_history(store, client.clone(), run_id);
    });

    Effect::new(move |_| dom::set_document_lang(store.locale.get()));

    let codex_configured = move || store.codex_configured() || panel_store.net_codex_configured();

    let toggle_locale = move |_| {
        let next = store.locale.get_untracked().toggled();
        store.set_locale(next);
        panel_store.locale.set(next);
    };

    let refresh_all = move |_| {
        panel_store.reload();
        if let Some(c) = store.client() {
            leptos::task::spawn_local(async move {
                if let Ok(snapshot) = c.snapshot().await {
                    store.snapshot.set(Some(snapshot));
                }
            });
        }
    };

    view! {
        <div class="unified-layout">
            // Top Window Titlebar (macOS traffic lights overlay integration & sidebar toggle)
            <header class="unified-titlebar" class:native-macos=dom::query("desktop").as_deref() == Some("macos")
                on:mousedown=move |event| {
                    if event.button() == 0 {
                        event.prevent_default();
                        dom::shell_action("drag");
                    }
                }
            >
                <div class="titlebar-traffic-lights-spacer" data-tauri-drag-region="true"></div>
                <div class="titlebar-actions" on:mousedown=move |event| event.stop_propagation()>
                    <button
                        type="button"
                        class="titlebar-sidebar-toggle"
                        aria-label=move || if sidebar_open.get() { t.t("timeline.collapse") } else { t.t("timeline.expand") }
                        aria-expanded=move || sidebar_open.get().to_string()
                        aria-controls="session-sidebar"
                        disabled=move || panel_store.tab.get() != PanelTab::Sessions
                        title=move || if sidebar_open.get() { t.t("timeline.collapse") } else { t.t("timeline.expand") }
                        on:click=move |_| sidebar_open.set(!sidebar_open.get())
                    >
                        {move || if sidebar_open.get() {
                            icon(icondata::LuPanelLeftClose, "icon icon-sm")
                        } else {
                            icon(icondata::LuPanelLeftOpen, "icon icon-sm")
                        }}
                    </button>
                </div>
                <div class="titlebar-center" data-tauri-drag-region="true">
                    <span class="titlebar-app-name">"Relay"</span>
                    <span class="titlebar-tab-separator">"·"</span>
                    <span class="titlebar-tab-name">{move || t.t(panel_store.tab.get().key())}</span>
                </div>
            </header>

            <div class="unified-body">
                // Left Navigation Rail
                <aside class="unified-rail">
                    <div class="unified-rail-header">
                        {crate::components::controls::relay_logo("unified-rail-logo")}
                    </div>

                <div class="unified-rail-nav">
                    {PanelTab::ORDER
                        .into_iter()
                        .map(|tab| {
                            let is_active = move || panel_store.tab.get() == tab;
                            let icon_name = match tab {
                                PanelTab::Sessions => icondata::LuMessageSquare,
                                PanelTab::Agents => icondata::LuBot,
                                PanelTab::Runtimes => icondata::LuCpu,
                                PanelTab::Policy => icondata::LuShield,
                                PanelTab::Codex => icondata::LuSlidersHorizontal,
                                PanelTab::Status => icondata::LuActivity,
                            };
                            view! {
                                <button
                                    type="button"
                                    class=move || {
                                        if is_active() {
                                            "unified-rail-btn unified-rail-btn-active"
                                        } else {
                                            "unified-rail-btn"
                                        }
                                    }
                                    title=move || t.t(tab.key())
                                    on:click=move |_| {
                                        panel_store.tab.set(tab);
                                        panel_store.consume_intent();
                                    }
                                >
                                    {icon(icon_name, "icon unified-rail-icon")}
                                </button>
                            }
                        })
                        .collect_view()}
                </div>

                <div class="unified-rail-footer">
                    // Codex Status Indicator
                    <div
                        class="unified-rail-status"
                        title=move || {
                            if codex_configured() {
                                t.t("menu.codexConnected")
                            } else {
                                t.t("menu.codexMissing")
                            }
                        }
                    >
                        <i
                            class=move || {
                                if codex_configured() { "dot dot-ok" } else { "dot dot-danger" }
                            }
                            aria-hidden="true"
                        ></i>
                    </div>

                    // Theme Toggle (Auto / Light / Dark)
                    <button
                        type="button"
                        class="unified-rail-btn"
                        title=move || match store.theme.get() { dom::ThemeMode::Auto => t.t("theme.auto"), dom::ThemeMode::Light => t.t("theme.lightNext"), dom::ThemeMode::Dark => t.t("theme.darkNext") }
                        on:click=move |_| store.toggle_theme()
                    >
                        {move || match store.theme.get() {
                            dom::ThemeMode::Auto => icon(icondata::LuMonitor, "icon unified-rail-icon"),
                            dom::ThemeMode::Light => icon(icondata::LuSun, "icon unified-rail-icon"),
                            dom::ThemeMode::Dark => icon(icondata::LuMoon, "icon unified-rail-icon"),
                        }}
                    </button>

                    // Locale Toggle
                    <button
                        type="button"
                        class="unified-rail-btn"
                        title=move || {
                            if store.locale.get() == Locale::En {
                                t.t("language.zh-CN")
                            } else {
                                t.t("language.en")
                            }
                        }
                        on:click=toggle_locale
                    >
                        <span class="unified-locale-tag">
                            {move || if store.locale.get() == Locale::En { "EN" } else { "中" }}
                        </span>
                    </button>

                    // Open in external browser
                    <button
                        type="button"
                        class="unified-rail-btn"
                        title=move || t.t("panel.openInspector")
                        on:click=move |_| panel_store.open_inspector()
                    >
                        {icon(icondata::LuExternalLink, "icon unified-rail-icon")}
                    </button>

                    // Refresh Button
                    <button
                        type="button"
                        class="unified-rail-btn"
                        title=move || t.t("common.refresh")
                        on:click=refresh_all
                    >
                        {icon(icondata::LuRefreshCw, "icon unified-rail-icon")}
                    </button>
                </div>
            </aside>

            // Main View Surface
            <main class="unified-main">
                // API error banner
                {move || {
                    let err = panel_store.error.get().or_else(|| store.error.get());
                    err.map(|msg| {
                        view! {
                            <div class="panel-error-banner">
                                <div class="panel-error-banner-content">
                                    {icon(icondata::LuCircleAlert, "icon icon-xs")}
                                    <span>{msg}</span>
                                </div>
                                <button
                                    type="button"
                                    class="btn-icon"
                                    title=t.t("common.retry")
                                    on:click=move |_| {
                                        panel_store.error.set(None);
                                        store.error.set(None);
                                        panel_store.reload();
                                    }
                                >
                                    {icon(icondata::LuRefreshCw, "icon icon-xs")}
                                </button>
                            </div>
                        }
                    })
                }}

                // System / Codex warning banner
                {move || {
                    let missing = !codex_configured();
                    let warnings = panel_store
                        .config
                        .with(|config| config.as_ref().map(|v| v.warnings.clone()).unwrap_or_default());

                    if missing {
                        view! {
                            <div class="codex-banner">
                                <div class="codex-banner-content">
                                    {icon(icondata::LuTriangleAlert, "icon icon-xs tone-warn")}
                                    <span class="banner-strong">{t.t("inspector.codexMissing")}</span>
                                    <span class="dim">{t.t("inspector.codexMissingBody")}</span>
                                </div>
                                <button
                                    type="button"
                                    class="btn btn-outline btn-xs codex-banner-btn"
                                    on:click=move |_| panel_store.tab.set(PanelTab::Codex)
                                >
                                    <span>{t.t("codex.configureNow")}</span>
                                    {icon(icondata::LuChevronRight, "icon icon-xs")}
                                </button>
                            </div>
                        }
                        .into_any()
                    } else if !warnings.is_empty() {
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
                        .into_any()
                    } else {
                        ().into_any()
                    }
                }}

                // Tab Switcher Body
                <div class="unified-content">
                    {move || match panel_store.tab.get() {
                        PanelTab::Sessions => {
                            view! {
                                <div class=move || format!("unified-sessions-pane {}", if sidebar_open.get() { "" } else { "sidebar-collapsed" })>
                                    {session_rail_collapsible(route, Some(sidebar_open))}
                                    <div class="unified-timeline-column">
                                        <TimelineStream route=route sidebar_open=sidebar_open />
                                    </div>
                                </div>
                            }
                            .into_any()
                        }
                        PanelTab::Agents => {
                            view! {
                                <div class="unified-settings-scroll">
                                    {agents::agents_view(panel_store)}
                                </div>
                            }
                            .into_any()
                        }
                        PanelTab::Runtimes => {
                            view! {
                                <div class="unified-settings-scroll">
                                    {runtimes::runtimes_view(panel_store)}
                                </div>
                            }
                            .into_any()
                        }
                        PanelTab::Policy => {
                            view! {
                                <div class="unified-settings-scroll">
                                    {policy::policy_view(panel_store)}
                                </div>
                            }
                            .into_any()
                        }
                        PanelTab::Codex => {
                            view! {
                                <div class="unified-settings-scroll">
                                    {codex::codex_view(panel_store)}
                                </div>
                            }
                            .into_any()
                        }
                        PanelTab::Status => {
                            view! {
                                <div class="unified-settings-scroll">
                                    {status::status_view(panel_store)}
                                </div>
                            }
                            .into_any()
                        }
                    }}
                </div>
            </main>
            </div>

            // Global Notice Pill
            {crate::components::notice::notice_pill_closable(
                move || store.notice.get().or_else(|| panel_store.notice.get()),
                move || {
                    store.notice.set(None);
                    panel_store.notice.set(None);
                },
            )}
        </div>
    }
    .into_any()
}
