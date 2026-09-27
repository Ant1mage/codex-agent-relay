//! The Codex integration lifecycle: detect, install, repair, update, remove.
//!
//! Relay enters Codex through a **local plugin marketplace plus an MCP server**.
//! The MCP entry points at Relay's own Rust binary — there is no Node, no
//! `process.execPath` and no Electron runtime to reach.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use relay_api::{
    CodexAction, CodexCheck, CodexCheckId, CodexCheckStatus, CodexIntegration, CodexStatus,
    InstallResult,
};
use sha2::{Digest, Sha256};

use crate::cli::{codex, find_codex_cli, CodexExecutable};

const MARKETPLACE_NAME: &str = "relay";
const PLUGIN_NAME: &str = "relay";

/// Where the shipped integration assets live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelaySources {
    pub root: PathBuf,
    pub plugin_manifest: PathBuf,
    pub hooks: PathBuf,
    pub skill: PathBuf,
}

/// Packaged builds place the tree next to the app bundle; a source checkout is
/// found by walking up from the running executable.
pub fn relay_sources() -> Option<RelaySources> {
    let mut bases: Vec<PathBuf> = Vec::new();
    if let Ok(override_root) = std::env::var("RELAY_INSTALL_ROOT") {
        if !override_root.is_empty() {
            bases.push(PathBuf::from(&override_root).join("integrations/codex"));
            bases.push(PathBuf::from(override_root));
        }
    }
    if let Some(resources) = relay_config::resources_dir() {
        bases.push(resources.join("codex"));
        bases.push(resources.join("integrations/codex"));
    }
    if let Ok(current) = std::env::current_exe() {
        let mut directory = current.parent().map(Path::to_path_buf);
        for _ in 0..6 {
            let Some(candidate) = directory.clone() else {
                break;
            };
            bases.push(candidate.join("integrations/codex"));
            bases.push(candidate.join("Resources/codex"));
            directory = candidate.parent().map(Path::to_path_buf);
        }
    }
    // Source checkout: the crate directory is <root>/crates/relay-codex.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    bases.push(manifest_dir.join("../../integrations/codex"));

    for base in bases {
        let plugin_manifest = base.join("plugin.json");
        let hooks = base.join("hooks/hooks.json");
        let skill = base.join("skills/relay/SKILL.md");
        if plugin_manifest.is_file() && hooks.is_file() && skill.is_file() {
            return Some(RelaySources {
                root: base
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| base.clone()),
                plugin_manifest,
                hooks,
                skill,
            });
        }
    }
    None
}

/// The MCP entry Codex should run: Relay's own binary, next to this one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpCommand {
    pub command: String,
    pub args: Vec<String>,
}

impl McpCommand {
    /// The exact program and arguments Codex's `config.toml` should carry.
    pub fn argv(&self) -> Vec<String> {
        let mut argv = vec![self.command.clone()];
        argv.extend(self.args.clone());
        argv
    }
}

pub fn mcp_executable() -> PathBuf {
    if let Ok(explicit) = std::env::var("RELAY_MCP_ENTRY") {
        if !explicit.is_empty() {
            return PathBuf::from(explicit);
        }
    }
    if let Ok(current) = std::env::current_exe() {
        if let Some(directory) = current.parent() {
            let sibling = directory.join("relay-mcp");
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    if let Some(resources) = relay_config::resources_dir() {
        for candidate in [resources.join("relay-mcp"), resources.join("bin/relay-mcp")] {
            if candidate.is_file() {
                return candidate;
            }
        }
    }
    PathBuf::from("relay-mcp")
}

pub fn desired_mcp_command() -> McpCommand {
    McpCommand {
        command: mcp_executable().display().to_string(),
        args: Vec::new(),
    }
}

/// The interpreter and file an MCP entry must match to count as configured.
fn mcp_entry_file(command: &str, args: &[String]) -> Option<PathBuf> {
    if command.ends_with("relay-mcp") || command.contains("relay-mcp") {
        return Some(PathBuf::from(command));
    }
    args.first().map(PathBuf::from)
}

/// Reads just the relay table out of `config.toml`; enough for a status check.
pub fn read_mcp_entry(contents: &str) -> Option<(String, Vec<String>)> {
    let lines: Vec<&str> = contents.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.trim() == "[mcp_servers.relay]")?;
    let mut command: Option<String> = None;
    let mut args: Vec<String> = Vec::new();
    for line in lines.iter().skip(start + 1) {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("command") {
            if let Some(value) = value.split('=').nth(1) {
                command = Some(value.trim().trim_matches('"').to_string());
            }
        }
        if let Some(value) = trimmed.strip_prefix("args") {
            if let Some(value) = value.split('=').nth(1) {
                args = value
                    .trim()
                    .trim_start_matches('[')
                    .trim_end_matches(']')
                    .split(',')
                    .map(|part| part.trim().trim_matches('"').to_string())
                    .filter(|part| !part.is_empty())
                    .collect();
            }
        }
    }
    command.map(|command| (command, args))
}

/// Relay's plugin version: the app version plus a hash of the installed payload,
/// so an edited skill shows up as `outdated` rather than silently staying behind.
pub fn relay_plugin_version(sources: &RelaySources) -> String {
    let mut hasher = Sha256::new();
    if let Ok(skill) = std::fs::read(&sources.skill) {
        hasher.update(&skill);
    }
    if let Ok(hooks) = std::fs::read(&sources.hooks) {
        hasher.update(&hooks);
    }
    let digest = hasher.finalize();
    let short: String = digest
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect();
    format!("{}+{}", relay_config::relay_version(), short)
}

/// Parses `codex plugin list` output for our own plugin.
///
/// Current Codex prints a dedicated VERSION column, while older versions omit
/// it. Keep both shapes so status is portable, but use the version whenever it
/// is available: "installed, enabled" alone does not prove that the currently
/// materialised Relay plugin was installed.
pub fn parse_installed_plugin(
    output: &str,
) -> Option<(Option<String>, Option<String>, Option<String>)> {
    for line in output.lines() {
        let fields: Vec<&str> = line
            .split("  ")
            .map(str::trim)
            .filter(|field| !field.is_empty())
            .collect();
        if fields.first() != Some(&format!("{PLUGIN_NAME}@{MARKETPLACE_NAME}").as_str()) {
            continue;
        }
        let status = fields.get(1).map(|value| value.to_string());
        let (version, source) = if fields.len() >= 4 {
            (
                fields.get(2).map(|value| value.to_string()),
                fields.last().map(|value| value.to_string()),
            )
        } else {
            (None, fields.get(2).map(|value| value.to_string()))
        };
        return Some((status, version, source));
    }
    None
}

/// Parses `codex plugin marketplace list` output for our marketplace's root.
pub fn parse_marketplace_root(output: &str) -> Option<String> {
    for line in output.lines() {
        let fields: Vec<&str> = line
            .split("  ")
            .map(str::trim)
            .filter(|field| !field.is_empty())
            .collect();
        if fields.first() == Some(&MARKETPLACE_NAME) {
            return fields.get(1).map(|value| value.to_string());
        }
    }
    None
}

/// Expands the portable hook template with this installation's MCP command.
pub fn materialised_hook_document(source: &str, command: &McpCommand) -> Result<String, String> {
    let mut document: serde_json::Value =
        serde_json::from_str(source).map_err(|error| format!("hooks.json 无法解析: {error}"))?;
    let quote = |value: &str| format!("'{}'", value.replace('\'', r#"'"'"'"#));
    let mut parts: Vec<String> = Vec::new();
    parts.push(quote(&command.command));
    parts.extend(command.args.iter().map(|arg| quote(arg)));
    parts.push(quote("--session-end-hook"));
    let hook_command = parts.join(" ");

    let hooks = document
        .get_mut("hooks")
        .and_then(|hooks| hooks.as_object_mut())
        .ok_or_else(|| "hooks.json 缺少 hooks 对象".to_string())?;
    hooks.insert(
        "SessionEnd".to_string(),
        serde_json::json!([
            { "hooks": [ { "type": "command", "command": hook_command, "timeout": 3 } ] }
        ]),
    );
    serde_json::to_string_pretty(&document)
        .map(|body| format!("{body}\n"))
        .map_err(|error| error.to_string())
}

/* ------------------------------------------------------------------ */
/* Checks                                                              */
/* ------------------------------------------------------------------ */

fn probe_codex_cli(executable: Option<&CodexExecutable>) -> CodexCheck {
    match executable {
        None => CodexCheck {
            id: CodexCheckId::CodexCli,
            ok: false,
            status: CodexCheckStatus::Missing,
            detail: "codex not found on PATH or in a known Codex location".to_string(),
            hint: Some("安装 Codex CLI 或 VS Code 扩展后重新检测".to_string()),
        },
        Some(executable) => CodexCheck {
            id: CodexCheckId::CodexCli,
            ok: true,
            status: CodexCheckStatus::Ok,
            detail: if executable.path == "codex" {
                format!("codex · {}", executable.version)
            } else {
                format!("{} · {}", executable.path, executable.version)
            },
            hint: None,
        },
    }
}

fn probe_relay_mcp() -> CodexCheck {
    let config_file = relay_config::codex_home().join("config.toml");
    let missing = || CodexCheck {
        id: CodexCheckId::RelayMcp,
        ok: false,
        status: CodexCheckStatus::Missing,
        detail: config_file.display().to_string(),
        hint: Some("运行\"安装到 Codex\"".to_string()),
    };
    let Ok(contents) = std::fs::read_to_string(&config_file) else {
        return missing();
    };
    let Some((command, args)) = read_mcp_entry(&contents) else {
        return missing();
    };
    let desired = desired_mcp_command();
    if command == desired.command && args == desired.args {
        let entry_file = mcp_entry_file(&command, &args);
        let exists = entry_file
            .as_ref()
            .map(|path| path.is_file())
            .unwrap_or(true);
        if exists {
            return CodexCheck {
                id: CodexCheckId::RelayMcp,
                ok: true,
                status: CodexCheckStatus::Ok,
                detail: format!("{command} {}", args.join(" ")).trim().to_string(),
                hint: None,
            };
        }
        return CodexCheck {
            id: CodexCheckId::RelayMcp,
            ok: false,
            status: CodexCheckStatus::Stale,
            detail: format!("已配置: {command} {}", args.join(" ")),
            hint: Some(format!(
                "入口不存在: {} — 运行\"修复\"",
                entry_file.unwrap().display()
            )),
        };
    }
    CodexCheck {
        id: CodexCheckId::RelayMcp,
        ok: false,
        status: CodexCheckStatus::Stale,
        detail: format!("已配置: {command} {}", args.join(" ")),
        hint: Some(format!(
            "期望: {} {} — 运行\"修复\"",
            desired.command,
            desired.args.join(" ")
        )),
    }
}

fn probe_relay_skill() -> CodexCheck {
    let installed = relay_config::marketplace_root()
        .join("plugins")
        .join(PLUGIN_NAME)
        .join("skills")
        .join(PLUGIN_NAME)
        .join("SKILL.md");
    let legacy = relay_config::codex_home()
        .join("skills")
        .join(PLUGIN_NAME)
        .join("SKILL.md");
    let sources = relay_sources();

    if legacy.is_file() && !installed.is_file() {
        return CodexCheck {
            id: CodexCheckId::RelaySkill,
            ok: false,
            status: CodexCheckStatus::Legacy,
            detail: format!("旧版手工复制: {}", legacy.display()),
            hint: Some("运行\"修复\"迁移到插件安装".to_string()),
        };
    }
    if !installed.is_file() {
        return CodexCheck {
            id: CodexCheckId::RelaySkill,
            ok: false,
            status: CodexCheckStatus::Missing,
            detail: installed.display().to_string(),
            hint: Some("运行\"安装到 Codex\"".to_string()),
        };
    }
    let Some(sources) = sources else {
        return CodexCheck {
            id: CodexCheckId::RelaySkill,
            ok: true,
            status: CodexCheckStatus::Ok,
            detail: installed.display().to_string(),
            hint: None,
        };
    };
    if std::fs::read(&sources.skill).ok() != std::fs::read(&installed).ok() {
        return CodexCheck {
            id: CodexCheckId::RelaySkill,
            ok: false,
            status: CodexCheckStatus::Outdated,
            detail: "已安装的 skill 与当前 Relay 版本不一致".to_string(),
            hint: Some("运行\"更新\"".to_string()),
        };
    }
    CodexCheck {
        id: CodexCheckId::RelaySkill,
        ok: true,
        status: CodexCheckStatus::Ok,
        detail: installed.display().to_string(),
        hint: None,
    }
}

async fn probe_relay_plugin(executable: Option<&CodexExecutable>) -> CodexCheck {
    let Some(executable) = executable else {
        return CodexCheck {
            id: CodexCheckId::RelayPlugin,
            ok: false,
            status: CodexCheckStatus::Missing,
            detail: "需要 codex CLI 才能检测插件".to_string(),
            hint: None,
        };
    };
    let listed = codex(executable, &["plugin", "list"]).await;
    if !listed.ok {
        return CodexCheck {
            id: CodexCheckId::RelayPlugin,
            ok: false,
            status: CodexCheckStatus::Missing,
            detail: "codex plugin list 失败".to_string(),
            hint: Some(listed.text.chars().take(200).collect()),
        };
    }
    let Some((status, version, source)) = parse_installed_plugin(&listed.text) else {
        return CodexCheck {
            id: CodexCheckId::RelayPlugin,
            ok: false,
            status: CodexCheckStatus::Missing,
            detail: format!("{PLUGIN_NAME}@{MARKETPLACE_NAME} 未安装"),
            hint: Some("运行\"安装到 Codex\"".to_string()),
        };
    };
    let status = status.unwrap_or_default();
    if !status.contains("installed") {
        return CodexCheck {
            id: CodexCheckId::RelayPlugin,
            ok: false,
            status: CodexCheckStatus::Missing,
            detail: format!("{PLUGIN_NAME}@{MARKETPLACE_NAME} 未安装"),
            hint: Some("运行\"安装到 Codex\"".to_string()),
        };
    }
    if !status.contains("enabled") {
        return CodexCheck {
            id: CodexCheckId::RelayPlugin,
            ok: false,
            status: CodexCheckStatus::Stale,
            detail: "插件已安装但被禁用".to_string(),
            hint: Some("在 Codex 中启用 relay 插件".to_string()),
        };
    }
    if let Some(expected) = relay_sources().map(|sources| relay_plugin_version(&sources)) {
        if let Some(version) = version.filter(|version| !version.is_empty()) {
            if version != expected {
                return CodexCheck {
                    id: CodexCheckId::RelayPlugin,
                    ok: false,
                    status: CodexCheckStatus::Outdated,
                    detail: format!("已安装 {version}，当前 Relay 需要 {expected}"),
                    hint: Some("运行\"更新\"或\"修复\"".to_string()),
                };
            }
        }
    }
    CodexCheck {
        id: CodexCheckId::RelayPlugin,
        ok: true,
        status: CodexCheckStatus::Ok,
        detail: source.unwrap_or_else(|| format!("{PLUGIN_NAME}@{MARKETPLACE_NAME}")),
        hint: None,
    }
}

fn probe_relay_hooks() -> CodexCheck {
    let hooks_file = relay_config::marketplace_root()
        .join("plugins")
        .join(PLUGIN_NAME)
        .join("hooks")
        .join("hooks.json");
    if !hooks_file.is_file() {
        return CodexCheck {
            id: CodexCheckId::RelayHooks,
            ok: false,
            status: CodexCheckStatus::Missing,
            detail: hooks_file.display().to_string(),
            hint: Some("hooks 随插件安装；运行\"安装到 Codex\"".to_string()),
        };
    }
    let Ok(installed) = std::fs::read_to_string(&hooks_file) else {
        return CodexCheck {
            id: CodexCheckId::RelayHooks,
            ok: false,
            status: CodexCheckStatus::Stale,
            detail: "hooks.json 无法读取".to_string(),
            hint: Some("运行\"修复\"".to_string()),
        };
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&installed) else {
        return CodexCheck {
            id: CodexCheckId::RelayHooks,
            ok: false,
            status: CodexCheckStatus::Stale,
            detail: "hooks.json 无法解析".to_string(),
            hint: Some("运行\"修复\"".to_string()),
        };
    };
    let events: Vec<String> = parsed
        .get("hooks")
        .and_then(|hooks| hooks.as_object())
        .map(|hooks| hooks.keys().cloned().collect())
        .unwrap_or_default();

    if let Some(sources) = relay_sources() {
        if let Ok(source) = std::fs::read_to_string(&sources.hooks) {
            match materialised_hook_document(&source, &desired_mcp_command()) {
                Ok(expected) => {
                    let expected: serde_json::Value =
                        serde_json::from_str(&expected).unwrap_or_default();
                    if expected != parsed {
                        return CodexCheck {
                            id: CodexCheckId::RelayHooks,
                            ok: false,
                            status: CodexCheckStatus::Outdated,
                            detail: "hooks 与当前 Relay 版本不一致".to_string(),
                            hint: Some("运行\"更新\"".to_string()),
                        };
                    }
                }
                Err(error) => {
                    return CodexCheck {
                        id: CodexCheckId::RelayHooks,
                        ok: false,
                        status: CodexCheckStatus::Stale,
                        detail: error,
                        hint: Some("运行\"修复\"".to_string()),
                    }
                }
            }
        }
    }
    CodexCheck {
        id: CodexCheckId::RelayHooks,
        ok: true,
        status: CodexCheckStatus::Ok,
        detail: format!("{} · 首次使用需在 Codex 中信任", events.join(" / ")),
        hint: None,
    }
}

pub async fn codex_status() -> CodexStatus {
    let executable = find_codex_cli().await;
    let checks = vec![
        probe_codex_cli(executable.as_ref()),
        probe_relay_mcp(),
        probe_relay_skill(),
        probe_relay_plugin(executable.as_ref()).await,
        probe_relay_hooks(),
    ];
    let configured = checks.iter().all(|check| check.ok);
    CodexStatus { checks, configured }
}

/* ------------------------------------------------------------------ */
/* Install / remove                                                    */
/* ------------------------------------------------------------------ */

/// Writes the plugin tree Relay asks Codex to install from.
pub fn materialise_plugin() -> Result<(PathBuf, String), String> {
    let sources = relay_sources()
        .ok_or_else(|| "这个构建里没有 Relay 集成源文件，无法安装到 Codex".to_string())?;
    let root = relay_config::marketplace_root();
    let plugin = root.join("plugins").join(PLUGIN_NAME);
    let version = relay_plugin_version(&sources);

    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join(".agents/plugins")).map_err(|error| error.to_string())?;
    std::fs::create_dir_all(plugin.join(".codex-plugin")).map_err(|error| error.to_string())?;

    let marketplace = serde_json::json!({
        "name": MARKETPLACE_NAME,
        "interface": { "displayName": "Relay (local)" },
        "plugins": [{
            "name": PLUGIN_NAME,
            "source": { "source": "local", "path": format!("./plugins/{PLUGIN_NAME}") },
            "policy": { "installation": "AVAILABLE", "authentication": "ON_INSTALL" },
            "category": "Developer Tools",
        }]
    });
    write_json(&root.join(".agents/plugins/marketplace.json"), &marketplace)?;

    let mut manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(&sources.plugin_manifest).map_err(|error| error.to_string())?,
    )
    .map_err(|error| format!("plugin.json 无法解析: {error}"))?;
    let object = manifest
        .as_object_mut()
        .ok_or_else(|| "plugin.json 必须是 JSON object".to_string())?;
    object.insert(
        "name".to_string(),
        serde_json::Value::String(PLUGIN_NAME.to_string()),
    );
    object.insert(
        "version".to_string(),
        serde_json::Value::String(version.clone()),
    );
    // Codex's marketplace loader reads `plugin.json` at the plugin root. The
    // `.codex-plugin` copy keeps compatibility with older local installations.
    write_json(&plugin.join("plugin.json"), &manifest)?;
    write_json(&plugin.join(".codex-plugin/plugin.json"), &manifest)?;

    let hooks_dir = sources
        .hooks
        .parent()
        .ok_or_else(|| "hooks 路径无效".to_string())?;
    copy_tree(hooks_dir, &plugin.join("hooks"))?;
    let source_hooks =
        std::fs::read_to_string(&sources.hooks).map_err(|error| error.to_string())?;
    let materialised = materialised_hook_document(&source_hooks, &desired_mcp_command())?;
    std::fs::write(plugin.join("hooks/hooks.json"), materialised)
        .map_err(|error| error.to_string())?;

    let skills_dir = sources
        .skill
        .parent()
        .and_then(|path| path.parent())
        .ok_or_else(|| "skill 路径无效".to_string())?;
    copy_tree(skills_dir, &plugin.join("skills"))?;
    Ok((root, version))
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let body = serde_json::to_string_pretty(value).map_err(|error| error.to_string())?;
    std::fs::write(path, format!("{body}\n")).map_err(|error| error.to_string())
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    std::fs::create_dir_all(to).map_err(|error| error.to_string())?;
    let entries = std::fs::read_dir(from).map_err(|error| error.to_string())?;
    for entry in entries.flatten() {
        let source = entry.path();
        let destination = to.join(entry.file_name());
        if source.is_dir() {
            copy_tree(&source, &destination)?;
        } else {
            std::fs::copy(&source, &destination).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

/// Installs (or repairs/updates) the Codex side. Every step is idempotent and
/// reported individually.
pub async fn install_codex() -> InstallResult {
    let mut messages: Vec<String> = Vec::new();
    let Some(executable) = find_codex_cli().await else {
        return InstallResult {
            status: codex_status().await,
            messages: vec!["未找到 codex CLI，无法配置 Codex 集成".to_string()],
        };
    };

    let (root, version) = match materialise_plugin() {
        Ok(value) => value,
        Err(message) => {
            return InstallResult {
                status: codex_status().await,
                messages: vec![message],
            }
        }
    };
    messages.push(format!("已生成插件 {version} → {}", root.display()));

    let marketplaces = codex(&executable, &["plugin", "marketplace", "list"]).await;
    let current_root = if marketplaces.ok {
        parse_marketplace_root(&marketplaces.text)
    } else {
        None
    };
    let root_text = root.display().to_string();
    if let Some(current) = &current_root {
        if current != &root_text {
            codex(
                &executable,
                &["plugin", "marketplace", "remove", MARKETPLACE_NAME],
            )
            .await;
            messages.push(format!("已移除指向旧路径的 marketplace: {current}"));
        }
    }
    if current_root.as_deref() != Some(root_text.as_str()) {
        let added = codex(&executable, &["plugin", "marketplace", "add", &root_text]).await;
        messages.push(if added.ok {
            "已注册本地 marketplace".to_string()
        } else {
            format!("marketplace 注册失败: {}", truncate(&added.text, 200))
        });
    }

    let listed = codex(&executable, &["plugin", "list"]).await;
    let installed = if listed.ok {
        parse_installed_plugin(&listed.text)
    } else {
        None
    };
    if installed
        .as_ref()
        .and_then(|(status, _, _)| status.clone())
        .map(|status| status.contains("installed"))
        .unwrap_or(false)
    {
        // Removing first refreshes the cached copy, so a newer plugin version lands.
        codex(
            &executable,
            &[
                "plugin",
                "remove",
                &format!("{PLUGIN_NAME}@{MARKETPLACE_NAME}"),
            ],
        )
        .await;
    }
    let added = codex(
        &executable,
        &[
            "plugin",
            "add",
            &format!("{PLUGIN_NAME}@{MARKETPLACE_NAME}"),
        ],
    )
    .await;
    messages.push(if added.ok {
        "已安装 relay 插件（skill + hooks）".to_string()
    } else {
        format!("插件安装失败: {}", truncate(&added.text, 200))
    });

    let desired = desired_mcp_command();
    let config_file = relay_config::codex_home().join("config.toml");
    let entry = std::fs::read_to_string(&config_file)
        .ok()
        .and_then(|body| read_mcp_entry(&body));
    if !probe_relay_mcp().ok {
        if entry.is_some() {
            codex(&executable, &["mcp", "remove", PLUGIN_NAME]).await;
        }
        let mut args: Vec<&str> = vec!["mcp", "add", PLUGIN_NAME, "--"];
        let command_args = desired.argv();
        for value in &command_args {
            args.push(value);
        }
        let mcp_added = codex(&executable, &args).await;
        messages.push(if mcp_added.ok {
            "已配置 relay MCP server".to_string()
        } else {
            format!("MCP 配置失败: {}", truncate(&mcp_added.text, 200))
        });
    } else {
        messages.push("relay MCP server 已是当前配置".to_string());
    }

    let legacy = relay_config::codex_home().join("skills").join(PLUGIN_NAME);
    if legacy.exists() {
        let _ = std::fs::remove_dir_all(&legacy);
        messages.push("已清理旧版手工复制的 skill".to_string());
    }

    let status = codex_status().await;
    if status.checks.iter().any(|check| {
        matches!(check.id, CodexCheckId::RelayMcp | CodexCheckId::RelayPlugin) && !check.ok
    }) {
        messages.push("Codex 集成修复未完成；请查看失败的检查项".to_string());
    }
    InstallResult { status, messages }
}

/// Undoes everything Relay installed into Codex.
pub async fn remove_codex() -> InstallResult {
    let mut messages: Vec<String> = Vec::new();
    match find_codex_cli().await {
        Some(executable) => {
            let plugin = codex(
                &executable,
                &[
                    "plugin",
                    "remove",
                    &format!("{PLUGIN_NAME}@{MARKETPLACE_NAME}"),
                ],
            )
            .await;
            messages.push(if plugin.ok {
                "已卸载 relay 插件".to_string()
            } else {
                "relay 插件未安装或已卸载".to_string()
            });
            let marketplace = codex(
                &executable,
                &["plugin", "marketplace", "remove", MARKETPLACE_NAME],
            )
            .await;
            messages.push(if marketplace.ok {
                "已移除 relay marketplace".to_string()
            } else {
                "relay marketplace 未注册或已移除".to_string()
            });
            let mcp = codex(&executable, &["mcp", "remove", PLUGIN_NAME]).await;
            messages.push(if mcp.ok {
                "已移除 relay MCP server".to_string()
            } else {
                "relay MCP server 未配置或已移除".to_string()
            });
        }
        None => messages.push("未找到 codex CLI；只能清理本地文件".to_string()),
    }
    let _ = std::fs::remove_dir_all(relay_config::marketplace_root());
    let legacy = relay_config::codex_home().join("skills").join(PLUGIN_NAME);
    if legacy.exists() {
        let _ = std::fs::remove_dir_all(&legacy);
    }
    messages.push("已删除本地插件与旧 skill 副本".to_string());
    InstallResult {
        status: codex_status().await,
        messages,
    }
}

fn truncate(value: &str, max: usize) -> String {
    value.chars().take(max).collect()
}

/// The service the daemon's HTTP layer calls.
#[derive(Default)]
pub struct CodexIntegrationService;

impl CodexIntegrationService {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl CodexIntegration for CodexIntegrationService {
    async fn status(&self) -> CodexStatus {
        codex_status().await
    }

    async fn run(&self, action: CodexAction) -> InstallResult {
        match action {
            CodexAction::Remove => remove_codex().await,
            CodexAction::Install | CodexAction::Repair | CodexAction::Update => {
                install_codex().await
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn the_mcp_entry_parser_reads_only_our_table() {
        let contents = r#"
[mcp_servers.other]
command = "other"

[mcp_servers.relay]
command = "/Applications/Relay.app/Contents/MacOS/relay-mcp"
args = []

[other]
command = "ignored"
"#;
        let (command, args) = read_mcp_entry(contents).unwrap();
        assert_eq!(command, "/Applications/Relay.app/Contents/MacOS/relay-mcp");
        assert!(args.is_empty());
        assert!(read_mcp_entry("[mcp_servers.other]\ncommand = \"x\"").is_none());
    }

    #[test]
    fn the_mcp_command_is_a_rust_binary_with_no_node_environment() {
        let command = desired_mcp_command();
        assert!(command.command.contains("relay-mcp"));
        assert!(command.args.is_empty());
    }

    #[test]
    fn the_hook_document_embeds_our_binary_and_the_session_end_flag() {
        let source = r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"mcp_tool","server":"relay","tool":"sync_session"}]}]}}"#;
        let command = McpCommand {
            command: "/Applications/Relay.app/Contents/MacOS/relay-mcp".to_string(),
            args: Vec::new(),
        };
        let document = materialised_hook_document(source, &command).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&document).unwrap();
        let hook = &parsed["hooks"]["SessionEnd"][0]["hooks"][0];
        assert_eq!(hook["type"], "command");
        assert!(hook["command"].as_str().unwrap().contains("relay-mcp"));
        assert!(hook["command"]
            .as_str()
            .unwrap()
            .contains("--session-end-hook"));
        // The SessionStart hook survives untouched.
        assert!(parsed["hooks"]["SessionStart"].is_array());
    }

    #[test]
    fn plugin_list_output_is_parsed() {
        let output =
            "relay@relay  installed, enabled  /Users/me/.relay/codex-plugin\nother@x  installed\n";
        let (status, version, source) = parse_installed_plugin(output).unwrap();
        assert_eq!(status.as_deref(), Some("installed, enabled"));
        assert_eq!(version, None);
        assert_eq!(source.as_deref(), Some("/Users/me/.relay/codex-plugin"));

        let current =
            "relay@relay  installed, enabled  0.1.0+deadbeef  /Users/me/.relay/codex-plugin\n";
        let (_, version, _) = parse_installed_plugin(current).unwrap();
        assert_eq!(version.as_deref(), Some("0.1.0+deadbeef"));
        assert!(parse_installed_plugin("nothing here").is_none());
    }

    #[test]
    fn materialised_plugin_has_the_marketplace_manifest_and_current_version() {
        let _guard = env_lock().lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("marketplace");
        let entry = directory.path().join("relay-mcp");
        std::fs::write(&entry, b"fixture").unwrap();
        let root_previous = std::env::var_os("RELAY_CODEX_PLUGIN_ROOT");
        let entry_previous = std::env::var_os("RELAY_MCP_ENTRY");
        std::env::set_var("RELAY_CODEX_PLUGIN_ROOT", &root);
        std::env::set_var("RELAY_MCP_ENTRY", &entry);

        let result = materialise_plugin();

        match root_previous {
            Some(value) => std::env::set_var("RELAY_CODEX_PLUGIN_ROOT", value),
            None => std::env::remove_var("RELAY_CODEX_PLUGIN_ROOT"),
        }
        match entry_previous {
            Some(value) => std::env::set_var("RELAY_MCP_ENTRY", value),
            None => std::env::remove_var("RELAY_MCP_ENTRY"),
        }

        let (_, version) = result.unwrap();
        for manifest in [
            root.join("plugins/relay/plugin.json"),
            root.join("plugins/relay/.codex-plugin/plugin.json"),
        ] {
            let installed: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(manifest).unwrap()).unwrap();
            assert_eq!(installed["name"], PLUGIN_NAME);
            assert_eq!(installed["version"], version);
            assert!(installed["extensions"]["com.openai"]["hooks"].is_string());
        }
    }

    #[tokio::test]
    async fn repair_installs_current_plugin_and_mcp_then_stays_correct_on_repeat() {
        let _guard = env_lock().lock().unwrap();
        let directory = tempfile::tempdir().unwrap();
        let codex_home = directory.path().join("codex");
        let marketplace = directory.path().join("marketplace");
        let mcp = directory.path().join("relay-mcp");
        let cli = directory.path().join("fixture-codex");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::write(&mcp, b"fixture").unwrap();
        std::fs::write(
            &cli,
            r#"#!/bin/sh
set -eu
state="$CODEX_HOME/.relay-fixture"
mkdir -p "$state"
case "${1:-} ${2:-} ${3:-}" in
  "--version  ") printf 'codex-cli fixture\n' ;;
  "plugin marketplace list")
    [ -f "$state/marketplace" ] && printf 'relay  %s\n' "$(cat "$state/marketplace")" || true ;;
  "plugin marketplace add") printf '%s' "$4" > "$state/marketplace" ;;
  "plugin marketplace remove") rm -f "$state/marketplace" ;;
  "plugin list ")
    if [ -f "$state/plugin" ]; then
      root="$(cat "$state/marketplace")"
      version="$(sed -n 's/.*\"version\": \"\([^\"]*\)\".*/\1/p' "$root/plugins/relay/plugin.json")"
      printf 'relay@relay  installed, enabled  %s  %s/plugins/relay\n' "$version" "$root"
    fi ;;
  "plugin remove") rm -f "$state/plugin" ;;
  "plugin add relay@relay")
    root="$(cat "$state/marketplace")"
    test -f "$root/plugins/relay/plugin.json"
    printf installed > "$state/plugin" ;;
  "mcp remove") rm -f "$state/mcp" ;;
  "mcp add relay")
    printf '%s' "$5" > "$state/mcp"
    mkdir -p "$CODEX_HOME"
    printf '[mcp_servers.relay]\ncommand = \"%s\"\n' "$5" > "$CODEX_HOME/config.toml" ;;
  *) printf 'unexpected codex invocation: %s\n' "$*" >&2; exit 2 ;;
esac
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        let variables = [
            ("CODEX_HOME", codex_home.as_os_str().to_owned()),
            ("CODEX_PATH", cli.as_os_str().to_owned()),
            (
                "RELAY_CODEX_PLUGIN_ROOT",
                marketplace.as_os_str().to_owned(),
            ),
            ("RELAY_MCP_ENTRY", mcp.as_os_str().to_owned()),
        ];
        let previous: Vec<(&str, Option<std::ffi::OsString>)> = variables
            .iter()
            .map(|(key, _)| (*key, std::env::var_os(key)))
            .collect();
        for (key, value) in &variables {
            std::env::set_var(key, value);
        }

        let first = install_codex().await;
        let second = install_codex().await;

        for (key, value) in previous {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }

        for result in [&first, &second] {
            assert!(result.status.configured, "{:?}", result.messages);
            assert!(result.status.checks.iter().all(|check| check.ok));
        }
        assert!(marketplace.join("plugins/relay/plugin.json").is_file());
        assert!(codex_home.join("config.toml").is_file());
    }

    #[test]
    fn marketplace_output_is_parsed() {
        let output = "relay  /Users/me/.relay/codex-plugin\n";
        assert_eq!(
            parse_marketplace_root(output).as_deref(),
            Some("/Users/me/.relay/codex-plugin")
        );
    }

    #[test]
    fn the_shipped_sources_are_found_in_a_checkout() {
        let sources = relay_sources().expect("integration assets must be in the tree");
        assert!(sources
            .skill
            .ends_with("integrations/codex/skills/relay/SKILL.md"));
        assert!(sources
            .hooks
            .ends_with("integrations/codex/hooks/hooks.json"));
    }

    #[test]
    fn the_plugin_version_follows_the_payload() {
        let sources = relay_sources().unwrap();
        let version = relay_plugin_version(&sources);
        assert!(version.starts_with(&relay_config::relay_version()));
        assert!(version.contains('+'));
    }

    #[test]
    fn the_status_reports_five_checks() {
        // The environment decides the outcome; the shape must always be complete.
        let status = futures_lite_block(codex_status());
        assert_eq!(status.checks.len(), 5);
        assert_eq!(status.checks[0].id, CodexCheckId::CodexCli);
        assert_eq!(status.checks[4].id, CodexCheckId::RelayHooks);
        assert_eq!(
            status.configured,
            status.checks.iter().all(|check| check.ok)
        );
    }

    /// Small blocking helper so the check shape can be asserted without a runtime.
    fn futures_lite_block<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }
}
