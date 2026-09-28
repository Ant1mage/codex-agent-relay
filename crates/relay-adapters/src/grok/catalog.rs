//! Native CLI catalogues keep grok.com subscriptions on their own auth route.

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use relay_core::{ModelOption, ReasoningLevel};
use serde::Deserialize;

pub(super) fn default_model(stdout: &str) -> Option<&str> {
    stdout
        .lines()
        .find_map(|line| line.trim().strip_prefix("Default model: "))
        .map(str::trim)
        .filter(|id| valid_id(id))
}

pub(super) fn model_names(stdout: &str, stderr: &str) -> Vec<ModelOption> {
    let diagnostics = format!("{stdout}\n{stderr}").to_lowercase();
    if [
        "failed to fetch",
        "fallback",
        "bundled models",
        "using cached",
    ]
    .iter()
    .any(|needle| diagnostics.contains(needle))
    {
        return Vec::new();
    }
    let mut in_models = false;
    let mut models = Vec::new();
    for line in stdout.lines() {
        if line.trim() == "Available models:" {
            in_models = true;
            continue;
        }
        if !in_models {
            continue;
        }
        let Some(rest) = line.trim().strip_prefix("* ") else {
            continue;
        };
        let Some(id) = rest.split_whitespace().next() else {
            continue;
        };
        if valid_id(id) && !models.iter().any(|model: &ModelOption| model.value == id) {
            models.push(ModelOption::new(id, None));
        }
    }
    models
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 128
        && id.starts_with(|ch: char| ch.is_ascii_alphanumeric())
        && id
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || "._-:/".contains(ch))
}

// Deserialize only public metadata. Cached keys, identity, headers and endpoints
// are deliberately absent from these structures and never reach Relay.
#[derive(Deserialize)]
struct Cache {
    grok_version: String,
    models: HashMap<String, CachedModel>,
}

#[derive(Deserialize)]
struct CachedModel {
    info: Info,
}

#[derive(Deserialize)]
struct Info {
    name: Option<String>,
    #[serde(default)]
    supports_reasoning_effort: bool,
    reasoning_effort: Option<String>,
    #[serde(default)]
    reasoning_efforts: Vec<Effort>,
}

#[derive(Deserialize)]
struct Effort {
    value: String,
    label: Option<String>,
}

pub(super) fn enrich(models: &mut [ModelOption], path: &Path, version: &str) {
    let Ok(file) = std::fs::File::open(path) else {
        return;
    };
    let Ok(metadata) = file.metadata() else {
        return;
    };
    if !metadata.is_file() || metadata.len() > 1_048_576 {
        return;
    }
    let mut bytes = Vec::new();
    if file.take(1_048_577).read_to_end(&mut bytes).is_err() || bytes.len() > 1_048_576 {
        return;
    }
    enrich_bytes(models, &bytes, version);
}

fn enrich_bytes(models: &mut [ModelOption], bytes: &[u8], version: &str) {
    let Ok(cache) = serde_json::from_slice::<Cache>(bytes) else {
        return;
    };
    if version.split_whitespace().nth(1) != Some(cache.grok_version.as_str()) {
        return;
    }
    for model in models {
        let Some(cached) = cache.models.get(&model.value) else {
            continue;
        };
        model.label.clone_from(&cached.info.name);
        if !cached.info.supports_reasoning_effort {
            continue;
        }
        let mut efforts = cached.info.reasoning_efforts.iter().collect::<Vec<_>>();
        // Strength follows the known CLI order; unrecognized future values are
        // retained after it because their availability came from the CLI.
        efforts.sort_by_key(|effort| match effort.value.as_str() {
            "none" | "off" => 0,
            "minimal" => 1,
            "low" => 2,
            "medium" => 3,
            "high" => 4,
            "xhigh" => 5,
            "max" => 6,
            _ => 7,
        });
        for effort in efforts {
            if !valid_id(&effort.value)
                || model
                    .reasoning_levels
                    .iter()
                    .any(|item| item.value == effort.value)
            {
                continue;
            }
            model.reasoning_levels.push(ReasoningLevel {
                strength: (model.reasoning_levels.len() + 1).min(255) as u8,
                label: effort.label.clone().unwrap_or_else(|| effort.value.clone()),
                value: effort.value.clone(),
            });
        }
        model.default_reasoning = cached.info.reasoning_effort.clone().filter(|default| {
            model
                .reasoning_levels
                .iter()
                .any(|effort| &effort.value == default)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_catalogue_rejects_offline_defaults_and_duplicates() {
        let text = "You are logged in with grok.com.\nDefault model: grok-new\nAvailable models:\n  * grok-new (default)\n  * grok-new\n  * custom-model\n";
        assert_eq!(model_names(text, "").len(), 2);
        assert!(model_names(text, "Failed to fetch models, using bundled models").is_empty());
    }

    #[test]
    fn metadata_enriches_only_live_models_and_never_forwards_credentials() {
        let bytes = br#"{"grok_version":"1.0.41","identity":"PRIVATE","models":{"grok-new":{"api_key":"SECRET","info":{"name":"Grok New","supports_reasoning_effort":true,"reasoning_effort":"high","reasoning_efforts":[{"value":"xhigh","label":"Extra High"},{"value":"low"},{"value":"high"}]}},"unavailable":{"info":{"name":"Unavailable"}}}}"#;
        let mut models = vec![ModelOption::new("grok-new", None)];
        enrich_bytes(&mut models, bytes, "grok 1.0.41 (build)");
        assert_eq!(
            models[0]
                .reasoning_levels
                .iter()
                .map(|level| level.value.as_str())
                .collect::<Vec<_>>(),
            vec!["low", "high", "xhigh"]
        );
        assert_eq!(models[0].default_reasoning.as_deref(), Some("high"));
        let serialized = serde_json::to_string(&models).unwrap();
        assert!(
            !serialized.contains("SECRET")
                && !serialized.contains("PRIVATE")
                && !serialized.contains("unavailable")
        );
        let mut stale = vec![ModelOption::new("grok-new", None)];
        enrich_bytes(&mut stale, bytes, "grok 2.0.0");
        assert!(stale[0].reasoning_levels.is_empty());
    }
}
