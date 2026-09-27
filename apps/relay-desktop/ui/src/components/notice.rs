//! Notices: the inspector's bottom-centre pill and the panel's footer line.

use leptos::prelude::*;

use crate::components::controls::icon;

/// The pill dismisses itself after 3.5s (see `Store::notify`).
pub fn notice_pill(text: impl Fn() -> Option<String> + Send + Sync + 'static) -> impl IntoView {
    view! {
        {move || {
            text()
                .map(|message| {
                    view! {
                        <div class="notice-pill">
                            {icon("check", "icon icon-sm icon-ok")}
                            <span class="notice-text">{message}</span>
                        </div>
                    }
                })
        }}
    }
}

/// The panel keeps its notice in the footer so the tab body never jumps.
pub fn notice_footer(text: impl Fn() -> Option<String> + Send + Sync + 'static) -> impl IntoView {
    view! {
        {move || {
            text()
                .map(|message| {
                    view! {
                        <footer class="panel-footer">
                            {icon("scroll", "icon icon-xs")}
                            <span class="notice-text">{message}</span>
                        </footer>
                    }
                })
        }}
    }
}
