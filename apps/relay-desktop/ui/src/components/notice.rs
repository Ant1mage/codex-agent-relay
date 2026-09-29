//! Notices: the inspector's bottom-centre pill and the panel's footer line.

use leptos::prelude::*;

use crate::components::controls::icon;

/// The pill dismisses itself after 3.5s (see `Store::notify` and `PanelStore::notify`),
/// or immediately upon click.
pub fn notice_pill(text: impl Fn() -> Option<String> + Send + Sync + 'static) -> impl IntoView {
    notice_pill_closable(text, || {})
}

/// A notice pill with an optional on_dismiss callback when clicked.
pub fn notice_pill_closable(
    text: impl Fn() -> Option<String> + Send + Sync + 'static,
    on_dismiss: impl Fn() + Send + Sync + 'static,
) -> impl IntoView {
    let on_dismiss = std::sync::Arc::new(on_dismiss);
    let t = expect_context::<crate::state::Store>().translator();
    view! {
        {move || {
            text()
                .map(|message| {
                    let on_dismiss = on_dismiss.clone();
                    let is_error = is_error_notice(&message);
                    let pill_class = if is_error {
                        "notice-pill notice-pill-danger"
                    } else {
                        "notice-pill"
                    };
                    let icon_view = if is_error {
                        icon(icondata::LuCircleAlert, "icon icon-sm tone-danger")
                    } else {
                        icon(icondata::LuCheck, "icon icon-sm icon-ok")
                    };
                    view! {
                        <button
                            type="button"
                            class=pill_class
                            role="status"
                            title=move || t.t("action.dismiss")
                            on:click=move |_| on_dismiss()
                        >
                            {icon_view}
                            <span class="notice-text">{message}</span>
                        </button>
                    }
                })
        }}
    }
}

fn is_error_notice(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    lower.contains("failed")
        || lower.contains("error")
        || lower.contains("cannot")
        || lower.contains("could not")
        || lower.contains("失败")
        || lower.contains("错误")
        || lower.contains("异常")
}

/// The panel keeps its notice in the footer so the tab body never jumps.
pub fn notice_footer(text: impl Fn() -> Option<String> + Send + Sync + 'static) -> impl IntoView {
    view! {
        {move || {
            text()
                .map(|message| {
                    view! {
                        <footer class="panel-footer">
                            {icon(icondata::LuScrollText, "icon icon-xs")}
                            <span class="notice-text">{message}</span>
                        </footer>
                    }
                })
        }}
    }
}
