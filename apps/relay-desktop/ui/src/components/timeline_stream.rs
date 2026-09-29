//! The mission timeline stream: consecutive tasks and steps rendered in chronological order.

use std::collections::HashSet;

use leptos::prelude::*;

use crate::api::Route;
use crate::components::controls::icon;
use crate::format::{self, ConsoleKind, ConsoleRow};
use crate::state::{self, Store};

#[component]
pub fn TimelineStream(
    route: RwSignal<Route>,
    #[prop(optional)] sidebar_open: Option<RwSignal<bool>>,
) -> impl IntoView {
    let _ = route;
    let _ = sidebar_open;
    let store = expect_context::<Store>();
    let t = store.translator();

    let session = move || {
        let selected = store.session.get()?;
        store.session_view(&selected)
    };

    let collapsed_steps = RwSignal::new(HashSet::<String>::new());
    let feedback_text = RwSignal::new(String::new());
    let scroller = NodeRef::<leptos::html::Div>::new();
    let scroll = leptos_use::use_scroll_with_options(
        scroller,
        leptos_use::UseScrollOptions::default().offset(leptos_use::ScrollOffset {
            bottom: 60.0,
            ..Default::default()
        }),
    );
    let sticking = Signal::derive(move || scroll.arrived_state.get().bottom);

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
                let Some(s_view) = session() else {
                    return view! {
                        <div class="empty-pane">
                            <div class="empty-pane-content">
                                <p class="empty-title">{t.t("timeline.empty")}</p>
                                <p class="empty-hint">{t.t("sessions.emptyHint")}</p>
                            </div>
                        </div>
                    }
                    .into_any();
                };

                let profiles = store.snapshot.with(|snapshot| {
                    snapshot.as_ref().map(|value| value.profiles.clone()).unwrap_or_default()
                });

                // Sort runs in chronological order (oldest first, latest last at the bottom)
                let mut chronological_runs = s_view.runs.clone();
                chronological_runs.sort_by(|a, b| a.run.created_at.cmp(&b.run.created_at));

                // Calculate cumulative step offsets for each run
                let mut run_offsets: Vec<(relay_api::RunView, usize, usize)> = Vec::new();
                let mut current_offset = 0;
                for (idx, r_view) in chronological_runs.iter().enumerate() {
                    let step_count = r_view.steps.len();
                    run_offsets.push((r_view.clone(), idx, current_offset));
                    current_offset += step_count;
                }
                let total_steps = current_offset;

                // Collect all steps across all runs for the Anchor Bar
                let mut all_steps_meta: Vec<(usize, String, String, &'static str)> = Vec::new();
                for (r_view, _run_idx, offset) in &run_offsets {
                    for (step_idx, step) in r_view.steps.iter().enumerate() {
                        let global_idx = offset + step_idx + 1;
                        let profile_name = profiles
                            .iter()
                            .find(|p| p.id == step.profile_id)
                            .map(|p| p.name.clone())
                            .unwrap_or_else(|| step.profile_id.clone());
                        let step_status = format::step_status(step.status);
                        all_steps_meta.push((global_idx, step.id.clone(), profile_name, step_status));
                    }
                }

                // Session Status and aggregate metrics
                let has_active_worker = s_view.active_workers > 0;
                let has_awaiting = s_view.awaiting_host > 0;
                let session_status = if has_active_worker {
                    "running"
                } else if has_awaiting {
                    "awaiting_host"
                } else {
                    "completed"
                };

                let active_worker = s_view.runs.iter()
                    .flat_map(|r| &r.workers)
                    .filter(|w| w.status.is_active())
                    .next_back()
                    .cloned();

                let is_running = session_status == "running";
                let is_awaiting = session_status == "awaiting_host";

                let session_display_name = if !s_view.session.display_name.is_empty() {
                    s_view.session.display_name.clone()
                } else {
                    s_view.session.id.clone()
                };

                view! {
                    // Session-Level Top Header
                    <div class="timeline-header">
                        <div class="timeline-header-main">
                            <span class="timeline-run-badge">
                                <i class=format!("not-italic {}", status_tone(session_status))>
                                    {format::status_glyph(session_status)}
                                </i>
                                <span class="capitalize">{t.t(&format!("run.status.{session_status}"))}</span>
                            </span>
                            <span class="timeline-session-title">{session_display_name}</span>
                            {(!s_view.session.cwd.is_empty()).then(|| view! {
                                <code class="timeline-session-cwd">{s_view.session.cwd.clone()}</code>
                            })}
                        </div>
                        <div class="timeline-header-meta">
                            <span class="dim">{format::date_time(&s_view.session.started_at, t.locale.get())}</span>
                            <span class="dot-separator" aria-hidden="true">"·"</span>
                            <span class="dim">{t.tp("counts.runs", chronological_runs.len())}</span>
                            <span class="dot-separator" aria-hidden="true">"·"</span>
                            <span class="dim">{t.tp("counts.steps", total_steps)}</span>
                        </div>
                    </div>

                    // Step Progress Anchor Bar (global step navigation across tasks)
                    <nav class="step-anchor-bar">
                        <span class="step-anchor-label">{t.t("steps.step")}:</span>
                        <div class="step-anchor-list">
                            {all_steps_meta
                                .into_iter()
                                .map(|(global_idx, step_id, profile_name, step_status)| {
                                    let s_id = step_id.clone();
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
                                                store.step.set(Some(s_id.clone()));
                                                crate::dom::scroll_to_element(&anchor_id);
                                            }
                                        >
                                            <span class=format!("step-num-badge {}", status_bg_tone(step_status))>
                                                {global_idx}
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
                    </nav>

                    // Continuous Timeline Stream Body (multi-task chronological stream)
                    <div
                        class="timeline-scroll-body"
                        node_ref=scroller
                    >
                        {run_offsets
                            .into_iter()
                            .map(|(run_view, run_idx, step_offset)| {
                                let r_id = run_view.run.id.clone();
                                let r_status = format::run_status(run_view.run.status);
                                let is_run_running = r_status == "running" || r_status == "starting";
                                let active_run_worker = run_view.workers.iter().filter(|w| w.status.is_active()).next_back().cloned();
                                let run_ended_at = if is_run_running {
                                    active_run_worker.as_ref().and_then(|w| w.ended_at.as_deref())
                                } else {
                                    run_view.workers.iter().rev().find_map(|w| w.ended_at.as_deref())
                                };
                                let duration = format::elapsed(&run_view.run.created_at, run_ended_at, t.locale.get());
                                let task_text = run_view.run.task.clone();
                                let steps = run_view.steps.clone();
                                let r_view_clone = run_view.clone();
                                let profiles_clone = profiles.clone();

                                view! {
                                    <div class="run-stream-block" id=format!("run-block-{}", r_id)>
                                        // Task Prompt Card
                                        <div class="task-card-section">
                                            <div class="task-card-header">
                                                <div class="task-card-header-left">
                                                    <span class="task-card-badge">
                                                        {t.tv("timeline.taskPrefix", &[("num", &(run_idx + 1).to_string())])}
                                                    </span>
                                                    <span class="task-card-time tabular dim">
                                                        {format::time_of_day(&run_view.run.created_at)}
                                                    </span>
                                                    <span class=format!("task-card-status {}", status_tone(r_status))>
                                                        <i class="not-italic">{format::status_glyph(r_status)}</i>
                                                        <span class="capitalize">{t.t(&format!("run.status.{r_status}"))}</span>
                                                    </span>
                                                    <span class="task-card-duration tabular dim">{duration}</span>
                                                </div>
                                            </div>
                                            <div class="task-card-prompt">
                                                <span class="task-card-prompt-icon">"👤"</span>
                                                <span class="task-card-prompt-text">{task_text}</span>
                                            </div>
                                        </div>

                                        // Step Cards for this Task
                                        <div class="run-steps-list">
                                            {steps
                                                .into_iter()
                                                .enumerate()
                                                .map(|(step_idx, step)| {
                                                    let sid = step.id.clone();
                                                    let global_idx = step_offset + step_idx + 1;
                                                    let profile_name = profiles_clone
                                                        .iter()
                                                        .find(|p| p.id == step.profile_id)
                                                        .map(|p| p.name.clone())
                                                        .unwrap_or_else(|| step.profile_id.clone());
                                                    let is_collapsed = Signal::derive({
                                                        let s = sid.clone();
                                                        move || collapsed_steps.get().contains(&s)
                                                    });
                                                    let r_view = r_view_clone.clone();
                                                    let toggle = {
                                                        let s = sid.clone();
                                                        Callback::new(move |()| toggle_step(s.clone()))
                                                    };

                                                    view! {
                                                        <StepCard
                                                            global_index=global_idx
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
                                    </div>
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
                                    }
                                }
                            >
                                {icon(icondata::LuArrowDown, "icon icon-xs")}
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
                                    {icon(icondata::LuSquare, "icon icon-xs")}
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
                                            {icon(icondata::LuPlay, "icon icon-xs")}
                                            <span>{t.t("timeline.resume")}</span>
                                        </button>
                                        <button
                                            class="btn btn-outline btn-sm"
                                            on:click=move |_| store.accept_worker(wid_accept.clone())
                                        >
                                            {icon(icondata::LuCheck, "icon icon-xs")}
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
    global_index: usize,
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

    let worker = run_view
        .workers
        .iter()
        .filter(|w| w.step_id == sid)
        .next_back()
        .cloned();
    let duration = format::elapsed(
        &step.created_at,
        worker.as_ref().and_then(|w| w.ended_at.as_deref()),
        t.locale.get(),
    );

    let run_id = run_view.run.id.clone();
    let r_view = run_view.clone();
    let s_id = step.id.clone();

    let events = Memo::new(move |_| {
        let cached = store.run_events(&run_id);
        format::events_for_step(&r_view, &s_id, &cached)
    });

    let expanded_diffs = RwSignal::new(HashSet::<String>::new());

    let toggle_file_diff = move |path: String| {
        expanded_diffs.update(|set| {
            if set.contains(&path) {
                set.remove(&path);
            } else {
                set.insert(path);
            }
        });
    };

    let is_active_step = step_status == "running" || step_status == "starting";

    view! {
        <section id=anchor_target class=move || format!("step-card-section {}", if is_collapsed.get() { "step-collapsed" } else { "" })>
            <header
                class="step-card-header"
                on:click=move |_| on_toggle.run(())
            >
                <div class="step-card-header-left">
                    <span class=format!("step-card-badge {}", status_bg_tone(step_status))>
                        {t.tv("timeline.stepFormat", &[("step", &global_index.to_string())])}
                    </span>
                    <span class="step-card-profile">{profile_name.clone()}</span>
                    <span class="step-card-status">
                        <i class=format!("not-italic {}", status_tone(step_status))>
                            {format::status_glyph(step_status)}
                        </i>
                        {t.t(&format!("run.status.{step_status}"))}
                    </span>
                    <span class="step-card-duration tabular">{duration.clone()}</span>
                </div>

                <div class="step-card-header-right">
                    <span class="step-card-count dim">
                        {move || {
                            let count = events.with(|evs| format::step_process_summary(evs).total_actions);
                            t.tv("timeline.actionsCount", &[("count", &count.to_string())])
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
                                {icon(icondata::LuChevronDown, "icon icon-xs")}
                                <span>{t.t("timeline.expand")}</span>
                            }
                            .into_any()
                        } else {
                            view! {
                                {icon(icondata::LuChevronUp, "icon icon-xs")}
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
                let (changes, rows, ai_messages, reasoning_text, summary, ev_list) = events.with(|evs| {
                    (
                        format::changed_files(evs),
                        format::console_rows(evs),
                        format::step_ai_messages(evs),
                        format::step_reasoning_text(evs),
                        format::step_process_summary(evs),
                        evs.clone(),
                    )
                });

                let has_actions = summary.total_actions > 0 || !rows.is_empty() || !changes.is_empty();
                let diff_label = state::diff_label(Some(summary.additions), Some(summary.deletions));
                let duration_copy = duration.clone();

                view! {
                    <div class="step-card-body">
                        // 1. Model Thinking / Reasoning Drawer (collapsible, separated and aggregated into one block)
                        {reasoning_text.as_ref().map(|reasoning| {
                            let total_chars = reasoning.len();
                            let html = format::markdown_to_html(reasoning);
                            view! {
                                <details class="step-thinking-drawer">
                                    <summary class="step-thinking-summary">
                                        <span class="step-thinking-icon">"💭"</span>
                                        <span class="step-thinking-title">{t.t("timeline.thinking")}</span>
                                        <span class="step-thinking-meta dim">
                                            {format!("{} {}", total_chars, t.t("timeline.characters"))}
                                        </span>
                                    </summary>
                                    <div class="step-thinking-content markdown-body" inner_html=html></div>
                                </details>
                            }
                        })}

                        // 2. Execution Process Drawer (Execution Trail - compact, default collapsed when done, open when active)
                        {has_actions.then(|| {
                            view! {
                                <details class="step-process-drawer" prop:open=is_active_step>
                                    <summary class="step-process-summary">
                                        <div class="step-process-summary-left">
                                            {icon(icondata::LuTerminal, "icon icon-xs")}
                                            <span class="step-process-title">{t.t("timeline.process")}</span>
                                            <span class="step-process-pill">
                                                {t.tv("timeline.actionsCount", &[("count", &summary.total_actions.to_string())])}
                                            </span>
                                            {if summary.reads > 0 {
                                                view! { <span class="step-process-sub-pill">{t.tv("timeline.reads", &[("count", &summary.reads.to_string())])}</span> }.into_any()
                                            } else { ().into_any() }}
                                            {if summary.edits > 0 {
                                                view! { <span class="step-process-sub-pill">{t.tv("timeline.edits", &[("count", &summary.edits.to_string())])}</span> }.into_any()
                                            } else { ().into_any() }}
                                            {if summary.commands > 0 {
                                                view! { <span class="step-process-sub-pill">{t.tv("timeline.commands", &[("count", &summary.commands.to_string())])}</span> }.into_any()
                                            } else { ().into_any() }}
                                            {if summary.subagents > 0 {
                                                view! { <span class="step-process-sub-pill">{t.tv("timeline.subagents", &[("count", &summary.subagents.to_string())])}</span> }.into_any()
                                            } else { ().into_any() }}
                                            {if !diff_label.is_empty() {
                                                view! { <span class="step-process-diff tabular">{diff_label}</span> }.into_any()
                                            } else { ().into_any() }}
                                        </div>
                                        <div class="step-process-summary-right">
                                            {if is_active_step {
                                                if let Some(active) = summary.active_action {
                                                    view! {
                                                        <span class="step-process-active-tag">
                                                            <span class="pulsing-dot-sm"></span>
                                                            <span class="step-process-active-text">{active}</span>
                                                        </span>
                                                    }.into_any()
                                                } else {
                                                    view! {
                                                        <span class="step-process-active-tag">
                                                            <span class="pulsing-dot-sm"></span>
                                                            <span>{t.t("timeline.running")}</span>
                                                        </span>
                                                    }.into_any()
                                                }
                                            } else {
                                                view! { <span class="dim tabular text-xs">{duration_copy}</span> }.into_any()
                                            }}
                                        </div>
                                    </summary>

                                    <div class="step-process-content">
                                        // Changed Files & Code Diff Preview Block
                                        {(!changes.is_empty()).then(|| {
                                            view! {
                                                <div class="step-changes-block">
                                                    <div class="step-changes-header">
                                                        {icon(icondata::LuDiff, "icon icon-xs")}
                                                        <span class="font-medium">{t.t("console.changes")}</span>
                                                        <span class="badge badge-secondary">{changes.len()}</span>
                                                    </div>
                                                    <ul class="step-changes-list">
                                                        {changes
                                                            .into_iter()
                                                            .map(|file| {
                                                                let path_for_diff = file.path.clone();
                                                                let path_key = file.path.clone();
                                                                let is_diff_open = Signal::derive({
                                                                    let p = path_key.clone();
                                                                    move || expanded_diffs.get().contains(&p)
                                                                });
                                                                let diff_content = format::diff_for_path(&ev_list, &path_for_diff);
                                                                let has_diff = diff_content.is_some();
                                                                let toggle_p = path_key.clone();

                                                                view! {
                                                                    <li class="step-change-item-wrap">
                                                                        <div
                                                                            class="step-change-item"
                                                                            on:click=move |_| toggle_file_diff(toggle_p.clone())
                                                                        >
                                                                            <code class="change-path">{file.path.clone()}</code>
                                                                            <div class="change-diff-stats">
                                                                                {(file.additions > 0.0)
                                                                                    .then(|| view! { <span class="change-add">"+" {file.additions}</span> })}
                                                                                {(file.deletions > 0.0)
                                                                                    .then(|| view! { <span class="change-del">"-" {file.deletions}</span> })}
                                                                                {has_diff.then(|| view! {
                                                                                    <span class="diff-preview-tag">
                                                                                        {icon(icondata::LuDiff, "icon icon-xs")}
                                                                                        {move || if is_diff_open.get() { t.t("timeline.collapse") } else { t.t("timeline.diff") }}
                                                                                    </span>
                                                                                })}
                                                                            </div>
                                                                        </div>

                                                                        // Inline unified diff preview
                                                                        {move || {
                                                                            if !is_diff_open.get() {
                                                                                return ().into_any();
                                                                            }
                                                                            let formatted_diff = diff_content.as_deref().map(format::diff_to_html).unwrap_or_else(|| {
                                                                                let escaped_path = format::html_escape(&file.path);
                                                                                format!("<div class=\"diff-container\"><div class=\"diff-line diff-file-header\"><code>{} (+{} -{})</code></div></div>", escaped_path, file.additions, file.deletions)
                                                                            });
                                                                            view! {
                                                                                <div class="step-inline-diff-viewer" inner_html=formatted_diff></div>
                                                                            }
                                                                            .into_any()
                                                                        }}
                                                                    </li>
                                                                }
                                                            })
                                                            .collect_view()}
                                                    </ul>
                                                </div>
                                            }
                                        })}

                                        // Activity Execution Log Rows (No nested secondary vertical scroll)
                                        {(!rows.is_empty()).then(|| {
                                            view! {
                                                <div class="step-logs-block">
                                                    <div class="log-stream-list log-stream-no-scroll">
                                                        {rows
                                                            .iter()
                                                            .map(|row| render_log_row(row.clone(), &ev_list, &t))
                                                            .collect_view()}
                                                    </div>
                                                </div>
                                            }
                                        })}
                                    </div>
                                </details>
                            }
                        })}

                        // 3. Final Answer / Response Section (Prominent Hero Markdown with contained row scrolling)
                        {(!ai_messages.is_empty()).then(|| {
                            view! {
                                <div class="step-final-answer">
                                    <div class="step-final-answer-header">
                                        <span class="step-final-icon">"💬"</span>
                                        <span class="step-final-title">{t.t("timeline.finalAnswer")}</span>
                                    </div>
                                    <div class="step-final-body markdown-body">
                                        {ai_messages
                                            .into_iter()
                                            .map(|msg| {
                                                let html = format::markdown_to_html(&msg);
                                                view! {
                                                    <div class="step-ai-bubble-content" inner_html=html></div>
                                                }
                                            })
                                            .collect_view()}
                                    </div>
                                </div>
                            }
                        })}
                    </div>
                }
                .into_any()
            }}
        </section>
    }
}

fn render_log_row(
    row: ConsoleRow,
    events: &[relay_core::RelayEvent],
    t: &crate::i18n::Translator,
) -> impl IntoView {
    let accent = match row.kind {
        ConsoleKind::Edit => "log-warn",
        ConsoleKind::Test | ConsoleKind::Result => "log-ok",
        ConsoleKind::Warning => "log-warn",
        ConsoleKind::Error => "log-danger",
        _ => "",
    };
    let time = format::time_of_day(&row.timestamp);
    let kind = t.t(row.kind.key());
    let label = match row.label.as_str() {
        "Worker started" => t.t("event.workerStarted"),
        "Accepted by Codex" => t.t("event.accepted"),
        "Awaiting host" => t.t("event.awaitingHost"),
        _ => row.label.clone(),
    };
    let count = row.count;
    let diff = state::diff_label(row.additions, row.deletions);

    // If this is a terminal command, render as a specialized terminal card with ANSI support
    if row.kind == ConsoleKind::Command {
        let output = format::command_output_for_event(events, &row.id);
        let has_output = output.is_some();
        let cmd_html = format::ansi_to_html(&label);

        return view! {
            <div class=format!("log-row terminal-capsule-row {}", accent)>
                <time class="log-time tabular">{time}</time>
                <span class="log-kind">{kind}</span>
                <div class="terminal-capsule">
                    <div class="terminal-cmd-bar">
                        <span class="terminal-prompt">"$"</span>
                        <code class="terminal-cmd-text" inner_html=cmd_html></code>
                    </div>
                    {has_output.then(|| {
                        let out_html = format::ansi_to_html(&output.unwrap());
                        view! {
                            <pre class="terminal-output" inner_html=out_html></pre>
                        }
                    })}
                </div>
            </div>
        }
        .into_any();
    }

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
                                view! { <i class="log-note">" " {t.tp("counts.files", count)}</i> }
                            })}
                        {(!diff.is_empty()).then(|| view! { <i class="log-note tabular">{diff}</i> })}
                    </div>
                }
                .into_any()
            } else {
                let text_html = format::ansi_to_html(&label);
                view! {
                    <code class="log-label">
                        <span inner_html=text_html></span>
                        {count
                            .map(|count| {
                                view! { <i class="log-note">" " {t.tp("counts.files", count)}</i> }
                            })}
                        {(!diff.is_empty()).then(|| view! { <i class="log-note tabular">{diff}</i> })}
                    </code>
                }
                .into_any()
            }}
        </div>
    }
    .into_any()
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
