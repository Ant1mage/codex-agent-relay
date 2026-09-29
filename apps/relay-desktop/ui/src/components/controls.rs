//! Form controls.
//!
//! Thin Rust wrappers around Thaw, preserving the store callbacks and native
//! select option binding while sharing theme and keyboard behavior.

use leptos::prelude::*;
use leptos::reactive::wrappers::write::SignalSetter;
use thaw::{
    Badge, BadgeAppearance, BadgeColor, BadgeSize, Dialog, DialogSurface, Input, Select,
    SpinButton, Switch, Textarea,
};

/// A `<select>` entry. `label` is what the user reads, `value` what the API gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub value: String,
    pub label: String,
}

impl Choice {
    pub fn new(value: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
        }
    }

    /// Both value and label, for ids that are also the readable text.
    pub fn same(value: impl Into<String>) -> Self {
        let value = value.into();
        Self {
            label: value.clone(),
            value,
        }
    }
}

pub fn badge(label: impl Into<String>, tone: &'static str) -> impl IntoView {
    let label = label.into();
    let color = match tone {
        "success" => BadgeColor::Success,
        "destructive" => BadgeColor::Danger,
        "warning" => BadgeColor::Warning,
        _ => BadgeColor::Subtle,
    };
    view! { <Badge class=format!("badge badge-{tone}") color=color appearance=BadgeAppearance::Outline size=BadgeSize::Small>{label}</Badge> }
}

pub fn switch(
    checked: impl Fn() -> bool + Send + Sync + 'static,
    on_change: impl Fn(bool) + Send + Sync + 'static,
) -> impl IntoView {
    let read = Signal::derive(checked);
    let write = SignalSetter::map(on_change);
    view! { <Switch checked=(read, write) class="relay-switch" /> }
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
    let current = untrack(value);
    let initial = if current.is_empty() {
        choices
            .first()
            .map(|choice| choice.value.clone())
            .unwrap_or_default()
    } else {
        current
    };
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
    let read = Signal::derive(value);
    // Select initialization must not refetch options or rebuild its own branch.
    let write = SignalSetter::map(move |next| {
        if read.get_untracked() != next {
            on_change(next);
        }
    });
    view! { <Select class="relay-input" value=(read, write) default_value=initial>{options}</Select> }
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
    let read = Signal::derive(value);
    let write = SignalSetter::map(on_input);
    view! { <Input class=format!("relay-input {class}") value=(read, write) placeholder=placeholder /> }
}

pub fn textarea(
    value: impl Fn() -> String + Send + Sync + 'static,
    rows: u32,
    on_input: impl Fn(String) + Send + Sync + 'static,
) -> impl IntoView {
    let read = Signal::derive(value);
    let write = SignalSetter::map(on_input);
    view! { <Textarea class="relay-input relay-textarea" value={(read, write)} {..} style=format!("min-height: {}em", rows as f64 * 1.5 + 1.5) /> }
}

/// A bounded integer field that commits on blur or Enter and snaps back to the
/// stored value when the draft is not a valid number in range — the same
/// behaviour as the panel's `NumberField`.
pub fn number_field(
    t: crate::i18n::Translator,
    value: impl Fn() -> u32 + Copy + Send + Sync + 'static,
    min: u32,
    max: u32,
    on_commit: impl Fn(u32) + Send + Sync + 'static,
) -> impl IntoView {
    let read = Signal::derive(value);
    let write = SignalSetter::map(on_commit);
    let root = NodeRef::<leptos::html::Span>::new();
    // Thaw currently exposes no props for its two button labels.
    Effect::new(move |_| {
        let increment = t.t("action.increment");
        let decrement = t.t("action.decrement");
        if let Some(root) = root.get() {
            if let Ok(buttons) = root.query_selector_all("button") {
                use wasm_bindgen::JsCast;
                for (index, label) in [increment, decrement].iter().enumerate() {
                    if let Some(button) = buttons
                        .item(index as u32)
                        .and_then(|node| node.dyn_into::<web_sys::Element>().ok())
                    {
                        let _ = button.set_attribute("aria-label", label);
                    }
                }
            }
        }
    });
    view! { <span node_ref=root class="number-field"><SpinButton<u32> class="relay-input" value=(read, write) min=min max=max step_page=1u32 /></span> }
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
    let cancel_ref = NodeRef::<leptos::html::Button>::new();
    let confirm_ref = NodeRef::<leptos::html::Button>::new();
    let previous_focus = StoredValue::new_local(None::<web_sys::HtmlElement>);
    Effect::new(move |_| {
        let shown = open.get();
        let root = crate::dom::document()
            .document_element()
            .expect("document root");
        let _ = root.class_list().toggle_with_force("modal-open", shown);
        if shown {
            use wasm_bindgen::JsCast;
            previous_focus.set_value(
                crate::dom::document()
                    .active_element()
                    .and_then(|el| el.dyn_into().ok()),
            );
            leptos::task::spawn_local(async move {
                gloo_timers::future::TimeoutFuture::new(30).await;
                if open.try_get_untracked() == Some(true) {
                    if let Some(button) = cancel_ref.get_untracked() {
                        let _ = button.focus();
                    }
                }
            });
        } else if let Some(element) = previous_focus.get_value() {
            let _ = element.focus();
            previous_focus.set_value(None);
        }
    });
    on_cleanup(move || {
        if open.try_get_untracked() == Some(true) {
            if let Some(root) = crate::dom::document().document_element() {
                let _ = root.class_list().remove_1("modal-open");
            }
            if let Some(element) = previous_focus.get_value() {
                let _ = element.focus();
            }
        }
    });
    let accessible_title = title.clone();
    view! {
        <Dialog open=open mask_closeable=false>
            <DialogSurface class="relay-dialog" {..} aria-label=accessible_title
                on:keydown=move |event: web_sys::KeyboardEvent| {
                    if event.key() == "Tab" {
                        event.prevent_default();
                        let active = crate::dom::document().active_element();
                        let cancel = cancel_ref.get_untracked();
                        if active.as_ref().zip(cancel.as_ref()).is_some_and(|(active, cancel)| active == cancel.as_ref()) {
                            if let Some(button) = confirm_ref.get_untracked() { let _ = button.focus(); }
                        } else if let Some(button) = cancel { let _ = button.focus(); }
                    }
                }
            >
                <h3 class="dialog-title">{title}</h3>
                <p class="dialog-body">{description}</p>
                <div class="dialog-actions">
                    <button node_ref=cancel_ref class="btn btn-outline btn-sm" on:click=move |_| open.set(false)>{cancel_label}</button>
                    <button node_ref=confirm_ref class="btn btn-danger btn-sm" on:click=move |_| { open.set(false); on_confirm.run(()); }>{confirm_label}</button>
                </div>
            </DialogSurface>
        </Dialog>
    }
}

pub fn relay_logo(class: &'static str) -> impl IntoView {
    view! {
        <svg
            class=class
            viewBox="0 0 512 512"
            fill="none"
            aria-hidden="true"
        >
            <g stroke="currentColor" stroke-width="42" stroke-linecap="round" stroke-linejoin="round">
                <path d="M92 256 H196" />
                <path d="M196 256 C250 256 260 126 336 126 H410" />
                <path d="M196 256 H410" />
                <path d="M196 256 C250 256 260 386 336 386 H410" />
            </g>
            <g fill="currentColor">
                <circle cx="76" cy="256" r="28" />
                <circle cx="436" cy="126" r="28" />
                <circle cx="436" cy="256" r="28" />
                <circle cx="436" cy="386" r="28" />
            </g>
        </svg>
    }
}

/// Icons are typed library values; misspellings fail at compile time.
pub fn icon(icon: icondata::Icon, class: &'static str) -> impl IntoView {
    view! { <leptos_icons::Icon icon={icon} {..} class=class aria-hidden="true" /> }
}
