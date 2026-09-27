//! Relay App updates, and only App updates.
//!
//! Ported from `apps/menu-bar/src/updater.ts`. The Tauri updater plugin drives
//! download/verify/replace; Relay implements none of it. The Codex integration
//! has its own lifecycle and is never touched here: installing a new Relay does
//! not mean the Codex MCP server, its skill or its hooks were updated, and the
//! integration checks say so on their own.
//!
//! The menu renders this state exactly like the Electron block did, so the
//! statuses stay the same: `unsupported | idle | checking | available |
//! downloading | downloaded | none | error`.

use std::sync::Mutex;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, Runtime, Url};
use tauri_plugin_updater::{Update, UpdaterExt};

use crate::daemon;

/// Endpoint override used to exercise the update path without a release.
pub const UPDATE_FEED_ENV: &str = "RELAY_UPDATE_FEED";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateStatus {
    /// No feed is configured (or this build is unpackaged): there is nothing to
    /// check, and saying so beats reporting an error.
    Unsupported,
    #[default]
    Idle,
    Checking,
    Available,
    Downloading,
    Downloaded,
    None,
    Error,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    pub status: UpdateStatus,
    /// Version offered by the feed, when one is known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Recent progress 0-100 while downloading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub percent: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl UpdateState {
    fn error(message: impl Into<String>) -> Self {
        let message: String = message.into();
        Self {
            status: UpdateStatus::Error,
            message: Some(message.chars().take(200).collect()),
            ..Self::default()
        }
    }
}

/// The update state plus the handles of the update currently on offer.
///
/// Tauri's `Update` is not `Clone`, so the check keeps it for `download` and
/// `install` to use; the bytes of a downloaded update are held until the user
/// asks for the restart, which is what makes "downloaded" a separate menu entry.
#[derive(Default)]
pub struct UpdateStore {
    inner: Mutex<UpdateInner>,
}

#[derive(Default)]
struct UpdateInner {
    state: UpdateState,
    update: Option<Update>,
    bytes: Option<Vec<u8>>,
}

impl UpdateStore {
    pub fn state(&self) -> UpdateState {
        self.inner
            .lock()
            .map(|inner| inner.state.clone())
            .unwrap_or_default()
    }

    fn publish(&self, state: UpdateState) -> UpdateState {
        if let Ok(mut inner) = self.inner.lock() {
            inner.state = state.clone();
        }
        state
    }

    fn remember(&self, update: Update) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.update = Some(update);
            inner.bytes = None;
        }
    }

    fn take_offer(&self) -> Option<(Update, Option<Vec<u8>>)> {
        let inner = self.inner.lock().ok()?;
        inner
            .update
            .as_ref()
            .map(|update| (update.clone(), inner.bytes.clone()))
    }

    fn store_bytes(&self, bytes: Vec<u8>, state: UpdateState) -> UpdateState {
        if let Ok(mut inner) = self.inner.lock() {
            inner.bytes = Some(bytes);
            inner.state = state.clone();
        }
        state
    }
}

/// The feed the updater reads, when it was pointed somewhere explicitly.
pub fn update_feed() -> Option<String> {
    std::env::var(UPDATE_FEED_ENV)
        .ok()
        .filter(|value| !value.is_empty())
}

fn configured_pubkey<R: Runtime>(app: &AppHandle<R>) -> String {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|config| config.get("pubkey"))
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string()
}

/// A packaged, signed build reads its configured endpoint. Development and
/// unpacked builds have no feed, so they report `unsupported` instead of
/// pretending to check — unless `RELAY_UPDATE_FEED` points at one, which is how
/// the update path is tested locally.
pub fn updates_supported<R: Runtime>(app: &AppHandle<R>) -> bool {
    if update_feed().is_some() {
        return true;
    }
    if !daemon::is_packaged() {
        return false;
    }
    // Without a public key the plugin cannot verify a signature, so there is no
    // usable feed. Saying "unsupported" beats reporting a verification error.
    !configured_pubkey(app).is_empty()
}

/// A check or download that cannot finish must not hold the menu in `checking`
/// forever: GitHub is contacted over the network, and the tray polls regardless.
const UPDATE_TIMEOUT: Duration = Duration::from_secs(30);

fn build_updater<R: Runtime>(app: &AppHandle<R>) -> Result<tauri_plugin_updater::Updater, String> {
    let builder = app.updater_builder().timeout(UPDATE_TIMEOUT);
    let builder = match update_feed() {
        Some(feed) => {
            let url = Url::parse(&feed)
                .map_err(|error| format!("{UPDATE_FEED_ENV} is not a URL: {error}"))?;
            builder
                .endpoints(vec![url])
                .map_err(|error| error.to_string())?
        }
        None => builder,
    };
    builder.build().map_err(|error| error.to_string())
}

/// Checks the feed once and remembers whatever it offers.
pub async fn check<R: Runtime>(
    app: &AppHandle<R>,
    on_change: &(dyn Fn() + Send + Sync),
) -> UpdateState {
    let store = app.state::<UpdateStore>();
    if !updates_supported(app) {
        return store.publish(UpdateState {
            status: UpdateStatus::Unsupported,
            ..UpdateState::default()
        });
    }
    store.publish(UpdateState {
        status: UpdateStatus::Checking,
        ..UpdateState::default()
    });
    on_change();
    let updater = match build_updater(app) {
        Ok(updater) => updater,
        Err(message) => {
            let state = store.publish(UpdateState::error(message));
            on_change();
            return state;
        }
    };
    let state = match updater.check().await {
        Ok(Some(update)) => {
            let version = update.version.clone();
            store.remember(update);
            store.publish(UpdateState {
                status: UpdateStatus::Available,
                version: Some(version),
                ..UpdateState::default()
            })
        }
        Ok(None) => store.publish(UpdateState {
            status: UpdateStatus::None,
            ..UpdateState::default()
        }),
        Err(error) => store.publish(UpdateState::error(error.to_string())),
    };
    on_change();
    state
}

/// Downloads the offered update, reporting progress into the menu.
pub async fn download<R: Runtime>(
    app: &AppHandle<R>,
    on_change: &(dyn Fn() + Send + Sync),
) -> UpdateState {
    let store = app.state::<UpdateStore>();
    if !updates_supported(app) {
        return store.state();
    }
    // The menu only offers "Download update" for an update it was told about; a
    // direct call still works by checking first.
    if store.take_offer().is_none() {
        let state = check(app, on_change).await;
        if state.status != UpdateStatus::Available {
            return state;
        }
    }
    let Some((update, _)) = store.take_offer() else {
        return store.state();
    };
    let version = update.version.clone();
    store.publish(UpdateState {
        status: UpdateStatus::Downloading,
        version: Some(version.clone()),
        percent: Some(0),
        message: None,
    });
    on_change();

    let mut last_percent = 0u32;
    let mut downloaded = 0u64;
    let result = update
        .download(
            |chunk_length, content_length| {
                downloaded += chunk_length as u64;
                let percent = match content_length {
                    Some(total) if total > 0 => ((downloaded * 100) / total).min(100) as u32,
                    _ => 0,
                };
                // One menu rebuild per whole percent, not per chunk: the menu is
                // only rebuilt when the model actually changes anyway.
                if percent != last_percent {
                    last_percent = percent;
                    let state = UpdateState {
                        status: UpdateStatus::Downloading,
                        version: Some(version.clone()),
                        percent: Some(percent),
                        message: None,
                    };
                    store.publish(state);
                    on_change();
                }
            },
            || {},
        )
        .await;

    match result {
        Ok(bytes) => {
            let state = UpdateState {
                status: UpdateStatus::Downloaded,
                version: Some(version),
                percent: None,
                message: None,
            };
            store.store_bytes(bytes, state.clone());
            on_change();
            state
        }
        Err(error) => {
            let state = store.publish(UpdateState::error(error.to_string()));
            on_change();
            state
        }
    }
}

/// Installs a downloaded update. On macOS the plugin does not relaunch, so the
/// caller restarts the app once this returns.
pub fn install<R: Runtime>(app: &AppHandle<R>) -> Result<(), String> {
    let store = app.state::<UpdateStore>();
    let Some((update, Some(bytes))) = store.take_offer() else {
        return Err("no downloaded update".to_string());
    };
    update.install(bytes).map_err(|error| error.to_string())
}
