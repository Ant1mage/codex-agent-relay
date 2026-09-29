//! Window chrome: identity, liveness, machine facts and the page-level actions.

use leptos::prelude::*;
use leptos::task::spawn_local;

use crate::components::controls::icon;
use crate::i18n::Locale;
use crate::state::{self, Connection, Store};

pub fn header() -> impl IntoView {
    let store = expect_context::<Store>();
    let t = store.translator();
    let refreshing = RwSignal::new(false);

    let sessions = move || {
        store.snapshot.with(|snapshot| {
            snapshot
                .as_ref()
                .map(|value| value.sessions.len())
                .unwrap_or(0)
        })
    };
    let runs = move || {
        store.snapshot.with(|snapshot| {
            snapshot
                .as_ref()
                .map(|value| {
                    value
                        .sessions
                        .iter()
                        .map(|view| view.runs.len())
                        .sum::<usize>()
                })
                .unwrap_or(0)
        })
    };
    let active = move || {
        store.snapshot.with(|snapshot| {
            snapshot
                .as_ref()
                .map(|value| {
                    value
                        .sessions
                        .iter()
                        .map(|view| view.active_workers)
                        .sum::<u32>()
                })
                .unwrap_or(0)
        })
    };
    let codex_ready = move || store.codex_configured();

    let refresh = move |_| {
        let Some(client) = store.client() else {
            return;
        };
        refreshing.set(true);
        spawn_local(async move {
            match client.snapshot().await {
                Ok(snapshot) => store.snapshot.set(Some(snapshot)),
                Err(error) => store.notify(error.to_string()),
            }
            refreshing.set(false);
        });
    };

    let toggle_locale = move |_| {
        store.set_locale(store.locale.get_untracked().toggled());
    };

    view! {
        <header class="app-header">
            <span class="mark" role="img" aria-label="Relay"></span>
            <h1 class="app-title">{t.t("inspector.title")}</h1>
            {connection_badge()}
            <div class="header-counts">
                <span>{sessions} " " {t.t("nav.sessions")}</span>
                <span aria-hidden="true">"·"</span>
                <span>{move || t.tp("counts.runs", runs())}</span>
                <span aria-hidden="true">"·"</span>
                <span class="tabular">{active} " " {t.t("common.active")}</span>
            </div>

            <div class="header-actions">
                <span class=move || if codex_ready() { "badge badge-secondary" } else { "badge badge-destructive" }>
                    {move || {
                        if codex_ready() {
                            view! {
                                {icon(icondata::LuCircleCheck, "icon icon-xs")}
                                <span>{t.t("codex.check.codex-cli")}</span>
                            }
                                .into_any()
                        } else {
                            view! { <span>{t.t("inspector.codexMissing")}</span> }.into_any()
                        }
                    }}
                </span>
                <button
                    class="btn-icon"
                    title=move || {
                        if store.locale.get() == Locale::En { t.t("language.zh-CN") } else { t.t("language.en") }
                    }
                    on:click=toggle_locale
                >
                    {icon(icondata::LuGlobe, "icon")}
                </button>
                <button
                    class="btn-icon"
                    title=move || t.t("inspector.copyDiagnostics")
                    on:click=move |_| state::copy_diagnostics(store)
                >
                    {icon(icondata::LuCopy, "icon")}
                </button>
                <button
                    class=move || if refreshing.get() { "btn-icon spin" } else { "btn-icon" }
                    title=move || t.t("common.refresh")
                    on:click=refresh
                >
                    {icon(icondata::LuRefreshCw, "icon")}
                </button>
            </div>
        </header>
    }
}

fn connection_badge() -> impl IntoView {
    let store = expect_context::<Store>();
    let t = store.translator();
    let tone = move || match store.connection.get() {
        Connection::Live => "dot dot-ok",
        Connection::Connecting => "dot dot-warn",
        Connection::Error => "dot dot-danger",
    };
    let label = move || {
        t.t(match store.connection.get() {
            Connection::Live => "inspector.live",
            Connection::Connecting => "inspector.connecting",
            Connection::Error => "inspector.offline",
        })
    };
    view! {
        <span class="connection">
            <i class=tone aria-hidden="true"></i>
            {label}
        </span>
    }
}
