//! Agent Profiles: the list and the editor.
//!
//! Two views rather than one long page: in a 420px popover an editor appended
//! under the list lands below the fold and reads as "the button did nothing".
//! Nothing here creates a profile on its own — an empty list shows a button.

use leptos::prelude::*;
use leptos::task::spawn_local;
use relay_api::RuntimeOptionsView;
use relay_core::{AgentProfile, CapabilitySet, Runtime};

use crate::components::controls::{badge, confirm_dialog, icon, select_input, switch, text_input, textarea, Choice};
use crate::dom;
use crate::i18n::Translator;
use crate::state::{PanelIntent, PanelStore};

/// What the editor binds to. `persisted` decides between "New agent" and
/// "Edit profile", and whether Delete is offered at all.
#[derive(Debug, Clone, PartialEq)]
struct AgentDraft {
    id: String,
    name: String,
    description: String,
    runtime_id: String,
    model: Option<String>,
    reasoning: Option<String>,
    instructions: String,
    capabilities: CapabilitySet,
    enabled: bool,
    persisted: bool,
}

impl AgentDraft {
    fn blank(runtime_id: String) -> Self {
        Self {
            id: format!("agent-{}", dom::now_ms() as u64),
            name: String::new(),
            description: String::new(),
            runtime_id,
            model: None,
            reasoning: None,
            instructions: String::new(),
            capabilities: CapabilitySet::default(),
            enabled: true,
            persisted: false,
        }
    }

    fn from_profile(profile: &AgentProfile) -> Self {
        Self {
            id: profile.id.clone(),
            name: profile.name.clone(),
            description: profile.description.clone(),
            runtime_id: profile.runtime_id.clone(),
            model: profile.model.clone(),
            reasoning: profile.reasoning.clone(),
            instructions: profile.instructions.clone().unwrap_or_default(),
            capabilities: profile.capabilities,
            enabled: profile.enabled,
            persisted: true,
        }
    }

    /// The panel has always used the name as the description when none was given.
    fn to_profile(&self) -> AgentProfile {
        AgentProfile {
            id: self.id.clone(),
            name: self.name.clone(),
            description: if self.description.is_empty() { self.name.clone() } else { self.description.clone() },
            runtime_id: self.runtime_id.clone(),
            instructions: if self.instructions.is_empty() { None } else { Some(self.instructions.clone()) },
            model: self.model.clone(),
            reasoning: self.reasoning.clone(),
            capabilities: self.capabilities,
            enabled: self.enabled,
        }
    }
}

const DEFAULT_MODEL: &str = "__default__";

pub fn agents_view(store: PanelStore) -> AnyView {
    let t = store.translator();

    let editing = RwSignal::new(false);
    let draft = RwSignal::new(AgentDraft::blank(String::new()));
    // Tracked on its own so typing a name does not refetch model options.
    let runtime_id = RwSignal::new(String::new());
    let options = RwSignal::new(None::<RuntimeOptionsView>);
    let options_for = RwSignal::new(String::new());
    let delete_open = RwSignal::new(false);

    // Model and reasoning are whatever the CLI advertises, never an invented list.
    Effect::new(move |_| {
        let current = runtime_id.get();
        if current.is_empty() {
            options.set(None);
            return;
        }
        options.set(None);
        options_for.set(current.clone());
        let task = store.runtime_options(current.clone());
        spawn_local(async move {
            match task.await {
                Ok(loaded) => {
                    if options_for.get_untracked() == loaded.runtime_id {
                        options.set(Some(loaded));
                    }
                }
                Err(_) => {
                    if options_for.get_untracked() == current {
                        options.set(None);
                    }
                }
            }
        });
    });

    // The tray navigates the open panel with an intent instead of a reload.
    Effect::new(move |_| {
        match store.intent.get() {
            Some(PanelIntent::NewAgent) => {
                open_new(store, editing, draft, runtime_id);
                store.consume_intent();
            }
            Some(PanelIntent::EditAgent) => {
                let wanted = store.intent_profile.get();
                let profile = wanted.and_then(|id| {
                    store
                        .config
                        .get_untracked()
                        .and_then(|config| config.profiles.into_iter().find(|profile| profile.id == id))
                });
                if let Some(profile) = profile {
                    open_existing(editing, draft, runtime_id, &profile);
                }
                store.consume_intent();
            }
            _ => {}
        }
    });

    view! {
        {move || {
            if editing.get() {
                agent_editor(store, t, editing, draft, runtime_id, options, delete_open).into_any()
            } else {
                agent_list(store, t, editing, draft, runtime_id, options, delete_open).into_any()
            }
        }}
    }
    .into_any()
}

fn runtimes_snapshot(store: PanelStore) -> Vec<Runtime> {
    store.snapshot.get_untracked().map(|snapshot| snapshot.runtimes).unwrap_or_default()
}

fn runtime_name(runtimes: &[Runtime], id: &str) -> String {
    runtimes.iter().find(|runtime| runtime.id == id).map(|runtime| runtime.adapter_id.clone()).unwrap_or_else(|| id.to_string())
}

fn open_new(
    store: PanelStore,
    editing: RwSignal<bool>,
    draft: RwSignal<AgentDraft>,
    runtime_id: RwSignal<String>,
) {
    let runtimes = runtimes_snapshot(store);
    let preferred = runtimes
        .iter()
        .find(|runtime| runtime.health == relay_core::RuntimeHealth::Available)
        .or_else(|| runtimes.first())
        .map(|runtime| runtime.id.clone())
        .unwrap_or_default();
    runtime_id.set(preferred.clone());
    draft.set(AgentDraft::blank(preferred));
    editing.set(true);
}

fn open_existing(
    editing: RwSignal<bool>,
    draft: RwSignal<AgentDraft>,
    runtime_id: RwSignal<String>,
    profile: &AgentProfile,
) {
    runtime_id.set(profile.runtime_id.clone());
    draft.set(AgentDraft::from_profile(profile));
    editing.set(true);
}

fn agent_list(
    store: PanelStore,
    t: Translator,
    editing: RwSignal<bool>,
    draft: RwSignal<AgentDraft>,
    runtime_id: RwSignal<String>,
    _options: RwSignal<Option<RuntimeOptionsView>>,
    _delete_open: RwSignal<bool>,
) -> impl IntoView {
    let profiles = move || store.config.with(|config| config.as_ref().map(|value| value.profiles.clone()).unwrap_or_default());
    let runtimes = move || store.snapshot.with(|snapshot| snapshot.as_ref().map(|value| value.runtimes.clone()).unwrap_or_default());

    view! {
        <div class="stack">
            <div class="view-head">
                <div class="view-head-text">
                    <h2 class="view-title truncate">{t.t("nav.agents")}</h2>
                    <p class="view-sub tabular">
                        {move || profiles().len()} " " {t.t("panel.agent.profiles")}
                    </p>
                </div>
                <button
                    class="btn btn-primary btn-xs"
                    disabled=move || runtimes().is_empty()
                    on:click=move |_| open_new(store, editing, draft, runtime_id)
                >
                    {icon("plus", "icon icon-xs")}
                    <span>{t.t("panel.newAgent")}</span>
                </button>
            </div>

            {move || {
                (profiles().is_empty())
                    .then(|| view! { <p class="empty-box">{t.t("panel.agents.empty")}</p> })
            }}

            <div class="item-list">
                {move || {
                    let runtimes_now = runtimes();
                    profiles()
                        .into_iter()
                        .map(|profile| {
                            let runtime = runtimes_now.iter().find(|candidate| candidate.id == profile.runtime_id);
                            let adapter = runtime_name(&runtimes_now, &profile.runtime_id);
                            let health = runtime.map(|runtime| runtime.health);
                            let meta = {
                                let mut parts = vec![adapter];
                                if let Some(model) = profile.model.clone() {
                                    parts.push(model);
                                }
                                if let Some(reasoning) = profile.reasoning.clone() {
                                    parts.push(reasoning);
                                }
                                parts.join(" · ")
                            };
                            let open_profile = profile.clone();
                            let toggle_profile = profile.clone();
                            let enabled = profile.enabled;
                            view! {
                                <div class="item">
                                    <button
                                        class="item-main"
                                        on:click=move |_| open_existing(editing, draft, runtime_id, &open_profile)
                                    >
                                        <span class="item-title-row">
                                            <span class="item-title truncate">{profile.name.clone()}</span>
                                            {health
                                                .filter(|health| *health != relay_core::RuntimeHealth::Available)
                                                .map(|health| {
                                                    let label = if health == relay_core::RuntimeHealth::AuthenticationRequired {
                                                        t.t("agents.authRequired")
                                                    } else {
                                                        t.t("agents.notInstalled")
                                                    };
                                                    badge(label, "destructive")
                                                })}
                                        </span>
                                        <span class="item-meta truncate">{meta}</span>
                                    </button>
                                    <span class="item-actions">
                                        {switch(
                                            move || enabled,
                                            move |next| {
                                                let mut updated = toggle_profile.clone();
                                                updated.enabled = next;
                                                store.save_profile(updated);
                                            },
                                        )}
                                    </span>
                                </div>
                            }
                        })
                        .collect_view()
                }}
            </div>
        </div>
    }
}

#[allow(clippy::too_many_arguments)]
fn agent_editor(
    store: PanelStore,
    t: Translator,
    editing: RwSignal<bool>,
    draft: RwSignal<AgentDraft>,
    runtime_id: RwSignal<String>,
    options: RwSignal<Option<RuntimeOptionsView>>,
    delete_open: RwSignal<bool>,
) -> impl IntoView {
    // Read once when the editor opens: later snapshot updates must not rebuild
    // the form (that would drop what the user is typing).
    let runtimes = runtimes_snapshot(store);
    let runtime_choices: Vec<Choice> = runtimes
        .iter()
        .map(|runtime| {
            let suffix = if runtime.health == relay_core::RuntimeHealth::Available {
                String::new()
            } else {
                format!(" ({})", crate::format::health_name(runtime.health))
            };
            Choice::new(runtime.id.clone(), format!("{}{suffix}", runtime.adapter_id))
        })
        .collect();

    let model_value = move || draft.get().model.unwrap_or_else(|| DEFAULT_MODEL.to_string());
    let reasoning_value = move || draft.get().reasoning.unwrap_or_else(|| DEFAULT_MODEL.to_string());

    let model_choices = move || {
        let mut choices = vec![Choice::new(DEFAULT_MODEL, t.t("panel.modelAuto"))];
        if let Some(loaded) = options.get() {
            for model in loaded.models {
                let label = model.label.clone().unwrap_or_else(|| model.value.clone());
                choices.push(Choice::new(model.value, label));
            }
        }
        choices
    };
    let reasoning_choices = move || {
        let mut choices = vec![Choice::new(DEFAULT_MODEL, t.t("agents.runtimeDefault"))];
        if let Some(loaded) = options.get() {
            for level in loaded.levels {
                choices.push(Choice::new(level.value, level.label));
            }
        }
        choices
    };

    let capabilities = [
        ("read", "agents.read"),
        ("write", "agents.write"),
        ("shell", "agents.shell"),
        ("network", "agents.network"),
    ];

    view! {
        <div class="stack">
            <div class="view-head">
                <button class="btn-icon" title=move || t.t("action.cancel") on:click=move |_| editing.set(false)>
                    {icon("back", "icon icon-xs")}
                </button>
                <span class="view-title truncate">
                    {move || if draft.get().persisted { t.t("agents.edit") } else { t.t("panel.newAgent") }}
                </span>
            </div>

            <div class="field-group">
                <div class="field">
                    <label class="field-label">{t.t("agents.name")}</label>
                    {text_input(
                        move || draft.get().name,
                        t.t("panel.newAgent"),
                        move |value| draft.update(|draft| draft.name = value),
                    )}
                </div>
                <div class="field">
                    <label class="field-label">{t.t("agents.description")}</label>
                    {text_input(
                        move || draft.get().description,
                        String::new(),
                        move |value| draft.update(|draft| draft.description = value),
                    )}
                </div>
                <div class="field">
                    <label class="field-label">{t.t("cliInfo.runtime")}</label>
                    {select_input(
                        move || runtime_id.get(),
                        runtime_choices,
                        move |value| {
                            runtime_id.set(value.clone());
                            draft.update(|draft| draft.runtime_id = value);
                        },
                    )}
                </div>
                <div class="field">
                    <label class="field-label">{t.t("agents.model")}</label>
                    <div class="select-row">{move || select_input(model_value, model_choices(), move |value| {
                        draft.update(|draft| {
                            draft.model = if value == DEFAULT_MODEL { None } else { Some(value) };
                        });
                    })}
                    </div>
                    <p class="field-hint wrap">
                        {move || match options.get() {
                            Some(loaded) if !loaded.models.is_empty() => t.t("agents.modelHint"),
                            Some(_) => t.t("agents.noModelList"),
                            None => t.t("agents.readingRuntime"),
                        }}
                    </p>
                </div>
                <div class="field">
                    <label class="field-label">{t.t("agents.reasoning")}</label>
                    <div class="select-row">{move || select_input(reasoning_value, reasoning_choices(), move |value| {
                        draft.update(|draft| {
                            draft.reasoning = if value == DEFAULT_MODEL { None } else { Some(value) };
                        });
                    })}
                    </div>
                    {move || {
                        options
                            .get()
                            .filter(|loaded| loaded.levels.is_empty())
                            .map(|_| view! { <p class="field-hint wrap">{t.t("agents.noReasoningLevels")}</p> })
                    }}
                </div>
                <div class="field">
                    <label class="field-label">{t.t("panel.instructions")}</label>
                    {textarea(
                        move || draft.get().instructions,
                        3,
                        move |value| draft.update(|draft| draft.instructions = value),
                    )}
                </div>
                <div class="field">
                    <label class="field-label">{t.t("agents.permissions")}</label>
                    <div class="switch-rows">
                        {capabilities
                            .into_iter()
                            .map(|(key, label)| {
                                view! {
                                    <div class="switch-row">
                                        <span class="switch-label truncate">{t.t(label)}</span>
                                        {switch(
                                            move || capability(draft.get().capabilities, key),
                                            move |next| {
                                                draft.update(|draft| {
                                                    draft.capabilities = with_capability(draft.capabilities, key, next)
                                                })
                                            },
                                        )}
                                    </div>
                                }
                            })
                            .collect_view()}
                    </div>
                </div>
                <div class="switch-row">
                    <span class="switch-label truncate">{t.t("agents.enabled")}</span>
                    {switch(
                        move || draft.get().enabled,
                        move |next| draft.update(|draft| draft.enabled = next),
                    )}
                </div>
            </div>

            <div class="row-actions">
                <button
                    class="btn btn-primary btn-sm grow"
                    disabled=move || {
                        let current = draft.get();
                        current.name.trim().is_empty() || current.runtime_id.is_empty()
                    }
                    on:click=move |_| {
                        let profile = draft.get_untracked().to_profile();
                        store.save_profile(profile);
                        editing.set(false);
                    }
                >
                    {t.t("action.save")}
                </button>
                <button class="btn btn-outline btn-sm" on:click=move |_| editing.set(false)>
                    {t.t("action.cancel")}
                </button>
                {move || {
                    draft
                        .get()
                        .persisted
                        .then(|| {
                            view! {
                                <button
                                    class="btn-icon btn-icon-danger"
                                    title=move || t.t("panel.delete")
                                    on:click=move |_| delete_open.set(true)
                                >
                                    {icon("trash", "icon icon-xs")}
                                </button>
                            }
                        })
                }}
            </div>

            {move || {
                let profile_id = draft.get_untracked().id;
                Some(
                    confirm_dialog(
                        delete_open,
                        t.t("panel.agent.deleteTitle"),
                        t.t("panel.deleteConfirm"),
                        t.t("action.cancel"),
                        t.t("panel.delete"),
                        Callback::new(move |_| {
                            store.delete_profile(profile_id.clone());
                            editing.set(false);
                        }),
                    ),
                )
            }}
        </div>
    }
}

fn capability(capabilities: CapabilitySet, key: &str) -> bool {
    match key {
        "read" => capabilities.read_workspace,
        "write" => capabilities.write_workspace,
        "shell" => capabilities.execute_commands,
        _ => capabilities.network_access,
    }
}

fn with_capability(mut capabilities: CapabilitySet, key: &str, value: bool) -> CapabilitySet {
    match key {
        "read" => capabilities.read_workspace = value,
        "write" => capabilities.write_workspace = value,
        "shell" => capabilities.execute_commands = value,
        _ => capabilities.network_access = value,
    }
    capabilities
}
