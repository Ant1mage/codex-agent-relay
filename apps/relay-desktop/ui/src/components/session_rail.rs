//! The session list.
//!
//! Sessions are Codex threads, not directories: the name comes from Codex and
//! Relay never invents one.

use leptos::prelude::*;

use crate::api::Route;
use crate::format;
use crate::state::{self, preferred_run, Store};

pub fn session_rail(route: RwSignal<Route>) -> impl IntoView {
    let store = expect_context::<Store>();
    let t = store.translator();

    let sessions = move || {
        store.snapshot.with(|snapshot| snapshot.as_ref().map(|value| value.sessions.clone()).unwrap_or_default())
    };

    view! {
        <aside class="rail">
            <div class="rail-head">
                <span class="rail-title">{t.t("nav.sessions")}</span>
                <span class="rail-count tabular">{move || sessions().len()}</span>
            </div>
            <div class="rail-scroll">
                {move || {
                    let list = sessions();
                    let selected_id = store.session.get();
                    if list.is_empty() {
                        return view! {
                            <div class="empty-block">
                                <p class="empty-title">{t.t("sessions.empty")}</p>
                                <p class="empty-hint">{t.t("sessions.emptyHint")}</p>
                            </div>
                        }
                            .into_any();
                    }
                    list.into_iter()
                        .map(|view_item| {
                            let session = view_item.session.clone();
                            let selected = selected_id.as_deref() == Some(session.id.as_str());
                            let run_id = preferred_run(&view_item).map(|run| run.run.id.clone());
                            let next = Route { session: Some(session.id.clone()), run: run_id };
                            let active = view_item.active_workers;
                            let awaiting = view_item.awaiting_host;
                            let runs = view_item.runs.len();
                            let relative = format::relative_time(&session.updated_at, format::host_status(session.status), &t);
                            let dot = if active > 0 {
                                "dot dot-ok"
                            } else if awaiting > 0 {
                                "dot dot-warn"
                            } else {
                                "dot dot-idle"
                            };
                            view! {
                                <button
                                    type="button"
                                    class=if selected { "rail-item rail-item-active" } else { "rail-item" }
                                    on:click=move |_| state::go(route, next.clone())
                                >
                                    <span class="rail-name">{session.display_name.clone()}</span>
                                    <span class="rail-meta">
                                        <i class=dot aria-hidden="true"></i>
                                        {relative}
                                        <span aria-hidden="true">"·"</span>
                                        {runs} " " {t.t("runs.title")}
                                        {(active > 0)
                                            .then(|| view! { <span class="ok">"● " {active}</span> })}
                                        {(awaiting > 0)
                                            .then(|| view! { <span class="warn">"◆ " {awaiting}</span> })}
                                    </span>
                                </button>
                            }
                        })
                        .collect_view()
                        .into_any()
                }}
            </div>
        </aside>
    }
}
