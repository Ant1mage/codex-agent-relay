//! The log itself: steps, observable activity, changes and raw events.

use leptos::prelude::*;

use crate::components::controls::icon;
use crate::format::{self, ConsoleKind, ConsoleRow};
use crate::state::{self, InspectorTab, Store};

pub fn console_pane() -> impl IntoView {
    let store = expect_context::<Store>();
    let t = store.translator();

    let selected_view = move || {
        let session_id = store.session.get()?;
        let run_id = store.run.get()?;
        store.run_view(&session_id, &run_id)
    };
    let selected_step = move || {
        let view = selected_view()?;
        let selected = store.step.get();
        selected
            .as_ref()
            .and_then(|id| view.steps.iter().find(|step| &step.id == id))
            .or_else(|| view.steps.first())
            .cloned()
    };
    let step_events = Signal::derive(move || {
        let (Some(view), Some(step)) = (selected_view(), selected_step()) else {
            return Vec::new();
        };
        let cached = store.run_events(&view.run.id);
        format::events_for_step(&view, &step.id, &cached)
    });
    let rows = Signal::derive(move || format::console_rows(&step_events.get()));
    let changes = Signal::derive(move || format::changed_files(&step_events.get()));
    let raw = Signal::derive(move || format::visible_events(&step_events.get()));
    let worker = move || {
        let (Some(view), Some(step)) = (selected_view(), selected_step()) else {
            return None;
        };
        view.workers
            .iter()
            .filter(|worker| worker.step_id == step.id)
            .next_back()
            .cloned()
    };
    let sticking = RwSignal::new(true);
    let scroller = NodeRef::<leptos::html::Div>::new();

    Effect::new(move |_| {
        let _ = rows.get().len();
        let _ = store.tab.get();
        if !sticking.get_untracked() {
            return;
        }
        if let Some(node) = scroller.get() {
            node.set_scroll_top(node.scroll_height());
        }
    });

    view! {
        <section class="console-pane">
            {move || {
                let (Some(view), Some(step)) = (selected_view(), selected_step()) else {
                    return view! {
                        <div class="empty-pane">
                            <p class="empty-title">{t.t("inspector.noRun")}</p>
                            <p class="empty-hint">{t.t("inspector.noRunHint")}</p>
                        </div>
                    }
                        .into_any();
                };
                let worker = worker();
                let status = format::step_status(step.status);
                let running = worker.as_ref().map(|worker| worker.status.is_active()).unwrap_or(false);
                let worker_label = worker
                    .as_ref()
                    .map(|worker| worker.runtime_id.clone())
                    .unwrap_or_else(|| t.t("inspector.stopWorker"));
                let duration = format::elapsed(&step.created_at, worker.as_ref().and_then(|w| w.ended_at.as_deref()), t.locale.get());
                let stop_id = worker.as_ref().map(|worker| worker.id.clone());
                let steps = view.steps.clone();
                let current_step = step.id.clone();
                view! {
                    <div class="console-head">
                        <span class="badge badge-secondary">
                            <i class=if status == "running" { "not-italic tone-ok" } else { "not-italic tone-muted" }>
                                {format::status_glyph(status)}
                            </i>
                            <span>{t.t(&format!("run.status.{status}"))}</span>
                        </span>
                        <span class="dim">{worker_label}</span>
                        <span class="dim">{duration}</span>
                        <span class="console-head-right">
                            <span class="dim tabular">
                                {move || rows.get().len()} " · " {move || t.tp("counts.events", step_events.get().len())}
                            </span>
                            {running
                                .then(|| {
                                    let worker_id = stop_id.clone().unwrap_or_default();
                                    view! {
                                        <button
                                            class="btn btn-outline btn-sm"
                                            on:click=move |_| state::cancel_worker(store, worker_id.clone())
                                        >
                                            {icon(icondata::LuSquare, "icon icon-xs")}
                                            <span>{t.t("inspector.stopWorker")}</span>
                                        </button>
                                    }
                                })}
                        </span>
                    </div>

                    {(steps.len() > 1)
                        .then(|| {
                            view! {
                                <div class="step-chips">
                                    {steps
                                        .iter()
                                        .map(|candidate| {
                                            let id = candidate.id.clone();
                                            let active = id == current_step;
                                            let number = candidate.iteration;
                                            let glyph = format::status_glyph(format::step_status(candidate.status));
                                            view! {
                                                <button
                                                    type="button"
                                                    class=if active { "chip chip-active" } else { "chip" }
                                                    on:click=move |_| store.step.set(Some(id.clone()))
                                                >
                                                    <span>{t.tv("timeline.stepFormat", &[("step", &number.to_string())])}</span>
                                                    <i class="not-italic tone-muted">{glyph}</i>
                                                </button>
                                            }
                                        })
                                        .collect_view()}
                                </div>
                            }
                        })}

                    <div class="tab-list">
                        {InspectorTab::ALL
                            .into_iter()
                            .map(|tab| {
                                let label = match tab {
                                    InspectorTab::Console => t.t("console.title"),
                                    InspectorTab::Changes => t.t("console.changes"),
                                    InspectorTab::Raw => t.t("console.rawOutput"),
                                };
                                view! {
                                    <button
                                        type="button"
                                        class=move || {
                                            if store.tab.get() == tab { "tab tab-active" } else { "tab" }
                                        }
                                        on:click=move |_| store.tab.set(tab)
                                    >
                                        <span>{label}</span>
                                        {(tab == InspectorTab::Changes)
                                            .then(|| {
                                                view! {
                                                    <span class="tab-count">
                                                        {move || changes.get().len()}
                                                    </span>
                                                }
                                            })}
                                    </button>
                                }
                            })
                            .collect_view()}
                    </div>

                    <div
                        class="tab-body"
                        node_ref=scroller
                        on:scroll=move |event| {
                            let node = event_target::<web_sys::HtmlDivElement>(&event);
                            sticking
                                .set(node.scroll_height() - node.scroll_top() - node.client_height() < 60);
                        }
                    >
                        {move || match store.tab.get() {
                            InspectorTab::Console => rows_view(rows.get(), &t).into_any(),
                            InspectorTab::Changes => changes_view(changes.get(), &t).into_any(),
                            InspectorTab::Raw => raw_view(raw.get(), &t).into_any(),
                        }}
                    </div>
                }
                    .into_any()
            }}
        </section>
    }
}

fn rows_view(rows: Vec<ConsoleRow>, t: &crate::i18n::Translator) -> impl IntoView {
    if rows.is_empty() {
        return view! { <p class="tab-empty">{t.t("console.empty")}</p> }.into_any();
    }
    let rendered = rows
        .into_iter()
        .map(|row| {
            let accent = match row.kind {
                ConsoleKind::Edit => "log-warn",
                ConsoleKind::Test | ConsoleKind::Result => "log-ok",
                ConsoleKind::Warning => "log-warn",
                ConsoleKind::Error => "log-danger",
                _ => "",
            };
            let time = format::time_of_day(&row.timestamp);
            let kind = t.t(row.kind.key());
            let label = row.label.clone();
            let count = row.count;
            let diff = state::diff_label(row.additions, row.deletions);
            view! {
                <div class=format!("log-row {accent}")>
                    <time class="log-time tabular">{time}</time>
                    <span class="log-kind">{kind}</span>
                    <code class="log-label">
                        {label}
                        {count
                            .map(|count| {
                                view! {
                                    <i class="log-note">" " {t.tp("counts.files", count)}</i>
                                }
                            })}
                        {(!diff.is_empty()).then(|| view! { <i class="log-note tabular">{diff}</i> })}
                    </code>
                </div>
            }
        })
        .collect_view();
    rendered.into_any()
}

fn changes_view(changes: Vec<format::ChangedFile>, t: &crate::i18n::Translator) -> impl IntoView {
    if changes.is_empty() {
        return view! { <p class="tab-empty">{t.t("console.noChanges")}</p> }.into_any();
    }
    let rendered = changes
        .into_iter()
        .map(|file| {
            view! {
                <li class="change-row">
                    <code class="change-path">{file.path}</code>
                    <span class="change-add tabular">
                        {(file.additions != 0.0).then(|| format!("+{}", format::number(file.additions)))}
                    </span>
                    <span class="change-del tabular">
                        {(file.deletions != 0.0).then(|| format!("-{}", format::number(file.deletions)))}
                    </span>
                </li>
            }
        })
        .collect_view();
    view! { <ul class="change-list">{rendered}</ul> }.into_any()
}

fn raw_view(raw: Vec<relay_core::RelayEvent>, t: &crate::i18n::Translator) -> impl IntoView {
    if raw.is_empty() {
        return view! { <p class="tab-empty">{t.t("console.noRawOutput")}</p> }.into_any();
    }
    let pretty = serde_json::to_string_pretty(&raw).unwrap_or_else(|_| "[]".to_string());
    view! { <pre class="raw-block">{pretty}</pre> }.into_any()
}
