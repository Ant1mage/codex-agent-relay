//! "Copy diagnostics" is the one support action a window-less Relay can still
//! perform. The report is plain text so it can be pasted into an issue without a
//! viewer, and every line comes from the same projection the inspector shows.

use crate::contract::InspectorSnapshot;

pub struct DiagnosticsInput<'a> {
    pub app_version: &'a str,
    pub runtime_version: &'a str,
    pub platform: &'a str,
    pub arch: &'a str,
    pub port: u16,
    pub database_path: &'a str,
    pub snapshot: &'a InspectorSnapshot,
    /// Injected so the report is reproducible in tests.
    pub generated_at: Option<String>,
}

fn count_by_status(values: impl Iterator<Item = String>) -> String {
    let mut counts: std::collections::BTreeMap<String, u32> = std::collections::BTreeMap::new();
    for value in values {
        *counts.entry(value).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .map(|(status, count)| format!("{status} {count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn build_diagnostics_report(input: &DiagnosticsInput<'_>) -> String {
    let snapshot = input.snapshot;
    let runs: Vec<&crate::contract::RunView> = snapshot
        .sessions
        .iter()
        .flat_map(|view| view.runs.iter())
        .collect();
    let workers: Vec<&relay_core::WorkerSession> =
        runs.iter().flat_map(|view| view.workers.iter()).collect();
    let active_workers = workers
        .iter()
        .filter(|worker| worker.status.is_active())
        .count();
    let health = count_by_status(
        snapshot
            .runtimes
            .iter()
            .map(|runtime| health_name(runtime.health)),
    );

    let mut lines: Vec<String> = vec![
        "Relay diagnostics".to_string(),
        format!(
            "Generated: {}",
            input.generated_at.clone().unwrap_or_else(relay_core::now)
        ),
        format!(
            "Relay {} · Rust {} · {} {}",
            input.app_version, input.runtime_version, input.platform, input.arch
        ),
        format!("Daemon: http://127.0.0.1:{}", input.port),
        format!("Database: {}", input.database_path),
        String::new(),
        "State".to_string(),
        format!("Sessions: {}", snapshot.sessions.len()),
        if runs.is_empty() {
            format!("Runs: {}", runs.len())
        } else {
            format!(
                "Runs: {} ({})",
                runs.len(),
                count_by_status(runs.iter().map(|view| status_name(view.run.status)))
            )
        },
        format!("Workers: {} ({active_workers} active)", workers.len()),
        if health.is_empty() {
            format!("Runtimes: {}", snapshot.runtimes.len())
        } else {
            format!("Runtimes: {} ({health})", snapshot.runtimes.len())
        },
        format!(
            "Profiles: {} ({} enabled)",
            snapshot.profiles.len(),
            snapshot
                .profiles
                .iter()
                .filter(|profile| profile.enabled)
                .count()
        ),
        String::new(),
        "Codex integration".to_string(),
    ];
    for check in &snapshot.codex.checks {
        lines.push(format!(
            "{} {} — {}",
            if check.ok { "✓" } else { "✗" },
            check.id.as_str(),
            check.detail
        ));
    }
    lines.push(String::new());
    lines.push("Runtimes".to_string());
    if snapshot.runtimes.is_empty() {
        lines.push("(none detected)".to_string());
    } else {
        for runtime in &snapshot.runtimes {
            lines.push(format!(
                "- {} · {} · {}{}",
                runtime.adapter_id,
                health_name(runtime.health),
                runtime.executable_path,
                runtime
                    .version
                    .as_ref()
                    .map(|version| format!(" · {version}"))
                    .unwrap_or_default()
            ));
        }
    }
    if !snapshot.diagnostics.is_empty() {
        lines.push(String::new());
        lines.push("Adapter diagnostics".to_string());
        for line in &snapshot.diagnostics {
            lines.push(format!("- {line}"));
        }
    }
    format!("{}\n", lines.join("\n"))
}

fn status_name(status: relay_core::RunStatus) -> String {
    serde_json::to_value(status)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn health_name(health: relay_core::RuntimeHealth) -> String {
    serde_json::to_value(health)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::{CodexStatus, InspectorSnapshot};

    #[test]
    fn the_report_describes_the_same_projection_the_inspector_shows() {
        let snapshot = InspectorSnapshot {
            sessions: Vec::new(),
            runtimes: Vec::new(),
            profiles: Vec::new(),
            diagnostics: vec!["dsh was not found".to_string()],
            codex: CodexStatus::unknown(),
            generated_at: relay_core::now(),
        };
        let report = build_diagnostics_report(&DiagnosticsInput {
            app_version: "0.1.0",
            runtime_version: "1.98.1",
            platform: "macos",
            arch: "aarch64",
            port: 7352,
            database_path: "/tmp/relay.sqlite",
            snapshot: &snapshot,
            generated_at: Some("2026-01-01T00:00:00.000Z".to_string()),
        });
        assert!(report.starts_with("Relay diagnostics\n"));
        assert!(report.contains("Generated: 2026-01-01T00:00:00.000Z"));
        assert!(report.contains("Relay 0.1.0 · Rust 1.98.1 · macos aarch64"));
        assert!(report.contains("Runtimes: 0"));
        assert!(report.contains("(none detected)"));
        assert!(report.contains("- dsh was not found"));
        assert!(report.ends_with('\n'));
    }
}
