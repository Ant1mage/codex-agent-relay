//! Routing policy: global defaults plus optional per-workspace overrides.
//!
//! This is the same file relay-mcp resolves before every run, so a change applies
//! to the next delegation without restarting Codex.

use leptos::prelude::*;
use relay_core::{RelayPolicy, RelayPolicyOverride};

use crate::components::controls::{icon, number_field, select_input, switch, Choice};
use crate::state::PanelStore;

pub fn policy_view(store: PanelStore) -> AnyView {
    let t = store.translator();

    let policy = RwSignal::new(
        store
            .config
            .get_untracked()
            .map(|config| config.policy)
            .unwrap_or_default(),
    );
    let overrides = RwSignal::new(
        store
            .config
            .get_untracked()
            .map(|config| config.workspace_overrides)
            .unwrap_or_default(),
    );
    let config_synced = RwSignal::new(store.config.get_untracked().is_some());
    Effect::new(move |_| {
        if let Some(config) = store.config.get() {
            if !config_synced.get_untracked() {
                policy.set(config.policy);
                overrides.set(config.workspace_overrides);
                config_synced.set(true);
            }
        }
    });
    let workspace = RwSignal::new(
        store
            .snapshot
            .get_untracked()
            .and_then(|snapshot| {
                snapshot
                    .sessions
                    .first()
                    .map(|view| view.session.cwd.clone())
            })
            .unwrap_or_default(),
    );

    // Policy is a boundary only where the runtime has one. Saying so is the
    // difference between a security setting and a wish.
    let enforcement_note = move || {
        let runtimes = store.snapshot.with(|snapshot| {
            snapshot
                .as_ref()
                .map(|value| value.runtimes.clone())
                .unwrap_or_default()
        });
        if runtimes.is_empty() {
            return None;
        }
        let enforcing: Vec<String> = runtimes
            .iter()
            .filter(|runtime| runtime.capabilities.enforcement.workspace)
            .map(|runtime| runtime.adapter_id.clone())
            .collect();
        Some(if enforcing.is_empty() {
            view! { <p class="field-hint wrap">{t.t("settings.policyAdvisory")}</p> }.into_any()
        } else {
            let mut names = enforcing;
            names.sort();
            names.dedup();
            view! {
                <p class="field-hint wrap">
                    {t.t("settings.policyEnforced")} {names.join(", ")}
                </p>
            }
            .into_any()
        })
    };

    let workspaces = move || {
        let mut seen: Vec<String> = Vec::new();
        if let Some(snapshot) = store.snapshot.get() {
            for view in snapshot.sessions {
                if !seen.contains(&view.session.cwd) {
                    seen.push(view.session.cwd);
                }
            }
        }
        seen
    };

    let patch_override = move |change: RelayPolicyOverride| {
        let key = workspace.get_untracked();
        if key.is_empty() {
            return;
        }
        overrides.update(|all| {
            let entry = all.entry(key).or_default();
            if change.max_concurrent_runs.is_some() {
                entry.max_concurrent_runs = change.max_concurrent_runs;
            }
            if change.max_concurrent_writers.is_some() {
                entry.max_concurrent_writers = change.max_concurrent_writers;
            }
            if change.require_worktree_for_parallel_writers.is_some() {
                entry.require_worktree_for_parallel_writers =
                    change.require_worktree_for_parallel_writers;
            }
            if change.allow_write.is_some() {
                entry.allow_write = change.allow_write;
            }
            if change.allow_commands.is_some() {
                entry.allow_commands = change.allow_commands;
            }
            if change.allow_network.is_some() {
                entry.allow_network = change.allow_network;
            }
        });
    };

    let switches: [(&'static str, &'static str); 4] = [
        ("allow_write", "settings.allowWrite"),
        ("allow_commands", "settings.allowCommands"),
        ("allow_network", "settings.allowNetwork"),
        ("require_worktree", "settings.requireWorktree"),
    ];

    view! {
        <div class="editor-layout">
            <div class="view-head editor-head">
                <span class="view-title">{t.t("nav.policy")}</span>
            </div>
            <div class="editor-content">
            <div class="editor-grid">
            <div class="editor-card">
                <h2 class="editor-card-title">{t.t("settings.global")}</h2>
                <div class="field-group">
                    <div class="field">
                        <label class="field-label">{t.t("settings.maxRuns")}</label>
                        {number_field(
                            t,
                            move || policy.get().max_concurrent_runs,
                            1,
                            32,
                            move |value| policy.update(|policy| policy.max_concurrent_runs = value),
                        )}
                    </div>
                    <div class="field">
                        <label class="field-label">{t.t("settings.maxWriters")}</label>
                        {number_field(
                            t,
                            move || policy.get().max_concurrent_writers,
                            1,
                            16,
                            move |value| policy.update(|policy| policy.max_concurrent_writers = value),
                        )}
                    </div>
                    {switches
                        .into_iter()
                        .map(|(key, label)| {
                            view! {
                                <div class="switch-row">
                                    <span class="switch-label truncate">{t.t(label)}</span>
                                    {switch(
                                        move || policy_flag(policy.get(), key),
                                        move |value| {
                                            policy.update(|policy| set_policy_flag(policy, key, value))
                                        },
                                    )}
                                </div>
                            }
                        })
                        .collect_view()}
                    {enforcement_note}
                </div>
            </div>

            <div class="editor-card">
                <h2 class="editor-card-title">{t.t("settings.workspaceSection")}</h2>
                <div class="field-group">
                    {move || select_input(
                        move || workspace.get(),
                        workspaces().into_iter().map(Choice::same).collect(),
                        move |value| workspace.set(value),
                    )}

                    {move || {
                        let current = workspace.get();
                        if current.is_empty() {
                            return view! { <p class="field-hint wrap">{t.t("panel.workspace.none")}</p> }.into_any();
                        }
                        let override_now = overrides.get().get(&current).copied().unwrap_or_default();
                        let global = policy.get();
                        view! {
                            <div class="field-group">
                                {switches
                                    .into_iter()
                                    .map(|(key, label)| {
                                        view! {
                                            <div class="switch-row">
                                                <span class="switch-label truncate">{t.t(label)}</span>
                                                {switch(
                                                    move || override_flag(override_now, global, key),
                                                    move |value| patch_override(override_flag_patch(key, value)),
                                                )}
                                            </div>
                                        }
                                    })
                                    .collect_view()}
                                <div class="row-actions">
                                    <button
                                        class="btn btn-outline btn-sm"
                                        on:click=move |_| {
                                            let current = workspace.get_untracked();
                                            overrides.update(|all| {
                                                all.remove(&current);
                                            });
                                        }
                                    >
                                        {icon(icondata::LuX, "icon icon-xs")}
                                        <span>{t.t("panel.workspace.clear")}</span>
                                    </button>
                                </div>
                            </div>
                        }
                            .into_any()
                    }}
                </div>
            </div>
            </div>
            </div>
            <div class="row-actions editor-actions">
            <button
                class="btn btn-primary btn-sm"
                disabled=move || store.busy.get() || store.config.get().is_none()
                on:click=move |_| {
                    store.save_policy(policy.get_untracked(), overrides.get_untracked());
                }
            >
                {t.t("action.save")}
            </button>
            </div>
        </div>
    }
    .into_any()
}

fn policy_flag(policy: RelayPolicy, key: &str) -> bool {
    match key {
        "allow_write" => policy.allow_write,
        "allow_commands" => policy.allow_commands,
        "allow_network" => policy.allow_network,
        _ => policy.require_worktree_for_parallel_writers,
    }
}

fn set_policy_flag(policy: &mut RelayPolicy, key: &str, value: bool) {
    match key {
        "allow_write" => policy.allow_write = value,
        "allow_commands" => policy.allow_commands = value,
        "allow_network" => policy.allow_network = value,
        _ => policy.require_worktree_for_parallel_writers = value,
    }
}

/// The override shows the effective value until it is touched, exactly like the
/// panel's `override[key] ?? policy[key]`.
fn override_flag(overrides: RelayPolicyOverride, policy: RelayPolicy, key: &str) -> bool {
    let explicit = match key {
        "allow_write" => overrides.allow_write,
        "allow_commands" => overrides.allow_commands,
        "allow_network" => overrides.allow_network,
        _ => overrides.require_worktree_for_parallel_writers,
    };
    explicit.unwrap_or_else(|| policy_flag(policy, key))
}

/// Sparse patch: only the touched field is written, so the rest keeps following
/// the global policy.
fn override_flag_patch(key: &str, value: bool) -> RelayPolicyOverride {
    match key {
        "allow_write" => RelayPolicyOverride {
            allow_write: Some(value),
            ..Default::default()
        },
        "allow_commands" => RelayPolicyOverride {
            allow_commands: Some(value),
            ..Default::default()
        },
        "allow_network" => RelayPolicyOverride {
            allow_network: Some(value),
            ..Default::default()
        },
        _ => RelayPolicyOverride {
            require_worktree_for_parallel_writers: Some(value),
            ..Default::default()
        },
    }
}
