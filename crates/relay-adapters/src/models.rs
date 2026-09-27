//! Official-HTTP fallback for model discovery.
//!
//! Some runtime CLIs do not publish their model list. When the CLI cannot answer,
//! Relay asks the provider's official API instead.
//!
//! Two rules this module holds to:
//!  * the API key is read from the environment only — never written to disk,
//!    never sent to a renderer, never included in a diagnostic;
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
        models.push(ModelOption { value, label: Some(label) });
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
                label: {
                    let mut chars = value.chars();
                    match chars.next() {
                        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                        None => String::new(),
                    }
                },
                value,
            })
            .collect();
    }
    Vec::new()
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
    let Some(query) = query_for(key) else {
        return HttpModelsResult::default();
    };
    let Some(credential) = api_key(&query) else {
        return HttpModelsResult {
            auth_required: true,
            diagnostics: vec![format!(
                "{} publishes no model list on its CLI. Set {} to let Relay read the model list from the official API",
                query.provider, query.key_env[0]
            )],
            ..HttpModelsResult::default()
        };
    };

    let client = match reqwest::Client::builder().timeout(Duration::from_secs(8)).build() {
        Ok(client) => client,
        Err(error) => {
            return HttpModelsResult {
                diagnostics: vec![format!("{} model list request failed: {error}", query.provider)],
                ..HttpModelsResult::default()
            }
        }
    };
    let response = client
        .get(query.endpoint)
        .header("accept", "application/json")
        .bearer_auth(&credential)
        .send()
        .await;
    let response = match response {
        Ok(response) => response,
        Err(error) => {
            return HttpModelsResult {
                diagnostics: vec![format!(
                    "{} model list request failed: {}",
                    query.provider,
                    redact(&error.to_string(), &credential)
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
                    redact(&error.to_string(), &credential)
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
pub async fn with_model_fallback(cli: RuntimeOptions, provider_key: Option<&str>) -> RuntimeOptions {
    let needs_models = cli.models.is_empty();
    let needs_levels = cli.levels.is_empty();
    let Some(provider_key) = provider_key else {
        return cli;
    };
    if !needs_models && !needs_levels {
        return cli;
    }
    let http = list_models_over_http(provider_key).await;
    let used_api = (needs_models && !http.models.is_empty()) || (needs_levels && !http.levels.is_empty());
    RuntimeOptions {
        models: if needs_models { http.models } else { cli.models },
        levels: if needs_levels { http.levels } else { cli.levels },
        source: if used_api { OptionsSource::Api } else { cli.source },
        // CLI-derived notes stay first: they explain why the fallback ran.
        diagnostics: cli.diagnostics.into_iter().chain(http.diagnostics).collect(),
        ..cli
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openai_style_payloads_are_understood() {
        let payload = serde_json::json!({
            "data": [
                { "id": "deepseek-chat", "display_name": "DeepSeek Chat" },
                { "id": "models/deepseek-reasoner" },
                { "id": "deepseek-chat" }
            ]
        });
        let models = parse_model_payload(&payload);
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].value, "deepseek-chat");
        assert_eq!(models[0].label.as_deref(), Some("DeepSeek Chat"));
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
    fn unknown_providers_are_left_out_rather_than_guessed() {
        assert!(query_for("unknown").is_none());
        assert!(query_for("deepseek").is_some());
    }

    #[test]
    fn credentials_are_never_echoed() {
        assert_eq!(redact("failed with key sk-secret", "sk-secret"), "failed with key <redacted>");
        assert_eq!(redact("plain failure", ""), "plain failure");
    }

    #[tokio::test]
    async fn a_missing_api_key_is_reported_as_auth_required() {
        std::env::remove_var("DEEPSEEK_API_KEY");
        let result = list_models_over_http("deepseek").await;
        assert!(result.auth_required);
        assert!(result.models.is_empty());
        assert!(result.diagnostics[0].contains("DEEPSEEK_API_KEY"));
    }

    #[tokio::test]
    async fn a_cli_that_already_lists_models_is_left_alone() {
        let cli = RuntimeOptions {
            runtime_id: "r".into(),
            adapter_id: "a".into(),
            models: vec![ModelOption { value: "m".into(), label: None }],
            levels: vec![ReasoningLevel { strength: 1, label: "Low".into(), value: "low".into() }],
            model_flag: None,
            reasoning_flag: None,
            source: OptionsSource::Cli,
            diagnostics: Vec::new(),
        };
        let merged = with_model_fallback(cli.clone(), Some("deepseek")).await;
        assert_eq!(merged, cli);
    }
}
