//! The daemon Relay's MCP server talks to.
//!
//! The MCP process is a protocol front-end only: it owns no worker, no adapter
//! and no RunController. Every tool call becomes one loopback HTTP request to the
//! daemon that does own them (see docs/architecture.md). If the daemon is not
//! running, Relay says so; it never starts a second execution runtime.

use std::time::Duration;

use relay_api::server_info::{read_server_info, ServerInfo};
use relay_config::server_info_path;
use serde::de::DeserializeOwned;
use serde::Serialize;

/// The one honest answer when Relay's backend is not there.
pub const NOT_RUNNING: &str =
    "Relay daemon is not running. Start Relay (the menu bar app or `relayd`), then try again.";

pub struct DaemonClient {
    base: String,
    token: String,
    http: reqwest::Client,
}

impl DaemonClient {
    /// Reads the daemon's own server record. A missing or unusable record means
    /// the daemon is not running.
    pub fn discover() -> Result<Self, String> {
        let Some(info) = read_server_info(&server_info_path()) else {
            return Err(NOT_RUNNING.to_string());
        };
        Self::from_info(&info)
    }

    pub fn from_info(info: &ServerInfo) -> Result<Self, String> {
        let http = reqwest::Client::builder()
            .no_proxy()
            .connect_timeout(Duration::from_secs(5))
            .build()
            .map_err(|error| error.to_string())?;
        Ok(Self {
            base: info.url.trim_end_matches('/').to_string(),
            token: info.token.clone(),
            http,
        })
    }

    pub async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let response = self
            .http
            .get(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .send()
            .await
            .map_err(reachability)?;
        decode(response).await
    }

    pub async fn post<T: DeserializeOwned, B: Serialize>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, String> {
        let response = self
            .http
            .post(format!("{}{path}", self.base))
            .bearer_auth(&self.token)
            .json(body)
            .send()
            .await
            .map_err(reachability)?;
        decode(response).await
    }
}

/// A connection failure is almost always "the daemon is not running"; say that
/// instead of leaking a socket error to the host.
fn reachability(error: reqwest::Error) -> String {
    if error.is_connect() || error.is_request() {
        format!("{NOT_RUNNING} ({error})")
    } else {
        error.to_string()
    }
}

async fn decode<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, String> {
    let status = response.status();
    let body = response.text().await.map_err(|error| error.to_string())?;
    if !status.is_success() {
        // The daemon answers its failures as {"error": "..."} so the host sees
        // the real reason instead of a status code.
        if let Ok(api_error) = serde_json::from_str::<relay_api::ApiError>(&body) {
            return Err(api_error.error);
        }
        return Err(format!("Relay daemon answered {status}: {body}"));
    }
    serde_json::from_str::<T>(&body)
        .map_err(|error| format!("Relay daemon answered an unexpected payload: {error}"))
}
