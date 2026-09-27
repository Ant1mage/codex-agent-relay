//! Runtimes: automatic detection and hand-registered CLIs in one list; their
//! origin is metadata, not a second visual hierarchy.

use leptos::prelude::*;
use relay_core::{ManualRuntime, Runtime, RuntimeHealth};

use crate::components::controls::{badge, confirm_dialog, icon, select_input, text_input, text_input_with_class, Choice};
use crate::dom;
use crate::i18n::Translator;
use crate::state::{PanelIntent, PanelStore};

#[derive(Debug, Clone, PartialEq)]
struct RuntimeDraft {
    id: String,
    adapter_id: String,
    executable_path: String,
    label: String,
    is_new: bool,
}

impl RuntimeDraft {
    fn blank(adapter_id: String) -> Self {
        Self {
            id: format!("manual-runtime-{}", dom::base36(dom::now_ms() as u64)),
            adapter_id,
            executable_path: String::new(),
            label: String::new(),
            is_new: true,
        }
    }
}

pub fn runtimes_view(store: PanelStore) -> AnyView {
    let t = store.translator();

    let editing = RwSignal::new(false);
    let draft = RwSignal::new(RuntimeDraft::blank(String::new()));
    let delete_open = RwSignal::new(false);
    let delete_entry = RwSignal::new(None::<ManualRuntime>);

    // Tray intent: "Add runtime…" opens the editor directly.
    Effect::new(move |_| {
        if store.intent.get() == Some(PanelIntent::AddRuntime) {
            open_editor(store, draft, editing);
            store.consume_intent();
        }
    });

    // The adapter catalogue used to be fetched by an effect that watched the
    // very signal it wrote: on failure it reset the list to empty, which re-ran
    // the effect and fired the next request forever. The catalogue now lives in
    // PanelStore behind an in-flight marker, and ensure_adapters is only called
    // when the editor opens, so a failed request is retried by the user.
    view! {
        {move || {
            if editing.get() {
                runtime_editor(store, t, editing, draft).into_any()
            } else {
                runtime_list(store, t, editing, draft, delete_open, delete_entry).into_any()
            }
        }}
        {confirm_dialog(
            delete_open,
            t.t("panel.runtime.deleteTitle"),
            t.t("panel.runtime.deleteConfirm"),
            t.t("action.cancel"),
            t.t("panel.delete"),
            Callback::new(move |_| {
                if let Some(entry) = delete_entry.get_untracked() {
                    store.delete_runtime(entry.id);
                }
                delete_entry.set(None);
            }),
        )}
    }
    .into_any()
}

/// Opens the runtime editor: a fresh draft seeded from the catalogue when it is
/// already cached, no stale probe, and a catalogue request that the store owns
/// rather than this view.
fn open_editor(store: PanelStore, draft: RwSignal<RuntimeDraft>, editing: RwSignal<bool>) {
    let known = store.adapters_cached().unwrap_or_default();
    draft.set(RuntimeDraft::blank(known.first().cloned().unwrap_or_default()));
    store.clear_probe();
    store.ensure_adapters();
    editing.set(true);
}

/// Automatic and manual entries stay in one list, matched by id.
fn runtime_rows(runtimes: &[Runtime], manual: &[ManualRuntime]) -> Vec<(Option<Runtime>, Option<ManualRuntime>)> {
    let mut rows: Vec<(Option<Runtime>, Option<ManualRuntime>)> = runtimes
        .iter()
        .map(|runtime| {
            let entry = manual.iter().find(|entry| entry.id == runtime.id).cloned();
            (Some(runtime.clone()), entry)
        })
        .collect();
    for entry in manual {
        if !runtimes.iter().any(|runtime| runtime.id == entry.id) {
            rows.push((None, Some(entry.clone())));
        }
    }
    rows
}

fn health_tone(health: Option<RuntimeHealth>) -> &'static str {
    match health {
        Some(RuntimeHealth::Available) => "success",
        Some(RuntimeHealth::AuthenticationRequired) => "warning",
        _ => "destructive",
    }
}

fn health_key(health: Option<RuntimeHealth>) -> &'static str {
    match health {
        Some(RuntimeHealth::Available) => "panel.runtime.health.available",
        Some(RuntimeHealth::AuthenticationRequired) => "panel.runtime.health.authentication_required",
        None | Some(RuntimeHealth::Unavailable) => "panel.runtime.health.unavailable",
    }
}

fn capability_count(runtime: &Runtime) -> u32 {
    let capabilities = runtime.capabilities;
    [
        capabilities.non_interactive,
        capabilities.structured_events,
        capabilities.cwd,
        capabilities.resume,
        capabilities.send,
        capabilities.cancel,
        capabilities.child_sessions,
        capabilities.model_selection.unwrap_or(false),
    ]
    .into_iter()
    .filter(|value| *value)
    .count() as u32
}

#[allow(clippy::too_many_arguments)]
fn runtime_list(
    store: PanelStore,
    t: Translator,
    editing: RwSignal<bool>,
    draft: RwSignal<RuntimeDraft>,
    delete_open: RwSignal<bool>,
    delete_entry: RwSignal<Option<ManualRuntime>>,
) -> impl IntoView {
    let rows = move || {
        let runtimes = store.snapshot.with(|snapshot| snapshot.as_ref().map(|value| value.runtimes.clone()).unwrap_or_default());
        let manual =
            store.config.with(|config| config.as_ref().map(|value| value.manual_runtimes.clone()).unwrap_or_default());
        runtime_rows(&runtimes, &manual)
    };

    view! {
        <div class="stack">
            <div class="view-head">
                <div class="view-head-text">
                    <h2 class="view-title truncate">{t.t("panel.runtime")}</h2>
                    <p class="view-sub tabular">{move || rows().len()} " " {t.t("panel.runtime.onThisMac")}</p>
                </div>
                <button
                    class="btn-icon"
                    title=move || t.t("panel.rescan")
                    disabled=move || store.busy.get()
                    on:click=move |_| store.rescan()
                >
                    {icon("refresh", "icon icon-xs")}
                </button>
                <button class="btn btn-primary btn-xs" on:click=move |_| open_editor(store, draft, editing)>
                    {icon("plus", "icon icon-xs")}
                    <span>{t.t("panel.addRuntime")}</span>
                </button>
            </div>

            {move || {
                (rows().is_empty()).then(|| view! { <p class="empty-box">{t.t("panel.noRuntimes")}</p> })
            }}

            <div class="item-list">
                {move || {
                    rows()
                        .into_iter()
                        .map(|(runtime, manual)| {
                            let health = runtime.as_ref().map(|runtime| runtime.health);
                            let adapter_id = runtime
                                .as_ref()
                                .map(|runtime| runtime.adapter_id.clone())
                                .or_else(|| manual.as_ref().map(|entry| entry.adapter_id.clone()))
                                .unwrap_or_else(|| "unknown".to_string());
                            let name = manual
                                .as_ref()
                                .and_then(|entry| entry.label.clone())
                                .unwrap_or_else(|| adapter_id.clone());
                            let path = runtime
                                .as_ref()
                                .map(|runtime| runtime.executable_path.clone())
                                .or_else(|| manual.as_ref().map(|entry| entry.executable_path.clone()))
                                .unwrap_or_default();
                            let version = runtime.as_ref().and_then(|runtime| runtime.version.clone());
                            let capabilities = runtime.as_ref().map(capability_count).unwrap_or(0);
                            let is_manual = manual.is_some();
                            let edit_entry = manual.clone();
                            let remove_entry = manual.clone();
                            view! {
                                <div class="item">
                                    <div class="item-main item-static">
                                        <span class="item-title-row">
                                            <span class="item-title truncate">{name.clone()}</span>
                                            {(name != adapter_id)
                                                .then(|| view! { <span class="item-adapter">{adapter_id.clone()}</span> })}
                                        </span>
                                        <span class="item-badges">
                                            {badge(
                                                if is_manual {
                                                    t.t("panel.runtime.sourceManual")
                                                } else {
                                                    t.t("panel.runtime.sourceDetected")
                                                },
                                                "outline",
                                            )}
                                            {badge(t.t(health_key(health)), health_tone(health))}
                                            {version.map(|version| view! { <span class="item-note">{version}</span> })}
                                            {(capabilities > 0)
                                                .then(|| {
                                                    view! {
                                                        <span class="item-note">
                                                            "· " {capabilities} " " {t.t("panel.runtime.capabilities")}
                                                        </span>
                                                    }
                                                })}
                                        </span>
                                        <span class="item-path wrap" title=path.clone()>{path.clone()}</span>
                                    </div>
                                    {is_manual
                                        .then(|| {
                                            view! {
                                                <span class="item-actions">
                                                    <button
                                                        class="btn-icon"
                                                        title=move || t.t("panel.runtime.edit")
                                                        on:click=move |_| {
                                                            if let Some(entry) = edit_entry.clone() {
                                                                store.clear_probe();
                                                                store.ensure_adapters();
                                                                draft.set(RuntimeDraft {
                                                                    id: entry.id,
                                                                    adapter_id: entry.adapter_id,
                                                                    executable_path: entry.executable_path,
                                                                    label: entry.label.unwrap_or_default(),
                                                                    is_new: false,
                                                                });
                                                                editing.set(true);
                                                            }
                                                        }
                                                    >
                                                        {icon("pencil", "icon icon-xs")}
                                                    </button>
                                                    <button
                                                        class="btn-icon btn-icon-danger"
                                                        title=move || t.t("panel.delete")
                                                        on:click=move |_| {
                                                            delete_entry.set(remove_entry.clone());
                                                            delete_open.set(true);
                                                        }
                                                    >
                                                        {icon("trash", "icon icon-xs")}
                                                    </button>
                                                </span>
                                            }
                                        })}
                                </div>
                            }
                        })
                        .collect_view()
                }}
            </div>

            {move || {
                let diagnostics =
                    store.snapshot.with(|snapshot| snapshot.as_ref().map(|value| value.diagnostics.clone()).unwrap_or_default());
                (!diagnostics.is_empty())
                    .then(|| {
                        view! {
                            <div class="diagnostics">
                                <p class="diagnostics-title">{t.t("panel.runtime.diagnostics")}</p>
                                {diagnostics
                                    .into_iter()
                                    .map(|line| view! { <p class="diagnostics-line wrap">{line}</p> })
                                    .collect_view()}
                            </div>
                        }
                    })
            }}
        </div>
    }
}

fn runtime_editor(
    store: PanelStore,
    t: Translator,
    editing: RwSignal<bool>,
    draft: RwSignal<RuntimeDraft>,
) -> impl IntoView {
    let adapter_choices =
        move || store.adapters().unwrap_or_default().into_iter().map(Choice::same).collect::<Vec<Choice>>();
    let label_placeholder = {
        let current = draft.get_untracked();
        if current.adapter_id.is_empty() { t.t("cliInfo.runtime") } else { current.adapter_id }
    };

    // The catalogue can arrive after the editor opened: seed the form with its
    // first entry. This is a plain effect (no await), so it is disposed with the
    // editor instead of writing into it from a detached task.
    Effect::new(move |_| {
        if draft.get_untracked().adapter_id.is_empty() {
            if let Some(first) = store.adapters().and_then(|list| list.first().cloned()) {
                draft.update(|current| current.adapter_id = first);
            }
        }
    });

    let check = move |_| {
        let current = draft.get_untracked();
        store.probe_runtime(current.adapter_id, current.executable_path);
    };

    view! {
        <div class="stack">
            <div class="view-head">
                <button class="btn-icon" title=move || t.t("onboarding.back") on:click=move |_| editing.set(false)>
                    {icon("back", "icon icon-xs")}
                </button>
                <span class="view-title truncate">
                    {move || if draft.get().is_new { t.t("panel.addRuntime") } else { t.t("panel.runtime.edit") }}
                </span>
            </div>

            <div class="field-group">
                <div class="field">
                    <label class="field-label">{t.t("panel.runtime.name")}</label>
                    {text_input(
                        move || draft.get().label,
                        label_placeholder,
                        move |value| draft.update(|draft| draft.label = value),
                    )}
                    <p class="field-hint wrap">{t.t("panel.runtime.nameHint")}</p>
                </div>
                <div class="field">
                    <label class="field-label">{t.t("cliInfo.runtime")}</label>
                    {move || select_input(
                        move || draft.get().adapter_id,
                        adapter_choices(),
                        move |value| {
                            draft.update(|draft| draft.adapter_id = value);
                            store.clear_probe();
                        },
                    )}
                </div>
                <div class="field">
                    <label class="field-label">{t.t("panel.runtime.path")}</label>
                    {text_input_with_class(
                        move || draft.get().executable_path,
                        "/usr/local/bin/dsh".to_string(),
                        "mono",
                        move |value| {
                            draft.update(|draft| draft.executable_path = value);
                            store.clear_probe();
                        },
                    )}
                    <p class="field-hint wrap">{t.t("panel.runtime.pathHint")}</p>
                </div>
            </div>

            {move || {
                store.probe.get().map(|result| {
                    let (class, message) = if result.ok {
                        let suffix = result.version.map(|version| format!(" · {version}")).unwrap_or_default();
                        ("probe-ok", format!("{}{suffix}", t.t("panel.runtime.probeOk")))
                    } else {
                        ("probe-fail", result.error.unwrap_or_else(|| t.t("panel.runtime.checking")))
                    };
                    view! { <p class=format!("probe {class} wrap")>{message}</p> }
                })
            }}

            <div class="row-actions">
                <button
                    class="btn btn-outline btn-sm"
                    disabled=move || {
                        let current = draft.get();
                        current.adapter_id.is_empty() || current.executable_path.is_empty() || store.probing.get()
                    }
                    on:click=check
                >
                    {icon("zap", "icon icon-xs")}
                    <span>{move || if store.probing.get() { t.t("panel.runtime.checking") } else { t.t("panel.runtime.check") }}</span>
                </button>
                <button
                    class="btn btn-primary btn-sm grow"
                    disabled=move || {
                        let current = draft.get();
                        current.adapter_id.is_empty() || current.executable_path.is_empty() || store.busy.get()
                    }
                    on:click=move |_| {
                        let current = draft.get_untracked();
                        let label = current.label.trim().to_string();
                        store.save_runtime(
                            current.id,
                            current.adapter_id,
                            current.executable_path,
                            if label.is_empty() { None } else { Some(label) },
                        );
                        editing.set(false);
                    }
                >
                    {t.t("action.save")}
                </button>
                <button class="btn btn-outline btn-sm" on:click=move |_| editing.set(false)>
                    {t.t("action.cancel")}
                </button>
            </div>
        </div>
    }
}
