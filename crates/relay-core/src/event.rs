//! The append-only Relay event log.
//!
//! Every observable fact about a delegation is one of these rows. Nothing else
//! keeps progress state: projections, the inspector and the menu bar all read
//! from here.

use serde::{Deserialize, Serialize};

use crate::domain::Timestamp;

/// The full event vocabulary. The wire names are stable and are what the
/// inspector, the panel and stored rows already use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RelayEventType {
    #[serde(rename = "run/created")]
    RunCreated,
    #[serde(rename = "run/awaiting_host")]
    RunAwaitingHost,
    #[serde(rename = "run/accepted")]
    RunAccepted,
    #[serde(rename = "step/created")]
    StepCreated,
    #[serde(rename = "step/iteration_started")]
    StepIterationStarted,
    #[serde(rename = "worker/started")]
    WorkerStarted,
    #[serde(rename = "worker/message")]
    WorkerMessage,
    #[serde(rename = "worker/status")]
    WorkerStatus,
    /// Kept for provider compatibility. The console intentionally hides it.
    #[serde(rename = "worker/reasoning")]
    WorkerReasoning,
    #[serde(rename = "tool/read")]
    ToolRead,
    #[serde(rename = "tool/search")]
    ToolSearch,
    #[serde(rename = "tool/edit")]
    ToolEdit,
    #[serde(rename = "tool/command")]
    ToolCommand,
    #[serde(rename = "tool/result")]
    ToolResult,
    #[serde(rename = "test/result")]
    TestResult,
    #[serde(rename = "child/started")]
    ChildStarted,
    #[serde(rename = "child/completed")]
    ChildCompleted,
    #[serde(rename = "worker/completed")]
    WorkerCompleted,
    #[serde(rename = "worker/failed")]
    WorkerFailed,
    #[serde(rename = "worker/cancelled")]
    WorkerCancelled,
    #[serde(rename = "worker/interrupted")]
    WorkerInterrupted,
    #[serde(rename = "worker/orphaned")]
    WorkerOrphaned,
}

impl RelayEventType {
    pub const ALL: [RelayEventType; 22] = [
        RelayEventType::RunCreated,
        RelayEventType::RunAwaitingHost,
        RelayEventType::RunAccepted,
        RelayEventType::StepCreated,
        RelayEventType::StepIterationStarted,
        RelayEventType::WorkerStarted,
        RelayEventType::WorkerMessage,
        RelayEventType::WorkerStatus,
        RelayEventType::WorkerReasoning,
        RelayEventType::ToolRead,
        RelayEventType::ToolSearch,
        RelayEventType::ToolEdit,
        RelayEventType::ToolCommand,
        RelayEventType::ToolResult,
        RelayEventType::TestResult,
        RelayEventType::ChildStarted,
        RelayEventType::ChildCompleted,
        RelayEventType::WorkerCompleted,
        RelayEventType::WorkerFailed,
        RelayEventType::WorkerCancelled,
        RelayEventType::WorkerInterrupted,
        RelayEventType::WorkerOrphaned,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            RelayEventType::RunCreated => "run/created",
            RelayEventType::RunAwaitingHost => "run/awaiting_host",
            RelayEventType::RunAccepted => "run/accepted",
            RelayEventType::StepCreated => "step/created",
            RelayEventType::StepIterationStarted => "step/iteration_started",
            RelayEventType::WorkerStarted => "worker/started",
            RelayEventType::WorkerMessage => "worker/message",
            RelayEventType::WorkerStatus => "worker/status",
            RelayEventType::WorkerReasoning => "worker/reasoning",
            RelayEventType::ToolRead => "tool/read",
            RelayEventType::ToolSearch => "tool/search",
            RelayEventType::ToolEdit => "tool/edit",
            RelayEventType::ToolCommand => "tool/command",
            RelayEventType::ToolResult => "tool/result",
            RelayEventType::TestResult => "test/result",
            RelayEventType::ChildStarted => "child/started",
            RelayEventType::ChildCompleted => "child/completed",
            RelayEventType::WorkerCompleted => "worker/completed",
            RelayEventType::WorkerFailed => "worker/failed",
            RelayEventType::WorkerCancelled => "worker/cancelled",
            RelayEventType::WorkerInterrupted => "worker/interrupted",
            RelayEventType::WorkerOrphaned => "worker/orphaned",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        RelayEventType::ALL
            .into_iter()
            .find(|candidate| candidate.as_str() == value)
    }

    /// The three events that end a worker.
    pub fn terminal_worker_status(self) -> Option<crate::domain::WorkerStatus> {
        use crate::domain::WorkerStatus;
        match self {
            RelayEventType::WorkerCompleted => Some(WorkerStatus::Completed),
            RelayEventType::WorkerFailed => Some(WorkerStatus::Failed),
            RelayEventType::WorkerCancelled => Some(WorkerStatus::Cancelled),
            _ => None,
        }
    }
}

/// A normalized, provider-independent observable fact.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelayEvent {
    pub id: String,
    pub run_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub step_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_session_id: Option<String>,
    pub seq: u64,
    pub timestamp: Timestamp,
    #[serde(rename = "type")]
    pub event_type: RelayEventType,
    pub data: serde_json::Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_event: Option<serde_json::Value>,
}

/// Native payloads are kept for debugging, but a runaway CLI must not fill the
/// log: anything past this size is replaced by a truncated preview.
const MAX_NATIVE_EVENT_BYTES: usize = 128 * 1024;

pub fn bound_native_event(native_event: serde_json::Value) -> serde_json::Value {
    match serde_json::to_string(&native_event) {
        Ok(serialized) if serialized.len() <= MAX_NATIVE_EVENT_BYTES => native_event,
        Ok(serialized) => serde_json::json!({
            "truncated": true,
            "originalBytes": serialized.len(),
            "preview": &serialized[..floor_char_boundary(&serialized, MAX_NATIVE_EVENT_BYTES)],
        }),
        Err(_) => serde_json::json!({ "truncated": true, "preview": native_event.to_string() }),
    }
}

/// Cuts at a UTF-8 boundary so a multi-byte character is never split.
fn floor_char_boundary(value: &str, index: usize) -> usize {
    if index >= value.len() {
        return value.len();
    }
    let mut end = index;
    while end > 0 && !value.is_char_boundary(end) {
        end -= 1;
    }
    end
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_names_are_stable() {
        assert_eq!(RelayEventType::RunCreated.as_str(), "run/created");
        assert_eq!(
            RelayEventType::StepIterationStarted.as_str(),
            "step/iteration_started"
        );
        for event in RelayEventType::ALL {
            assert_eq!(RelayEventType::parse(event.as_str()), Some(event));
        }
        assert_eq!(RelayEventType::parse("nope"), None);
    }

    #[test]
    fn event_serializes_type_field_like_the_stored_rows() {
        let event = RelayEvent {
            id: "e1".into(),
            run_id: "r1".into(),
            step_id: None,
            worker_session_id: Some("w1".into()),
            seq: 3,
            timestamp: "2026-01-01T00:00:00.000Z".into(),
            event_type: RelayEventType::WorkerMessage,
            data: serde_json::json!({ "text": "hi" }),
            native_event: None,
        };
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "worker/message");
        assert_eq!(json["workerSessionId"], "w1");
        assert!(json.get("stepId").is_none());
        assert!(json.get("nativeEvent").is_none());
    }

    #[test]
    fn large_native_events_are_truncated() {
        let huge = serde_json::json!({ "text": "x".repeat(MAX_NATIVE_EVENT_BYTES + 10) });
        let bounded = bound_native_event(huge);
        assert_eq!(bounded["truncated"], true);
        assert!(bounded["originalBytes"].as_u64().unwrap() > MAX_NATIVE_EVENT_BYTES as u64);
    }

    #[test]
    fn terminal_status_mapping_matches_worker_lifecycle() {
        use crate::domain::WorkerStatus;
        assert_eq!(
            RelayEventType::WorkerCompleted.terminal_worker_status(),
            Some(WorkerStatus::Completed)
        );
        assert_eq!(RelayEventType::WorkerMessage.terminal_worker_status(), None);
    }
}
