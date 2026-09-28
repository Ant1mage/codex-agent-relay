//! The API client: a port of `packages/relay-api/src/client.ts`.
//!
//! Every call carries the run token twice — `Authorization: Bearer …` for the
//! router and `?token=…` because `EventSource` cannot set headers — and every
//! response is parsed into the shared serde contract instead of a local copy of
//! the shape.

use std::collections::BTreeMap;
use std::fmt;
use std::rc::Rc;

use gloo_net::http::{Request, RequestBuilder};
use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::JsCast;

use relay_api::{
    AdapterCatalog, CancelResult, CodexAction, EventBatch, Health, InspectorSnapshot, InstallResult, PolicyBody,
    ProbeBody, RefreshResult, RelayConfigView, RuntimeBody, RuntimeMutation, RuntimeOptionsView, RuntimeProbe,
    StreamMessage,
};
use relay_core::{AgentProfile, RelayPolicy, RelayPolicyOverride};

use crate::dom;

/// What the inspector's address bar selects. `None` means "not in the URL".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Route {
    pub session: Option<String>,
    pub run: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApiError {
    pub status: u16,
    pub message: String,
}

impl ApiError {
    pub fn new(status: u16, message: impl Into<String>) -> Self {
        Self { status, message: message.into() }
    }
}

impl fmt::Display for ApiError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.status == 0 {
            write!(formatter, "{}", self.message)
        } else {
            write!(formatter, "{} {}", self.status, self.message)
        }
    }
}

impl std::error::Error for ApiError {}

#[derive(Debug, Clone)]
pub struct Client {
    base: String,
    token: String,
}

impl Client {
    pub fn new(base: &str, token: &str) -> Self {
        Self { base: base.trim_end_matches('/').to_string(), token: token.to_string() }
    }

    fn authorization(&self) -> String {
        format!("Bearer {}", self.token)
    }

    /// Absolute URL for an API path, carrying the token as a query parameter.
    pub fn url(&self, path: &str, params: &[(&str, String)]) -> String {
        let mut query: Vec<String> =
            params.iter().map(|(key, value)| format!("{}={}", dom::encode(key), dom::encode(value))).collect();
        query.push(format!("token={}", dom::encode(&self.token)));
        format!("{}{}?{}", self.base, path, query.join("&"))
    }

    pub fn stream_url(&self) -> String {
        self.url("/api/stream", &[])
    }

    async fn decode<T: DeserializeOwned>(response: gloo_net::http::Response) -> Result<T, ApiError> {
        let status = response.status();
        let body = response.text().await.map_err(|error| ApiError::new(status, error.to_string()))?;
        if !(200..300).contains(&status) {
            let message = if body.is_empty() { status.to_string() } else { body };
            return Err(ApiError::new(status, message));
        }
        serde_json::from_str(&body).map_err(|error| ApiError::new(status, error.to_string()))
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, params: &[(&str, String)]) -> Result<T, ApiError> {
        let url = self.url(path, params);
        let response = Request::get(&url)
            .header("authorization", &self.authorization())
            .send()
            .await
            .map_err(|error| ApiError::new(0, error.to_string()))?;
        Self::decode(response).await
    }

    async fn plain(&self, path: &str) -> Result<String, ApiError> {
        let url = self.url(path, &[]);
        let response = Request::get(&url)
            .header("authorization", &self.authorization())
            .send()
            .await
            .map_err(|error| ApiError::new(0, error.to_string()))?;
        let status = response.status();
        let body = response.text().await.map_err(|error| ApiError::new(status, error.to_string()))?;
        if !(200..300).contains(&status) {
            let message = if body.is_empty() { status.to_string() } else { body };
            return Err(ApiError::new(status, message));
        }
        Ok(body)
    }

    async fn send<T: DeserializeOwned, B: Serialize + ?Sized>(
        &self,
        method: &str,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, ApiError> {
        let url = self.url(path, &[]);
        let builder = request(method, &url).header("authorization", &self.authorization());
        let response = match body {
            Some(payload) => {
                let request = builder.json(payload).map_err(|error| ApiError::new(0, error.to_string()))?;
                request.send().await
            }
            None => builder.send().await,
        }
        .map_err(|error| ApiError::new(0, error.to_string()))?;
        Self::decode(response).await
    }

    pub async fn health(&self) -> Result<Health, ApiError> {
        self.get("/api/health", &[]).await
    }

    pub async fn snapshot(&self) -> Result<InspectorSnapshot, ApiError> {
        self.get("/api/snapshot", &[]).await
    }

    /// Relay's own configuration (Agent Profiles + policy) as stored on disk.
    pub async fn config(&self) -> Result<RelayConfigView, ApiError> {
        self.get("/api/config", &[]).await
    }

    /// Every adapter Relay can drive, including CLIs it did not detect.
    pub async fn adapters(&self) -> Result<AdapterCatalog, ApiError> {
        self.get("/api/adapters", &[]).await
    }

    pub async fn events(&self, run_id: &str, after: u64) -> Result<EventBatch, ApiError> {
        self.get(&format!("/api/runs/{}/events", dom::encode(run_id)), &[("after", after.to_string())]).await
    }

    /// The plain-text support report; the caller copies it to the clipboard.
    pub async fn diagnostics(&self) -> Result<String, ApiError> {
        self.plain("/api/diagnostics").await
    }

    /// Model and reasoning values this runtime's CLI actually advertises.
    pub async fn runtime_options(&self, runtime_id: &str) -> Result<RuntimeOptionsView, ApiError> {
        self.get(&format!("/api/runtimes/{}/options", dom::encode(runtime_id)), &[]).await
    }

    /// Checks an executable before it is saved, so the form can report failures.
    pub async fn probe_runtime(&self, adapter_id: &str, executable_path: &str) -> Result<RuntimeProbe, ApiError> {
        let body = ProbeBody { adapter_id: adapter_id.to_string(), executable_path: executable_path.to_string() };
        self.send("POST", "/api/runtimes/probe", Some(&body)).await
    }

    /// Registers a runtime by hand; the daemon probes it before saving.
    pub async fn save_runtime(
        &self,
        id: &str,
        adapter_id: &str,
        executable_path: &str,
        label: Option<String>,
    ) -> Result<RuntimeMutation, ApiError> {
        let body = RuntimeBody {
            adapter_id: adapter_id.to_string(),
            executable_path: executable_path.to_string(),
            label,
        };
        self.send("PUT", &format!("/api/config/runtimes/{}", dom::encode(id)), Some(&body)).await
    }

    pub async fn delete_runtime(&self, runtime_id: &str) -> Result<RelayConfigView, ApiError> {
        self.send::<RelayConfigView, ()>(
            "DELETE",
            &format!("/api/config/runtimes/{}", dom::encode(runtime_id)),
            None,
        )
        .await
    }

    pub async fn save_profile(&self, profile: &AgentProfile) -> Result<RelayConfigView, ApiError> {
        self.send("PUT", &format!("/api/config/profiles/{}", dom::encode(&profile.id)), Some(profile)).await
    }

    pub async fn delete_profile(&self, profile_id: &str) -> Result<RelayConfigView, ApiError> {
        self.send::<RelayConfigView, ()>(
            "DELETE",
            &format!("/api/config/profiles/{}", dom::encode(profile_id)),
            None,
        )
        .await
    }

    pub async fn save_policy(
        &self,
        policy: &RelayPolicy,
        workspace_overrides: &BTreeMap<String, RelayPolicyOverride>,
    ) -> Result<RelayConfigView, ApiError> {
        let body = PolicyBody { policy: *policy, workspace_overrides: workspace_overrides.clone() };
        self.send("PUT", "/api/config/policy", Some(&body)).await
    }

    /// Re-detects runtimes and re-reads configuration.
    pub async fn refresh(&self) -> Result<RefreshResult, ApiError> {
        self.send::<RefreshResult, ()>("POST", "/api/refresh", None).await
    }

    /// Codex integration lifecycle: install, repair, update, remove.
    pub async fn codex(&self, action: CodexAction) -> Result<InstallResult, ApiError> {
        let name = serde_json::to_value(action).ok().and_then(|value| value.as_str().map(str::to_string));
        let name = name.unwrap_or_else(|| "install".to_string());
        self.send::<InstallResult, ()>("POST", &format!("/api/codex/{name}"), None).await
    }

    pub async fn cancel_worker(&self, worker_session_id: &str) -> Result<CancelResult, ApiError> {
        self.send::<CancelResult, ()>(
            "POST",
            &format!("/api/workers/{}/cancel", dom::encode(worker_session_id)),
            None,
        )
        .await
    }

    pub async fn accept_worker(&self, worker_session_id: &str) -> Result<serde_json::Value, ApiError> {
        self.send::<serde_json::Value, ()>(
            "POST",
            &format!("/api/workers/{}/accept", dom::encode(worker_session_id)),
            None,
        )
        .await
    }

    pub async fn resume_worker(&self, worker_session_id: &str, feedback: &str) -> Result<serde_json::Value, ApiError> {
        let body = serde_json::json!({ "feedback": feedback });
        self.send(
            "POST",
            &format!("/api/workers/{}/resume", dom::encode(worker_session_id)),
            Some(&body),
        )
        .await
    }

    pub async fn cancel_session(&self, host_session_id: &str) -> Result<CancelResult, ApiError> {
        self.send::<CancelResult, ()>(
            "POST",
            &format!("/api/sessions/{}/cancel", dom::encode(host_session_id)),
            None,
        )
        .await
    }

    pub async fn delete_session(&self, host_session_id: &str) -> Result<serde_json::Value, ApiError> {
        self.send::<serde_json::Value, ()>(
            "DELETE",
            &format!("/api/sessions/{}", dom::encode(host_session_id)),
            None,
        )
        .await
    }
}

/// A live SSE subscription. The page opens exactly one and keeps it for its whole
/// lifetime (see `state::start_stream`), and dropping this handle closes the
/// connection — the browser would keep an open `EventSource` alive, but relying
/// on that is a trap for the next reader.
pub struct Stream {
    source: web_sys::EventSource,
}

impl Drop for Stream {
    fn drop(&mut self) {
        self.source.close();
    }
}

/// Subscribes to `/api/stream`. Browser-only by design: the daemon pushes a
/// snapshot, then deltas, and the page merges both.
pub fn connect_stream(
    url: &str,
    on_open: impl Fn() + 'static,
    on_error: impl Fn(String) + 'static,
    on_message: impl Fn(StreamMessage) + 'static,
) -> Result<Stream, ApiError> {
    let source = web_sys::EventSource::new(url).map_err(|error| ApiError::new(0, dom::error_message(&error)))?;
    let on_error = Rc::new(on_error);

    let opened = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || on_open());
    source.set_onopen(Some(opened.as_ref().unchecked_ref()));
    opened.forget();

    let failed = {
        let on_error = Rc::clone(&on_error);
        wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            on_error("the event stream was interrupted".to_string())
        })
    };
    source.set_onerror(Some(failed.as_ref().unchecked_ref()));
    failed.forget();

    let received = {
        let on_error = Rc::clone(&on_error);
        wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event: web_sys::MessageEvent| {
            let Ok(text) = event.data().dyn_into::<js_sys::JsString>() else {
                on_error("the event stream sent an unreadable frame".to_string());
                return;
            };
            match serde_json::from_str::<StreamMessage>(&String::from(text)) {
                Ok(message) => on_message(message),
                Err(error) => on_error(error.to_string()),
            }
        })
    };
    source.set_onmessage(Some(received.as_ref().unchecked_ref()));
    received.forget();

    Ok(Stream { source })
}

/// The four verbs the API uses, spelled out so a typo cannot become a GET.
fn request(method: &str, url: &str) -> RequestBuilder {
    match method {
        "POST" => Request::post(url),
        "PUT" => Request::put(url),
        "DELETE" => Request::delete(url),
        _ => Request::get(url),
    }
}
