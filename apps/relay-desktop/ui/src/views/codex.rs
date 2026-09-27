//! The Codex integration as a lifecycle: install, repair, update and remove,
//! with a reason next to every check.

use leptos::prelude::*;
use relay_api::{CodexAction, CodexCheckId, CodexCheckStatus};

use crate::components::controls::{badge, confirm_dialog, icon};
use crate::state::PanelStore;

pub fn codex_view(store: PanelStore) -> AnyView {
    let t = store.translator();
    let remove_open = RwSignal::new(false);
    let highlight = store.intent.get_untracked() == Some(crate::state::PanelIntent::CodexActions);

    view! {
        <div class="stack">
            <div class="card">
                {move || {
                    let checks = store
                        .codex
                        .with(|status| status.as_ref().map(|value| value.checks.clone()).unwrap_or_default());
                    checks
                        .into_iter()
                        .map(|check| {
                            let tone = if check.ok { "tone-ok" } else { tone_for(check.status) };
                            view! {
                                <div class="check-row">
                                    <div class="check-head">
                                        <i class=format!("check-glyph not-italic {tone}")>
                                            {if check.ok { "✓" } else { "✗" }}
                                        </i>
                                        <span class="check-label truncate">{t.t(label_key(check.id))}</span>
                                        {badge(status_name(check.status), "outline")}
                                    </div>
                                    <p class="check-detail wrap">{check.detail.clone()}</p>
                                    {check.hint.map(|hint| view! { <p class="check-hint wrap">{hint}</p> })}
                                </div>
                            }
                        })
                        .collect_view()
                }}
            </div>

            <div class="actions-block">
                <div class="actions-head">
                    <span class="actions-title">{t.t("panel.actions")}</span>
                    <span class="tabular dim">
                        {move || {
                            let checks = store
                                .codex
                                .with(|status| status.as_ref().map(|value| value.checks.clone()).unwrap_or_default());
                            format!("{}/{}", checks.iter().filter(|check| check.ok).count(), checks.len())
                        }}
                    </span>
                </div>
                <div class=if highlight { "actions-highlight" } else { "actions-plain" }>
                    <button
                        class="btn btn-primary btn-sm full"
                        disabled=move || store.busy.get()
                        on:click=move |_| store.run_codex(CodexAction::Install)
                    >
                        {icon("download", "icon icon-xs")}
                        <span>{t.t("panel.install")}</span>
                    </button>
                    <div class="row-actions">
                        <button
                            class="btn btn-outline btn-sm grow"
                            disabled=move || store.busy.get()
                            on:click=move |_| store.run_codex(CodexAction::Repair)
                        >
                            {icon("wrench", "icon icon-xs")}
                            <span>{t.t("panel.repair")}</span>
                        </button>
                        <button
                            class="btn btn-outline btn-sm grow"
                            disabled=move || store.busy.get()
                            on:click=move |_| store.run_codex(CodexAction::Update)
                        >
                            {icon("refresh", "icon icon-xs")}
                            <span>{t.t("panel.update")}</span>
                        </button>
                    </div>
                    <button
                        class="btn btn-ghost btn-sm full danger-text"
                        disabled=move || store.busy.get()
                        on:click=move |_| remove_open.set(true)
                    >
                        {icon("trash", "icon icon-xs")}
                        <span>{t.t("panel.remove")}</span>
                    </button>
                </div>
            </div>

            {confirm_dialog(
                remove_open,
                t.t("panel.codex.removeTitle"),
                t.t("panel.removeConfirm"),
                t.t("action.cancel"),
                t.t("panel.remove"),
                Callback::new(move |_| store.run_codex(CodexAction::Remove)),
            )}
        </div>
    }
    .into_any()
}

fn label_key(id: CodexCheckId) -> &'static str {
    id.label_key()
}

fn status_name(status: CodexCheckStatus) -> &'static str {
    match status {
        CodexCheckStatus::Ok => "ok",
        CodexCheckStatus::Missing => "missing",
        CodexCheckStatus::Stale => "stale",
        CodexCheckStatus::Outdated => "outdated",
        CodexCheckStatus::Legacy => "legacy",
    }
}

fn tone_for(status: CodexCheckStatus) -> &'static str {
    match status {
        CodexCheckStatus::Ok => "tone-ok",
        CodexCheckStatus::Outdated | CodexCheckStatus::Legacy => "tone-warn",
        CodexCheckStatus::Stale => "tone-danger",
        CodexCheckStatus::Missing => "tone-muted",
    }
}
