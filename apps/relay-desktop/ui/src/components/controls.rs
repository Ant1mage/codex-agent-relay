//! Form controls.
//!
//! Hand-written rather than pulled from a component library: the panel is a
//! 420×640 popover, so every control here is single-column and sized by its
//! container, never by its content.

use leptos::prelude::*;

/// A `<select>` entry. `label` is what the user reads, `value` what the API gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

impl Choice {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self { value: value.into(), label: label.into() }
    }

    /// Both value and label, for ids that are also the readable text.
    pub fn same(value: impl Into<String>) -> Self {
        let value = value.into();
        Self { label: value.clone(), value }
    }
}

pub fn badge(label: impl Into<String>, tone: &'static str) -> impl IntoView {
    let label = label.into();
    view! { <span class=format!("badge badge-{tone}")>{label}</span> }
}

/// The panel's switch: a checkbox with a styled track, so it stays a real form
/// control (keyboard, screen readers) while looking like the desktop toggle.
pub fn switch(
    checked: impl Fn() -> bool + Send + Sync + 'static,
    on_change: impl Fn(bool) + Send + Sync + 'static,
) -> impl IntoView {
    view! {
        <label class="switch">
            <input
                type="checkbox"
                prop:checked=move || checked()
                on:change=move |event| on_change(event_target_checked(&event))
            />
            <span class="switch-track" aria-hidden="true"></span>
        </label>
    }
}

/// A controlled `<select>`.
///
/// Each option carries its own `selected` binding instead of relying on the
/// select's `value` property alone: options can be appended after the element
/// exists (the model list arrives from the daemon), and a `value` written before
/// the matching option exists is silently dropped by the browser.
pub fn select_input(
    value: impl Fn() -> String + Copy + Send + Sync + 'static,
    choices: Vec<Choice>,
    on_change: impl Fn(String) + Send + Sync + 'static,
) -> impl IntoView {
    let options = choices
        .into_iter()
        .map(|choice| {
            let option_value = choice.value.clone();
            view! {
                <option value=choice.value.clone() prop:selected=move || value() == option_value>
                    {choice.label.clone()}
                </option>
            }
        })
        .collect_view();
    view! {
        <select class="input" prop:value=move || value() on:change=move |event| on_change(event_target_value(&event))>
            {options}
        </select>
    }
}

pub fn text_input(
    value: impl Fn() -> String + Send + Sync + 'static,
    placeholder: String,
    on_input: impl Fn(String) + Send + Sync + 'static,
) -> impl IntoView {
    text_input_with_class(value, placeholder, "", on_input)
}

pub fn text_input_with_class(
    value: impl Fn() -> String + Send + Sync + 'static,
    placeholder: String,
    class: &'static str,
    on_input: impl Fn(String) + Send + Sync + 'static,
) -> impl IntoView {
    view! {
        <input
            class=format!("input {class}")
            type="text"
            placeholder=placeholder
            prop:value=move || value()
            on:input=move |event| on_input(event_target_value(&event))
        />
    }
}

pub fn textarea(
    value: impl Fn() -> String + Send + Sync + 'static,
    rows: u32,
    on_input: impl Fn(String) + Send + Sync + 'static,
) -> impl IntoView {
    view! {
        <textarea
            class="input textarea"
            rows=rows.to_string()
            prop:value=move || value()
            on:input=move |event| on_input(event_target_value(&event))
        ></textarea>
    }
}

/// A bounded integer field that commits on blur or Enter and snaps back to the
/// stored value when the draft is not a valid number in range — the same
/// behaviour as the panel's `NumberField`.
pub fn number_field(
    value: impl Fn() -> u32 + Copy + Send + Sync + 'static,
    min: u32,
    max: u32,
    on_commit: impl Fn(u32) + Send + Sync + 'static,
) -> impl IntoView {
    let draft = RwSignal::new(value().to_string());
    let commit = move || {
        let parsed = draft.get_untracked().trim().parse::<f64>().ok();
        let valid = parsed.filter(|next| next.fract() == 0.0 && *next >= min as f64 && *next <= max as f64);
        match valid {
            Some(next) => on_commit(next as u32),
            None => draft.set(value().to_string()),
        }
    };
    // Follows the store when configuration is reloaded.
    Effect::new(move |_| draft.set(value().to_string()));
    view! {
        <input
            class="input"
            type="number"
            min=min.to_string()
            max=max.to_string()
            prop:value=move || draft.get()
            on:input=move |event| draft.set(event_target_value(&event))
            on:blur=move |_| commit()
            on:keydown=move |event| {
                if event.key() == "Enter" {
                    event_target::<web_sys::HtmlInputElement>(&event).blur().ok();
                }
            }
        />
    }
}

/// A modal confirmation. The markup is always mounted and revealed with a class
/// so the confirm handler can stay a plain closure.
pub fn confirm_dialog(
    open: RwSignal<bool>,
    title: String,
    description: String,
    cancel_label: String,
    confirm_label: String,
    on_confirm: Callback<()>,
) -> impl IntoView {
    view! {
        <div class="dialog-backdrop" class:open=move || open.get()>
            <div class="dialog" role="dialog" aria-modal="true">
                <h3 class="dialog-title">{title}</h3>
                <p class="dialog-body">{description}</p>
                <div class="dialog-actions">
                    <button class="btn btn-outline btn-sm" on:click=move |_| open.set(false)>
                        {cancel_label}
                    </button>
                    <button
                        class="btn btn-danger btn-sm"
                        on:click=move |_| {
                            open.set(false);
                            on_confirm.run(());
                        }
                    >
                        {confirm_label}
                    </button>
                </div>
            </div>
        </div>
    }
}

/// Lucide-shaped icons, inline: no icon font, no dependency, no network.
pub fn icon(name: &'static str, class: &'static str) -> impl IntoView {
    view! {
        <svg
            class=class
            viewBox="0 0 24 24"
            fill="none"
            stroke="currentColor"
            stroke-width="2"
            stroke-linecap="round"
            stroke-linejoin="round"
            aria-hidden="true"
            inner_html=paths(name).to_string()
        ></svg>
    }
}

fn paths(name: &'static str) -> &'static str {
    match name {
        "copy" => {
            r#"<rect width="14" height="14" x="8" y="8" rx="2" ry="2"/><path d="M4 16c-1.1 0-2-.9-2-2V4c0-1.1.9-2 2-2h10c1.1 0 2 .9 2 2"/>"#
        }
        "check" => r#"<path d="M20 6 9 17l-5-5"/>"#,
        "check-circle" => r#"<circle cx="12" cy="12" r="10"/><path d="m9 12 2 2 4-4"/>"#,
        "globe" => {
            r#"<circle cx="12" cy="12" r="10"/><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20"/><path d="M2 12h20"/>"#
        }
        "refresh" => {
            r#"<path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/><path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/>"#
        }
        "stop" => r#"<rect width="18" height="18" x="3" y="3" rx="2"/>"#,
        "close" => r#"<path d="M18 6 6 18"/><path d="m6 6 12 12"/>"#,
        "plus" => r#"<path d="M5 12h14"/><path d="M12 5v14"/>"#,
        "trash" => {
            r#"<path d="M3 6h18"/><path d="M19 6v14c0 1-1 2-2 2H7c-1 0-2-1-2-2V6"/><path d="M8 6V4c0-1 1-2 2-2h4c1 0 2 1 2 2v2"/>"#
        }
        "pencil" => {
            r#"<path d="M21.174 6.812a1 1 0 0 0-3.986-3.987L3.842 16.174a2 2 0 0 0-.5.83l-1.321 4.352a.5.5 0 0 0 .623.622l4.353-1.32a2 2 0 0 0 .83-.497z"/>"#
        }
        "zap" => r#"<polygon points="13 2 3 14 12 14 11 22 21 10 12 10 13 2"/>"#,
        "download" => {
            r#"<path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"/><polyline points="7 10 12 15 17 10"/><line x1="12" x2="12" y1="15" y2="3"/>"#
        }
        "wrench" => {
            r#"<path d="M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z"/>"#
        }
        "external" => {
            r#"<path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/>"#
        }
        "scroll" => {
            r#"<path d="M15 12h-5"/><path d="M15 8h-5"/><path d="M19 17V5a2 2 0 0 0-2-2H4"/><path d="M8 21h12a2 2 0 0 0 2-2v-1a1 1 0 0 0-1-1H11a1 1 0 0 0-1 1v1a2 2 0 1 1-4 0V5a2 2 0 1 0-4 0v2a1 1 0 0 0 1 1h3"/>"#
        }
        "warning" => {
            r#"<path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/>"#
        }
        "alert" => {
            r#"<circle cx="12" cy="12" r="10"/><line x1="12" x2="12" y1="8" y2="12"/><line x1="12" x2="12.01" y1="16" y2="16"/>"#
        }
        "back" => r#"<path d="m12 19-7-7 7-7"/><path d="M19 12H5"/>"#,
        "terminal" => r#"<polyline points="4 17 10 11 4 5"/><line x1="12" x2="20" y1="19" y2="19"/>"#,
        "message-square" => r#"<path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/>"#,
        "bot" => r#"<path d="M12 8V4H8"/><rect width="16" height="12" x="4" y="8" rx="2"/><path d="M2 14h2"/><path d="M20 14h2"/><path d="M15 13v2"/><path d="M9 13v2"/>"#,
        "shield" => r#"<path d="M12 22s8-4 8-10V5l-8-3-8 3v7c0 6 8 10 8 10"/>"#,
        "cpu" => r#"<rect width="16" height="16" x="4" y="4" rx="2"/><rect width="6" height="6" x="9" y="9" rx="1"/><path d="M15 2v2"/><path d="M15 20v2"/><path d="M2 15h2"/><path d="M2 9h2"/><path d="M20 15h2"/><path d="M20 9h2"/><path d="M9 2v2"/><path d="M9 20v2"/>"#,
        "sliders" => r#"<line x1="4" x2="4" y1="21" y2="14"/><line x1="4" x2="4" y1="10" y2="3"/><line x1="12" x2="12" y1="21" y2="12"/><line x1="12" x2="12" y1="8" y2="3"/><line x1="20" x2="20" y1="21" y2="16"/><line x1="20" x2="20" y1="12" y2="3"/><line x1="1" x2="7" y1="14" y2="14"/><line x1="9" x2="15" y1="8" y2="8"/><line x1="17" x2="23" y1="16" y2="16"/>"#,
        "chevron-down" => r#"<path d="m6 9 6 6 6-6"/>"#,
        "chevron-right" => r#"<path d="m9 18 6-6-6-6"/>"#,
        "chevron-up" => r#"<path d="m18 15-6-6-6 6"/>"#,
        "arrow-down" => r#"<line x1="12" x2="12" y1="5" y2="19"/><polyline points="19 12 12 19 5 12"/>"#,
        "play" => r#"<polygon points="5 3 19 12 5 21 5 3"/>"#,
        "file-text" => r#"<path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z"/><path d="M14 2v4a2 2 0 0 0 2 2h4"/><path d="M10 9H8"/><path d="M16 13H8"/><path d="M16 17H8"/>"#,
        "activity" => r#"<path d="M22 12h-4l-3 9L9 3l-3 9H2"/>"#,
        _ => r#"<circle cx="12" cy="12" r="10"/>"#,
    }
}
