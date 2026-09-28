//! Check the latest GitHub release and let the user choose whether to open it.
//!
//! Relay never downloads or installs an update itself. The release page is the
//! source of truth, and the user downloads and installs the package manually.

use std::sync::Mutex;
use std::time::Duration;

use semver::Version;
use serde::Deserialize;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_opener::OpenerExt;

use crate::i18n::{self, key, Translator};

const LATEST_RELEASE_API: &str =
    "https://api.github.com/repos/Ant1mage/codex-agent-relay/releases/latest";
const LATEST_RELEASE_PAGE: &str = "https://github.com/Ant1mage/codex-agent-relay/releases/latest";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    Available,
    None,
    Error,
}

#[derive(Debug, Clone, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateState {
    pub status: UpdateStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Default)]
pub struct UpdateStore {
    state: Mutex<UpdateState>,
}

impl UpdateStore {
    pub fn state(&self) -> UpdateState {
        self.state
            .lock()
            .map(|state| state.clone())
            .unwrap_or_default()
    }

    fn publish(&self, state: UpdateState) -> UpdateState {
        if let Ok(mut current) = self.state.lock() {
            *current = state.clone();
        }
        state
    }
}

#[derive(Debug, Deserialize)]
struct LatestRelease {
    tag_name: String,
}

async fn latest_release_tag() -> Result<String, reqwest::Error> {
    // Only the public GitHub request follows environment/macOS proxies. Relay's
    // daemon and model-discovery clients explicitly use no_proxy().
    let client = reqwest::Client::builder()
        .user_agent(concat!(
            env!("CARGO_PKG_NAME"),
            "/",
            env!("CARGO_PKG_VERSION")
        ))
        .timeout(REQUEST_TIMEOUT)
        .build()?;
    Ok(client
        .get(LATEST_RELEASE_API)
        .send()
        .await?
        .error_for_status()?
        .json::<LatestRelease>()
        .await?
        .tag_name)
}

/// Compare the app's semantic version with a GitHub release tag such as `v0.2.0`.
fn is_newer_release(current: &str, tag: &str) -> Result<bool, String> {
    let current = Version::parse(current).map_err(|error| format!("当前版本号无效: {error}"))?;
    let tag = tag.strip_prefix('v').unwrap_or(tag);
    let latest =
        Version::parse(tag).map_err(|error| format!("GitHub release tag 无效: {error}"))?;
    Ok(latest > current)
}

fn update_prompt<R: Runtime>(app: &AppHandle<R>, version: &str) {
    let t = Translator::new(i18n::system_locale());
    let title = t.t(key::UPDATE_DIALOG_TITLE);
    let message = t.tv(key::UPDATE_DIALOG_MESSAGE, &[("version", version)]);
    let open = t.t(key::UPDATE_DIALOG_OPEN);
    let close = t.t(key::UPDATE_DIALOG_CLOSE);
    let handle = app.clone();

    app.dialog()
        .message(message)
        .title(title)
        .buttons(MessageDialogButtons::OkCancelCustom(open, close))
        .show(move |should_open| {
            if should_open {
                if let Err(error) = handle.opener().open_url(LATEST_RELEASE_PAGE, None::<&str>) {
                    eprintln!("[relay] failed to open the GitHub release page: {error}");
                }
            }
        });
}

/// Check once against GitHub's latest published (non-prerelease) release.
pub async fn check<R: Runtime>(
    app: &AppHandle<R>,
    on_change: &(dyn Fn() + Send + Sync),
) -> UpdateState {
    let store = app.state::<UpdateStore>();
    store.publish(UpdateState {
        status: UpdateStatus::Checking,
        ..UpdateState::default()
    });
    on_change();

    let result = async {
        let tag = latest_release_tag()
            .await
            .map_err(|error| error.to_string())?;
        let current = app.package_info().version.to_string();
        let newer = is_newer_release(&current, &tag)?;
        Ok::<_, String>((tag, newer))
    }
    .await;

    let state = match result {
        Ok((version, true)) => {
            let state = store.publish(UpdateState {
                status: UpdateStatus::Available,
                version: Some(version.clone()),
                ..UpdateState::default()
            });
            update_prompt(app, &version);
            state
        }
        Ok(_) => store.publish(UpdateState {
            status: UpdateStatus::None,
            ..UpdateState::default()
        }),
        Err(message) => store.publish(UpdateState {
            status: UpdateStatus::Error,
            message: Some(message.chars().take(200).collect()),
            ..UpdateState::default()
        }),
    };
    on_change();
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires access to GitHub using the machine's current network/proxy settings"]
    async fn github_release_check_uses_current_network_settings() {
        let tag = latest_release_tag()
            .await
            .expect("GitHub release check failed");
        assert!(Version::parse(tag.trim_start_matches('v')).is_ok());
        println!("GitHub latest release: {tag}");
    }

    #[test]
    fn release_tags_are_compared_as_semantic_versions() {
        assert!(is_newer_release("0.1.0", "v0.2.0").unwrap());
        assert!(is_newer_release("0.1.0", "0.1.1").unwrap());
        assert!(!is_newer_release("0.1.0", "v0.1.0").unwrap());
        assert!(!is_newer_release("0.2.0", "v0.1.9").unwrap());
    }

    #[test]
    fn invalid_versions_are_reported_instead_of_guessed() {
        assert!(is_newer_release("development", "v0.2.0").is_err());
        assert!(is_newer_release("0.1.0", "not-a-version").is_err());
    }
}
