//! The session list.
//!
//! Sessions are Codex threads, not directories: the name comes from Codex and
//! Relay never invents one.

use leptos::prelude::*;

use crate::api::Route;
use crate::components::controls::{confirm_dialog, icon};
use crate::format;
use crate::state::{self, preferred_run, Store};

pub fn session_rail(route: RwSignal<Route>) -> impl IntoView {
    session_rail_collapsible(route, None)
}

pub fn session_rail_collapsible(
    route: RwSignal<Route>,
    _sidebar_open: Option<RwSignal<bool>>,
) -> impl IntoView {
    let store = expect_context::<Store>();
    let t = store.translator();

    let search_query = RwSignal::new(String::new());
    let delete_target = RwSignal::new(None::<(String, String)>);
    let confirm_open = RwSignal::new(false);

    let sessions = move || {
        store.snapshot.with(|snapshot| {
            snapshot
                .as_ref()
                .map(|value| value.sessions.clone())
                .unwrap_or_default()
        })
    };

    view! {
        <aside class="rail" id="session-sidebar">
            <div class="rail-head">
                <span class="rail-title">{t.t("nav.sessions")}</span>
                <span class="rail-count tabular">{move || sessions().len()}</span>
            </div>

            // Search filter input
            <div class="rail-search-bar">
                <div class="rail-search-wrap">
                    {icon(icondata::LuSearch, "icon icon-xs rail-search-icon")}
                    <input
                        type="text"
                        class="rail-search-input"
                        placeholder=t.t("sessions.searchPlaceholder")
                        prop:value=move || search_query.get()
                        on:input=move |ev| search_query.set(event_target_value(&ev))
                    />
                    {move || (!search_query.get().is_empty()).then(|| view! {
                        <button
                            type="button"
                            class="rail-search-clear-btn"
                            title=t.t("action.clear")
                            on:click=move |_| search_query.set(String::new())
                        >
                            {icon(icondata::LuX, "icon icon-xs")}
                        </button>
                    })}
                </div>
            </div>

            <div class="rail-scroll">
                {move || {
                    let all_sessions = sessions();
                    let selected_id = store.session.get();
                    let query = search_query.get().trim().to_lowercase();

                    if all_sessions.is_empty() {
                        return view! {
                            <div class="empty-block">
                                <p class="empty-title">{t.t("sessions.empty")}</p>
                                <p class="empty-hint">{t.t("sessions.emptyHint")}</p>
                            </div>
                        }
                        .into_any();
                    }

                    let filtered: Vec<_> = all_sessions
                        .into_iter()
                        .filter(|item| {
                            if query.is_empty() {
                                true
                            } else {
                                item.session.display_name.to_lowercase().contains(&query)
                                    || item.session.id.to_lowercase().contains(&query)
                                    || item.session.cwd.to_lowercase().contains(&query)
                            }
                        })
                        .collect();

                    if filtered.is_empty() {
                        return view! {
                            <div class="empty-block">
                                <p class="empty-title">{t.t("sessions.noSearchResults")}</p>
                            </div>
                        }
                        .into_any();
                    }

                    filtered
                        .into_iter()
                        .map(|view_item| {
                            let session = view_item.session.clone();
                            let del_id = session.id.clone();
                            let del_name = session.display_name.clone();
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
                                <div
                                    class=if selected { "rail-item rail-item-active" } else { "rail-item" }
                                    on:click=move |_| state::go(route, next.clone())
                                >
                                    <div class="rail-item-main">
                                        <span class="rail-name">{session.display_name.clone()}</span>
                                        {(!session.cwd.is_empty()).then(|| view! {
                                            <span class="rail-cwd truncate dim" title=session.cwd.clone()>{session.cwd.clone()}</span>
                                        })}
                                        <span class="rail-meta">
                                            <i class=dot aria-hidden="true"></i>
                                            {relative}
                                            <span aria-hidden="true">"·"</span>
                                            {t.tp("counts.runs", runs)}
                                            {(active > 0)
                                                .then(|| view! { <span class="ok">"● " {active}</span> })}
                                            {(awaiting > 0)
                                                .then(|| view! { <span class="warn">"◆ " {awaiting}</span> })}
                                        </span>
                                    </div>
                                    <button
                                        type="button"
                                        class="rail-item-del"
                                        title=t.t("sessions.delete")
                                        on:click=move |ev| {
                                            ev.stop_propagation();
                                            delete_target.set(Some((del_id.clone(), del_name.clone())));
                                            confirm_open.set(true);
                                        }
                                    >
                                        {icon(icondata::LuTrash2, "icon icon-xs")}
                                    </button>
                                </div>
                            }
                        })
                        .collect_view()
                        .into_any()
                }}
            </div>

            // Confirm delete modal
            {confirm_dialog(
                confirm_open,
                t.t("sessions.deleteConfirmTitle"),
                t.t("sessions.deleteConfirmBody"),
                t.t("action.cancel"),
                t.t("sessions.deleteConfirmBtn"),
                Callback::new(move |_| {
                    if let Some((id, _)) = delete_target.get_untracked() {
                        store.delete_session(id);
                    }
                    delete_target.set(None);
                }),
            )}
        </aside>
    }
}
