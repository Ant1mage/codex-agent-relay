//! Relay's MCP server.
//!
//! Codex starts this process over stdio, and it is a protocol front-end and
//! nothing else: it translates MCP tool calls into loopback HTTP calls on the
//! Relay daemon. The daemon owns the RunController, the worker processes, the
//! event writes and the schema — which is why this process can crash, restart or
//! run beside another one without taking a running worker with it.
//!
//! The tool surface is deliberately generic — one tool set for every profile — so
//! adding a runtime never changes what Codex sees.

mod context;
mod daemon;

use relay_api::{
    CancelResult, EndSessionBody, ResumeBody, RunProjectionView, RunStartBody, RunStarted,
    SendBody, SessionContext,
};
use relay_config::relay_version;
use relay_core::{AccessMode, AgentProfile, HostSession, Isolation};
use rmcp::handler::server::ServerHandler;
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
};
use rmcp::service::{RequestContext, RoleServer};
use rmcp::transport::io::stdio;
use rmcp::{serve_server, ErrorData as McpError};

use crate::context::invocation_context;
use crate::daemon::DaemonClient;

const INSTRUCTIONS: &str = "List agents before delegation. Give run_agent a bounded task. Use wait_agent before reviewing the result, then accept_agent or resume_agent after Codex review.";

struct RelayMcp;

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

/// Sends a value the daemon answered with back to the host, or reports why not.
fn payload_of<T: serde::Serialize>(value: T) -> CallToolResult {
    match serde_json::to_value(&value) {
        Ok(value) => payload(&value),
        Err(error) => failure(error),
    }
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
    let schema =
        |value: serde_json::Value| -> std::sync::Arc<serde_json::Map<String, serde_json::Value>> {
            match value {
                serde_json::Value::Object(object) => std::sync::Arc::new(object),
                _ => std::sync::Arc::new(serde_json::Map::new()),
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

/// Worker ids are opaque; percent-encode them so one can never change the path.
fn segment(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

impl RelayMcp {
    fn invocation(
        &self,
        arguments: Option<&serde_json::Map<String, serde_json::Value>>,
    ) -> Result<context::CodexInvocationContext, McpError> {
        let environment: Vec<(String, String)> = std::env::vars().collect();
        let metadata = arguments.map(|arguments| serde_json::Value::Object(arguments.clone()));
        invocation_context(metadata.as_ref(), &environment)
            .map_err(|message| McpError::invalid_params(message, None))
    }

    /// The daemon is discovered per call: restarting the daemon must not require
    /// restarting Codex.
    fn client(&self) -> Result<DaemonClient, String> {
        DaemonClient::discover()
    }

    async fn dispatch(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> CallToolResult {
        let arguments = request.arguments.clone();
        let name = request.name.to_string();
        let client = match self.client() {
            Ok(client) => client,
            Err(message) => return failure(message),
        };
        match name.as_str() {
            "list_agents" => {
                let invocation = match self.invocation(arguments.as_ref()) {
                    Ok(invocation) => invocation,
                    Err(error) => return failure(error.message),
                };
                let session = SessionContext {
                    thread_id: invocation.thread_id,
                    turn_id: invocation.turn_id,
                };
                match client
                    .post::<Vec<AgentProfile>, _>("/api/agents", &session)
                    .await
                {
                    Ok(profiles) => payload_of(profiles),
                    Err(error) => failure(error),
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
                match client
                    .post::<RunStarted, _>(
                        "/api/runs",
                        &RunStartBody {
                            session: SessionContext {
                                thread_id: invocation.thread_id,
                                turn_id: invocation.turn_id,
                            },
                            agent_id,
                            task,
                            access_mode,
                            isolation,
                        },
                    )
                    .await
                {
                    Ok(started) => payload_of(started),
                    Err(error) => failure(error),
                }
            }
            "get_agent_status" => {
                let worker = match required_argument(arguments.as_ref(), "worker_session_id") {
                    Ok(value) => value,
                    Err(error) => return failure(error.message),
                };
                match client
                    .get::<RunProjectionView>(&format!("/api/workers/{}", segment(&worker)))
                    .await
                {
                    Ok(projection) => payload_of(projection),
                    Err(error) => failure(error),
                }
            }
            "wait_agent" => {
                let worker = match required_argument(arguments.as_ref(), "worker_session_id") {
                    Ok(value) => value,
                    Err(error) => return failure(error.message),
                };
                match client
                    .post::<RunProjectionView, _>(
                        &format!("/api/workers/{}/wait", segment(&worker)),
                        &serde_json::json!({}),
                    )
                    .await
                {
                    Ok(projection) => payload_of(projection),
                    Err(error) => failure(error),
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
                match client
                    .post::<serde_json::Value, _>(
                        &format!("/api/workers/{}/send", segment(&worker)),
                        &SendBody { message },
                    )
                    .await
                {
                    Ok(value) => payload(&value),
                    Err(error) => failure(error),
                }
            }
            "cancel_agent" => {
                let worker = match required_argument(arguments.as_ref(), "worker_session_id") {
                    Ok(value) => value,
                    Err(error) => return failure(error.message),
                };
                match client
                    .post::<CancelResult, _>(
                        &format!("/api/workers/{}/cancel", segment(&worker)),
                        &serde_json::json!({}),
                    )
                    .await
                {
                    Ok(result) if result.accepted => {
                        payload(&serde_json::json!({ "cancelled": true }))
                    }
                    Ok(result) => failure(
                        result
                            .message
                            .unwrap_or_else(|| format!("Relay refused to cancel {worker}")),
                    ),
                    Err(error) => failure(error),
                }
            }
            "accept_agent" => {
                let worker = match required_argument(arguments.as_ref(), "worker_session_id") {
                    Ok(value) => value,
                    Err(error) => return failure(error.message),
                };
                match client
                    .post::<RunProjectionView, _>(
                        &format!("/api/workers/{}/accept", segment(&worker)),
                        &serde_json::json!({}),
                    )
                    .await
                {
                    Ok(projection) => payload_of(projection),
                    Err(error) => failure(error),
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
                match client
                    .post::<RunStarted, _>(
                        &format!("/api/workers/{}/resume", segment(&worker)),
                        &ResumeBody { feedback },
                    )
                    .await
                {
                    Ok(started) => payload(&serde_json::json!({
                        "runId": started.run_id,
                        "workerSessionId": started.worker_session_id,
                    })),
                    Err(error) => failure(error),
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
                match client
                    .post::<HostSession, _>(
                        "/api/sessions/sync",
                        &SessionContext {
                            thread_id: invocation.thread_id,
                            turn_id: invocation.turn_id,
                        },
                    )
                    .await
                {
                    Ok(session) => payload_of(session),
                    Err(error) => failure(error),
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
                match client
                    .post::<Option<HostSession>, _>(
                        "/api/sessions/end",
                        &EndSessionBody {
                            session: SessionContext {
                                thread_id: invocation.thread_id,
                                turn_id: invocation.turn_id,
                            },
                            native_session_id: None,
                        },
                    )
                    .await
                {
                    Ok(_) => payload(&serde_json::json!({ "ended": true })),
                    Err(error) => failure(error),
                }
            }
            other => failure(format!("Unknown Relay tool: {other}")),
        }
    }
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
            "relay-mcp {} — Relay's MCP server\n\nUSAGE: relay-mcp [--session-end-hook]\n\nCodex starts this process over stdio and calls its MCP tools. Every tool call\nis forwarded to the Relay daemon over loopback HTTP; this process never starts\nan agent CLI itself.\n--session-end-hook marks a Codex session as ended from the SessionEnd hook.",
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

    let client = DaemonClient::discover()?;
    let _: Option<HostSession> = client
        .post(
            "/api/sessions/end",
            &EndSessionBody {
                session: SessionContext::default(),
                native_session_id: Some(session_id),
            },
        )
        .await?;
    Ok(())
}

async fn serve() -> Result<(), String> {
    let handler = RelayMcp;
    let running = serve_server(handler, stdio())
        .await
        .map_err(|error| error.to_string())?;
    let _ = running.waiting().await;
    Ok(())
}
