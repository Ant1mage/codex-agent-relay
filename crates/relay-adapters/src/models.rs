//! Official-HTTP fallback for model discovery.
//!
//! Some runtime CLIs do not publish their model list. When the CLI cannot answer,
//! Relay asks the provider's official API instead.
//!
//! Two rules this module holds to:
//!  * the API key is read from the environment or supplied by the runtime's
//!    local credential resolver — never sent to a renderer or diagnostic;
//!  * a provider whose endpoint or response shape is not verified is left out
//!    entirely rather than guessed at.

use std::time::Duration;

use relay_core::{ModelOption, OptionsSource, ReasoningLevel, RuntimeOptions};

pub struct HttpModelQuery {
    pub provider: &'static str,
    pub endpoint: &'static str,
    /// Environment variables checked, in order, for a bearer token.
    pub key_env: &'static [&'static str],
}

pub fn query_for(key: &str) -> Option<HttpModelQuery> {
    match key {
        "deepseek" => Some(HttpModelQuery {
            provider: "DeepSeek",
            endpoint: "https://api.deepseek.com/models",
            key_env: &["DEEPSEEK_API_KEY"],
        }),
        "kimi" => Some(HttpModelQuery {
            provider: "Kimi",
            endpoint: "https://api.moonshot.ai/v1/models",
            key_env: &["MOONSHOT_API_KEY", "KIMI_API_KEY"],
        }),
        "grok" => Some(HttpModelQuery {
            provider: "Grok",
            endpoint: "https://api.x.ai/v1/models",
            key_env: &["XAI_API_KEY", "GROK_API_KEY"],
        }),
        _ => None,
    }
}

#[derive(Debug, Default)]
pub struct HttpModelsResult {
    pub models: Vec<ModelOption>,
    pub levels: Vec<ReasoningLevel>,
    pub auth_required: bool,
    pub diagnostics: Vec<String>,
}

/// Reads models out of the documented response shapes: an OpenAI-style
/// `{ data: [...] }`, or `{ models: [...] }`.
pub fn parse_model_payload(payload: &serde_json::Value) -> Vec<ModelOption> {
    let list = payload
        .get("data")
        .and_then(|value| value.as_array())
        .or_else(|| payload.get("models").and_then(|value| value.as_array()));
    let Some(list) = list else {
        return Vec::new();
    };
    let mut models: Vec<ModelOption> = Vec::new();
    for entry in list {
        let raw_id = entry
            .get("id")
            .and_then(|value| value.as_str())
            .or_else(|| entry.get("name").and_then(|value| value.as_str()));
        let Some(raw_id) = raw_id else {
            continue;
        };
        // Some APIs report "models/<id>"; the CLI wants the bare id.
        let value = raw_id.strip_prefix("models/").unwrap_or(raw_id).to_string();
        if value.is_empty() {
            continue;
        }
        let label = entry
            .get("display_name")
            .and_then(|value| value.as_str())
            .or_else(|| entry.get("displayName").and_then(|value| value.as_str()))
            .or_else(|| entry.get("name").and_then(|value| value.as_str()))
            .unwrap_or(&value)
            .to_string();
        if models.iter().any(|model| model.value == value) {
            continue;
        }
        let reasoning_levels = entry
            .get("effort")
            .and_then(|effort| effort.get("supported_levels"))
            .and_then(|levels| levels.as_array())
            .map(|levels| {
                levels
                    .iter()
                    .filter_map(|level| level.as_str())
                    .filter(|level| !level.is_empty())
                    .enumerate()
                    .map(|(index, value)| ReasoningLevel {
                        strength: (index + 1) as u8,
                        label: display_effort(value),
                        value: value.to_string(),
                    })
                    .take(5)
                    .collect()
            })
            .unwrap_or_default();
        let default_reasoning = entry
            .get("effort")
            .and_then(|effort| effort.get("default_level"))
            .and_then(|level| level.as_str())
            .map(str::to_string);
        models.push(ModelOption {
            value,
            label: Some(label),
            reasoning_levels,
            default_reasoning,
        });
    }
    models
}

/// DeepSeek is the one provider whose model endpoint declares the reasoning
/// levels a model accepts.
pub fn parse_reasoning_payload(payload: &serde_json::Value) -> Vec<ReasoningLevel> {
    let Some(list) = payload.get("data").and_then(|value| value.as_array()) else {
        return Vec::new();
    };
    for entry in list {
        let Some(levels) = entry
            .get("effort")
            .and_then(|effort| effort.get("supported_levels"))
            .and_then(|levels| levels.as_array())
        else {
            continue;
        };
        let values: Vec<String> = levels
            .iter()
            .filter_map(|value| value.as_str())
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect();
        if values.is_empty() {
            continue;
        }
        return values
            .into_iter()
            .take(5)
            .enumerate()
            .map(|(index, value)| ReasoningLevel {
                strength: (index + 1) as u8,
                label: display_effort(&value),
                value,
            })
            .collect();
    }
    Vec::new()
}

fn display_effort(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn api_key(query: &HttpModelQuery) -> Option<String> {
    for key in query.key_env {
        if let Ok(value) = std::env::var(key) {
            let trimmed = value.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Never let a credential reach a diagnostic string.
fn redact(message: &str, key: &str) -> String {
    if key.is_empty() {
        message.to_string()
    } else {
        message.replace(key, "<redacted>")
    }
}

pub async fn list_models_over_http(key: &str) -> HttpModelsResult {
    let credential = query_for(key).and_then(|query| api_key(&query));
    list_models_over_http_with_key(key, credential.as_deref()).await
}

async fn list_models_over_http_with_key(key: &str, supplied: Option<&str>) -> HttpModelsResult {
    let Some(query) = query_for(key) else {
        return HttpModelsResult::default();
    };
    request_models(query, supplied).await
}

async fn request_models(query: HttpModelQuery, supplied: Option<&str>) -> HttpModelsResult {
    let Some(credential) = supplied.map(str::trim).filter(|value| !value.is_empty()) else {
        return HttpModelsResult {
            auth_required: true,
            diagnostics: vec![if query.provider == "DeepSeek" {
                "DeepSeek model discovery needs the DSH credential or DEEPSEEK_API_KEY in Relay's launching environment".to_string()
            } else {
                format!(
                    "{} publishes no model list on its CLI. Set {} to let Relay read the model list from the official API",
                    query.provider, query.key_env[0]
                )
            }],
            ..HttpModelsResult::default()
        };
    };

    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(8))
        .build()
    {
        Ok(client) => client,
        Err(error) => {
            return HttpModelsResult {
                diagnostics: vec![format!(
                    "{} model list request failed: {error}",
                    query.provider
                )],
                ..HttpModelsResult::default()
            }
        }
    };
    let response = client
        .get(query.endpoint)
        .header("accept", "application/json")
        .bearer_auth(credential)
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            return HttpModelsResult {
                diagnostics: vec![format!(
                    "{} model list request failed: {}",
                    query.provider,
                    redact(&error.to_string(), credential)
                )],
                ..HttpModelsResult::default()
            }
        }
    };
    if !response.status().is_success() {
        return HttpModelsResult {
            diagnostics: vec![format!(
                "{} model list request failed with HTTP {}",
                query.provider,
                response.status().as_u16()
            )],
            ..HttpModelsResult::default()
        };
    }
    let payload: serde_json::Value = match response.json().await {
        Ok(payload) => payload,
        Err(error) => {
            return HttpModelsResult {
                diagnostics: vec![format!(
                    "{} model list request failed: {}",
                    query.provider,
                    redact(&error.to_string(), credential)
                )],
                ..HttpModelsResult::default()
            }
        }
    };
    let models = parse_model_payload(&payload);
    if models.is_empty() {
        return HttpModelsResult {
            diagnostics: vec![format!(
                "{} returned no models; Relay will use the runtime default",
                query.provider
            )],
            ..HttpModelsResult::default()
        };
    }
    HttpModelsResult {
        levels: parse_reasoning_payload(&payload),
        models,
        auth_required: false,
        diagnostics: Vec::new(),
    }
}

/// Merges CLI-reported options with the official API. The CLI wins for each list
/// it can actually enumerate; the API fills only a missing list.
///
/// A list is only ever filled in behind a flag the adapter will use to apply the
/// answer: offering a model the run cannot apply would be a lie told by the UI.
pub async fn with_model_fallback(
    cli: RuntimeOptions,
    provider_key: Option<&str>,
) -> RuntimeOptions {
    with_model_fallback_applicable(cli, provider_key, false).await
}

/// Uses the provider HTTP catalogue when the runtime applies selections through
/// a mechanism other than CLI flags (for example DSH's per-run config overlay).
pub async fn with_model_fallback_applicable(
    cli: RuntimeOptions,
    provider_key: Option<&str>,
    applies_without_flags: bool,
) -> RuntimeOptions {
    let credential = provider_key
        .and_then(query_for)
        .and_then(|query| api_key(&query));
    with_model_fallback_applicable_with_key(
        cli,
        provider_key,
        applies_without_flags,
        credential.as_deref(),
    )
    .await
}

/// Applies the same fallback using a credential resolved by the runtime's own
/// local store. The caller keeps ownership; only the HTTP authorization header
/// receives it.
pub async fn with_model_fallback_applicable_with_key(
    cli: RuntimeOptions,
    provider_key: Option<&str>,
    applies_without_flags: bool,
    supplied: Option<&str>,
) -> RuntimeOptions {
    let needs_models = cli.models.is_empty() && (cli.model_flag.is_some() || applies_without_flags);
    let needs_levels = (cli.levels.is_empty()
        || cli
            .models
            .iter()
            .any(|model| model.reasoning_levels.is_empty()))
        && (cli.reasoning_flag.is_some() || applies_without_flags);
    let Some(provider_key) = provider_key else {
        return cli;
    };
    if !needs_models && !needs_levels {
        return cli;
    }
    let http = list_models_over_http_with_key(provider_key, supplied).await;
    merge_http_fallback(cli, http, needs_models, needs_levels)
}

fn merge_http_fallback(
    cli: RuntimeOptions,
    http: HttpModelsResult,
    needs_models: bool,
    needs_levels: bool,
) -> RuntimeOptions {
    let used_api =
        (needs_models && !http.models.is_empty()) || (needs_levels && !http.levels.is_empty());
    let mut models = if needs_models {
        http.models.clone()
    } else {
        cli.models.clone()
    };
    // The API can provide per-model effort metadata even when the CLI already
    // reported its model names. Enrich matching entries without replacing the
    // runtime's own labels, ordering, or model set.
    if !needs_models {
        for model in &mut models {
            if let Some(reported) = http.models.iter().find(|item| item.value == model.value) {
                if model.reasoning_levels.is_empty() {
                    model.reasoning_levels = reported.reasoning_levels.clone();
                }
                if model.default_reasoning.is_none() {
                    model.default_reasoning = reported.default_reasoning.clone();
                }
            }
        }
    }
    RuntimeOptions {
        models,
        levels: if needs_levels {
            http.levels.clone()
        } else {
            cli.levels.clone()
        },
        source: if used_api {
            OptionsSource::Api
        } else {
            cli.source
        },
        // CLI-derived notes stay first: they explain why the fallback ran.
        diagnostics: cli
            .diagnostics
            .into_iter()
            .chain(http.diagnostics)
            .collect(),
        ..cli
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_dsh_credential_authenticates_the_http_catalogue_request() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!("http://{}/models", listener.local_addr().unwrap());
        let response = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut buffer = [0_u8; 4096];
            let count = stream.read(&mut buffer).await.unwrap();
            let request = String::from_utf8_lossy(&buffer[..count]);
            assert!(
                request.contains("Bearer local-test-key"),
                "missing credential header"
            );
            assert!(request.starts_with("GET /models HTTP/1.1"));
            let body = r#"{"data":[{"id":"deepseek-flash","effort":{"supported_levels":["low","high"],"default_level":"high"}}]}"#;
            stream
                .write_all(format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                ).as_bytes())
                .await
                .unwrap();
        });
        let query = HttpModelQuery {
            provider: "DeepSeek",
            endpoint: Box::leak(endpoint.into_boxed_str()),
            key_env: &["DEEPSEEK_API_KEY"],
        };
        let result = request_models(query, Some("local-test-key")).await;
        response.await.unwrap();

        assert_eq!(result.models[0].value, "deepseek-flash");
        assert_eq!(result.models[0].reasoning_levels[1].value, "high");
        assert!(result.diagnostics.is_empty());
    }

    #[test]
    fn openai_style_payloads_are_understood() {
        let payload = serde_json::json!({
            "data": [
                {
                    "id": "deepseek-chat",
                    "display_name": "DeepSeek Chat",
                    "effort": {
                        "supported_levels": ["low", "high", "max"],
                        "default_level": "high"
                    }
                },
                { "id": "models/deepseek-reasoner" },
                { "id": "deepseek-chat" }
            ]
        });
        let models = parse_model_payload(&payload);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].value, "deepseek-chat");
        assert_eq!(models[0].label.as_deref(), Some("DeepSeek Chat"));
        assert_eq!(
            models[0]
                .reasoning_levels
                .iter()
                .map(|level| level.value.as_str())
                .collect::<Vec<_>>(),
            vec!["low", "high", "max"]
        );
        assert_eq!(models[0].default_reasoning.as_deref(), Some("high"));
        assert_eq!(models[1].value, "deepseek-reasoner");
    }

    #[test]
    fn reasoning_levels_come_from_the_effort_block() {
        let payload = serde_json::json!({
            "data": [{ "id": "m", "effort": { "supported_levels": ["low", "medium", "high"] } }]
        });
        let levels = parse_reasoning_payload(&payload);
        assert_eq!(levels.len(), 3);
        assert_eq!(levels[2].strength, 3);
        assert_eq!(levels[2].label, "High");
    }

    #[test]
    fn an_overlay_adapter_gets_model_specific_http_options_without_cli_flags() {
        let api_model = ModelOption {
            value: "deepseek-v4-pro".to_string(),
            label: Some("DeepSeek-V4-Pro".to_string()),
            reasoning_levels: vec![ReasoningLevel {
                strength: 1,
                label: "High".to_string(),
                value: "high".to_string(),
            }],
            default_reasoning: Some("high".to_string()),
        };
        let cli = RuntimeOptions::empty("dsh", "deepseek-harness", "no CLI flags");
        let merged = merge_http_fallback(
            cli,
            HttpModelsResult {
                models: vec![api_model.clone()],
                levels: api_model.reasoning_levels.clone(),
                ..HttpModelsResult::default()
            },
            true,
            true,
        );

        assert_eq!(merged.models, vec![api_model]);
        assert_eq!(merged.levels[0].value, "high");
        assert_eq!(merged.source, OptionsSource::Api);
        assert_eq!(merged.model_flag, None);
        assert_eq!(merged.reasoning_flag, None);
    }

    #[test]
    fn unknown_providers_are_left_out_rather_than_guessed() {
        assert!(query_for("unknown").is_none());
        assert!(query_for("deepseek").is_some());
    }

    #[test]
    fn credentials_are_never_echoed() {
        assert_eq!(
            redact("failed with key sk-secret", "sk-secret"),
            "failed with key <redacted>"
        );
        assert_eq!(redact("plain failure", ""), "plain failure");
    }

    #[tokio::test]
    async fn a_missing_api_key_is_reported_as_auth_required() {
        std::env::remove_var("DEEPSEEK_API_KEY");
        let result = list_models_over_http("deepseek").await;
        assert!(result.auth_required);
        assert!(result.models.is_empty());
        assert!(result.diagnostics[0].contains("DEEPSEEK_API_KEY"));

        let dsh_overlay_options = RuntimeOptions::empty("dsh", "deepseek-harness", "no CLI list");
        let dsh_options =
            with_model_fallback_applicable(dsh_overlay_options, Some("deepseek"), true).await;
        assert!(dsh_options.models.is_empty());
        assert!(dsh_options
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.contains("DEEPSEEK_API_KEY")));
    }

    #[tokio::test]
    async fn a_cli_that_already_lists_models_is_left_alone() {
        let cli = RuntimeOptions {
            runtime_id: "r".into(),
            adapter_id: "a".into(),
            models: vec![ModelOption::new("m", None)],
            levels: vec![ReasoningLevel {
                strength: 1,
                label: "Low".into(),
                value: "low".into(),
            }],
            model_flag: None,
            reasoning_flag: None,
            source: OptionsSource::Cli,
            diagnostics: Vec::new(),
        };
        let merged = with_model_fallback(cli.clone(), Some("deepseek")).await;
        assert_eq!(merged, cli);
    }

    /// A list the runtime cannot apply must never be fetched, let alone shown.
    #[tokio::test]
    async fn a_cli_without_a_model_flag_gets_no_model_list() {
        std::env::remove_var("DEEPSEEK_API_KEY");
        let cli = RuntimeOptions {
            runtime_id: "r".into(),
            adapter_id: "a".into(),
            models: Vec::new(),
            levels: Vec::new(),
            model_flag: None,
            reasoning_flag: None,
            source: OptionsSource::Default,
            diagnostics: Vec::new(),
        };
        let merged = with_model_fallback(cli.clone(), Some("deepseek")).await;
        assert_eq!(merged, cli, "no flag means no way to apply a choice");
    }
}
