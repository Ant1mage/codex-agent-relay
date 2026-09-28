//! The shell's HTTP client for relayd.
//!
//! The tray is a client: it reads `~/.relay/server.json`, asks the daemon for the
//! projection, and issues the few actions the menu offers. It never touches the
//! database and contains no Relay business logic. Ported from
//! `packages/relay-api/src/client.ts`, including the request shape: the run token
//! rides both the query string and the bearer header, and path segments are
//! percent-encoded.

use std::time::Duration;

use relay_api::{CancelResult, Health, InstallResult, MenuView, RefreshResult};
use serde::de::DeserializeOwned;

/// The daemon did not answer, or answered with a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiError {
    pub message: String,
    pub status: Option<u16>,
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ApiError {}

impl ApiError {
    pub fn transport(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: None,
        }
    }

    pub fn status(status: u16, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            status: Some(status),
        }
    }
}

/// Talks to one daemon generation. The token rotates on every restart, so a client
/// is built from a `ServerInfo` and never cached across probes.
#[derive(Clone)]
pub struct RelayClient {
    base_url: String,
    token: String,
    http: reqwest::Client,
}

impl RelayClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        Self::with_timeout(base_url, token, Duration::from_secs(3))
    }

    pub fn with_timeout(
        base_url: impl Into<String>,
        token: impl Into<String>,
        timeout: Duration,
    ) -> Self {
        let http = reqwest::Client::builder()
            // server.json always points to relayd on loopback. macOS system
            // proxies can intercept reqwest's default client even for 127.0.0.1.
            .no_proxy()
            .timeout(timeout)
            .build()
            .expect("Relay's direct loopback HTTP client must be constructible");
        Self {
            base_url: normalize_base_url(&base_url.into()),
            token: token.into(),
            http,
        }
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Absolute URL for an API path, carrying the token as a query parameter.
    pub fn url(&self, path: &str) -> String {
        format!(
            "{}{}?token={}",
            self.base_url,
            path,
            encode_segment(&self.token)
        )
    }

    pub async fn health(&self) -> Result<Health, ApiError> {
        self.get_json("/api/health").await
    }

    pub async fn menu(&self) -> Result<MenuView, ApiError> {
        self.get_json("/api/menu").await
    }

    /// Re-detects runtimes and re-reads configuration.
    pub async fn refresh(&self) -> Result<RefreshResult, ApiError> {
        self.post_json("/api/refresh").await
    }

    /// Codex integration lifecycle. The tray only ever asks for `repair`, which
    /// installs what is missing and leaves what already works.
    pub async fn codex_repair(&self) -> Result<InstallResult, ApiError> {
        self.post_json("/api/codex/repair").await
    }

    pub async fn cancel_worker(&self, worker_session_id: &str) -> Result<CancelResult, ApiError> {
        self.post_json(&format!(
            "/api/workers/{}/cancel",
            encode_segment(worker_session_id)
        ))
        .await
    }

    pub async fn cancel_session(&self, host_session_id: &str) -> Result<CancelResult, ApiError> {
        self.post_json(&format!(
            "/api/sessions/{}/cancel",
            encode_segment(host_session_id)
        ))
        .await
    }

    pub async fn delete_session(&self, host_session_id: &str) -> Result<serde_json::Value, ApiError> {
        let response = self
            .http
            .delete(self.url(&format!(
                "/api/sessions/{}",
                encode_segment(host_session_id)
            )))
            .header("authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|error| ApiError::transport(error.to_string()))?;
        decode(response).await
    }

    /// The diagnostics report is plain text: it is pasted into an issue as-is.
    pub async fn diagnostics(&self) -> Result<String, ApiError> {
        let response = self
            .http
            .get(self.url("/api/diagnostics"))
            .header("authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|error| ApiError::transport(error.to_string()))?;
        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|error| ApiError::transport(error.to_string()))?;
        if !status.is_success() {
            return Err(ApiError::status(status.as_u16(), body));
        }
        Ok(body)
    }

    async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let response = self
            .http
            .get(self.url(path))
            .header("authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|error| ApiError::transport(error.to_string()))?;
        decode(response).await
    }

    async fn post_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, ApiError> {
        let response = self
            .http
            .post(self.url(path))
            .header("authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|error| ApiError::transport(error.to_string()))?;
        decode(response).await
    }
}

async fn decode<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, ApiError> {
    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|error| ApiError::transport(error.to_string()))?;
    if !status.is_success() {
        return Err(ApiError::status(status.as_u16(), body));
    }
    serde_json::from_str(&body).map_err(|error| ApiError::transport(format!("{error}: {body}")))
}

fn normalize_base_url(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}

/// `encodeURIComponent`, restricted to what the daemon actually receives: ids and
/// tokens are ASCII, so unreserved characters pass through untouched.
pub fn encode_segment(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn request_urls_carry_the_token_both_ways_and_encode_ids() {
        let client = RelayClient::new("http://127.0.0.1:7352/", "abc123");
        assert_eq!(
            client.url("/api/health"),
            "http://127.0.0.1:7352/api/health?token=abc123"
        );
        assert_eq!(client.base_url(), "http://127.0.0.1:7352");
        assert_eq!(
            client.url(&format!(
                "/api/workers/{}/cancel",
                encode_segment("codex:s 1")
            )),
            "http://127.0.0.1:7352/api/workers/codex%3As%201/cancel?token=abc123"
        );
        assert_eq!(encode_segment("run/1"), "run%2F1");
    }

    #[tokio::test]
    async fn health_reaches_the_loopback_daemon_without_a_system_proxy() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = [0_u8; 1024];
            let _ = socket.read(&mut request).await.unwrap();
            let body = serde_json::to_string(&Health {
                ok: true,
                pid: 42,
                nonce: "nonce".into(),
                port: address.port(),
                started_at: "now".into(),
                version: "0.1.0".into(),
                database: "/tmp/relay.sqlite".into(),
                sessions: 0,
                runs: 0,
            })
            .unwrap();
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        });

        let health = RelayClient::new(format!("http://{address}"), "token")
            .health()
            .await
            .expect("the tray must reach relayd directly on loopback");
        assert_eq!(health.pid, 42);
        server.await.unwrap();
    }
}
