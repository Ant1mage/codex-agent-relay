//! Relay's MCP server.
//!
//! Codex starts this process over stdio. It owns execution: the RunController,
//! the worker processes, the event writes and the schema. Core never learns about
//! MCP; this module only translates requests into Relay calls.
//!
//! The tool surface is deliberately generic — one tool set for every profile — so
//! adding a runtime never changes what Codex sees.

mod config_reloader;
mod context;
mod service;

use std::sync::Arc;

use relay_adapters::adapters;
use relay_codex::CodexAppServerThreadResolver;
use relay_config::{config_path, database_path, relay_version, ConfigStore};
use relay_core::{AccessMode, Isolation, RunController};
use relay_storage::{SqliteControlQueue, SqliteEventStore, SqliteHostSessionStore};
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::io::stdio;
use rmcp::{serve_server, ErrorData as McpError};

use crate::config_reloader::RuntimeConfigReloader;
use crate::context::invocation_context;
use crate::service::{RelayService, RunAgentInput};

const INSTRUCTIONS: &str = "List agents before delegation. Give run_agent a bounded task. Use wait_agent before reviewing the result, then accept_agent or resume_agent after Codex review.";

struct RelayMcp {
    service: Arc<RelayService>,
}

/// The result shape Codex already knows: the payload as text plus structured
/// content under `result`, so `wait_agent` keeps returning `result.summary`.
fn payload(value: &serde_json::Value) -> CallToolResult {
    let mut result = CallToolResult::success(vec![ContentBlock::text(value.to_string())]);
    result.structured_content = Some(serde_json::json!({ "result": value }));
    result
}

fn failure(error: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(error.to_string())])
}

fn worker_schema(extra: Option<(&str, &str)>) -> serde_json::Value {
    let mut properties = serde_json::json!({
        "worker_session_id": { "type": "string", "minLength": 1 }
    });
    let mut required = vec![serde_json::json!("worker_session_id")];
    if let Some((name, description)) = extra {
        properties[name] =
            serde_json::json!({ "type": "string", "minLength": 1, "description": description });
        required.push(serde_json::json!(name));
    }
    serde_json::json!({
        "type": "object",
        "properties": properties,
        "required": required,
        "additionalProperties": false
    })
}

fn tool_definitions() -> Vec<Tool> {
    let schema = |value: serde_json::Value| -> Arc<serde_json::Map<String, serde_json::Value>> {
        match value {
            serde_json::Value::Object(object) => Arc::new(object),
            _ => Arc::new(serde_json::Map::new()),
        }
    };
    vec![
        Tool::new(
            "list_agents",
            "List enabled Relay agent profiles available to this Codex session.",
            schema(serde_json::json!({ "type": "object", "properties": {}, "additionalProperties": false })),
        ),
        Tool::new(
            "run_agent",
            "Start one bounded task using an enabled Relay agent profile.",
            schema(serde_json::json!({
                "type": "object",
                "properties": {
                    "agent_id": { "type": "string", "minLength": 1 },
                    "task": { "type": "string", "minLength": 1 },
                    "access_mode": { "type": "string", "enum": ["read_only", "propose", "write"] },
                    "isolation": { "type": "string", "enum": ["shared", "worktree"] }
                },
                "required": ["agent_id", "task"],
                "additionalProperties": false
            })),
        ),
        Tool::new(
            "get_agent_status",
            "Read the current projected state of a Relay worker.",
            schema(worker_schema(None)),
        ),
        Tool::new(
            "wait_agent",
            "Wait for a Relay worker to reach a terminal state.",
            schema(worker_schema(None)),
        ),
        Tool::new(
            "send_agent",
            "Send a follow-up message when the selected Runtime supports it.",
            schema(worker_schema(Some(("message", "Follow-up message for the worker")))),
        ),
        Tool::new("cancel_agent", "Cancel an active Relay worker.", schema(worker_schema(None))),
        Tool::new(
            "accept_agent",
            "Mark an awaiting Relay task complete after Codex has reviewed the worker result.",
            schema(worker_schema(None)),
        ),
        Tool::new(
            "resume_agent",
            "Resume the same Relay Step after review feedback when its Runtime supports native session resume.",
            schema(worker_schema(Some(("feedback", "Review feedback for the worker")))),
        ),
        Tool::new(
            "sync_session",
            "Synchronize the current Codex session identity and exact display name.",
            schema(serde_json::json!({
                "type": "object",
                "properties": { "session_id": { "type": "string" } },
                "additionalProperties": false
            })),
        ),
        Tool::new(
            "end_session",
            "Mark the current Codex session as ended and clear temporary policy.",
            schema(serde_json::json!({
                "type": "object",
                "properties": { "session_id": { "type": "string" } },
                "additionalProperties": false
            })),
        ),
    ]
}

fn argument<'a>(
    arguments: Option<&'a serde_json::Map<String, serde_json::Value>>,
    key: &str,
) -> Option<&'a str> {
    arguments?.get(key)?.as_str()
}

fn required_argument(
    arguments: Option<&serde_json::Map<String, serde_json::Value>>,
    key: &str,
) -> Result<String, McpError> {
    match argument(arguments, key) {
        Some(value) if !value.is_empty() => Ok(value.to_string()),
        _ => Err(McpError::invalid_params(format!("{key} is required"), None)),
    }
}

impl RelayMcp {
    fn invocation(
        &self,
        arguments: Option<&serde_json::Map<String, serde_json::Value>>,
    ) -> Result<crate::context::CodexInvocationContext, McpError> {
        let environment: Vec<(String, String)> = std::env::vars().collect();
        let metadata = arguments.map(|arguments| serde_json::Value::Object(arguments.clone()));
        invocation_context(metadata.as_ref(), &environment)
            .map_err(|message| McpError::invalid_params(message, None))
    }

    async fn dispatch(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let arguments = request.arguments.clone();
        let name = request.name.to_string();
        match name.as_str() {
            "list_agents" => {
                let invocation = match self.invocation(arguments.as_ref()) {
                    Ok(invocation) => invocation,
                    Err(error) => return failure(error.message),
                };
                match self.service.list_agents(&invocation).await {
                    Ok(profiles) => match serde_json::to_value(&profiles) {
                        Ok(value) => payload(&value),
                        Err(error) => failure(error),
                    },
                    Err(error) => failure(error.message()),
                }
            }
            "run_agent" => {
                let invocation = match self.invocation(arguments.as_ref()) {
                    Ok(invocation) => invocation,
                    Err(error) => return failure(error.message),
                };
                let agent_id = match required_argument(arguments.as_ref(), "agent_id") {
                    Ok(value) => value,
                    Err(error) => return failure(error.message),
                };
                let task = match required_argument(arguments.as_ref(), "task") {
                    Ok(value) => value,
                    Err(error) => return failure(error.message),
                };
                let access_mode = match argument(arguments.as_ref(), "access_mode") {
                    Some("read_only") => Some(AccessMode::ReadOnly),
                    Some("propose") => Some(AccessMode::Propose),
                    Some("write") => Some(AccessMode::Write),
                    Some(other) => return failure(format!("Unknown access_mode: {other}")),
                    None => None,
                };
                let isolation = match argument(arguments.as_ref(), "isolation") {
                    Some("shared") => Some(Isolation::Shared),
                    Some("worktree") => Some(Isolation::Worktree),
                    Some(other) => return failure(format!("Unknown isolation: {other}")),
                    None => None,
                };
                match self
                    .service
                    .run_agent(
                        &invocation,
                        RunAgentInput {
                            agent_id,
                            task,
                            access_mode,
                            isolation,
                        },
                    )
                    .await
                {
                    Ok(value) => match serde_json::to_value(&value) {
                        Ok(value) => payload(&value),
                        Err(error) => failure(error),
                    },
                    Err(error) => failure(error.message()),
                }
            }
            "get_agent_status" => {
                let worker = match required_argument(arguments.as_ref(), "worker_session_id") {
                    Ok(value) => value,
                    Err(error) => return failure(error.message),
                };
                match self.service.status(&worker) {
                    Ok(projection) => match projection_payload(&projection) {
                        Ok(value) => payload(&value),
                        Err(error) => failure(error),
                    },
                    Err(error) => failure(error.message()),
                }
            }
            "wait_agent" => {
                let worker = match required_argument(arguments.as_ref(), "worker_session_id") {
                    Ok(value) => value,
                    Err(error) => return failure(error.message),
                };
                match self.service.wait(&worker).await {
                    Ok(projection) => match projection_payload(&projection) {
                        Ok(value) => payload(&value),
                        Err(error) => failure(error),
                    },
                    Err(error) => failure(error.message()),
                }
            }
            "send_agent" => {
                let worker = match required_argument(arguments.as_ref(), "worker_session_id") {
                    Ok(value) => value,
                    Err(error) => return failure(error.message),
                };
                let message = match required_argument(arguments.as_ref(), "message") {
                    Ok(value) => value,
                    Err(error) => return failure(error.message),
                };
                match self.service.send(&worker, &message).await {
                    Ok(()) => payload(&serde_json::json!({ "sent": true })),
                    Err(error) => failure(error.message()),
                }
            }
            "cancel_agent" => {
                let worker = match required_argument(arguments.as_ref(), "worker_session_id") {
                    Ok(value) => value,
                    Err(error) => return failure(error.message),
                };
                match self.service.cancel(&worker).await {
                    Ok(()) => payload(&serde_json::json!({ "cancelled": true })),
                    Err(error) => failure(error.message()),
                }
            }
            "accept_agent" => {
                let worker = match required_argument(arguments.as_ref(), "worker_session_id") {
                    Ok(value) => value,
                    Err(error) => return failure(error.message),
                };
                match self.service.accept(&worker) {
                    Ok(projection) => match projection_payload(&projection) {
                        Ok(value) => payload(&value),
                        Err(error) => failure(error),
                    },
                    Err(error) => failure(error.message()),
                }
            }
            "resume_agent" => {
                let worker = match required_argument(arguments.as_ref(), "worker_session_id") {
                    Ok(value) => value,
                    Err(error) => return failure(error.message),
                };
                let feedback = match required_argument(arguments.as_ref(), "feedback") {
                    Ok(value) => value,
                    Err(error) => return failure(error.message),
                };
                match self.service.resume(&worker, &feedback).await {
                    Ok(value) => match serde_json::to_value(&value) {
                        Ok(value) => payload(&value),
                        Err(error) => failure(error),
                    },
                    Err(error) => failure(error.message()),
                }
            }
            "sync_session" => {
                let invocation = match self.invocation(arguments.as_ref()) {
                    Ok(invocation) => invocation,
                    Err(error) => return failure(error.message),
                };
                // The hook supplies the session id it saw; it must match.
                if let Some(session_id) = argument(arguments.as_ref(), "session_id") {
                    if session_id != invocation.thread_id {
                        return failure(
                            "Hook session identity does not match the Codex request identity",
                        );
                    }
                }
                match self.service.sync_session(&invocation).await {
                    Ok(session) => match serde_json::to_value(&session) {
                        Ok(value) => payload(&value),
                        Err(error) => failure(error),
                    },
                    Err(error) => failure(error.message()),
                }
            }
            "end_session" => {
                let invocation = match self.invocation(arguments.as_ref()) {
                    Ok(invocation) => invocation,
                    Err(error) => return failure(error.message),
                };
                if let Some(session_id) = argument(arguments.as_ref(), "session_id") {
                    if session_id != invocation.thread_id {
                        return failure(
                            "Hook session identity does not match the Codex request identity",
                        );
                    }
                }
                match self.service.end_session(&invocation).await {
                    Ok(()) => payload(&serde_json::json!({ "ended": true })),
                    Err(error) => failure(error.message()),
                }
            }
            other => failure(format!("Unknown Relay tool: {other}")),
        }
    }
}

/// The projection Codex reviews: run, steps, workers, the terminal result and
/// the last event.
fn projection_payload(
    projection: &relay_core::RunProjection,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serde_json::json!({
        "run": projection.run,
        "steps": projection.steps,
        "workers": projection.workers,
    });
    if let Some(result) = &projection.result {
        value["result"] = result.clone();
    }
    if let Some(last) = &projection.last_event {
        value["lastEvent"] = serde_json::to_value(last)?;
    }
    Ok(value)
}

impl ServerHandler for RelayMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("relay", relay_version()))
            .with_instructions(INSTRUCTIONS)
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListToolsResult::with_all_items(tool_definitions())))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        Ok(self.dispatch(request, context).await.into())
    }
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .with_target(false)
        .init();

    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if arguments
        .iter()
        .any(|argument| argument == "--version" || argument == "-V")
    {
        println!("relay-mcp {}", relay_version());
        return;
    }
    if arguments
        .iter()
        .any(|argument| argument == "--help" || argument == "-h")
    {
        println!(
            "relay-mcp {} — Relay's MCP server\n\nUSAGE: relay-mcp [--session-end-hook]\n\nCodex starts this process over stdio and calls its MCP tools.\n--session-end-hook marks a Codex session as ended from the SessionEnd hook.",
            relay_version()
        );
        return;
    }
    if arguments
        .iter()
        .any(|argument| argument == "--session-end-hook")
    {
        if let Err(error) = run_session_end_hook().await {
            eprintln!("[relay] session end hook: {error}");
            std::process::exit(1);
        }
        return;
    }

    if let Err(error) = serve().await {
        eprintln!("[relay] {error}");
        std::process::exit(1);
    }
}

/// The `SessionEnd` hook runs this process in one-shot cleanup mode: the session
/// is marked ended without depending on a live stdio MCP connection.
async fn run_session_end_hook() -> Result<(), String> {
    use tokio::io::AsyncReadExt;
    let mut input = String::new();
    tokio::io::stdin()
        .read_to_string(&mut input)
        .await
        .map_err(|error| error.to_string())?;
    let payload: serde_json::Value = if input.trim().is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_str(&input).map_err(|error| error.to_string())?
    };
    let session_id = context::session_id_from_hook(&payload)
        .ok_or_else(|| "SessionEnd hook did not include a session id".to_string())?;

    let database = Arc::new(
        relay_storage::Database::open(database_path())
            .map_err(|error| error.message().to_string())?,
    );
    let sessions = Arc::new(SqliteHostSessionStore::new(database));
    let service = build_service(Arc::clone(&sessions))?;
    service
        .end_session_by_native_id(&session_id)
        .map_err(|error| error.message().to_string())?;
    Ok(())
}

fn build_service(sessions: Arc<SqliteHostSessionStore>) -> Result<Arc<RelayService>, String> {
    let database = Arc::new(
        relay_storage::Database::open(database_path())
            .map_err(|error| error.message().to_string())?,
    );
    let controller = Arc::new(RunController::new(Arc::new(SqliteEventStore::new(
        Arc::clone(&database),
    ))));
    for adapter in adapters() {
        controller
            .adapters
            .register(adapter)
            .map_err(|error| error.message().to_string())?;
    }
    let reloader = Arc::new(RuntimeConfigReloader::new(
        Arc::clone(&controller),
        ConfigStore::new(config_path()),
    ));
    Ok(Arc::new(RelayService {
        controller,
        sessions,
        codex_threads: Arc::new(CodexAppServerThreadResolver::new()),
        reloader,
    }))
}

async fn serve() -> Result<(), String> {
    let database_path = database_path();
    std::fs::create_dir_all(
        database_path
            .parent()
            .ok_or_else(|| "invalid database path".to_string())?,
    )
    .map_err(|error| error.to_string())?;

    let database = Arc::new(
        relay_storage::Database::open(&database_path)
            .map_err(|error| error.message().to_string())?,
    );
    let sessions = Arc::new(SqliteHostSessionStore::new(Arc::clone(&database)));
    let commands = Arc::new(SqliteControlQueue::new(Arc::clone(&database)));
    let service = build_service(Arc::clone(&sessions))?;
    service
        .reloader
        .refresh()
        .await
        .map_err(|error| error.message().to_string())?;

    // The daemon cannot reach a worker process, so cancellation crosses processes
    // through the control queue.
    let drain = {
        let service = Arc::clone(&service);
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                let Ok(Some(command)) = commands.claim_next() else {
                    continue;
                };
                if !command.is_cancel_worker() {
                    let _ = commands.fail(&command.id, "unsupported control command");
                    continue;
                }
                match service.cancel(&command.worker_session_id).await {
                    Ok(()) => {
                        let _ = commands.complete(&command.id);
                    }
                    Err(error) => {
                        let _ = commands.fail(&command.id, error.message());
                    }
                }
            }
        })
    };

    let handler = RelayMcp { service };
    let running = serve_server(handler, stdio())
        .await
        .map_err(|error| error.to_string())?;
    let _ = running.waiting().await;
    drain.abort();
    Ok(())
}
