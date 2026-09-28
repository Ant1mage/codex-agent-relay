//! The mission timeline stream: consecutive steps rendered as sections in chronological order.

use std::collections::HashSet;

use leptos::prelude::*;

use crate::api::Route;
use crate::components::controls::icon;
use crate::format::{self, ConsoleKind, ConsoleRow};
use crate::state::{self, Store};

#[component]
pub fn TimelineStream(route: RwSignal<Route>) -> impl IntoView {
    let _ = route;
    let store = expect_context::<Store>();
    let t = store.translator();

    let session = move || {
        let selected = store.session.get()?;
        store.session_view(&selected)
    };

    let selected_run_view = move || {
        let view = session()?;
        let run_id = store.run.get().or_else(|| view.runs.first().map(|r| r.run.id.clone()))?;
        view.runs.iter().find(|r| r.run.id == run_id).cloned()
    };

    let sticking = RwSignal::new(true);
    let collapsed_steps = RwSignal::new(HashSet::<String>::new());
    let feedback_text = RwSignal::new(String::new());
    let scroller = NodeRef::<leptos::html::Div>::new();

    // Auto-scroll when new events or steps arrive
    Effect::new(move |_| {
        let _ = store.events.get();
        if sticking.get_untracked() {
            if let Some(node) = scroller.get() {
                node.set_scroll_top(node.scroll_height());
            }
        }
    });

    let toggle_step = move |step_id: String| {
        collapsed_steps.update(|set| {
            if set.contains(&step_id) {
                set.remove(&step_id);
            } else {
                set.insert(step_id);
            }
        });
    };

    view! {
        <div class="timeline-stream-container">
            {move || {
                let Some(run_view) = selected_run_view() else {
                    return view! {
                        <div class="empty-pane">
                            <p class="empty-title">{t.t("timeline.empty")}</p>
                            <p class="empty-hint">{t.t("sessions.emptyHint")}</p>
                        </div>
                    }
                    .into_any();
                };

                let profiles = store.snapshot.with(|snapshot| {
                    snapshot.as_ref().map(|value| value.profiles.clone()).unwrap_or_default()
                });
                let steps = run_view.steps.clone();
                let run_status = format::run_status(run_view.run.status);
                let is_running = run_status == "running" || run_status == "starting";
                let is_awaiting = run_status == "awaiting_host";
                let active_worker = run_view.workers.iter().filter(|w| w.status.is_active()).next_back().cloned();
                let expand_all = {
                    let collapsed = collapsed_steps;
                    move |_| collapsed.set(HashSet::new())
                };
                let collapse_all = {
                    let collapsed = collapsed_steps;
                    let s_ids: Vec<String> = steps.iter().map(|s| s.id.clone()).collect();
                    move |_| collapsed.set(s_ids.iter().cloned().collect())
                };

                view! {
                    <div class="timeline-header">
                        <div class="timeline-header-main">
                            <span class="timeline-run-badge">
                                <i class=format!("not-italic {}", status_tone(run_status))>
                                    {format::status_glyph(run_status)}
                                </i>
                                <span class="capitalize">{t.t(&format!("run.status.{run_status}"))}</span>
                            </span>
                            <span class="timeline-task-title">{run_view.run.task.clone()}</span>
                        </div>
                        <div class="timeline-header-meta">
                            <span class="dim">{format::date_time(&run_view.run.created_at)}</span>
                            <span class="dot-separator" aria-hidden="true">"·"</span>
                            <span class="dim">{steps.len()} " " {t.t("steps.step")}</span>
                            <span class="dot-separator" aria-hidden="true">"·"</span>
                            <span class="dim">{format::elapsed(&run_view.run.created_at, active_worker.as_ref().and_then(|w| w.ended_at.as_deref()))}</span>
                        </div>
                    </div>

                    // Step Progress Anchor Bar
                    <nav class="step-anchor-bar">
                        <span class="step-anchor-label">{t.t("steps.step")}:</span>
                        <div class="step-anchor-list">
                            {steps
                                .iter()
                                .enumerate()
                                .map(|(index, step)| {
                                    let step_id = step.id.clone();
                                    let step_status = format::step_status(step.status);
                                    let profile_name = profiles
                                        .iter()
                                        .find(|p| p.id == step.profile_id)
                                        .map(|p| p.name.clone())
                                        .unwrap_or_else(|| step.profile_id.clone());
                                    let anchor_id = format!("step-sec-{}", step_id);
                                    let is_active = store.step.get().as_deref() == Some(&step_id);

                                    view! {
                                        <button
                                            type="button"
                                            class=format!(
                                                "step-anchor-pill {} {}",
                                                if is_active { "step-anchor-active" } else { "" },
                                                status_border_tone(step_status)
                                            )
                                            on:click=move |_| {
                                                store.step.set(Some(step_id.clone()));
                                                crate::dom::scroll_to_element(&anchor_id);
                                            }
                                        >
                                            <span class=format!("step-num-badge {}", status_bg_tone(step_status))>
                                                {index + 1}
                                            </span>
                                            <span class="step-anchor-name">{profile_name}</span>
                                            <i class=format!("not-italic {}", status_tone(step_status))>
                                                {format::status_glyph(step_status)}
                                            </i>
                                        </button>
                                    }
                                })
                                .collect_view()}
                        </div>
                        <div class="step-stream-actions">
                            <button type="button" class="btn btn-ghost btn-xs step-action-btn" on:click=expand_all>
                                {icon("chevrons-down", "icon icon-xs")}
                                <span>{t.t("timeline.expandAll")}</span>
                            </button>
                            <button type="button" class="btn btn-ghost btn-xs step-action-btn" on:click=collapse_all>
                                {icon("chevrons-up", "icon icon-xs")}
                                <span>{t.t("timeline.collapseAll")}</span>
                            </button>
                        </div>
                    </nav>

                    // Continuous Timeline Stream Body
                    <div
                        class="timeline-scroll-body"
                        node_ref=scroller
                        on:scroll=move |event| {
                            let node = event_target::<web_sys::HtmlDivElement>(&event);
                            sticking.set(node.scroll_height() - node.scroll_top() - node.client_height() < 60);
                        }
                    >
                        {steps
                            .into_iter()
                            .enumerate()
                            .map(|(index, step)| {
                                let sid = step.id.clone();
                                let profile_name = profiles
                                    .iter()
                                    .find(|p| p.id == step.profile_id)
                                    .map(|p| p.name.clone())
                                    .unwrap_or_else(|| step.profile_id.clone());
                                let is_collapsed = Signal::derive({
                                    let s = sid.clone();
                                    move || collapsed_steps.get().contains(&s)
                                });
                                let r_view = run_view.clone();
                                let toggle = {
                                    let s = sid.clone();
                                    Callback::new(move |()| toggle_step(s.clone()))
                                };

                                view! {
                                    <StepCard
                                        index=index
                                        step=step
                                        profile_name=profile_name
                                        run_view=r_view
                                        is_collapsed=is_collapsed
                                        on_toggle=toggle
                                    />
                                }
                            })
                            .collect_view()}
                    </div>

                    // Smart Floating Pin-to-Bottom / Jump-to-Latest Button
                    {move || (!sticking.get() && is_running).then(|| {
                        view! {
                            <button
                                class="jump-latest-btn"
                                on:click=move |_| {
                                    if let Some(node) = scroller.get() {
                                        node.set_scroll_top(node.scroll_height());
                                        sticking.set(true);
                                    }
                                }
                            >
                                {icon("arrow-down", "icon icon-xs")}
                                <span>{t.t("timeline.scrollToBottom")}</span>
                            </button>
                        }
                    })}

                    // Bottom Action Controls
                    {if is_running {
                        let worker_id = active_worker.as_ref().map(|w| w.id.clone()).unwrap_or_default();
                        view! {
                            <div class="timeline-action-footer">
                                <div class="action-footer-status">
                                    <span class="pulsing-dot"></span>
                                    <span>{t.t("common.active")}</span>
                                </div>
                                <button
                                    class="btn btn-danger btn-sm"
                                    on:click=move |_| store.cancel_worker(worker_id.clone())
                                >
                                    {icon("stop", "icon icon-xs")}
                                    <span>{t.t("inspector.stopWorker")}</span>
                                </button>
                            </div>
                        }
                        .into_any()
                    } else if is_awaiting {
                        let worker_id = active_worker.as_ref().map(|w| w.id.clone()).unwrap_or_default();
                        let wid_accept = worker_id.clone();
                        let wid_resume = worker_id.clone();
                        view! {
                            <div class="timeline-action-footer awaiting-footer">
                                <div class="awaiting-form">
                                    <input
                                        type="text"
                                        class="input awaiting-input"
                                        placeholder=t.t("timeline.feedbackPlaceholder")
                                        prop:value=move || feedback_text.get()
                                        on:input=move |ev| feedback_text.set(event_target_value(&ev))
                                    />
                                    <div class="awaiting-buttons">
                                        <button
                                            class="btn btn-primary btn-sm"
                                            on:click=move |_| {
                                                store.resume_worker(wid_resume.clone(), feedback_text.get());
                                                feedback_text.set(String::new());
                                            }
                                        >
                                            {icon("play", "icon icon-xs")}
                                            <span>{t.t("timeline.resume")}</span>
                                        </button>
                                        <button
                                            class="btn btn-outline btn-sm"
                                            on:click=move |_| store.accept_worker(wid_accept.clone())
                                        >
                                            {icon("check", "icon icon-xs")}
                                            <span>{t.t("timeline.accept")}</span>
                                        </button>
                                    </div>
                                </div>
                            </div>
                        }
                        .into_any()
                    } else {
                        ().into_any()
                    }}
                }
                .into_any()
            }}
        </div>
    }
}

#[component]
fn StepCard(
    index: usize,
    step: relay_core::domain::Step,
    profile_name: String,
    run_view: relay_api::RunView,
    is_collapsed: Signal<bool>,
    on_toggle: Callback<()>,
) -> impl IntoView {
    let store = expect_context::<Store>();
    let t = store.translator();

    let step_status = format::step_status(step.status);
    let sid = step.id.clone();
    let anchor_target = format!("step-sec-{}", step.id);

    let worker = run_view.workers.iter().filter(|w| w.step_id == sid).next_back().cloned();
    let duration = format::elapsed(&step.created_at, worker.as_ref().and_then(|w| w.ended_at.as_deref()));

    let run_id = run_view.run.id.clone();
    let r_view = run_view.clone();
    let s_id = step.id.clone();

    let events = Memo::new(move |_| {
        let cached = store.run_events(&run_id);
        format::events_for_step(&r_view, &s_id, &cached)
    });

    view! {
        <section id=anchor_target class=move || format!("step-card-section {}", if is_collapsed.get() { "step-collapsed" } else { "" })>
            <header
                class="step-card-header"
                on:click=move |_| on_toggle.run(())
            >
                <div class="step-card-header-left">
                    <span class=format!("step-card-badge {}", status_bg_tone(step_status))>
                        {format!("Step {}", index + 1)}
                    </span>
                    <span class="step-card-profile">{profile_name.clone()}</span>
                    <span class="step-card-status">
                        <i class=format!("not-italic {}", status_tone(step_status))>
                            {format::status_glyph(step_status)}
                        </i>
                        {t.t(&format!("run.status.{step_status}"))}
                    </span>
                    <span class="step-card-duration tabular">{duration}</span>
                </div>

                <div class="step-card-header-right">
                    <span class="step-card-count dim">
                        {move || {
                            let count = events.with(|evs| format::console_rows(evs).len());
                            format!("{} {}", count, t.t("inspector.events"))
                        }}
                    </span>
                    <button
                        type="button"
                        class="btn btn-outline btn-xs step-card-toggle-btn"
                        on:click=move |ev| {
                            ev.stop_propagation();
                            on_toggle.run(());
                        }
                    >
                        {move || if is_collapsed.get() {
                            view! {
                                {icon("chevron-down", "icon icon-xs")}
                                <span>{t.t("timeline.expand")}</span>
                            }
                            .into_any()
                        } else {
                            view! {
                                {icon("chevron-up", "icon icon-xs")}
                                <span>{t.t("timeline.collapse")}</span>
                            }
                            .into_any()
                        }}
                    </button>
                </div>
            </header>

            {move || {
                if is_collapsed.get() {
                    return ().into_any();
                }
                let (changes, rows) = events.with(|evs| {
                    (format::changed_files(evs), format::console_rows(evs))
                });

                view! {
                    <div class="step-card-body">
                        {(!changes.is_empty()).then(|| {
                            view! {
                                <div class="step-changes-block">
                                    <div class="step-changes-header">
                                        {icon("diff", "icon icon-xs")}
                                        <span class="font-medium">{t.t("console.changes")}</span>
                                        <span class="badge badge-secondary">{changes.len()}</span>
                                    </div>
                                    <ul class="step-changes-list">
                                        {changes
                                            .into_iter()
                                            .map(|file| {
                                                view! {
                                                    <li class="step-change-item">
                                                        <code class="change-path">{file.path.clone()}</code>
                                                        <div class="change-diff-stats">
                                                            {(file.additions > 0.0)
                                                                .then(|| view! { <span class="change-add">"+" {file.additions}</span> })}
                                                            {(file.deletions > 0.0)
                                                                .then(|| view! { <span class="change-del">"-" {file.deletions}</span> })}
                                                        </div>
                                                    </li>
                                                }
                                            })
                                            .collect_view()}
                                    </ul>
                                </div>
                            }
                        })}

                        <div class="step-logs-block">
                            {if rows.is_empty() {
                                view! { <p class="tab-empty">{t.t("console.empty")}</p> }.into_any()
                            } else {
                                view! {
                                    <div class="log-stream-list">
                                        {rows
                                            .into_iter()
                                            .map(|row| render_log_row(row, &t))
                                            .collect_view()}
                                    </div>
                                }
                                .into_any()
                            }}
                        </div>
                    </div>
                }
                .into_any()
            }}
        </section>
    }
}

fn render_log_row(row: ConsoleRow, t: &crate::i18n::Translator) -> impl IntoView {
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
    let is_markdown = format::is_markdown_content(&label);

    view! {
        <div class=format!("log-row {} {}", accent, if is_markdown { "log-row-markdown" } else { "" })>
            <time class="log-time tabular">{time}</time>
            <span class="log-kind">{kind}</span>
            {if is_markdown {
                let html = format::markdown_to_html(&label);
                view! {
                    <div class="log-content-wrap">
                        <div class="log-markdown-block markdown-body" inner_html=html></div>
                        {count
                            .map(|count| {
                                view! { <i class="log-note">" " {count} " " {t.t("console.files")}</i> }
                            })}
                        {(!diff.is_empty()).then(|| view! { <i class="log-note tabular">{diff}</i> })}
                    </div>
                }
                .into_any()
            } else {
                view! {
                    <code class="log-label">
                        {label}
                        {count
                            .map(|count| {
                                view! { <i class="log-note">" " {count} " " {t.t("console.files")}</i> }
                            })}
                        {(!diff.is_empty()).then(|| view! { <i class="log-note tabular">{diff}</i> })}
                    </code>
                }
                .into_any()
            }}
        </div>
    }
}

fn status_tone(status: &str) -> &'static str {
    match status {
        "running" | "starting" => "tone-ok",
        "awaiting_host" => "tone-warn",
        "completed" => "tone-ok",
        "failed" | "cancelled" | "interrupted" | "orphaned" => "tone-danger",
        _ => "tone-muted",
    }
}

fn status_border_tone(status: &str) -> &'static str {
    match status {
        "running" | "starting" => "border-ok",
        "awaiting_host" => "border-warn",
        "completed" => "border-ok",
        "failed" | "cancelled" | "interrupted" => "border-danger",
        _ => "",
    }
}

fn status_bg_tone(status: &str) -> &'static str {
    match status {
        "running" | "starting" => "badge-running",
        "awaiting_host" => "badge-warn",
        "completed" => "badge-success",
        "failed" | "cancelled" | "interrupted" => "badge-danger",
        _ => "badge-secondary",
    }
}
