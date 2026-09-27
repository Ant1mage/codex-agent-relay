//! The inspector: read-only observation of Relay's live projection.
//!
//! One stream per page, the URL as the source of truth for what is selected, and
//! history pulled lazily per run.

use leptos::prelude::*;

use crate::api::Route;
use crate::components::console_pane::console_pane;
use crate::components::controls::icon;
use crate::components::header::header;
use crate::components::notice::notice_pill;
use crate::components::run_strip::run_strip;
use crate::components::session_rail::session_rail;
use crate::dom;
use crate::state::{self, preferred_run, Store};

pub fn inspector_app() -> AnyView {
    let store = expect_context::<Store>();
    let t = store.translator();

    // No token: the page explains how to get one instead of failing silently.
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

    let route = RwSignal::new(dom::parse_path(&dom::pathname()));
    dom::on_popstate(move || route.set(dom::parse_path(&dom::pathname())));

    // One stream per page: the store owns the merge, this owns the socket.
    state::start_stream(store, client.clone());

    /*
     * The URL decides what is selected. A deep link from the tray (session or run)
     * wins; anything unresolved falls back to the newest session and its live run,
     * and the address bar is rewritten to match what is actually on screen.
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
            .map(|id| run.map(|view| view.steps.iter().any(|step| &step.id == id)).unwrap_or(false))
            .unwrap_or(false);
        let step = if step_exists { current_step } else { run.and_then(|view| view.steps.first().map(|step| step.id.clone())) };
        store.session.set(Some(session.session.id.clone()));
        store.run.set(run.map(|view| view.run.id.clone()));
        store.step.set(step.clone());

        let canonical = dom::route_path(&Route {
            session: Some(session.session.id.clone()),
            run: run.map(|view| view.run.id.clone()),
        });
        if dom::pathname() != canonical {
            dom::replace_url(&canonical);
        }
    });

    // History is pulled per run; the stream only carries what happens next.
    Effect::new(move |_| {
        let Some(run_id) = store.run.get() else {
            return;
        };
        let cached = store
            .events
            .with_untracked(|all| all.get(&run_id).map(|bucket| !bucket.is_empty()).unwrap_or(false));
        if cached {
            return;
        }
        state::fetch_run_history(store, client.clone(), run_id);
    });

    Effect::new(move |_| dom::set_document_lang(store.locale.get()));

    // The inspector has no theme switch: it follows the browser.
    view! {
        <div class="app-shell">
            {header()}

            {move || {
                let missing = store
                    .snapshot
                    .with(|snapshot| snapshot.as_ref().map(|value| !value.codex.configured).unwrap_or(false));
                missing
                    .then(|| {
                        view! {
                            <div class="codex-banner">
                                {icon("warning", "icon icon-xs tone-warn")}
                                <span class="banner-strong">{t.t("inspector.codexMissing")}</span>
                                <span class="dim">{t.t("inspector.codexMissingBody")}</span>
                                <span class="banner-tail">{t.t("inspector.codexFixInMenuBar")}</span>
                            </div>
                        }
                    })
            }}

            <div class="app-grid">
                {session_rail(route)}
                <main class="app-main">
                    {run_strip(route)}
                    {console_pane()}
                </main>
            </div>

            {notice_pill(move || store.notice.get())}
        </div>
    }
    .into_any()
}
