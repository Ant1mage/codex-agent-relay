//! Finding CLIs and reading what they say about themselves.
//!
//! Relay must not invent model names or reasoning levels: a picker that cannot
//! affect a run is worse than no picker. Everything here reads the CLI's own
//! output and reports only that.

use std::path::{Path, PathBuf};
use std::time::Duration;

use relay_core::{
    AdapterCapabilities, ModelOption, OptionsSource, ReasoningLevel, Runtime, RuntimeOptions,
};
use tokio::process::Command;

const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

pub fn can_execute(candidate: &Path) -> bool {
    if !candidate.is_file() {
        return false;
    }
    unsafe { libc::access(c_string(candidate).as_ptr(), libc::X_OK) == 0 }
}

fn c_string(path: &Path) -> std::ffi::CString {
    std::ffi::CString::new(path.as_os_str().as_encoded_bytes().to_vec())
        .unwrap_or_else(|_| std::ffi::CString::new("").unwrap())
}

/// Looks a CLI up on `PATH`, then in the extra locations a runtime installs
/// itself into. macOS only: there is no `PATHEXT` dance to do.
pub fn discover_executable(name: &str, extra: &[PathBuf]) -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("PATH") {
        for directory in std::env::split_paths(&path) {
            let candidate = directory.join(name);
            if can_execute(&candidate) {
                return Some(candidate);
            }
        }
    }
    extra
        .iter()
        .find(|candidate| can_execute(candidate))
        .cloned()
}

/// Runs a CLI once and captures stdout+stderr. Used for `--version` and `--help`.
pub async fn capture(executable: &str, args: &[String]) -> Option<(i32, String, String)> {
    capture_within(executable, args, PROBE_TIMEOUT).await
}

/// The same, with an explicit budget. Composing a whole configuration is heavier
/// than answering `--help`, so it gets its own.
pub async fn capture_within(
    executable: &str,
    args: &[String],
    timeout: Duration,
) -> Option<(i32, String, String)> {
    capture_with(executable, args, &[], timeout).await
}

/// The same, with the environment an adapter launches its CLI under.
pub async fn capture_with(
    executable: &str,
    args: &[String],
    env: &[(String, String)],
    timeout: Duration,
) -> Option<(i32, String, String)> {
    let mut command = Command::new(executable);
    command.args(args).kill_on_drop(true);
    for (key, value) in env {
        command.env(key, value);
    }
    let output = tokio::time::timeout(timeout, command.output())
        .await
        .ok()?
        .ok()?;
    Some((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
    ))
}

pub async fn version_of(executable: &str, prefix_args: &[String]) -> Option<String> {
    let mut args = prefix_args.to_vec();
    args.push("--version".to_string());
    let (code, stdout, stderr) = capture(executable, &args).await?;
    if code != 0 {
        return None;
    }
    let text = if stdout.trim().is_empty() {
        stderr
    } else {
        stdout
    };
    let first = text.lines().next().unwrap_or_default().trim().to_string();
    (!first.is_empty()).then_some(first)
}

/// One help snapshot, shared by the capability probe and the options report.
#[derive(Debug, Clone, Default)]
pub struct HelpEvidence {
    pub text: String,
    pub executable_path: String,
}

pub async fn read_help(executable: &str, prefix_args: &[String]) -> HelpEvidence {
    let mut args = prefix_args.to_vec();
    args.push("--help".to_string());
    match capture(executable, &args).await {
        Some((_, stdout, stderr)) => HelpEvidence {
            text: format!("{stdout}\n{stderr}"),
            executable_path: executable.to_string(),
        },
        // A CLI that refuses --help simply reports no options.
        None => HelpEvidence {
            executable_path: executable.to_string(),
            text: String::new(),
        },
    }
}

const MODEL_FLAG_NAMES: [&str; 3] = ["model", "models", "model-name"];
const REASONING_FLAG_NAMES: [&str; 4] = ["reasoning-effort", "reasoning", "thinking", "effort"];

/// Finds `--flag` in help text and collects the values the CLI lists for it,
/// e.g. `--model <model>  Model to use: flash, pro`.
///
/// The boundary after the flag name is checked by hand: `--model` must not match
/// `--model-name`, and the regex crate has no lookaround to express that.
fn find_flag(text: &str, names: &[&str]) -> Option<(String, Vec<String>)> {
    use std::sync::OnceLock;

    static AFTER_COLON: OnceLock<regex::Regex> = OnceLock::new();
    static IN_BRACKETS: OnceLock<regex::Regex> = OnceLock::new();
    static WORD: OnceLock<regex::Regex> = OnceLock::new();
    static PLACEHOLDER: OnceLock<regex::Regex> = OnceLock::new();

    let after_colon = AFTER_COLON.get_or_init(|| regex::Regex::new(r":\s*([^\n]+)$").unwrap());
    let in_brackets =
        IN_BRACKETS.get_or_init(|| regex::Regex::new(r"[\[(]([^)\]]+)[)\]]").unwrap());
    let word = WORD.get_or_init(|| regex::Regex::new(r"[A-Za-z][A-Za-z0-9_.-]*").unwrap());
    let placeholder = PLACEHOLDER
        .get_or_init(|| regex::Regex::new(r"[<\[](?P<name>[A-Za-z0-9_.-]+)[>\]]").unwrap());

    for name in names {
        let needle = format!("--{name}");
        for line in text.lines() {
            let Some(index) = line.find(&needle) else {
                continue;
            };
            let after = &line[index + needle.len()..];
            let boundary = after.is_empty()
                || after.starts_with(|character: char| {
                    character.is_whitespace()
                        || character == '='
                        || character == '<'
                        || character == '['
                });
            if !boundary {
                continue;
            }

            let mut values: Vec<String> = Vec::new();
            // Enumeration forms a CLI might use: "a, b, c", "(a|b|c)", "[a|b]".
            let colon = after_colon
                .captures(line)
                .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()));
            let brackets = in_brackets
                .captures(line)
                .and_then(|captures| captures.get(1).map(|value| value.as_str().to_string()));
            for segment in [colon, brackets].into_iter().flatten() {
                for found in word.find_iter(&segment) {
                    values.push(found.as_str().to_string());
                }
            }
            // A metavariable placeholder such as `<model>` is not a value.
            let placeholder_name = placeholder.captures(line).and_then(|captures| {
                captures
                    .name("name")
                    .map(|value| value.as_str().to_lowercase())
            });
            let mut choices: Vec<String> = Vec::new();
            for value in values {
                if Some(value.to_lowercase()) == placeholder_name {
                    continue;
                }
                if !choices.contains(&value) {
                    choices.push(value);
                }
            }
            return Some((needle, choices));
        }
    }
    None
}

fn parse_models(evidence: &HelpEvidence) -> (Vec<ModelOption>, Option<String>, Vec<String>) {
    let Some((flag, values)) = find_flag(&evidence.text, &MODEL_FLAG_NAMES) else {
        return (
            Vec::new(),
            None,
            vec![format!(
                "{} does not advertise a model flag in --help; Relay cannot offer model selection for it",
                evidence.executable_path
            )],
        );
    };
    if values.is_empty() {
        return (
            Vec::new(),
            Some(flag.clone()),
            vec![format!(
                "{} accepts {flag} but does not list model names in --help; enter one in Settings or rely on the CLI default",
                evidence.executable_path
            )],
        );
    }
    let models = values
        .into_iter()
        .map(|value| ModelOption {
            label: Some(value.clone()),
            value,
        })
        .collect();
    (models, Some(flag), Vec::new())
}

fn parse_reasoning(evidence: &HelpEvidence) -> (Vec<ReasoningLevel>, Option<String>, Vec<String>) {
    let Some((flag, values)) = find_flag(&evidence.text, &REASONING_FLAG_NAMES) else {
        return (
            Vec::new(),
            None,
            vec![format!(
                "{} does not advertise a reasoning flag in --help; Relay cannot offer reasoning control for it",
                evidence.executable_path
            )],
        );
    };
    if values.is_empty() {
        return (
            Vec::new(),
            Some(flag.clone()),
            vec![format!(
                "{} accepts {flag} but does not list reasoning levels in --help",
                evidence.executable_path
            )],
        );
    }
    let levels = values
        .into_iter()
        .take(5)
        .enumerate()
        .map(|(index, value)| ReasoningLevel {
            strength: (index + 1) as u8,
            label: capitalize(&value),
            value,
        })
        .collect();
    (levels, Some(flag), Vec::new())
}

fn capitalize(value: &str) -> String {
    let mut chars = value.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

/// Builds a runtime's reported options from one help snapshot. `modelSelection`
/// is true only when a model flag was found, so the UI can tell "no models"
/// apart from "selection unsupported".
pub fn probe_runtime_options(
    declared: AdapterCapabilities,
    evidence: &HelpEvidence,
    runtime_id: &str,
    adapter_id: &str,
) -> (AdapterCapabilities, RuntimeOptions) {
    let (models, model_flag, mut diagnostics) = parse_models(evidence);
    let (levels, reasoning_flag, reasoning_diagnostics) = parse_reasoning(evidence);
    diagnostics.extend(reasoning_diagnostics);
    let source = if !models.is_empty() || !levels.is_empty() {
        OptionsSource::Cli
    } else {
        OptionsSource::Default
    };
    (
        AdapterCapabilities {
            model_selection: Some(model_flag.is_some()),
            ..declared
        },
        RuntimeOptions {
            runtime_id: runtime_id.to_string(),
            adapter_id: adapter_id.to_string(),
            models,
            levels,
            model_flag,
            reasoning_flag,
            source,
            diagnostics,
        },
    )
}

/// Which executable an options probe must use.
///
/// A registered runtime carries the exact file the user chose — possibly one
/// discovery would never find — so that file is the only thing a probe may run.
/// Discovery is a fallback for a runtime that has no path at all, and it is
/// reported so the panel can say the probe used a different CLI.
pub fn probe_target(
    runtime: &Runtime,
    discovered: impl FnOnce() -> Option<String>,
) -> (Option<String>, Vec<String>) {
    let registered = runtime.executable_path.trim();
    if !registered.is_empty() {
        return (Some(registered.to_string()), Vec::new());
    }
    match discovered() {
        Some(path) => (
            Some(path.clone()),
            vec![format!(
                "Runtime {} has no registered executable; Relay probed {path} instead",
                runtime.id
            )],
        ),
        None => (
            None,
            vec![format!(
                "Runtime {} has no registered executable, so Relay cannot read its options",
                runtime.id
            )],
        ),
    }
}

/// Keeps only the lists this runtime can actually apply.
///
/// Some CLIs advertise a model flag but refuse to list the values for it, and the
/// provider's own API can fill that list in. That is only worth doing when there
/// is a flag to apply the answer with: a picker whose choice never reaches the
/// child process is worse than no picker.
pub fn applicable(options: RuntimeOptions) -> RuntimeOptions {
    let models = if options.model_flag.is_some() {
        options.models
    } else {
        Vec::new()
    };
    let levels = if options.reasoning_flag.is_some() {
        options.levels
    } else {
        Vec::new()
    };
    RuntimeOptions {
        models,
        levels,
        ..options
    }
}

/// Narrows a start/resume input to just the selection fields.
pub struct Selection {
    pub model: Option<String>,
    pub reasoning: Option<String>,
}

/// Appends model and reasoning flags to a CLI argument list.
///
/// Only flags the CLI advertised in its own `--help` are used, and only when the
/// profile actually carries a value.
pub fn with_selection_args(
    args: &[String],
    selection: &Selection,
    probed: &RuntimeOptions,
) -> Vec<String> {
    let mut next = args.to_vec();
    if let (Some(model), Some(flag)) = (&selection.model, &probed.model_flag) {
        next.push(flag.clone());
        next.push(model.clone());
    }
    if let (Some(reasoning), Some(flag)) = (&selection.reasoning, &probed.reasoning_flag) {
        next.push(flag.clone());
        next.push(reasoning.clone());
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(text: &str) -> HelpEvidence {
        HelpEvidence {
            text: text.to_string(),
            executable_path: "/usr/bin/fake".to_string(),
        }
    }

    #[test]
    fn a_model_flag_with_a_list_becomes_options() {
        let text = "Usage: fake [options]\n  --model <model>  Model to use: flash, pro\n";
        let (models, flag, diagnostics) = parse_models(&evidence(text));
        assert_eq!(flag.as_deref(), Some("--model"));
        assert_eq!(models.len(), 2);
        assert_eq!(models[0].value, "flash");
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn a_model_flag_without_a_list_reports_a_diagnostic() {
        let text = "Usage: fake [options]\n  --model <model>  Model to use\n";
        let (models, flag, diagnostics) = parse_models(&evidence(text));
        assert!(models.is_empty());
        assert_eq!(flag.as_deref(), Some("--model"));
        assert_eq!(diagnostics.len(), 1);
    }

    #[test]
    fn no_model_flag_yields_no_models_and_an_explanation() {
        let (models, flag, diagnostics) = parse_models(&evidence("Usage: fake [options]\n"));
        assert!(models.is_empty());
        assert!(flag.is_none());
        assert!(diagnostics[0].contains("does not advertise a model flag"));
    }

    #[test]
    fn model_name_is_not_mistaken_for_model() {
        let text = "  --model-name <name>  Something else\n";
        let (_, flag, _) = parse_models(&evidence(text));
        assert_eq!(flag.as_deref(), Some("--model-name"));
    }

    #[test]
    fn reasoning_levels_are_ordered_and_capped() {
        let text = "  --reasoning <level>  One of: low, medium, high, ultra, max, extreme\n";
        let (levels, flag, _) = parse_reasoning(&evidence(text));
        assert_eq!(flag.as_deref(), Some("--reasoning"));
        assert_eq!(levels.len(), 5);
        assert_eq!(levels[0].strength, 1);
        assert_eq!(levels[0].value, "low");
        assert_eq!(levels[0].label, "Low");
    }

    #[test]
    fn selection_flags_are_only_added_when_advertised() {
        let base = vec!["--profile".to_string(), "headless".to_string()];
        let options = RuntimeOptions {
            runtime_id: "r".into(),
            adapter_id: "a".into(),
            models: Vec::new(),
            levels: Vec::new(),
            model_flag: Some("--model".into()),
            reasoning_flag: None,
            source: OptionsSource::Cli,
            diagnostics: Vec::new(),
        };
        let selection = Selection {
            model: Some("pro".into()),
            reasoning: Some("high".into()),
        };
        let args = with_selection_args(&base, &selection, &options);
        assert_eq!(args, vec!["--profile", "headless", "--model", "pro"]);
    }
}
