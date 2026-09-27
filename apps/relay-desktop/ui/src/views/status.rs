//! Status: the surface the old panel did not have.
//!
//! Daemon health, the machine facts behind it, the projection's counts and the
//! adapter diagnostics — plus the one support action a window-less Relay can
//! still perform, "Copy diagnostics".

use leptos::prelude::*;

use crate::components::controls::{badge, icon};
use crate::state::PanelStore;

pub fn status_view(store: PanelStore) -> AnyView {
    let t = store.translator();
    // Health lands in PanelStore, which outlives this tab: leaving Status while
    // the request is in flight cannot write into a view that has been disposed.
    store.load_health();

    let base = store.base();
    let healthy = move || store.health.get().map(|value| value.ok).unwrap_or(false);

    view! {
        <div class="stack">
            <div class="card">
                <div class="status-head">
                    <h2 class="card-title">{t.t("status.daemon")}</h2>
                    {move || {
                        if healthy() {
                            badge(t.t("status.healthy"), "success")
                        } else {
                            badge(t.t("status.unreachable"), "destructive")
                        }
                    }}
                </div>
                {status_row(t.t("status.daemon"), base)}
                {move || {
                    store
                        .health
                        .get()
                        .map(|value| {
                            view! {
                                <div>
                                    {status_row(t.t("status.version"), value.version)}
                                    {status_row(t.t("status.port"), value.port.to_string())}
                                    {status_row(t.t("status.pid"), value.pid.to_string())}
                                    {status_row(t.t("status.started"), crate::format::date_time(&value.started_at))}
                                    {status_row(t.t("status.database"), value.database)}
                                </div>
                            }
                        })
                }}
                {move || {
                    store.health_error.get().map(|message| view! { <p class="probe probe-fail wrap">{message}</p> })
                }}
            </div>

            <div class="card">
                <h2 class="card-title">{t.t("status.counts")}</h2>
                <div class="status-grid">
                    {status_cell(
                        t.t("status.sessions"),
                        move || store.health.get().map(|value| value.sessions as usize).unwrap_or(0),
                    )}
                    {status_cell(
                        t.t("status.runs"),
                        move || store.health.get().map(|value| value.runs as usize).unwrap_or(0),
                    )}
                    {status_cell(
                        t.t("status.runtimes"),
                        move || store.snapshot.with(|snapshot| snapshot.as_ref().map(|value| value.runtimes.len()).unwrap_or(0)),
                    )}
                    {status_cell(
                        t.t("status.profiles"),
                        move || store.snapshot.with(|snapshot| snapshot.as_ref().map(|value| value.profiles.len()).unwrap_or(0)),
                    )}
                </div>
            </div>

            <div class="card">
                <h2 class="card-title">{t.t("panel.runtime.diagnostics")}</h2>
                {move || {
                    let diagnostics = store
                        .snapshot
                        .with(|snapshot| snapshot.as_ref().map(|value| value.diagnostics.clone()).unwrap_or_default());
                    if diagnostics.is_empty() {
                        return view! { <p class="field-hint wrap">{t.t("settings.diagnosticsEmpty")}</p> }.into_any();
                    }
                    diagnostics
                        .into_iter()
                        .map(|line| view! { <p class="diagnostics-line wrap">{line}</p> })
                        .collect_view()
                        .into_any()
                }}
            </div>

            <button class="btn btn-outline btn-sm full" on:click=move |_| store.copy_diagnostics()>
                {icon("copy", "icon icon-xs")}
                <span>{t.t("inspector.copyDiagnostics")}</span>
            </button>
        </div>
    }
    .into_any()
}

fn status_row(label: String, value: String) -> impl IntoView {
    view! {
        <div class="status-row">
            <span class="status-label truncate">{label}</span>
            <span class="status-value wrap">{value}</span>
        </div>
    }
}

fn status_cell(label: String, value: impl Fn() -> usize + Send + Sync + 'static) -> impl IntoView {
    view! {
        <div class="status-cell">
            <span class="status-cell-value tabular">{move || value()}</span>
            <span class="status-cell-label truncate">{label}</span>
        </div>
    }
}
