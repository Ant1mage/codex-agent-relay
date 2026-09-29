//! Every run in the selected session, as a horizontal strip of cards; the run
//! owns the steps the console shows.

use leptos::prelude::*;

use crate::api::Route;
use crate::format;
use crate::state::{self, Store};

pub fn run_strip(route: RwSignal<Route>) -> impl IntoView {
    let store = expect_context::<Store>();
    let t = store.translator();

    let session = move || {
        let selected = store.session.get()?;
        store.session_view(&selected)
    };

    view! {
        {move || {
            let Some(view) = session() else {
                return ().into_any();
            };
            let session_id = view.session.id.clone();
            let profiles = store.snapshot.with(|snapshot| {
                snapshot.as_ref().map(|value| value.profiles.clone()).unwrap_or_default()
            });
            let selected_run = store.run.get();
            let runs = view.runs.clone();
            let started = format::date_time(&view.session.started_at, t.locale.get());
            view! {
                <div class="run-strip">
                    <div class="run-strip-head">
                        <span class="run-strip-name">{view.session.display_name.clone()}</span>
                        <span class="run-strip-cwd">{view.session.cwd.clone()}</span>
                        <span class="run-strip-started">{t.t("cliInfo.started")} " " {started}</span>
                    </div>
                    {if runs.is_empty() {
                        view! { <p class="run-strip-empty">{t.t("inspector.noRun")}</p> }.into_any()
                    } else {
                        view! {
                            <div class="run-cards">
                                {runs
                                    .into_iter()
                                    .map(|run_view| {
                                        let active = selected_run.as_deref() == Some(run_view.run.id.as_str());
                                        let next = Route {
                                            session: Some(session_id.clone()),
                                            run: Some(run_view.run.id.clone()),
                                        };
                                        let worker_end = run_view.workers.last().and_then(|worker| worker.ended_at.clone());
                                        let duration = format::elapsed(&run_view.run.created_at, worker_end.as_deref(), t.locale.get());
                                        let status = format::run_status(run_view.run.status);
                                        let relative = format::relative_time(&run_view.run.updated_at, status, &t);
                                        let profile = profiles
                                            .iter()
                                            .find(|profile| profile.id == run_view.run.profile_id)
                                            .map(|profile| profile.name.clone())
                                            .unwrap_or_else(|| run_view.run.profile_id.clone());
                                        let steps = run_view.steps.len();
                                        view! {
                                            <button
                                                type="button"
                                                class=if active { "run-card run-card-active" } else { "run-card" }
                                                on:click=move |_| state::go(route, next.clone())
                                            >
                                                <span class="run-card-head">
                                                    <i class=format!("not-italic {}", status_tone(status))>
                                                        {format::status_glyph(status)}
                                                    </i>
                                                    <span class="run-card-profile">{profile}</span>
                                                    <span class="run-card-status">{t.t(&format!("run.status.{status}"))}</span>
                                                </span>
                                                <span class="run-card-task">{run_view.run.task.clone()}</span>
                                                <span class="run-card-meta">
                                                    <span>{steps} " " {t.t("inspector.steps")}</span>
                                                    <span aria-hidden="true">"·"</span>
                                                    <span>{duration}</span>
                                                    <span aria-hidden="true">"·"</span>
                                                    <span>{relative}</span>
                                                </span>
                                            </button>
                                        }
                                    })
                                    .collect_view()}
                            </div>
                        }
                            .into_any()
                    }}
                </div>
            }
                .into_any()
        }}
    }
}

fn status_tone(status: &str) -> &'static str {
    match status {
        "running" | "starting" => "tone-ok",
        "awaiting_host" => "tone-warn",
        "failed" | "orphaned" | "interrupted" => "tone-danger",
        _ => "tone-muted",
    }
}
