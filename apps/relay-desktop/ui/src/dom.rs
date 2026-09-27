//! The browser surface, wrapped once.
//!
//! Everything here is a direct port of what the React bundle did with `window`,
//! `history`, `localStorage` and `matchMedia`, so the bootstrap contract (token in
//! the URL fragment, stripped from the address bar, kept in `relay.token`) is
//! unchanged.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::UrlSearchParams;

use crate::api::Route;

/// `relay.token` in localStorage, exactly the key the inspector always used.
pub const TOKEN_KEY: &str = "relay.token";
/// `relay.locale`: the inspector remembers the language, the panel does not.
pub const LOCALE_KEY: &str = "relay.locale";

pub fn window() -> web_sys::Window {
    web_sys::window().expect("the relay ui only runs inside a window")
}

pub fn document() -> web_sys::Document {
    window().document().expect("the relay ui only runs inside a document")
}

pub fn location() -> web_sys::Location {
    window().location()
}

pub fn href() -> String {
    location().href().unwrap_or_else(|_| "/".to_string())
}

pub fn pathname() -> String {
    location().pathname().unwrap_or_else(|_| "/".to_string())
}

pub fn origin() -> String {
    location().origin().unwrap_or_default()
}

pub fn search() -> String {
    location().search().unwrap_or_default()
}

fn search_params() -> UrlSearchParams {
    UrlSearchParams::new_with_str(&search()).unwrap_or_else(|_| UrlSearchParams::new().expect("constructor"))
}

/// One query parameter, with an empty value treated as absent.
pub fn query(name: &str) -> Option<String> {
    search_params().get(name).filter(|value| !value.is_empty())
}

pub fn replace_url(url: &str) {
    if let Ok(history) = window().history() {
        let _ = history.replace_state_with_url(&JsValue::NULL, "", Some(url));
    }
}

pub fn push_url(url: &str) {
    if let Ok(history) = window().history() {
        let _ = history.push_state_with_url(&JsValue::NULL, "", Some(url));
    }
}

/// The daemon prints inspector URLs with the token in the fragment
/// (`http://127.0.0.1:7352/#t=…`): fragments never reach a server or a referrer.
/// The query form is accepted too because `/panel?t=…` is what the tray opens.
pub fn parse_token(href: &str) -> Option<String> {
    let bytes = href.as_bytes();
    for index in 1..bytes.len().saturating_sub(1) {
        let delimiter = matches!(bytes[index - 1], b'#' | b'&' | b'?');
        if delimiter && bytes[index] == b't' && bytes[index + 1] == b'=' {
            let rest = &href[index + 2..];
            let end = rest.find('&').unwrap_or(rest.len());
            let raw = &rest[..end];
            let decoded = js_sys::decode_uri_component(raw)
                .ok()
                .and_then(|value| value.as_string())
                .unwrap_or_else(|| raw.to_string());
            let token = decoded.trim().to_string();
            return if token.is_empty() { None } else { Some(token) };
        }
    }
    None
}

/// The same URL with `t` removed from both the query and the fragment, so the
/// token does not stay in the address bar.
pub fn without_token(href: &str) -> String {
    let Ok(url) = web_sys::Url::new(href) else {
        return href.to_string();
    };
    url.search_params().delete("t");
    let hash = url.hash();
    let trimmed = hash.strip_prefix('#').unwrap_or(&hash);
    let kept: Vec<&str> = trimmed
        .split('&')
        .filter(|part| !part.is_empty() && !part.starts_with("t="))
        .collect();
    let hash = kept.join("&");
    url.set_hash(if hash.is_empty() { "" } else { hash.as_str() });
    url.href()
}

/// Token for this page: URL first (fresh from the daemon or the tray), then
/// storage. A token that arrived in the URL is stored and taken out of the bar.
pub fn resolve_token() -> Option<String> {
    let current = href();
    if let Some(token) = parse_token(&current) {
        set_stored(TOKEN_KEY, &token);
        replace_url(&without_token(&current));
        return Some(token);
    }
    get_stored(TOKEN_KEY).map(|value| value.trim().to_string()).filter(|value| !value.is_empty())
}

/// Raw `localStorage.getItem`. `gloo-storage`'s typed helpers JSON-encode the
/// value, which would store the token as `\"…\"` and break the `relay.token`
/// contract every Relay surface shares.
pub fn get_stored(key: &str) -> Option<String> {
    window().local_storage().ok().flatten().and_then(|storage| storage.get_item(key).ok().flatten())
}

pub fn set_stored(key: &str, value: &str) {
    if let Ok(Some(storage)) = window().local_storage() {
        let _ = storage.set_item(key, value);
    }
}

/// `parsePath` from the TypeScript router: only `/s/<session>` and
/// `/s/<session>/r/<run>` mean anything, everything else is the newest session.
pub fn parse_path(pathname: &str) -> Route {
    let parts: Vec<&str> = pathname.split('/').filter(|part| !part.is_empty()).collect();
    if parts.first() != Some(&"s") {
        return Route::default();
    }
    let Some(session) = parts.get(1) else {
        return Route::default();
    };
    if parts.get(2) != Some(&"r") || parts.get(3).is_none() {
        return Route { session: Some(decode(session)), run: None };
    }
    Route { session: Some(decode(session)), run: parts.get(3).map(|value| decode(value)) }
}

fn decode(value: &str) -> String {
    js_sys::decode_uri_component(value)
        .ok()
        .and_then(|decoded| decoded.as_string())
        .unwrap_or_else(|| value.to_string())
}

/// `encodeURIComponent`, exactly the function the TypeScript router used.
pub fn encode(value: &str) -> String {
    String::from(js_sys::encode_uri_component(value))
}

/// `/` for "nothing selected", `/s/<id>` and `/s/<id>/r/<run>` otherwise.
pub fn route_path(route: &Route) -> String {
    let Some(session) = route.session.as_ref() else {
        return "/".to_string();
    };
    let base = format!("/s/{}", encode(session));
    match route.run.as_ref() {
        Some(run) => format!("{base}/r/{}", encode(run)),
        None => base,
    }
}

/// True when this page is the control panel rather than the inspector.
pub fn is_panel_path(pathname: &str) -> bool {
    pathname == "/panel" || pathname.starts_with("/panel/")
}

pub fn on_popstate(callback: impl Fn() + 'static) {
    let closure = Closure::<dyn FnMut()>::new(callback);
    let _ = window()
        .add_event_listener_with_callback("popstate", closure.as_ref().unchecked_ref());
    closure.forget();
}

/// `?lang=en` selects English; anything else is the panel's zh-CN default.
pub fn panel_locale() -> crate::i18n::Locale {
    match query("lang").as_deref() {
        Some("en") => crate::i18n::Locale::En,
        _ => crate::i18n::Locale::ZhCn,
    }
}

/// The inspector follows the browser's language, then remembers the choice.
pub fn inspector_locale() -> crate::i18n::Locale {
    if let Some(stored) = get_stored(LOCALE_KEY).and_then(|code| crate::i18n::Locale::from_code(&code)) {
        return stored;
    }
    crate::i18n::resolve_locale(&window().navigator().language().unwrap_or_default())
}

pub fn set_document_lang(locale: crate::i18n::Locale) {
    if let Some(root) = document().document_element() {
        let _ = root.set_attribute("lang", locale.code());
    }
}

fn prefers_dark() -> bool {
    window()
        .match_media("(prefers-color-scheme: dark)")
        .ok()
        .flatten()
        .map(|media| media.matches())
        .unwrap_or(false)
}

/// The inspector has no theme switch: it follows the browser, like any log view.
pub fn apply_theme() {
    let dark = prefers_dark();
    if let Some(root) = document().document_element() {
        let _ = root.class_list().toggle_with_force("dark", dark);
        let _ = root.set_attribute("style", if dark { "color-scheme: dark" } else { "color-scheme: light" });
    }
}

pub fn watch_theme() {
    apply_theme();
    let Ok(Some(media)) = window().match_media("(prefers-color-scheme: dark)") else {
        return;
    };
    let closure = Closure::<dyn FnMut()>::new(apply_theme);
    let _ = media.add_event_listener_with_callback("change", closure.as_ref().unchecked_ref());
    closure.forget();
}

pub fn open_new_tab(url: &str) {
    let _ = window().open_with_url_and_target(url, "_blank");
}

pub fn close_window() {
    window().close().ok();
}

/// `error instanceof Error ? error.message : String(error)`.
pub fn error_message(value: &JsValue) -> String {
    if let Some(error) = value.dyn_ref::<js_sys::Error>() {
        return String::from(error.message());
    }
    value.as_string().unwrap_or_else(|| format!("{value:?}"))
}

/// `navigator.clipboard.writeText`; resolves with a message on failure so the
/// caller can surface it in the notice pill.
pub async fn write_clipboard(text: &str) -> Result<(), String> {
    let promise = window().navigator().clipboard().write_text(text);
    wasm_bindgen_futures::JsFuture::from(promise).await.map(|_| ()).map_err(|error| error_message(&error))
}

/// Milliseconds since the epoch, the `Date.now()` the formatting helpers used.
pub fn now_ms() -> f64 {
    js_sys::Date::now()
}

/// `new Date(value).getTime()`, NaN for an unparseable timestamp.
pub fn parse_ms(timestamp: &str) -> f64 {
    js_sys::Date::parse(timestamp)
}

/// `x.toString(36)`, used to mint the same short ids the panel generated.
pub fn base36(value: u64) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_string();
    }
    let mut digits = Vec::new();
    let mut remaining = value;
    while remaining > 0 {
        digits.push(DIGITS[(remaining % 36) as usize] as char);
        remaining /= 36;
    }
    digits.iter().rev().collect()
}
