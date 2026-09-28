//! The workflows are code, and GitHub has to be able to parse them.
//!
//! b4a9742 shipped a release workflow that could not be parsed at all: every
//! `${{ ... }}` expression was missing its closing brace, and three snippets used
//! Bash array syntax inside an expression (`${{#archives[@]}}` instead of
//! `${#archives[@]}`). GitHub records an unparseable workflow file as a failed
//! run with **zero jobs** on the pushed commit — before any trigger filter, such
//! as the tag filter, is considered — so "the release ran and produced nothing"
//! was really "the release never existed".
//!
//! These tests fail on exactly those shapes, plus the second way this workflow
//! could not release anything: `cargo tauri build` without ever installing the
//! Tauri CLI the macOS runner image does not ship.

use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn workflow(name: &str) -> String {
    let path = repo_root().join(".github/workflows").join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("cannot read {path:?}: {error}"))
}

/// Every `${{ ... }}` in the text, as (line number, payload).
///
/// An expression that is not closed on its own line is reported with an empty
/// payload so the caller can fail on it instead of silently skipping it.
fn expressions(text: &str) -> Vec<(usize, String)> {
    let mut found = Vec::new();
    for (index, line) in text.lines().enumerate() {
        let mut rest = line;
        while let Some(start) = rest.find("${{") {
            let after = &rest[start + 3..];
            match after.find("}}") {
                Some(end) => {
                    found.push((index + 1, after[..end].to_string()));
                    rest = &after[end + 2..];
                }
                None => {
                    found.push((index + 1, String::new()));
                    break;
                }
            }
        }
    }
    found
}

/// Every `run:` step, as (line number of `run:`, script).
///
/// A block scalar is dedented the way YAML dedents it: by the indentation of its
/// first non-empty line, which is what GitHub hands to the shell. An inline
/// `run: cargo test` is simply a one-line script.
fn run_scripts(text: &str) -> Vec<(usize, String)> {
    let lines: Vec<&str> = text.lines().collect();
    let mut blocks = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim_start();
        // A block scalar falls through to the reader below; an inline script is
        // already complete; anything else is not a run step at all.
        if let Some(inline) = trimmed.strip_prefix("run: ") {
            if !matches!(inline.trim(), "|" | "|-" | "|+" | ">" | ">-" | ">+") {
                blocks.push((index + 1, inline.to_string()));
                index += 1;
                continue;
            }
        } else if !trimmed.starts_with("run:") {
            index += 1;
            continue;
        }
        let key_indent = line.len() - trimmed.len();
        let mut body: Vec<&str> = Vec::new();
        let mut next = index + 1;
        while next < lines.len() {
            let candidate = lines[next];
            if candidate.trim().is_empty() {
                body.push("");
                next += 1;
                continue;
            }
            let indent = candidate.len() - candidate.trim_start().len();
            if indent <= key_indent {
                break;
            }
            body.push(candidate);
            next += 1;
        }
        let strip = body
            .iter()
            .filter(|line| !line.trim().is_empty())
            .map(|line| line.len() - line.trim_start().len())
            .min()
            .unwrap_or(0);
        let script = body
            .iter()
            .map(|line| {
                if line.len() >= strip {
                    &line[strip..]
                } else {
                    line.trim_start()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        blocks.push((index + 1, script));
        index = next;
    }
    blocks
}

/// The workflow that exists to publish a release must not be unparseable.
///
/// An expression either closes on its own line or GitHub rejects the whole file;
/// and nothing that is Bash (`${#array[@]}`, `#`, `-eq`) belongs inside one.
#[test]
fn every_github_expression_is_closed_and_is_not_shell_syntax() {
    let mut expression_count = 0;
    for name in ["release.yml", "ci.yml"] {
        let text = workflow(name);
        let found = expressions(&text);
        expression_count += found.len();
        for (line, payload) in found {
            assert!(
                !payload.is_empty(),
                "{name}:{line} has an unterminated ${{ expression; GitHub rejects the whole workflow file for it"
            );
            assert!(
                !payload.contains("[@]"),
                "{name}:{line} puts Bash array syntax inside a GitHub expression: ${{{{{payload}}}}}"
            );
            assert!(
                !payload.trim_start().starts_with('#'),
                "{name}:{line} put a Bash comment inside a GitHub expression: ${{{{{payload}}}}}"
            );
            assert!(
                !payload.contains("-eq") && !payload.contains("-z ") && !payload.contains("-n "),
                "{name}:{line} puts a shell test inside a GitHub expression: ${{{{{payload}}}}}"
            );
            assert!(
                payload
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric()
                        || " ._-'[](),!<>=&|*".contains(character)),
                "{name}:{line} has a character GitHub would reject in an expression: ${{{{{payload}}}}}"
            );
        }
    }
    assert!(
        expression_count > 0,
        "the release workflows should exercise GitHub expressions"
    );
}

/// The release is published for a version tag and for nothing else.
#[test]
fn the_release_runs_only_for_version_tags() {
    let text = workflow("release.yml");
    let lines: Vec<&str> = text.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.trim_end() == "on:")
        .expect("the release workflow must declare its triggers");
    let mut trigger: Vec<String> = Vec::new();
    for line in &lines[start + 1..] {
        if !line.trim().is_empty() && !line.starts_with(char::is_whitespace) {
            break;
        }
        if !line.trim().is_empty() {
            trigger.push(line.trim().to_string());
        }
    }
    assert_eq!(
        trigger,
        vec!["push:", "tags: ['v*']"],
        "a release must be triggered by v* tags alone; a branch or PR trigger would publish unreviewed code"
    );
}

/// A job only exists if GitHub can read the file it lives in: every shell
/// snippet the job runs has to be valid Bash once the expressions are values.
#[test]
fn every_run_block_is_valid_shell_once_expressions_are_values() {
    let directory = tempfile::tempdir().expect("a temp directory");
    for name in ["release.yml", "ci.yml"] {
        let text = workflow(name);
        let blocks = run_scripts(&text);
        assert!(
            !blocks.is_empty(),
            "{name} declares no run steps; this test would no longer be checking anything"
        );
        for (line, script) in blocks {
            let mut substituted = String::new();
            let mut rest = script.as_str();
            while let Some(start) = rest.find("${{") {
                substituted.push_str(&rest[..start]);
                match rest[start..].find("}}") {
                    Some(end) => {
                        substituted.push_str("EXPR");
                        rest = &rest[start + end + 2..];
                    }
                    None => {
                        rest = "";
                        break;
                    }
                }
            }
            substituted.push_str(rest);

            let path = directory.path().join(format!("{name}-{line}.sh"));
            std::fs::write(&path, &substituted).expect("write the script");
            let output = Command::new("bash")
                .arg("-n")
                .arg(&path)
                .output()
                .expect("bash has to be available to validate a shell snippet");
            assert!(
                output.status.success(),
                "{name}:{line} is not valid Bash: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
    }
}

/// The macOS runner image has cargo and rustc but no Tauri CLI, and the app has
/// no package.json to fetch one: `cargo tauri build` needs an install step
/// before it, or the release job dies with "no such command: tauri".
#[test]
fn the_tauri_cli_is_installed_before_it_is_used() {
    for name in ["release.yml", "ci.yml"] {
        let text = workflow(name);
        let install = text
            .find("cargo install tauri-cli")
            .unwrap_or_else(|| panic!("{name} never installs the Tauri CLI it builds with"));
        let build = text
            .find("cargo tauri build")
            .unwrap_or_else(|| panic!("{name} no longer builds the desktop app"));
        assert!(
            install < build,
            "{name} uses cargo tauri before installing the CLI"
        );
    }
}

/// One job, and every step of it is either an action or a command — never both,
/// which is another way a workflow file parses but creates no usable job.
#[test]
fn the_release_job_declares_runnable_steps() {
    let text = workflow("release.yml");
    assert!(text.contains("\njobs:\n"), "the workflow declares no jobs");
    assert!(
        text.contains("\n  release:\n"),
        "the release job has to be named release"
    );
    assert!(
        text.contains("runs-on: macos-14"),
        "the release job has to run on the macOS runner"
    );

    let mut steps = 0;
    let mut uses = 0;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("- name:") {
            steps += 1;
        }
        if trimmed.starts_with("- uses:") {
            uses += 1;
        }
    }
    assert_eq!(uses, 3, "the workflow's actions changed: {uses} uses steps");
    assert!(
        steps >= 8,
        "the release job lost steps: only {steps} named steps"
    );
    for pattern in [
        "cargo test --workspace",
        "cargo build --release -p relayd -p relay-mcp",
        "cargo tauri build --target aarch64-apple-darwin",
    ] {
        assert!(
            text.contains(pattern),
            "the release no longer runs {pattern}"
        );
    }
}

/// Releases are manually installed from GitHub; the app must not require an
/// updater signing key or publish a Tauri updater feed.
#[test]
fn releases_publish_the_dmg_without_tauri_updater_artifacts() {
    let workflow = workflow("release.yml");
    assert!(workflow.contains("bundle/dmg/*.dmg"));
    assert!(!workflow.contains("TAURI_SIGNING_PRIVATE_KEY"));
    assert!(!workflow.contains("TAURI_UPDATER_PUBKEY"));
    assert!(!workflow.contains("latest.json"));
    assert!(!workflow.contains(".app.tar.gz"));

    let config_path = repo_root().join("apps/relay-desktop/src-tauri/tauri.conf.json");
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(config_path).expect("Tauri config exists"))
            .expect("Tauri config is valid JSON");
    assert!(config.get("plugins").is_none());
    assert_ne!(config["bundle"]["createUpdaterArtifacts"], true);
}
