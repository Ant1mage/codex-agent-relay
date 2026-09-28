//! DeepSeek Harness (`dsh`).
//!
//! The JSON stream is the supported path. Its semantics are Relay's contract
//! with this CLI and are deliberately unchanged:
//!
//! ```text
//! session      → native session id
//! thinking     → worker/reasoning
//! text         → worker/message
//! status       → worker/message (status) + turn_end kind
//! tool_call    → tool/read · tool/search · tool/edit · tool/command
//! tool_result  → tool/result
//! final        → worker/message (final) + the summary of worker/completed
//! error        → worker/message (diagnostic) + the failure message
//! ```
//!
//! Core never sees any of this: only the mapped Relay events leave this module.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use relay_core::{
    AdapterCapabilities, AdapterEvent, AgentAdapter, DetectionResult, EnforcementSet, ModelOption,
    OptionsSource, ReasoningLevel, RelayError, RelayEventType, Result, ResumeInput, Runtime,
    RuntimeHealth, RuntimeOptions, StartInput, WorkerHandle,
};
use serde::Deserialize;

use crate::cli::{run_cli, ParsedOutput, ProcessSupervisor, StreamMode, StreamOutcome, StreamSpec};
use crate::models::with_model_fallback_applicable_with_key;
use crate::probe::{
    capture_with, discover_executable, probe_runtime_options, probe_target, read_help_with,
    version_of, with_selection_args, Selection,
};

pub const ADAPTER_ID: &str = "deepseek-harness";
pub const RUNTIME_ID: &str = "runtime:deepseek-harness";

const DEFAULT_API_KEY_REF: &str = "DEEPSEEK_API_KEY";
const CREDENTIAL_FILE: &str = ".credentials.yaml";

#[derive(Deserialize)]
struct DshCredentials {
    version: u8,
    #[serde(default)]
    refs: std::collections::HashMap<String, String>,
}

/// DSH refuses credential files readable by other users. Match that check and
/// the versioned `refs` format before using a key for the official model API.
fn stored_api_key(path: &Path, reference: &str) -> Option<String> {
    let file = std::fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > 1024 * 1024 {
        return None;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return None;
        }
    }
    let mut body = String::new();
    file.take(1024 * 1024 + 1).read_to_string(&mut body).ok()?;
    if body.len() > 1024 * 1024 {
        return None;
    }
    let document: DshCredentials = serde_yaml::from_str(&body).ok()?;
    if document.version != 1 {
        return None;
    }
    document
        .refs
        .get(reference)
        .filter(|value| !value.trim().is_empty())
        .cloned()
}

fn credential_reference(value: &str) -> bool {
    let mut chars = value.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|character| character.is_ascii_alphanumeric() || character == '_')
}

fn add_deepseek_off(options: &mut RuntimeOptions, selected_model: Option<&str>) {
    let off = || ReasoningLevel {
        strength: 0,
        label: "Off".to_string(),
        value: "off".to_string(),
    };
    for model in &mut options.models {
        if !model
            .reasoning_levels
            .iter()
            .any(|level| level.value == "off")
        {
            model.reasoning_levels.insert(0, off());
        }
    }
    if let Some(selected) = selected_model {
        if let Some(model) = options.models.iter().find(|model| model.value == selected) {
            options.levels = model.reasoning_levels.clone();
        }
    }
    if !options.levels.iter().any(|level| level.value == "off") {
        options.levels.insert(0, off());
    }
}

/* ------------------------------------------------------------------ */
/* How model, reasoning and instructions reach a dsh run               */
/* ------------------------------------------------------------------ */

/// Composing a whole profile is heavier than answering --help.
const DUMP_TIMEOUT: Duration = Duration::from_secs(15);

/// The entry schema mounts every plugin's config shape, so it costs more still.
const SCHEMA_TIMEOUT: Duration = Duration::from_secs(30);

/// One model a provider route declares, with the reasoning levels it accepts.
#[derive(Debug, Clone, PartialEq)]
pub struct DshModelOption {
    pub value: String,
    pub label: Option<String>,
    /// True when the model states its own levels, which may be none at all.
    pub reasoning_declared: bool,
    /// The levels the model accepts, in the order the runtime lists them.
    pub reasoning_efforts: Vec<String>,
    pub default_reasoning_effort: Option<String>,
}

/// The profile's own system persona, resolved so Relay can restate it.
#[derive(Debug, Clone, PartialEq)]
pub struct DshPersona {
    pub prefix: String,
    pub suffix: String,
}

/// The complete model selection one dsh run is started with.
#[derive(Debug, Clone, PartialEq)]
pub struct DshSelection {
    pub provider: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
}

/// What one dsh profile says about its own model selection and its persona.
///
/// Everything here is read from the CLI itself: the composed configuration
/// (--dump-config) and the entry schema of that composition
/// (--dump-config-schema). Relay never guesses a key or invents a model name.
#[derive(Debug, Default, PartialEq)]
pub struct DshComposition {
    /// The provider route the profile's default model selection points at.
    pub provider: Option<String>,
    /// The model that selection names.
    pub model: Option<String>,
    /// The reasoning effort that selection names, when it names one.
    pub reasoning_effort: Option<String>,
    /// Name of the credential reference used by the official API route.
    pub api_key_env: Option<String>,
    /// The levels the provider route itself accepts.
    pub route_efforts: Vec<String>,
    /// The level the route falls back to, when the profile states one.
    pub route_default_effort: Option<String>,
    /// The catalogue the route declares.
    pub models: Vec<DshModelOption>,
    /// The composed persona, and only when Relay may restate the whole row.
    pub persona: Option<DshPersona>,
}

impl DshComposition {
    /// The composition row that serves a provider route.
    ///
    /// A route Relay cannot map offers no models: a picker whose choice never
    /// reaches the runtime is worse than no picker.
    fn provider_row(provider: &str) -> Option<&'static str> {
        match provider {
            "deepseek-official" => Some("llm-deepseek"),
            "deepseek-account" => Some("llm-deepseek-account"),
            _ => None,
        }
    }

    /// The catalogue entry a model id names, when the profile declares one.
    pub fn model_option(&self, value: &str) -> Option<&DshModelOption> {
        self.models.iter().find(|model| model.value == value)
    }

    /// The reasoning levels a model accepts: its own, or the route's.
    pub fn efforts_for(&self, model: &str) -> Vec<String> {
        match self.model_option(model) {
            Some(option) if option.reasoning_declared => option.reasoning_efforts.clone(),
            _ => self.route_efforts.clone(),
        }
    }
}

/// Reads everything Relay needs out of a composed profile dump.
pub fn parse_composition(text: &str) -> DshComposition {
    let rows = crate::yaml::rows(text);
    let mut composition = DshComposition::default();
    for row in &rows {
        match row.id.as_str() {
            "agent-default-model" => {
                let Some(config) = row.config() else { continue };
                composition.provider = text_of(config, "provider");
                composition.model = text_of(config, "model");
                composition.reasoning_effort = text_of(config, "reasoningEffort");
            }
            "system-prompt" => composition.persona = persona_of(row),
            _ => {}
        }
    }
    let Some(provider) = composition.provider.clone() else {
        return composition;
    };
    composition.models = catalogue_of(&rows, &provider);
    if let Some(row) = provider_row_of(&rows, &provider) {
        let config = row.config();
        if provider == "deepseek-official" {
            composition.api_key_env = config.and_then(|config| text_of(config, "apiKeyEnv"));
        }
        let thinking = config.and_then(|config| text_of(config, "thinking"));
        // A route that runs with thinking disabled accepts exactly one effort,
        // which is what its own adapter reports for every model it serves.
        if thinking.as_deref() == Some("disabled") {
            composition.route_efforts = vec!["off".to_string()];
        }
        composition.route_default_effort =
            config.and_then(|config| text_of(config, "reasoningEffort"));
    }
    // The route's own default is the model selection the profile already made.
    if composition.route_default_effort.is_none() {
        composition.route_default_effort = composition.reasoning_effort.clone();
    }
    for model in &mut composition.models {
        if model.default_reasoning_effort.is_none() {
            model.default_reasoning_effort = composition.route_default_effort.clone();
        }
    }
    composition
}

/// One scalar of a mapping, resolved.
fn text_of(node: &crate::yaml::Node, key: &str) -> Option<String> {
    node.get(key)
        .and_then(crate::yaml::Node::text)
        .map(str::to_string)
}

/// The persona row, and only when Relay may restate it.
///
/// A patch replaces the row's whole config object, so any other key —
/// includeRuntimeContext, toolOrder, anything a profile added — would silently
/// fall back to its schema default. Relay restates the row only when the
/// composed config is exactly the persona pair, and uses the prompt envelope
/// for the instructions otherwise.
fn persona_of(row: &crate::yaml::Row) -> Option<DshPersona> {
    let config = row.config()?;
    let mut keys = config.keys();
    keys.sort_unstable();
    if keys != ["personaPrefix", "personaSuffix"] {
        return None;
    }
    Some(DshPersona {
        prefix: text_of(config, "personaPrefix")?,
        suffix: text_of(config, "personaSuffix")?,
    })
}

/// The composed row that serves a provider route.
fn provider_row_of<'a>(
    rows: &'a [crate::yaml::Row],
    provider: &str,
) -> Option<&'a crate::yaml::Row> {
    if let Some(id) = DshComposition::provider_row(provider) {
        if let Some(row) = rows.iter().find(|row| row.id == id) {
            return Some(row);
        }
    }
    // A row that serves several routes names them under providers.
    rows.iter().find(|row| {
        row.config()
            .and_then(|config| config.get("providers"))
            .and_then(|providers| providers.get(provider))
            .is_some()
    })
}

/// The catalogue the profile itself declares for a route.
fn catalogue_of(rows: &[crate::yaml::Row], provider: &str) -> Vec<DshModelOption> {
    let Some(row) = provider_row_of(rows, provider) else {
        return Vec::new();
    };
    let Some(config) = row.config() else {
        return Vec::new();
    };
    let models = config.get("models").or_else(|| {
        config
            .get("providers")
            .and_then(|providers| providers.get(provider))
            .and_then(|route| route.get("models"))
    });
    let Some(items) = models.and_then(crate::yaml::Node::items) else {
        return Vec::new();
    };
    let mut catalogue = Vec::new();
    for item in items {
        let Some(value) = item.get("id").and_then(crate::yaml::Node::text) else {
            continue;
        };
        let (reasoning_declared, reasoning_efforts) = declared_efforts(item);
        catalogue.push(DshModelOption {
            value: value.to_string(),
            label: item
                .get("name")
                .and_then(crate::yaml::Node::text)
                .map(str::to_string),
            reasoning_declared,
            reasoning_efforts,
            default_reasoning_effort: None,
        });
    }
    catalogue
}

/// The levels a model declares for itself: a mapping of level to wire name, or
/// false for a model that takes no reasoning at all.
fn declared_efforts(item: &crate::yaml::Node) -> (bool, Vec<String>) {
    match item.get("reasoningEfforts") {
        Some(crate::yaml::Node::Mapping(entries)) => (
            true,
            entries.iter().map(|(level, _)| level.clone()).collect(),
        ),
        Some(crate::yaml::Node::Scalar(value)) if value == "false" => (true, Vec::new()),
        _ => (false, Vec::new()),
    }
}

/// Reads the catalogue and the effort levels the entry schema declares.
///
/// The composed configuration only shows what a profile overrode; dsh keeps its
/// own provider catalogue in code, and the schema of the composed entry is where
/// the CLI states it. Without this, a profile that declares no catalogue would
/// offer no models at all — and a model picker that offers nothing is the same as
/// no picker.
pub fn apply_schema(composition: &mut DshComposition, schema: &str) {
    let Ok(schema) = serde_json::from_str::<serde_json::Value>(schema) else {
        return;
    };
    let Some(provider) = composition.provider.clone() else {
        return;
    };
    let Some(properties) = schema_properties(&schema, &provider) else {
        return;
    };
    if composition.route_efforts.is_empty() {
        if let Some(field) = properties.get("reasoningEffort") {
            composition.route_efforts = enum_values(field);
        }
    }
    if composition.models.is_empty() {
        composition.models = schema_catalogue(&properties);
    }
    for model in &mut composition.models {
        if model.default_reasoning_effort.is_none() {
            model.default_reasoning_effort = composition.route_default_effort.clone();
        }
    }
}

/// The config properties of the entry that serves a provider route.
fn schema_properties(
    schema: &serde_json::Value,
    provider: &str,
) -> Option<serde_json::Map<String, serde_json::Value>> {
    let row_id = DshComposition::provider_row(provider)?;
    let reference = schema
        .get("x-cordis")?
        .get("entries")?
        .as_array()?
        .iter()
        .find(|entry| entry.get("id").and_then(|id| id.as_str()) == Some(row_id))?
        .get("configRef")?
        .as_str()?;
    let name = reference.rsplit('/').next()?;
    let definition = schema.get("$defs")?.get(name)?;
    let mut properties = serde_json::Map::new();
    let mut absorb = |node: &serde_json::Value| {
        if let Some(map) = node.get("properties").and_then(|value| value.as_object()) {
            for (key, value) in map {
                properties.insert(key.clone(), value.clone());
            }
        }
    };
    absorb(definition);
    if let Some(variants) = definition.get("anyOf").and_then(|value| value.as_array()) {
        for variant in variants {
            absorb(variant);
        }
    }
    Some(properties)
}

/// The provider's own catalogue, as the entry schema states it.
fn schema_catalogue(
    properties: &serde_json::Map<String, serde_json::Value>,
) -> Vec<DshModelOption> {
    let Some(defaults) = properties
        .get("models")
        .and_then(|models| models.get("default"))
        .and_then(|value| value.as_array())
    else {
        return Vec::new();
    };
    defaults
        .iter()
        .filter_map(|model| {
            let value = model.get("id").and_then(|id| id.as_str())?.to_string();
            Some(DshModelOption {
                value,
                label: model
                    .get("name")
                    .and_then(|name| name.as_str())
                    .map(str::to_string),
                reasoning_declared: false,
                reasoning_efforts: Vec::new(),
                default_reasoning_effort: None,
            })
        })
        .collect()
}

/// Every value a field enumerates, in schema order.
fn enum_values(field: &serde_json::Value) -> Vec<String> {
    let mut values = Vec::new();
    collect_consts(field, &mut values);
    values
}

fn collect_consts(node: &serde_json::Value, values: &mut Vec<String>) {
    match node {
        serde_json::Value::Object(map) => {
            if let Some(constant) = map.get("const").and_then(|value| value.as_str()) {
                if !values.iter().any(|existing| existing == constant) {
                    values.push(constant.to_string());
                }
            }
            for value in map.values() {
                collect_consts(value, values);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                collect_consts(item, values);
            }
        }
        _ => {}
    }
}

/// Composes the headless profile exactly the way a run would, and reads back
/// everything Relay needs from it.
async fn read_composition(
    executable: &str,
    prefix: &[String],
    env: &[(String, String)],
) -> Option<DshComposition> {
    let mut args = prefix.to_vec();
    args.extend(["--profile".to_string(), "headless".to_string()]);
    let mut dump_args = args.clone();
    dump_args.push("--dump-config".to_string());
    let (code, stdout, _) = capture_with(executable, &dump_args, env, DUMP_TIMEOUT).await?;
    if code != 0 {
        return None;
    }
    let mut composition = parse_composition(&stdout);
    // The schema is where dsh states what the route really accepts. A version
    // without the flag, or one that fails to compose it, simply leaves the
    // catalogue the dump declared.
    let mut schema_args = args;
    schema_args.push("--dump-config-schema".to_string());
    if let Some((code, schema, _)) =
        capture_with(executable, &schema_args, env, SCHEMA_TIMEOUT).await
    {
        if code == 0 {
            apply_schema(&mut composition, &schema);
        }
    }
    Some(composition)
}

/// Writes the one-run overlay that makes an Agent Profile real.
///
/// The launcher applies --patch files after the profile layer, and a patch
/// replaces a row's whole config object, so Relay only ever writes rows it can
/// restate completely: the default model selection (provider, model and the
/// optional reasoningEffort), and the system-prompt persona when the profile's
/// own row is exactly that persona. Every value is a double-quoted YAML scalar
/// with escaped newlines, so a model id or instruction text can never change the
/// shape of the document.
fn write_overlay(
    selection: Option<&DshSelection>,
    persona: Option<&DshPersona>,
    worker_session_id: &str,
) -> std::io::Result<Option<PathBuf>> {
    if selection.is_none() && persona.is_none() {
        return Ok(None);
    }
    let mut body = String::from("# Written by Relay for a single run.\n");
    if let Some(selection) = selection {
        body.push_str("- id: agent-default-model\n  config:\n    provider: ");
        body.push_str(&quoted(&selection.provider));
        body.push_str("\n    model: ");
        body.push_str(&quoted(&selection.model));
        if let Some(effort) = &selection.reasoning_effort {
            body.push_str("\n    reasoningEffort: ");
            body.push_str(&quoted(effort));
        }
        body.push('\n');
    }
    if let Some(persona) = persona {
        body.push_str("- id: system-prompt\n  config:\n    personaPrefix: ");
        body.push_str(&quoted(&persona.prefix));
        body.push_str("\n    personaSuffix: ");
        body.push_str(&quoted(&persona.suffix));
        body.push('\n');
    }
    let directory = std::env::temp_dir().join("relay-run-overlays");
    std::fs::create_dir_all(&directory)?;
    // The run's own identity names its overlay, and the file is created rather
    // than replaced: two runs that start in the same microsecond must never share
    // one --patch file, or one of them would be started with the other's model,
    // reasoning effort and persona.
    let key: String = worker_session_id
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .take(64)
        .collect();
    for attempt in 0..8 {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|value| value.as_nanos())
            .unwrap_or(0);
        let path = directory.join(format!(
            "run-{}-{key}-{stamp}-{attempt}.yml",
            std::process::id()
        ));
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(mut file) => {
                file.write_all(body.as_bytes())?;
                return Ok(Some(path));
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::AlreadyExists,
        "cannot create a unique run overlay",
    ))
}

/// The panel's shape for one effort list: ordered, labelled, and never invented.
fn levels(efforts: &[String]) -> Vec<ReasoningLevel> {
    efforts
        .iter()
        .enumerate()
        .map(|(index, value)| ReasoningLevel {
            strength: (index + 1) as u8,
            label: effort_label(value),
            value: value.clone(),
        })
        .collect()
}

fn effort_label(value: &str) -> String {
    let mut characters = value.chars();
    match characters.next() {
        Some(first) => first.to_uppercase().collect::<String>() + characters.as_str(),
        None => String::new(),
    }
}

/// A YAML double-quoted scalar: the only shape Relay writes.
fn quoted(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len() + 2);
    escaped.push('"');
    for character in value.chars() {
        match character {
            '"' => escaped.push_str("\\\""),
            '\\' => escaped.push_str("\\\\"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            other => escaped.push(other),
        }
    }
    escaped.push('"');
    escaped
}

/// Maps one native line into Relay events. Public so it can be tested directly.
pub fn parse_line(line: &str) -> ParsedOutput {
    let mut parsed = ParsedOutput::default();
    let value: serde_json::Value = match serde_json::from_str(line) {
        Ok(value) => value,
        Err(_) => {
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::WorkerMessage,
                serde_json::json!({ "kind": "diagnostic", "message": "DeepSeek Harness emitted malformed JSON" }),
                serde_json::Value::String(line.chars().take(8_192).collect()),
            ));
            return parsed;
        }
    };
    let Some(object) = value.as_object() else {
        parsed.events.push(AdapterEvent::with_native(
            RelayEventType::WorkerMessage,
            serde_json::json!({ "kind": "diagnostic", "message": "DeepSeek Harness emitted an invalid event" }),
            value,
        ));
        return parsed;
    };
    let event_type = object
        .get("type")
        .and_then(|value| value.as_str())
        .unwrap_or_default();

    match event_type {
        "session" => {
            parsed.session_id = object
                .get("sessionId")
                .and_then(|value| value.as_str())
                .map(str::to_string);
        }
        "thinking" => {
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::WorkerReasoning,
                serde_json::json!({ "text": object.get("text").cloned().unwrap_or(serde_json::Value::String(String::new())) }),
                value,
            ));
        }
        "text" => {
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::WorkerMessage,
                serde_json::json!({ "text": object.get("text").cloned().unwrap_or(serde_json::Value::String(String::new())) }),
                value,
            ));
        }
        "status" => {
            let reason_kind =
                if object.get("phase").and_then(|value| value.as_str()) == Some("turn_end") {
                    object
                        .get("reason")
                        .and_then(|reason| reason.get("kind"))
                        .and_then(|kind| kind.as_str())
                        .map(str::to_string)
                } else {
                    None
                };
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::WorkerMessage,
                serde_json::json!({
                    "kind": "status",
                    "phase": object.get("phase").cloned().unwrap_or(serde_json::Value::Null),
                    "turn": object.get("turn").cloned().unwrap_or(serde_json::Value::Null),
                    "step": object.get("step").cloned().unwrap_or(serde_json::Value::Null),
                    "reason": object.get("reason").cloned().unwrap_or(serde_json::Value::Null),
                    "usage": object.get("usage").cloned().unwrap_or(serde_json::Value::Null),
                }),
                value,
            ));
            parsed.turn_end_kind = reason_kind;
        }
        "tool_call" => {
            let tool = object
                .get("tool")
                .and_then(|value| value.as_str())
                .unwrap_or("unknown")
                .to_string();
            parsed.events.push(AdapterEvent::with_native(
                tool_event_type(&tool),
                serde_json::json!({
                    "callId": object.get("callId").cloned().unwrap_or(serde_json::Value::Null),
                    "tool": tool,
                    "input": object.get("input").cloned().unwrap_or(serde_json::Value::Null),
                }),
                value,
            ));
        }
        "tool_result" => {
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::ToolResult,
                serde_json::json!({
                    "callId": object.get("callId").cloned().unwrap_or(serde_json::Value::Null),
                    "status": object.get("status").cloned().unwrap_or(serde_json::Value::Null),
                    "result": object.get("result").cloned().unwrap_or(serde_json::Value::Null),
                }),
                value,
            ));
        }
        "final" => {
            let text = object
                .get("text")
                .and_then(|value| value.as_str())
                .unwrap_or_default()
                .to_string();
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::WorkerMessage,
                serde_json::json!({ "kind": "final", "text": text }),
                value,
            ));
            parsed.final_text = Some(text);
        }
        "error" => {
            let message = object
                .get("message")
                .and_then(|value| value.as_str())
                .unwrap_or("DeepSeek Harness failed")
                .to_string();
            parsed.events.push(AdapterEvent::with_native(
                RelayEventType::WorkerMessage,
                serde_json::json!({ "kind": "diagnostic", "level": "error", "message": message }),
                value,
            ));
            parsed.error_message = Some(message);
        }
        _ => {}
    }
    parsed
}

fn tool_event_type(tool: &str) -> RelayEventType {
    let name = tool.to_lowercase();
    if ["search", "grep", "glob", "find"]
        .iter()
        .any(|needle| name.contains(needle))
    {
        RelayEventType::ToolSearch
    } else if ["edit", "write", "patch", "replace", "delete", "move"]
        .iter()
        .any(|needle| name.contains(needle))
    {
        RelayEventType::ToolEdit
    } else if ["read", "view", "open", "list"]
        .iter()
        .any(|needle| name.contains(needle))
    {
        RelayEventType::ToolRead
    } else {
        RelayEventType::ToolCommand
    }
}

fn terminal_event(outcome: &StreamOutcome) -> AdapterEvent {
    let completed = outcome.exit_code == Some(0)
        && outcome.error_message.is_none()
        && outcome.spawn_error.is_none()
        && outcome
            .turn_end_kind
            .as_deref()
            .map(|kind| kind == "completed")
            .unwrap_or(true);
    if completed {
        AdapterEvent::new(
            RelayEventType::WorkerCompleted,
            serde_json::json!({
                "summary": outcome.final_text.clone().unwrap_or_default(),
                "exitCode": outcome.exit_code,
            }),
        )
    } else {
        let message = outcome
            .error_message
            .clone()
            .or_else(|| {
                let tail = outcome.stderr_tail.trim();
                (!tail.is_empty()).then(|| tail.to_string())
            })
            .unwrap_or_else(|| "DeepSeek Harness exited unsuccessfully".to_string());
        AdapterEvent::new(
            RelayEventType::WorkerFailed,
            serde_json::json!({
                "message": message,
                "exitCode": outcome.exit_code,
                "signal": outcome.signal,
                "turnEndKind": outcome.turn_end_kind,
            }),
        )
    }
}

#[derive(Debug, Clone, Copy)]
struct Features {
    json: bool,
    resume: bool,
}

impl Default for Features {
    fn default() -> Self {
        Self {
            json: true,
            resume: true,
        }
    }
}

/// A run's task plus the overlay that carries its per-run choices.
struct PreparedRun {
    task: String,
    overlay: Option<PathBuf>,
}

pub struct DeepSeekAdapter {
    configured_executable: Option<String>,
    prefix_args: Vec<String>,
    environment: Vec<(String, String)>,
    supervisor: Arc<ProcessSupervisor>,
    features: Mutex<Features>,
    options: Mutex<Option<RuntimeOptions>>,
}

impl Default for DeepSeekAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl DeepSeekAdapter {
    pub fn new() -> Self {
        Self {
            configured_executable: None,
            prefix_args: Vec::new(),
            environment: Vec::new(),
            supervisor: Arc::new(ProcessSupervisor::new()),
            features: Mutex::new(Features::default()),
            options: Mutex::new(None),
        }
    }

    pub fn with_executable(executable: impl Into<String>) -> Self {
        Self {
            configured_executable: Some(executable.into()),
            ..Self::new()
        }
    }

    pub fn with_prefix_args(mut self, args: Vec<String>) -> Self {
        self.prefix_args = args;
        self
    }

    pub fn with_environment(mut self, env: Vec<(String, String)>) -> Self {
        self.environment = env;
        self
    }

    pub fn with_supervisor(mut self, supervisor: Arc<ProcessSupervisor>) -> Self {
        self.supervisor = supervisor;
        self
    }

    fn environment_value(&self, name: &str) -> Option<String> {
        self.environment
            .iter()
            .rev()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
            .or_else(|| std::env::var(name).ok())
    }

    fn dsh_home(&self) -> Option<PathBuf> {
        let home = dirs::home_dir()?;
        let configured = self.environment_value("DSH_HOME");
        let path = configured
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".dsh"));
        let expanded = if path == Path::new("~") {
            home
        } else if let Ok(relative) = path.strip_prefix("~") {
            home.join(relative)
        } else {
            path
        };
        if expanded.is_absolute() {
            Some(expanded)
        } else {
            std::env::current_dir().ok().map(|cwd| cwd.join(expanded))
        }
    }

    fn model_api_key(&self, composition: &DshComposition) -> Option<String> {
        let reference = composition
            .api_key_env
            .as_deref()
            .unwrap_or(DEFAULT_API_KEY_REF);
        if !credential_reference(reference) {
            return None;
        }
        if let Some(value) = self
            .environment_value(reference)
            .filter(|value| !value.is_empty())
        {
            return Some(value);
        }
        stored_api_key(&self.dsh_home()?.join(CREDENTIAL_FILE), reference)
    }

    /// `dsh` may be installed through npx rather than on `PATH`.
    fn executable(&self) -> Option<String> {
        if let Some(configured) = &self.configured_executable {
            return Some(configured.clone());
        }
        let mut extra = Vec::new();
        if let Some(home) = std::env::var_os("HOME") {
            let npx_root = PathBuf::from(home).join(".npm").join("_npx");
            if let Ok(entries) = std::fs::read_dir(&npx_root) {
                let mut candidates: Vec<(std::time::SystemTime, PathBuf)> = entries
                    .flatten()
                    .map(|entry| entry.path().join("node_modules").join(".bin").join("dsh"))
                    .filter(|path| path.is_file())
                    .filter_map(|path| {
                        let modified = path.metadata().ok()?.modified().ok()?;
                        Some((modified, path))
                    })
                    .collect();
                candidates.sort_by_key(|candidate| std::cmp::Reverse(candidate.0));
                extra.extend(candidates.into_iter().map(|(_, path)| path));
            }
        }
        discover_executable("dsh", &extra).map(|path| path.to_string_lossy().to_string())
    }

    /// Probes `--help` once so model/reasoning flags are only ever sent when the
    /// CLI advertises them.
    async fn refresh_options(&self, executable: &str) -> RuntimeOptions {
        let mut prefix = self.prefix_args.clone();
        prefix.push("--profile".to_string());
        prefix.push("headless".to_string());
        let evidence = read_help_with(executable, &prefix, &self.environment).await;
        self.set_features(&evidence.text);
        let (_, options) =
            probe_runtime_options(self.capabilities(), &evidence, RUNTIME_ID, ADAPTER_ID);
        *self.options.lock().unwrap() = Some(options.clone());
        options
    }

    fn set_features(&self, help: &str) {
        *self.features.lock().unwrap() = Features {
            json: help.contains("--json"),
            resume: help.contains("--session-id"),
        };
    }

    fn features(&self) -> Features {
        *self.features.lock().unwrap()
    }

    fn selection(&self, input: &StartInput) -> Selection {
        Selection {
            model: input.model.clone(),
            reasoning: input.reasoning.clone(),
        }
    }

    /// The environment a run starts with.
    ///
    /// Relay's access mode is not advice for this runtime: the sandbox row reads
    /// DSH_PERMISSION_MODE, so a read-only run really is read-only. An explicit
    /// deployment override in the adapter's own environment wins.
    fn run_environment(&self, access_mode: relay_core::AccessMode) -> Vec<(String, String)> {
        let mut environment = self.environment.clone();
        if environment
            .iter()
            .any(|(key, _)| key == "DSH_PERMISSION_MODE")
        {
            return environment;
        }
        let mode = match access_mode {
            relay_core::AccessMode::ReadOnly | relay_core::AccessMode::Propose => "read-only",
            relay_core::AccessMode::Write => "workspace-write",
        };
        environment.push(("DSH_PERMISSION_MODE".to_string(), mode.to_string()));
        environment
    }

    /// Everything one run needs beyond its bare task.
    ///
    /// DeepSeek Harness applies per-run choices through a --patch overlay, so this
    /// is the whole mechanism behind the model picker and behind the profile's
    /// instructions: both become rows the launcher layers over the profile.
    async fn prepare_run(&self, executable: &str, input: &StartInput) -> Result<PreparedRun> {
        let trimmed = |value: Option<&String>| {
            value
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        let model = trimmed(input.model.as_ref());
        let reasoning = trimmed(input.reasoning.as_ref());
        let instructions = trimmed(input.instructions.as_ref());
        if model.is_none() && reasoning.is_none() && instructions.is_none() {
            return Ok(PreparedRun {
                task: input.task.clone(),
                overlay: None,
            });
        }

        let composition = read_composition(executable, &self.prefix_args, &self.environment).await;

        // A model and a reasoning effort are one selection: the row dsh reads --
        // agent-default-model -- requires a provider and a model and takes an
        // optional reasoningEffort. A choice the runtime cannot apply is refused
        // here instead of being launched and silently ignored.
        let mut selection = None;
        if model.is_some() || reasoning.is_some() {
            let Some(composition) = composition.as_ref() else {
                return Err(RelayError::new(
                    "OPERATION_UNSUPPORTED",
                    "This DeepSeek Harness installation did not report a composed configuration, so Relay cannot apply a model selection to it",
                ));
            };
            let Some(provider) = composition.provider.clone() else {
                return Err(RelayError::new(
                    "OPERATION_UNSUPPORTED",
                    "This DeepSeek Harness profile declares no default model selection, so Relay cannot apply a model to it",
                ));
            };
            // A reasoning choice without a model keeps the profile's own model:
            // the row requires one, and choosing another would change the model.
            let model_value = match &model {
                Some(value) => value.clone(),
                None => composition.model.clone().ok_or_else(|| {
                    RelayError::new(
                        "OPERATION_UNSUPPORTED",
                        "This DeepSeek Harness profile names no model, so Relay cannot apply a reasoning effort to it",
                    )
                })?,
            };
            if !composition.models.is_empty() && composition.model_option(&model_value).is_none() {
                return Err(RelayError::new(
                    "INVALID_REQUEST",
                    format!(
                        "This dsh profile does not declare the model {model_value}; Relay will not pass a model the runtime cannot apply"
                    ),
                ));
            }
            if let Some(effort) = &reasoning {
                let allowed = composition.efforts_for(&model_value);
                if !allowed.is_empty() && !allowed.iter().any(|level| level == effort) {
                    return Err(RelayError::new(
                        "INVALID_REQUEST",
                        format!(
                            "dsh does not accept reasoning effort {effort} for {model_value}; it accepts {}",
                            allowed.join(", ")
                        ),
                    ));
                }
            }
            selection = Some(DshSelection {
                provider,
                model: model_value,
                reasoning_effort: reasoning.clone(),
            });
        }

        // Instructions belong in the runtime's own system prompt rather than
        // appended to the task -- but only when Relay can restate the profile's own
        // persona, because a patch replaces that row's whole config object.
        // Otherwise the envelope carries them and the original persona stays.
        let mut task = input.task.clone();
        let mut persona = None;
        if let Some(instructions) = &instructions {
            match composition
                .as_ref()
                .and_then(|composition| composition.persona.as_ref())
            {
                Some(existing) => {
                    let suffix = if existing.suffix.trim().is_empty() {
                        instructions.clone()
                    } else {
                        format!("{}\n\n{instructions}", existing.suffix)
                    };
                    persona = Some(DshPersona {
                        prefix: existing.prefix.clone(),
                        suffix,
                    });
                }
                None => {
                    task = crate::instructions::enveloped(&input.task, Some(instructions));
                }
            }
        }

        let overlay = write_overlay(
            selection.as_ref(),
            persona.as_ref(),
            &input.worker_session_id,
        )
        .map_err(|error| {
            RelayError::new(
                "ADAPTER_FAILURE",
                format!("cannot write the run overlay: {error}"),
            )
        })?;
        Ok(PreparedRun { task, overlay })
    }
    async fn launch(
        &self,
        input: StartInput,
        resume_session_id: Option<String>,
    ) -> Result<WorkerHandle> {
        let executable = input
            .executable_path
            .clone()
            .or_else(|| self.executable())
            .ok_or_else(|| {
                RelayError::new(
                    "ADAPTER_FAILURE",
                    "DeepSeek Harness executable `dsh` was not found",
                )
            })?;
        let options = self.refresh_options(&executable).await;
        let features = self.features();
        let prepared = self.prepare_run(&executable, &input).await?;

        if !features.json {
            return self
                .launch_plain(&executable, input, resume_session_id, prepared)
                .await;
        }

        let mut args = self.prefix_args.clone();
        args.extend(["--profile".to_string(), "headless".to_string()]);
        if let Some(path) = &prepared.overlay {
            args.push("--patch".to_string());
            args.push(path.display().to_string());
        }
        args.push("--json".to_string());
        if let Some(session_id) = &resume_session_id {
            args.push("--session-id".to_string());
            args.push(session_id.clone());
        }
        let args = with_selection_args(&args, &self.selection(&input), &options);

        run_cli(
            StreamSpec {
                executable,
                args,
                cwd: input.cwd.clone(),
                env: self.run_environment(input.access_mode),
                stdin: Some(prepared.task.clone()),
                stdin_gate: None,
                supervisor_key: input.worker_session_id.clone(),
                cleanup: prepared.overlay,
            },
            StreamMode::Lines,
            Arc::new(parse_line),
            Arc::new(terminal_event),
            Arc::clone(&self.supervisor),
            true,
        )
        .await
    }

    async fn launch_plain(
        &self,
        executable: &str,
        input: StartInput,
        resume_session_id: Option<String>,
        prepared: PreparedRun,
    ) -> Result<WorkerHandle> {
        if resume_session_id.is_some() && !self.features().resume {
            return Err(RelayError::new(
                "OPERATION_UNSUPPORTED",
                "This DeepSeek Harness version does not support --session-id",
            ));
        }
        let options = self
            .options
            .lock()
            .unwrap()
            .clone()
            .unwrap_or_else(|| RuntimeOptions::empty(RUNTIME_ID, ADAPTER_ID, "plain mode"));
        let mut args = self.prefix_args.clone();
        args.extend(["--profile".to_string(), "headless".to_string()]);
        if let Some(path) = &prepared.overlay {
            args.push("--patch".to_string());
            args.push(path.display().to_string());
        }
        if let Some(session_id) = &resume_session_id {
            args.push("--session-id".to_string());
            args.push(session_id.clone());
        }
        args.push(prepared.task.clone());
        let args = with_selection_args(&args, &self.selection(&input), &options);

        run_cli(
            StreamSpec {
                executable: executable.to_string(),
                args,
                cwd: input.cwd.clone(),
                env: self.run_environment(input.access_mode),
                stdin: None,
                stdin_gate: None,
                supervisor_key: input.worker_session_id.clone(),
                cleanup: prepared.overlay,
            },
            StreamMode::WholeOutput,
            Arc::new(parse_plain_output),
            Arc::new(terminal_event),
            Arc::clone(&self.supervisor),
            false,
        )
        .await
    }
}

/// Plain-text fallback: everything the CLI printed becomes the final message.
fn parse_plain_output(output: &str) -> ParsedOutput {
    let mut parsed = ParsedOutput::default();
    let text = output.trim();
    if !text.is_empty() {
        parsed.final_text = Some(text.to_string());
        parsed.events.push(AdapterEvent::new(
            RelayEventType::WorkerMessage,
            serde_json::json!({ "kind": "final", "text": text }),
        ));
    }
    parsed
}

#[async_trait]
impl AgentAdapter for DeepSeekAdapter {
    fn id(&self) -> &str {
        ADAPTER_ID
    }

    fn capabilities(&self) -> AdapterCapabilities {
        let features = self.features();
        AdapterCapabilities {
            non_interactive: true,
            structured_events: features.json,
            cwd: true,
            resume: features.resume,
            send: false,
            cancel: true,
            child_sessions: false,
            model_selection: None,
            // DeepSeek Harness has a real sandbox: the sandbox-policy row reads its
            // mode from DSH_PERMISSION_MODE, which is exactly the deployment
            // override Relay maps an access mode onto. It has no way to remove
            // shell or network access, so those stay unenforced.
            enforcement: EnforcementSet {
                workspace: true,
                commands: false,
                network: false,
            },
        }
    }

    async fn detect(&self) -> DetectionResult {
        let Some(executable) = self.executable() else {
            return DetectionResult {
                runtimes: Vec::new(),
                diagnostics: vec!["DeepSeek Harness executable `dsh` was not found".to_string()],
            };
        };
        let mut prefix = self.prefix_args.clone();
        prefix.push("--profile".to_string());
        prefix.push("headless".to_string());
        let evidence = read_help_with(&executable, &prefix, &self.environment).await;
        self.set_features(&evidence.text);
        let version = version_of(&executable, &self.prefix_args).await;
        let capabilities = self.capabilities();

        let mut diagnostics = Vec::new();
        if version.is_none() {
            diagnostics.push("DeepSeek Harness version check failed".to_string());
        } else {
            diagnostics.push(
                "Authentication is validated by DeepSeek Harness when a run starts".to_string(),
            );
            if !capabilities.structured_events {
                diagnostics.push(
                    "This dsh version has no --json stream; Relay will use bounded plain-text mode"
                        .to_string(),
                );
            }
        }

        DetectionResult {
            runtimes: vec![Runtime {
                id: RUNTIME_ID.to_string(),
                adapter_id: ADAPTER_ID.to_string(),
                executable_path: executable,
                version,
                health: RuntimeHealth::Available,
                capabilities,
            }],
            diagnostics,
        }
    }

    /// What a run of this runtime can really apply.
    ///
    /// DeepSeek Harness's headless profile has neither a --model nor a
    /// --reasoning flag -- its own --help proves it -- so Relay never reports a
    /// flag for it. Both choices are applied through one per-run --patch overlay
    /// instead, and both the catalogue and the reasoning levels come from the
    /// profile's own composed configuration and the schema of that composition.
    async fn report_options(&self, runtime: &Runtime) -> RuntimeOptions {
        let (executable, mut diagnostics) = probe_target(runtime, || self.executable());
        let Some(executable) = executable else {
            return RuntimeOptions::empty(&runtime.id, ADAPTER_ID, diagnostics.join("; "));
        };
        let mut options = self.refresh_options(&executable).await;
        options.runtime_id = runtime.id.clone();
        diagnostics.append(&mut options.diagnostics);
        options.diagnostics = diagnostics;

        // Prefer any catalogue or constraints the profile itself exposes. DSH's
        // CLI release in use has no schema dump, so the provider API fills gaps.
        let composition = read_composition(&executable, &self.prefix_args, &self.environment).await;
        match composition.as_ref() {
            Some(composition) => match composition.provider {
                Some(ref provider) if !composition.models.is_empty() => {
                    options.models = composition
                        .models
                        .iter()
                        .map(|model| ModelOption {
                            value: model.value.clone(),
                            label: model.label.clone(),
                            reasoning_levels: levels(&composition.efforts_for(&model.value)),
                            default_reasoning: model.default_reasoning_effort.clone(),
                        })
                        .collect();
                    // The runtime-wide list belongs to the model the profile itself
                    // selects: choosing the runtime default must still show the
                    // levels that run would really accept.
                    let selected = composition.model.clone().unwrap_or_default();
                    options.levels = levels(&composition.efforts_for(&selected));
                    options.source = OptionsSource::Cli;
                    options.diagnostics.push(format!(
                        "DeepSeek Harness applies a model and a reasoning effort through one per-run --patch overlay on the {provider} route, not a command line flag; Relay writes that overlay for every run"
                    ));
                    if !composition.route_efforts.is_empty() {
                        options.diagnostics.push(format!(
                            "The {provider} route accepts the reasoning efforts {}",
                            composition.route_efforts.join(", ")
                        ));
                    }
                }
                Some(ref provider) => options.diagnostics.push(format!(
                    "This dsh profile declares no model catalogue for the {provider} route; Relay will try the provider API"
                )),
                None => options.diagnostics.push(
                    "This dsh profile declares no default model selection, so Relay cannot offer a model list"
                        .to_string(),
                ),
            },
            None => options.diagnostics.push(
                "dsh did not report a composed configuration, so Relay cannot offer a model list"
                    .to_string(),
            ),
        }
        // `dsh --help` has no model or reasoning flags, and this DSH release
        // also has no config-schema dump command. Its per-run --patch overlay
        // does apply both values, so use DeepSeek's documented /models API for
        // the missing catalogue and per-model effort capabilities.
        let provider_key = composition
            .as_ref()
            .and_then(|composition| composition.provider.as_deref())
            .filter(|provider| *provider == "deepseek-official")
            .map(|_| "deepseek");
        let credential =
            provider_key.and_then(|_| composition.as_ref().and_then(|c| self.model_api_key(c)));
        let mut options = with_model_fallback_applicable_with_key(
            options,
            provider_key,
            true,
            credential.as_deref(),
        )
        .await;
        if provider_key.is_some() {
            let selected = composition
                .as_ref()
                .and_then(|composition| composition.model.as_deref());
            add_deepseek_off(&mut options, selected);
        }
        options
    }

    async fn start(&self, input: StartInput) -> Result<WorkerHandle> {
        self.launch(input, None).await
    }

    async fn resume(&self, input: ResumeInput) -> Result<WorkerHandle> {
        let session_id = input.native_session_id.clone();
        // dsh adopts the named session but re-reads its default model selection
        // while it does, so the selection has to travel with the resume. Without
        // it the resumed run would quietly use whatever the profile points at
        // now, which is exactly the "resume changed my model" bug.
        self.launch(
            StartInput {
                run_id: input.run_id,
                worker_session_id: input.worker_session_id,
                task: input.task,
                cwd: input.cwd,
                access_mode: input.access_mode,
                executable_path: input.executable_path,
                model: input.model,
                reasoning: input.reasoning,
                instructions: input.instructions,
            },
            Some(session_id),
        )
        .await
    }

    async fn cancel(&self, native_session_id: &str) -> Result<()> {
        self.supervisor.terminate(native_session_id);
        Ok(())
    }

    async fn dispose(&self) {
        self.supervisor.terminate_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires an installed and authenticated DSH, and calls the official models API"]
    async fn installed_dsh_reports_models_from_its_saved_credential() {
        let adapter = DeepSeekAdapter::new();
        let executable = adapter.executable().expect("dsh must be installed");
        let options = adapter.report_options(&runtime_with(&executable)).await;
        assert!(
            !options.models.is_empty(),
            "DSH model discovery failed: {:?}",
            options.diagnostics
        );
        assert_eq!(options.source, OptionsSource::Api);
        assert!(options
            .models
            .iter()
            .any(|model| model.reasoning_levels.len() > 1));
    }

    #[cfg(unix)]
    #[test]
    fn model_catalogue_reuses_dshs_owner_only_credential_store() {
        use std::os::unix::fs::PermissionsExt;

        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(CREDENTIAL_FILE);
        std::fs::write(
            &path,
            "version: 1\nrefs:\n  DEEPSEEK_API_KEY: 'local-test-key'\nrecords: {}\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let adapter = DeepSeekAdapter::new().with_environment(vec![
            ("DSH_HOME".to_string(), home.path().display().to_string()),
            (DEFAULT_API_KEY_REF.to_string(), String::new()),
        ]);
        let composition = parse_composition(DUMP_WITHOUT_CATALOGUE);

        assert_eq!(
            adapter.model_api_key(&composition).as_deref(),
            Some("local-test-key")
        );

        let overridden = DeepSeekAdapter::new().with_environment(vec![
            ("DSH_HOME".to_string(), home.path().display().to_string()),
            (
                DEFAULT_API_KEY_REF.to_string(),
                "environment-key".to_string(),
            ),
        ]);
        assert_eq!(
            overridden.model_api_key(&composition).as_deref(),
            Some("environment-key"),
            "DSH gives the inherited environment priority over its store"
        );

        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(adapter.model_api_key(&composition), None);
    }

    #[cfg(unix)]
    #[test]
    fn model_catalogue_honours_a_custom_dsh_credential_reference() {
        use std::os::unix::fs::PermissionsExt;

        let home = tempfile::tempdir().unwrap();
        let path = home.path().join(CREDENTIAL_FILE);
        std::fs::write(
            &path,
            "version: 1\nrefs:\n  CUSTOM_DEEPSEEK_KEY: custom-key\n",
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let adapter = DeepSeekAdapter::new().with_environment(vec![
            ("DSH_HOME".to_string(), home.path().display().to_string()),
            ("CUSTOM_DEEPSEEK_KEY".to_string(), String::new()),
        ]);
        let composition = parse_composition(
            "- id: agent-default-model\n  config:\n    provider: deepseek-official\n    model: deepseek-flash\n- id: llm-deepseek\n  config:\n    apiKeyEnv: CUSTOM_DEEPSEEK_KEY\n",
        );
        assert_eq!(
            composition.api_key_env.as_deref(),
            Some("CUSTOM_DEEPSEEK_KEY")
        );
        assert_eq!(
            adapter.model_api_key(&composition).as_deref(),
            Some("custom-key")
        );
    }

    #[test]
    fn dsh_options_keep_efforts_per_model_and_add_the_runtime_off_choice() {
        let level = |value: &str| ReasoningLevel {
            strength: 1,
            label: value.to_string(),
            value: value.to_string(),
        };
        let mut options = RuntimeOptions {
            runtime_id: RUNTIME_ID.to_string(),
            adapter_id: ADAPTER_ID.to_string(),
            models: vec![
                ModelOption {
                    value: "reasoning-model".to_string(),
                    label: None,
                    reasoning_levels: vec![level("high"), level("max")],
                    default_reasoning: Some("high".to_string()),
                },
                ModelOption::new("plain-model", None),
            ],
            levels: vec![level("low")],
            model_flag: None,
            reasoning_flag: None,
            source: OptionsSource::Api,
            diagnostics: Vec::new(),
        };

        add_deepseek_off(&mut options, Some("reasoning-model"));

        assert_eq!(
            options.models[0]
                .reasoning_levels
                .iter()
                .map(|level| level.value.as_str())
                .collect::<Vec<_>>(),
            vec!["off", "high", "max"]
        );
        assert_eq!(
            options.models[1].reasoning_levels[0].value, "off",
            "DSH exposes off even when the model has no effort levels"
        );
        assert_eq!(options.levels, options.models[0].reasoning_levels);
    }

    #[test]
    fn the_documented_stream_maps_to_stable_relay_events() {
        let session = parse_line(r#"{"type":"session","sessionId":"session-1","cwd":"/tmp"}"#);
        assert_eq!(session.session_id.as_deref(), Some("session-1"));
        assert!(session.events.is_empty());

        let tool = parse_line(
            r#"{"type":"tool_call","callId":"c1","tool":"str_replace_editor","input":{"path":"a.ts"}}"#,
        );
        assert_eq!(tool.events[0].event_type, RelayEventType::ToolEdit);
        assert_eq!(tool.events[0].data["callId"], "c1");

        let status = parse_line(
            r#"{"type":"status","phase":"turn_end","turn":1,"reason":{"kind":"max-tokens"}}"#,
        );
        assert_eq!(status.turn_end_kind.as_deref(), Some("max-tokens"));
        assert_eq!(status.events[0].event_type, RelayEventType::WorkerMessage);
        assert_eq!(status.events[0].data["kind"], "status");
    }

    #[test]
    fn thinking_text_and_final_keep_their_meaning() {
        let thinking = parse_line(r#"{"type":"thinking","text":"consider"}"#);
        assert_eq!(
            thinking.events[0].event_type,
            RelayEventType::WorkerReasoning
        );

        let text = parse_line(r#"{"type":"text","text":"hello"}"#);
        assert_eq!(text.events[0].event_type, RelayEventType::WorkerMessage);
        assert_eq!(text.events[0].data["text"], "hello");

        let final_event = parse_line(r#"{"type":"final","text":"done"}"#);
        assert_eq!(final_event.final_text.as_deref(), Some("done"));
        assert_eq!(final_event.events[0].data["kind"], "final");
    }

    #[test]
    fn tool_results_and_errors_are_kept() {
        let result = parse_line(
            r#"{"type":"tool_result","callId":"c1","status":"completed","result":"ok"}"#,
        );
        assert_eq!(result.events[0].event_type, RelayEventType::ToolResult);
        assert_eq!(result.events[0].data["status"], "completed");

        let error = parse_line(r#"{"type":"error","message":"boom"}"#);
        assert_eq!(error.error_message.as_deref(), Some("boom"));
        assert_eq!(error.events[0].data["kind"], "diagnostic");
    }

    #[test]
    fn malformed_output_becomes_a_bounded_diagnostic() {
        let parsed = parse_line("not-json");
        assert_eq!(parsed.events[0].event_type, RelayEventType::WorkerMessage);
        assert_eq!(parsed.events[0].data["kind"], "diagnostic");
        assert!(parsed.events[0].native_event.is_some());
    }

    #[test]
    fn tool_names_choose_the_event_kind() {
        assert_eq!(tool_event_type("read_file"), RelayEventType::ToolRead);
        assert_eq!(tool_event_type("grep_search"), RelayEventType::ToolSearch);
        assert_eq!(tool_event_type("apply_patch"), RelayEventType::ToolEdit);
        assert_eq!(tool_event_type("run_shell"), RelayEventType::ToolCommand);
    }

    #[test]
    fn completion_carries_the_final_text_as_the_summary() {
        let outcome = StreamOutcome {
            exit_code: Some(0),
            final_text: Some("work complete".to_string()),
            turn_end_kind: Some("completed".to_string()),
            ..StreamOutcome::default()
        };
        let event = terminal_event(&outcome);
        assert_eq!(event.event_type, RelayEventType::WorkerCompleted);
        assert_eq!(event.data["summary"], "work complete");
    }

    #[test]
    fn a_non_zero_exit_or_turn_end_failure_reports_failure() {
        let failed_exit = terminal_event(&StreamOutcome {
            exit_code: Some(1),
            stderr_tail: "boom\n".to_string(),
            ..StreamOutcome::default()
        });
        assert_eq!(failed_exit.event_type, RelayEventType::WorkerFailed);
        assert_eq!(failed_exit.data["message"], "boom");

        let failed_turn = terminal_event(&StreamOutcome {
            exit_code: Some(0),
            turn_end_kind: Some("max-tokens".to_string()),
            ..StreamOutcome::default()
        });
        assert_eq!(failed_turn.event_type, RelayEventType::WorkerFailed);
        assert_eq!(failed_turn.data["turnEndKind"], "max-tokens");
    }

    /* ---------------- model selection for DeepSeek Harness ------------- */

    /// The dump shape is what dsh --dump-config actually prints. This fixture is
    /// the shipped headless profile: its persona is a folded block scalar, and the
    /// provider row declares no catalogue at all -- dsh keeps that in code, and the
    /// entry schema is where the CLI states it.
    const DUMP: &str = "\
# == @deepseek-ai/dsh-base
- id: timer
  name: '@deepseek-ai/cordis-plugin-timer'
- id: agent-default-model
  name: '@deepseek-ai/dsh-agent-default-model'
  config:
    provider: deepseek-official
    model: deepseek-flash
- id: llm-deepseek
  name: '@deepseek-ai/dsh-llm-deepseek-api-key'
- id: system-prompt
  name: '@deepseek-ai/dsh-system-prompt'
  config:
    personaPrefix: >-
      You are a coding agent powered by the {{model}} model.
    personaSuffix: Your working directory is {{cwd}}.
# == @deepseek-ai/dsh-headless
- id: headless-runner
  name: '@deepseek-ai/dsh-headless'
";

    /// The composed rows a real dsh prints after Relay's own overlay was applied
    /// to the shipped headless profile: the selection Relay wrote, and the
    /// profile's persona with the instructions appended to it. js-yaml emits that
    /// appended suffix as a literal block scalar, so a second run reads it back
    /// through the same resolver.
    const DUMP_AFTER_A_RELAY_OVERLAY: &str = "\
- id: agent-default-model
  name: '@deepseek-ai/dsh-agent-default-model'
  config:
    provider: deepseek-official
    model: deepseek-v4-pro
    reasoningEffort: max
- id: llm-deepseek
  name: '@deepseek-ai/dsh-llm-deepseek-api-key'
- id: system-prompt
  name: '@deepseek-ai/dsh-system-prompt'
  config:
    personaPrefix: You are a coding agent powered by the {{model}} model.
    personaSuffix: |-
      Your working directory is {{cwd}}.
      Focus on implementation.
";

    /// A profile that declares the catalogue itself, with one model stating its
    /// own levels: the two models must not receive the same list.
    const DUMP_WITH_DECLARED_CATALOGUE: &str = "\
- id: agent-default-model
  config:
    provider: deepseek-official
    model: deepseek-flash
- id: llm-deepseek
  config:
    models:
      - id: deepseek-v4-pro
        name: DeepSeek-V4-Pro
      - id: deepseek-flash
        name: DeepSeek-V41-Flash
        reasoningEfforts:
          off: null
          high: high
          max: max
- id: system-prompt
  config:
    personaPrefix: You are a coding agent powered by the {{model}} model.
    personaSuffix: Your working directory is {{cwd}}.
";

    const DUMP_WITHOUT_CATALOGUE: &str = "\
- id: agent-default-model
  name: '@deepseek-ai/dsh-agent-default-model'
  config:
    provider: deepseek-official
    model: deepseek-flash
- id: llm-deepseek
  name: '@deepseek-ai/dsh-llm-deepseek-api-key'
";

    /// The same profile with a config Relay must not restate: a patch replaces the
    /// row's whole config object, so any extra key means the envelope instead.
    const DUMP_PERSONA_WITH_EXTRA_KEYS: &str = "\
- id: agent-default-model
  config:
    provider: deepseek-official
    model: deepseek-flash
- id: llm-deepseek
  name: '@deepseek-ai/dsh-llm-deepseek-api-key'
- id: system-prompt
  config:
    personaPrefix: You are a coding agent powered by the {{model}} model.
    personaSuffix: Your working directory is {{cwd}}.
    includeRuntimeContext: false
";

    /// The entry schema dsh --dump-config-schema prints, trimmed to the parts
    /// Relay reads: the provider row's catalogue default and its effort enum.
    const SCHEMA: &str = r##"{
  "x-cordis": {
    "entries": [
      { "id": "system-prompt", "name": "@deepseek-ai/dsh-system-prompt", "configRef": "#/$defs/config59" },
      { "id": "llm-deepseek", "name": "@deepseek-ai/dsh-llm-deepseek-api-key", "configRef": "#/$defs/config62" }
    ]
  },
  "$defs": {
    "config62": {
      "anyOf": [
        {
          "properties": {
            "thinking": { "anyOf": [ { "anyOf": [ { "const": "enabled" }, { "const": "disabled" } ] } ] },
            "reasoningEffort": { "anyOf": [ { "anyOf": [
              { "const": "off" }, { "const": "low" }, { "const": "high" }, { "const": "max" }
            ] } ] },
            "models": { "default": [
              { "id": "deepseek-flash", "name": "DeepSeek-V41-Flash" },
              { "id": "deepseek-v4-pro", "name": "DeepSeek-V4-Pro" }
            ] }
          }
        }
      ]
    }
  }
}"##;

    #[test]
    fn a_profile_dump_yields_the_provider_its_model_and_its_catalogue() {
        let composition = parse_composition(DUMP);
        assert_eq!(composition.provider.as_deref(), Some("deepseek-official"));
        assert_eq!(composition.model.as_deref(), Some("deepseek-flash"));
        assert!(composition.models.is_empty(), "the profile declares none");
    }

    #[test]
    fn a_profile_without_a_catalogue_reports_none_until_the_schema_speaks() {
        let mut composition = parse_composition(DUMP_WITHOUT_CATALOGUE);
        assert_eq!(composition.provider.as_deref(), Some("deepseek-official"));
        assert!(composition.models.is_empty());
        apply_schema(&mut composition, SCHEMA);
        assert_eq!(composition.models.len(), 2);
        assert_eq!(composition.route_efforts, vec!["off", "low", "high", "max"]);
    }

    /// A folded persona is resolved, so Relay can restate it instead of losing it.
    #[test]
    fn a_folded_persona_is_resolved_into_a_restatable_form() {
        let composition = parse_composition(DUMP);
        let persona = composition.persona.expect("the persona is restatable");
        assert_eq!(
            persona.prefix,
            "You are a coding agent powered by the {{model}} model."
        );
        assert_eq!(persona.suffix, "Your working directory is {{cwd}}.");
    }

    /// A dump the real CLI produced from Relay's own overlay round-trips: the
    /// literal block scalar js-yaml wrote is resolved, not dropped, and the
    /// selection Relay wrote is read back as the profile's own.
    #[test]
    fn the_dump_a_real_overlay_produces_round_trips() {
        let composition = parse_composition(DUMP_AFTER_A_RELAY_OVERLAY);
        assert_eq!(composition.provider.as_deref(), Some("deepseek-official"));
        assert_eq!(composition.model.as_deref(), Some("deepseek-v4-pro"));
        assert_eq!(composition.reasoning_effort.as_deref(), Some("max"));
        let persona = composition.persona.expect("the persona is restatable");
        assert_eq!(
            persona.prefix,
            "You are a coding agent powered by the {{model}} model."
        );
        assert_eq!(
            persona.suffix,
            "Your working directory is {{cwd}}.\nFocus on implementation."
        );
    }

    /// A row carrying anything else is left alone: a patch would reset those keys.
    #[test]
    fn a_persona_row_with_other_keys_is_not_restatable() {
        let composition = parse_composition(DUMP_PERSONA_WITH_EXTRA_KEYS);
        assert!(composition.persona.is_none());
    }

    /// `defaultReasoningEffort` is not a dsh key, so Relay must not invent one: the
    /// route's own configured effort is the only default it reports.
    #[test]
    fn the_reported_default_is_one_the_profile_states() {
        let mut composition = parse_composition(DUMP_WITHOUT_CATALOGUE);
        apply_schema(&mut composition, SCHEMA);
        assert!(composition
            .models
            .iter()
            .all(|model| model.default_reasoning_effort.is_none()));
    }

    /// The headless profile of the real CLI has no model and no reasoning flag, so
    /// Relay must never claim one — and must never offer what it cannot apply.
    fn headless_help() -> &'static str {
        "Usage: dsh --profile headless [options] [task...]\n\nArguments:\n  task               the task text\n\nOptions:\n  --json             print one JSON event per line\n  --session-id <id>  adopt an existing session\n  -h, --help         show this help\n"
    }

    fn fake_dsh(directory: &std::path::Path, dump: &str) -> String {
        let script = directory.join("dsh");
        let body = "#!/bin/sh\n\
             for arg in \"$@\"; do\n\
               if [ \"$arg\" = \"--dump-config\" ]; then cat \"$DSH_DUMP\"; exit 0; fi\n\
               if [ \"$arg\" = \"--dump-config-schema\" ]; then cat \"$DSH_SCHEMA\"; exit 0; fi\n\
               if [ \"$arg\" = \"--help\" ]; then cat \"$DSH_HELP\"; exit 0; fi\n\
               if [ \"$arg\" = \"--version\" ]; then printf '0.1.5-rc.3\\n'; exit 0; fi\n\
             done\n\
             prev=\"\"\n\
             for arg in \"$@\"; do\n\
               if [ \"$prev\" = \"--patch\" ]; then\n\
                 echo \"PATCH-PATH $arg\" >> \"$DSH_RECORD\"\n\
                 cat \"$arg\" >> \"$DSH_RECORD\"\n\
               fi\n\
               prev=\"$arg\"\n\
             done\n\
             echo \"ARGV $*\" >> \"$DSH_RECORD\"\n\
             echo \"PERMISSION $DSH_PERMISSION_MODE\" >> \"$DSH_RECORD\"\n\
             payload=$(cat)\n\
             echo \"STDIN $payload\" >> \"$DSH_RECORD\"\n\
             printf '%s\\n' '{\"type\":\"session\",\"sessionId\":\"s-1\"}'\n\
             printf '%s\\n' '{\"type\":\"final\",\"text\":\"ok\"}'\n";
        std::fs::write(&script, body).unwrap();
        let mut permissions = std::fs::metadata(&script).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, 0o755);
        std::fs::set_permissions(&script, permissions).unwrap();
        std::fs::write(directory.join("dump.yml"), dump).unwrap();
        std::fs::write(directory.join("schema.json"), SCHEMA).unwrap();
        std::fs::write(directory.join("help.txt"), headless_help()).unwrap();
        script.display().to_string()
    }

    fn fixture_environment(
        directory: &std::path::Path,
        record: &std::path::Path,
    ) -> Vec<(String, String)> {
        let mut environment = fixture_environment_without_schema(directory, record);
        environment.push((
            "DSH_SCHEMA".to_string(),
            directory.join("schema.json").display().to_string(),
        ));
        environment
    }

    /// The same fixture for a dsh that will not compose an entry schema.
    fn fixture_environment_without_schema(
        directory: &std::path::Path,
        record: &std::path::Path,
    ) -> Vec<(String, String)> {
        vec![
            (
                "DSH_DUMP".to_string(),
                directory.join("dump.yml").display().to_string(),
            ),
            (
                "DSH_HELP".to_string(),
                directory.join("help.txt").display().to_string(),
            ),
            ("DSH_RECORD".to_string(), record.display().to_string()),
        ]
    }

    fn runtime_with(executable: &str) -> Runtime {
        Runtime {
            id: RUNTIME_ID.to_string(),
            adapter_id: ADAPTER_ID.to_string(),
            executable_path: executable.to_string(),
            version: None,
            health: RuntimeHealth::Available,
            capabilities: AdapterCapabilities::default(),
        }
    }

    fn start_input(executable: &str, model: Option<&str>) -> StartInput {
        StartInput {
            run_id: "run:1".to_string(),
            worker_session_id: "worker:1".to_string(),
            task: "do it".to_string(),
            cwd: std::env::temp_dir().display().to_string(),
            access_mode: relay_core::AccessMode::ReadOnly,
            executable_path: Some(executable.to_string()),
            model: model.map(str::to_string),
            reasoning: None,
            instructions: None,
        }
    }

    fn with_reasoning(mut input: StartInput, reasoning: &str) -> StartInput {
        input.reasoning = Some(reasoning.to_string());
        input
    }

    /// The registered executable is the one probed — never a second CLI that
    /// discovery happens to find first.
    #[tokio::test]
    async fn options_are_read_from_the_registered_executable() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        // The adapter's own discovery points somewhere useless; the runtime wins.
        let adapter = DeepSeekAdapter::with_executable("/nonexistent/dsh")
            .with_environment(fixture_environment(directory.path(), &record));

        let options = adapter.report_options(&runtime_with(&executable)).await;
        assert_eq!(options.runtime_id, RUNTIME_ID);
        assert_eq!(options.model_flag, None, "the headless CLI has no --model");
        assert_eq!(options.reasoning_flag, None);
        assert_eq!(options.models.len(), 2, "{:?}", options.diagnostics);
        let values: Vec<&str> = options
            .models
            .iter()
            .map(|model| model.value.as_str())
            .collect();
        assert!(values.contains(&"deepseek-flash"), "{values:?}");
        assert!(values.contains(&"deepseek-v4-pro"), "{values:?}");
        // Every offered model states the levels the runtime accepts for it, and
        // the runtime-wide list belongs to the profile's own model.
        assert!(options
            .models
            .iter()
            .all(|model| !model.reasoning_levels.is_empty()));
        assert_eq!(
            options
                .levels
                .iter()
                .map(|level| level.value.as_str())
                .collect::<Vec<_>>(),
            vec!["off", "low", "high", "max"]
        );
        assert!(options
            .diagnostics
            .iter()
            .any(|line| line.contains("--patch overlay")));
        assert!(options
            .diagnostics
            .iter()
            .any(|line| line.contains("accepts the reasoning efforts off, low, high, max")));
    }

    /// A model the profile does not declare must never be launched: that is
    /// exactly the "picker that does nothing" Relay refuses to offer.
    #[tokio::test]
    async fn a_run_refuses_a_model_the_profile_cannot_apply() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let error = adapter
            .start(start_input(&executable, Some("not-a-model")))
            .await
            .unwrap_err();
        assert_eq!(error.code(), "INVALID_REQUEST");
        assert!(error.message().contains("not-a-model"));
    }

    /// The whole point: a model that is offered is a model that is applied. The
    /// overlay reaches the child process, names the profile's own provider, and is
    /// gone once the run ends.
    #[tokio::test]
    async fn a_selected_model_reaches_the_child_process_and_is_cleaned_up() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let handle = adapter
            .start(start_input(&executable, Some("deepseek-v4-pro")))
            .await
            .unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}

        let log = std::fs::read_to_string(&record).unwrap();
        assert!(
            log.contains("--patch"),
            "the overlay never reached dsh: {log}"
        );
        assert!(log.contains("provider: \"deepseek-official\""), "{log}");
        assert!(log.contains("model: \"deepseek-v4-pro\""), "{log}");

        let patch_path = log
            .lines()
            .find_map(|line| line.strip_prefix("PATCH-PATH "))
            .expect("the run recorded the overlay path")
            .trim()
            .to_string();
        assert!(
            !std::path::Path::new(&patch_path).exists(),
            "the overlay outlived the run: {patch_path}"
        );
    }

    /// Instructions are the profile's whole point, so they must reach the child
    /// process — through the runtime's own system prompt, not appended to a task.
    #[tokio::test]
    async fn profile_instructions_reach_the_runtime_system_prompt() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let mut input = start_input(&executable, None);
        input.instructions = Some("You own the engineering work.".to_string());
        let handle = adapter.start(input).await.unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}

        let log = std::fs::read_to_string(&record).unwrap();
        assert!(log.contains("system-prompt"), "{log}");
        assert!(log.contains("You own the engineering work."), "{log}");
        // The profile's own persona survives: Relay restates what it composed
        // instead of replacing it.
        assert!(log.contains("{{cwd}}"), "{log}");
        assert!(log.contains("{{model}}"), "{log}");
        // The task itself is untouched: the instructions are not duplicated.
        assert!(!log.contains("[Relay agent profile instructions]"), "{log}");
    }

    /// Relay's access mode is a real boundary for this runtime: the sandbox row
    /// reads DSH_PERMISSION_MODE, so a read-only run really cannot write.
    #[tokio::test]
    async fn the_access_mode_reaches_the_runtime_sandbox() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let mut read_only = start_input(&executable, None);
        read_only.access_mode = relay_core::AccessMode::ReadOnly;
        let handle = adapter.start(read_only).await.unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}
        let log = std::fs::read_to_string(&record).unwrap();
        assert!(log.contains("PERMISSION read-only"), "{log}");
    }

    #[tokio::test]
    async fn a_write_run_gets_the_workspace_sandbox() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let mut write = start_input(&executable, None);
        write.access_mode = relay_core::AccessMode::Write;
        let handle = adapter.start(write).await.unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}
        let log = std::fs::read_to_string(&record).unwrap();
        assert!(log.contains("PERMISSION workspace-write"), "{log}");
    }

    /// An explicit deployment override is never overridden by Relay's mapping.
    #[test]
    fn an_explicit_permission_override_wins() {
        let adapter = DeepSeekAdapter::new().with_environment(vec![(
            "DSH_PERMISSION_MODE".to_string(),
            "danger-full-access".to_string(),
        )]);
        let environment = adapter.run_environment(relay_core::AccessMode::ReadOnly);
        assert_eq!(environment.len(), 1);
        assert_eq!(environment[0].1, "danger-full-access");
    }

    /// A profile that declares no catalogue still applies what a profile carries.
    #[tokio::test]
    async fn a_profile_without_a_catalogue_still_applies_its_model() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP_WITHOUT_CATALOGUE);
        // A dsh old enough to have no entry schema: nothing to check a model against.
        let adapter = DeepSeekAdapter::with_executable(executable.clone()).with_environment(
            fixture_environment_without_schema(directory.path(), &record),
        );

        let handle = adapter
            .start(start_input(&executable, Some("deepseek-flash")))
            .await
            .unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}
        let log = std::fs::read_to_string(&record).unwrap();
        assert!(log.contains("model: \"deepseek-flash\""), "{log}");
    }

    /// Relay instructions and the profile's own persona are not a choice: the
    /// overlay restates the resolved persona and appends the instructions to it.
    #[tokio::test]
    async fn instructions_keep_the_folded_persona_the_profile_composed() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let mut input = start_input(&executable, None);
        input.instructions = Some("Focus on implementation.".to_string());
        let handle = adapter.start(input).await.unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}

        let log = std::fs::read_to_string(&record).unwrap();
        assert!(log.contains("system-prompt"), "{log}");
        // The persona dsh composed is restated, not replaced by an empty string.
        assert!(
            log.contains(
                "personaPrefix: \"You are a coding agent powered by the {{model}} model.\""
            ),
            "{log}"
        );
        assert!(!log.contains("personaPrefix: \"\""), "{log}");
        assert!(log.contains("{{cwd}}"), "{log}");
        assert!(log.contains("Focus on implementation."), "{log}");
        assert!(!log.contains("[Relay agent profile instructions]"), "{log}");
    }

    /// A row Relay cannot restate is never touched: the instructions travel in the
    /// task envelope instead, and the profile's own prompt stays whole.
    #[tokio::test]
    async fn a_persona_relay_cannot_restate_is_never_overwritten() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP_PERSONA_WITH_EXTRA_KEYS);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let mut input = start_input(&executable, None);
        input.instructions = Some("Focus on implementation.".to_string());
        let handle = adapter.start(input).await.unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}

        let log = std::fs::read_to_string(&record).unwrap();
        assert!(
            !log.contains("system-prompt"),
            "the system-prompt row must not be patched at all: {log}"
        );
        assert!(log.contains("[Relay agent profile instructions]"), "{log}");
        assert!(log.contains("Focus on implementation."), "{log}");
    }

    /// The whole point of the feature: the effort a user picks is the effort the
    /// child process is really started with.
    #[tokio::test]
    async fn a_selected_reasoning_effort_reaches_the_child_process() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let input = with_reasoning(start_input(&executable, Some("deepseek-v4-pro")), "max");
        let handle = adapter.start(input).await.unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}

        let log = std::fs::read_to_string(&record).unwrap();
        assert!(log.contains("provider: \"deepseek-official\""), "{log}");
        assert!(log.contains("model: \"deepseek-v4-pro\""), "{log}");
        assert!(log.contains("reasoningEffort: \"max\""), "{log}");
    }

    /// An effort dsh does not accept for that model is refused, never rounded to
    /// something the runtime would take.
    #[tokio::test]
    async fn an_unsupported_reasoning_effort_is_refused() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let input = with_reasoning(start_input(&executable, Some("deepseek-flash")), "ultra");
        let error = adapter.start(input).await.unwrap_err();
        assert_eq!(error.code(), "INVALID_REQUEST");
        assert!(error.message().contains("ultra"), "{}", error.message());
        assert!(
            !record.exists(),
            "a refused selection must never reach the child process"
        );
    }

    /// Different models, different levels: one model's list must never be used
    /// for another.
    #[tokio::test]
    async fn each_model_reports_the_levels_it_accepts() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP_WITH_DECLARED_CATALOGUE);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let options = adapter.report_options(&runtime_with(&executable)).await;
        let levels_of = |value: &str| {
            options
                .models
                .iter()
                .find(|model| model.value == value)
                .map(|model| {
                    model
                        .reasoning_levels
                        .iter()
                        .map(|level| level.value.as_str())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_else(|| panic!("{value} is not offered: {:?}", options.models))
        };
        assert_eq!(levels_of("deepseek-flash"), vec!["off", "high", "max"]);
        assert_eq!(
            levels_of("deepseek-v4-pro"),
            vec!["off", "low", "high", "max"]
        );
    }

    /// Resuming adopts the named session and re-applies the profile's selection,
    /// so a resumed run cannot drift back to whatever the profile points at now.
    #[tokio::test]
    async fn resuming_reapplies_the_profile_selection() {
        let directory = tempfile::tempdir().unwrap();
        let record = directory.path().join("record.txt");
        let executable = fake_dsh(directory.path(), DUMP);
        let adapter = DeepSeekAdapter::with_executable(executable.clone())
            .with_environment(fixture_environment(directory.path(), &record));

        let handle = adapter
            .resume(ResumeInput {
                run_id: "run:1".to_string(),
                worker_session_id: "worker:2".to_string(),
                native_session_id: "session-1".to_string(),
                task: "carry on".to_string(),
                cwd: std::env::temp_dir().display().to_string(),
                access_mode: relay_core::AccessMode::ReadOnly,
                executable_path: Some(executable.clone()),
                model: Some("deepseek-v4-pro".to_string()),
                reasoning: Some("max".to_string()),
                instructions: None,
            })
            .await
            .unwrap();
        let mut receiver = handle.events;
        while receiver.recv().await.is_some() {}

        let log = std::fs::read_to_string(&record).unwrap();
        assert!(log.contains("--session-id session-1"), "{log}");
        assert!(log.contains("model: \"deepseek-v4-pro\""), "{log}");
        assert!(log.contains("reasoningEffort: \"max\""), "{log}");
    }

    /// One run's overlay is its own. Two runs used to share a file when they
    /// started in the same microsecond, and one of them was then started with the
    /// other's model, reasoning effort and persona.
    #[test]
    fn every_run_gets_its_own_overlay_file() {
        let persona = DshPersona {
            prefix: "persona".to_string(),
            suffix: "instructions".to_string(),
        };
        let first = write_overlay(None, Some(&persona), "worker-1")
            .unwrap()
            .unwrap();
        let second = write_overlay(None, Some(&persona), "worker-2")
            .unwrap()
            .unwrap();
        let third = write_overlay(None, Some(&persona), "worker-1")
            .unwrap()
            .unwrap();
        assert_ne!(first, second);
        assert_ne!(first, third, "the same worker session never reuses a file");
        for path in [&first, &second, &third] {
            let name = path.file_name().unwrap().to_string_lossy().to_string();
            assert!(name.starts_with("run-"), "{name}");
            assert!(
                std::fs::read_to_string(path).unwrap().contains("persona"),
                "{name} was empty"
            );
            let _ = std::fs::remove_file(path);
        }
        assert!(first
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("worker-1"));
    }
}
