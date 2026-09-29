//! The unified worker assistant-text contract.
//!
//! Runtime CLIs disagree about how they publish an answer. DeepSeek Harness
//! prints one committed assistant message per `--json` `text` frame, Antigravity
//! streams `agent_response.text_delta` chunks, Grok streams its own `text`
//! chunks. Relay normalizes all of them into one shape on `worker/message`:
//!
//! ```json
//! { "kind": "delta", "text": "<increment>", "messageStart": true }
//! ```
//!
//! - `text` is one increment; increments arrive in the order the model produced
//!   them and are appended as they arrive.
//! - `messageStart: true` opens a new assistant message; the increments after it
//!   extend that message. A CLI that publishes whole committed messages sets the
//!   flag on every frame, a chunked CLI sets it on the first chunk of a
//!   response. The absence of the flag never opens a message by itself, which
//!   keeps rows written before this contract readable.
//! - `kind: "final"` remains the authoritative complete answer and
//!   `kind: "status"` / `kind: "diagnostic"` are not assistant text at all.
//!
//! The provider frame is parsed in the adapter (`relay-adapters`) and kept in
//! `nativeEvent`; this module only understands the normalized contract, so
//! `relay-core` never learns a runtime's JSON.
//!
//! [`assistant_text`] is the one aggregation every surface consumes: it merges
//! increments into messages in order, lets an authoritative `final` supersede
//! the stream that produced it, and reports which completions merely repeat a
//! message already on screen. No surface re-implements those rules, and a
//! completion is never displayed as a second copy of the answer.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use crate::event::{RelayEvent, RelayEventType};

/// `kind` of a `worker/message` carrying one increment of assistant text.
pub const DELTA_KIND: &str = "delta";
/// `kind` of a `worker/message` carrying the authoritative complete answer.
pub const FINAL_KIND: &str = "final";
/// Field that opens a new assistant message inside a delta.
pub const MESSAGE_START_FIELD: &str = "messageStart";

/// One increment of assistant text, as it is stored on a `worker/message`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantTextDelta {
    pub text: String,
    /// Opens a new assistant message instead of extending the current one.
    pub starts_message: bool,
}

impl AssistantTextDelta {
    /// An increment that continues the message already being streamed.
    pub fn chunk(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            starts_message: false,
        }
    }

    /// An increment that opens a new assistant message.
    ///
    /// A CLI publishing whole committed messages (DeepSeek Harness today) uses
    /// this for every frame: the messages are complete, not token-level.
    pub fn message(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            starts_message: true,
        }
    }

    /// The normalized `worker/message` payload for this increment.
    pub fn into_data(self) -> Value {
        let mut data = Map::new();
        data.insert("kind".to_string(), Value::String(DELTA_KIND.to_string()));
        data.insert("text".to_string(), Value::String(self.text));
        if self.starts_message {
            data.insert(MESSAGE_START_FIELD.to_string(), Value::Bool(true));
        }
        Value::Object(data)
    }
}

/// Reads the assistant-text increment out of a `worker/message` event.
///
/// Only `kind: "delta"` is the contract. Rows that predate it and carry just
/// `{"text": …}` (Kimi, older DeepSeek rows, the short-lived Antigravity shape
/// with `text` and `delta`) are read as increments of the current message, which
/// is how the console already treated them.
pub fn delta(event: &RelayEvent) -> Option<AssistantTextDelta> {
    if event.event_type != RelayEventType::WorkerMessage {
        return None;
    }
    let data = event.data.as_object()?;
    match data.get("kind").and_then(Value::as_str) {
        Some(DELTA_KIND) => {}
        Some(_) => return None,
        None => {}
    }
    let text = data
        .get("text")
        .or_else(|| data.get("delta"))
        .and_then(Value::as_str)?;
    Some(AssistantTextDelta {
        text: text.to_string(),
        starts_message: data
            .get(MESSAGE_START_FIELD)
            .and_then(Value::as_bool)
            .unwrap_or(false),
    })
}

/// The authoritative complete answer of a `worker/message` event.
pub fn final_text(event: &RelayEvent) -> Option<&str> {
    if event.event_type != RelayEventType::WorkerMessage {
        return None;
    }
    let data = event.data.as_object()?;
    match data.get("kind").and_then(Value::as_str) {
        Some(FINAL_KIND) => data.get("text").and_then(Value::as_str),
        _ => None,
    }
}

/// The summary a `worker/completed` event carries, when it carries one.
pub fn completion_summary(event: &RelayEvent) -> Option<&str> {
    if event.event_type != RelayEventType::WorkerCompleted {
        return None;
    }
    event
        .data
        .as_object()?
        .get("summary")
        .and_then(Value::as_str)
}

/// Where a displayed message came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssistantTextSource {
    /// Increments merged into one message.
    Stream,
    /// An authoritative `kind: "final"` answer.
    Final,
}

/// One assistant message a surface can display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssistantMessage {
    pub text: String,
    pub source: AssistantTextSource,
}

/// The assistant text one worker produced, resolved for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkerAssistantText {
    pub worker_session_id: Option<String>,
    /// Messages to display, in the order the worker produced them. Each answer
    /// appears once: a `final` supersedes the stream it repeats, and a message
    /// repeating one already shown is dropped.
    pub messages: Vec<AssistantMessage>,
    /// `worker/completed` events whose summary repeats a message above. Their
    /// console row is a status, not a second copy of the answer.
    pub repeated_completion_ids: Vec<String>,
}

/// Resolves the assistant text of every worker in `events`, in first-appearance
/// order. `events` may span several steps; grouping is per worker session, which
/// is what keeps two iterations of the same step apart.
pub fn assistant_text(events: &[RelayEvent]) -> Vec<WorkerAssistantText> {
    let mut workers: Vec<WorkerStream> = Vec::new();
    for event in events {
        if let Some(delta) = delta(event) {
            if delta.text.is_empty() {
                continue;
            }
            let worker = worker_stream(&mut workers, event.worker_session_id.clone());
            match worker.messages.last_mut() {
                Some(message)
                    if message.source == AssistantTextSource::Stream && !delta.starts_message =>
                {
                    message.text.push_str(&delta.text);
                }
                _ => worker.messages.push(AssistantMessage {
                    text: delta.text,
                    source: AssistantTextSource::Stream,
                }),
            }
        } else if let Some(text) = final_text(event) {
            if text.trim().is_empty() {
                continue;
            }
            let worker = worker_stream(&mut workers, event.worker_session_id.clone());
            worker.messages.push(AssistantMessage {
                text: text.to_string(),
                source: AssistantTextSource::Final,
            });
        } else if let Some(summary) = completion_summary(event) {
            let worker = worker_stream(&mut workers, event.worker_session_id.clone());
            worker
                .completions
                .push((event.id.clone(), summary.to_string()));
        }
    }
    workers.into_iter().map(WorkerStream::resolve).collect()
}

struct WorkerStream {
    worker_session_id: Option<String>,
    messages: Vec<AssistantMessage>,
    /// `(event id, summary)` of every completion seen for this worker.
    completions: Vec<(String, String)>,
}

fn worker_stream(
    workers: &mut Vec<WorkerStream>,
    worker_session_id: Option<String>,
) -> &mut WorkerStream {
    if let Some(index) = workers
        .iter()
        .position(|worker| worker.worker_session_id == worker_session_id)
    {
        return &mut workers[index];
    }
    workers.push(WorkerStream {
        worker_session_id,
        messages: Vec::new(),
        completions: Vec::new(),
    });
    workers.last_mut().expect("just pushed")
}

impl WorkerStream {
    fn resolve(self) -> WorkerAssistantText {
        let WorkerStream {
            worker_session_id,
            mut messages,
            completions,
        } = self;

        let finals: Vec<String> = messages
            .iter()
            .filter(|message| message.source == AssistantTextSource::Final)
            .map(|message| message.text.trim().to_string())
            .collect();
        let joined_stream: String = messages
            .iter()
            .filter(|message| message.source == AssistantTextSource::Stream)
            .map(|message| message.text.as_str())
            .collect::<Vec<_>>()
            .concat();
        let joined_stream = joined_stream.trim().to_string();

        // An authoritative final supersedes the increments that spelled it out:
        // the same answer is not shown twice. A final that only covers part of
        // the stream leaves the messages it did not repeat on screen.
        if !joined_stream.is_empty() && finals.contains(&joined_stream) {
            messages.retain(|message| message.source != AssistantTextSource::Stream);
        } else {
            messages.retain(|message| {
                message.source != AssistantTextSource::Stream
                    || !finals
                        .iter()
                        .any(|final_text| *final_text == message.text.trim())
            });
        }

        // A message repeating one already shown for this worker adds nothing.
        let mut seen = BTreeSet::new();
        messages.retain(|message| seen.insert(message.text.trim().to_string()));

        let mut displayed: Vec<String> = messages
            .iter()
            .map(|message| message.text.trim().to_string())
            .filter(|text| !text.is_empty())
            .collect();
        if !joined_stream.is_empty() {
            displayed.push(joined_stream);
        }
        let repeated_completion_ids = completions
            .iter()
            .filter(|(_, summary)| {
                let summary = summary.trim();
                !summary.is_empty() && displayed.iter().any(|text| *text == summary)
            })
            .map(|(id, _)| id.clone())
            .collect();

        WorkerAssistantText {
            worker_session_id,
            messages,
            repeated_completion_ids,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::RelayEvent;

    fn event(
        id: &str,
        worker: Option<&str>,
        event_type: RelayEventType,
        data: Value,
    ) -> RelayEvent {
        RelayEvent {
            id: id.to_string(),
            run_id: "run-1".to_string(),
            step_id: Some("step-1".to_string()),
            worker_session_id: worker.map(str::to_string),
            seq: 1,
            timestamp: "2026-01-01T00:00:00.000Z".to_string(),
            event_type,
            data,
            native_event: None,
        }
    }

    fn delta(id: &str, worker: &str, text: &str, starts_message: bool) -> RelayEvent {
        let data = if starts_message {
            AssistantTextDelta::message(text).into_data()
        } else {
            AssistantTextDelta::chunk(text).into_data()
        };
        event(id, Some(worker), RelayEventType::WorkerMessage, data)
    }

    fn final_message(id: &str, worker: &str, text: &str) -> RelayEvent {
        event(
            id,
            Some(worker),
            RelayEventType::WorkerMessage,
            serde_json::json!({ "kind": "final", "text": text }),
        )
    }

    fn completed(id: &str, worker: &str, summary: &str) -> RelayEvent {
        event(
            id,
            Some(worker),
            RelayEventType::WorkerCompleted,
            serde_json::json!({ "summary": summary, "exitCode": 0 }),
        )
    }

    fn texts(events: &[RelayEvent]) -> Vec<String> {
        assistant_text(events)
            .into_iter()
            .flat_map(|worker| worker.messages)
            .map(|message| message.text)
            .collect()
    }

    #[test]
    fn the_delta_payload_is_the_normalized_contract() {
        let opening = AssistantTextDelta::message("hello").into_data();
        assert_eq!(opening["kind"], DELTA_KIND);
        assert_eq!(opening["text"], "hello");
        assert_eq!(opening[MESSAGE_START_FIELD], true);

        let chunk = AssistantTextDelta::chunk("hello").into_data();
        assert_eq!(chunk["kind"], DELTA_KIND);
        assert!(
            chunk.get(MESSAGE_START_FIELD).is_none(),
            "a continuing chunk carries no boundary"
        );
    }

    #[test]
    fn chunks_merge_and_a_boundary_opens_a_new_message() {
        let events = vec![
            delta("d1", "w1", "Hel", false),
            delta("d2", "w1", "lo", false),
            delta("d3", "w1", "Second", true),
        ];
        assert_eq!(texts(&events), vec!["Hello", "Second"]);
        let resolved = assistant_text(&events);
        assert!(resolved[0]
            .messages
            .iter()
            .all(|message| message.source == AssistantTextSource::Stream));
    }

    #[test]
    fn a_final_supersedes_the_increments_that_spelled_it() {
        let events = vec![
            delta("d1", "w1", "Hello ", true),
            delta("d2", "w1", "world", false),
            final_message("f1", "w1", "Hello world"),
        ];
        let resolved = assistant_text(&events);
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].messages.len(), 1);
        assert_eq!(resolved[0].messages[0].text, "Hello world");
        assert_eq!(resolved[0].messages[0].source, AssistantTextSource::Final);
    }

    #[test]
    fn a_final_that_repeats_only_the_last_message_keeps_the_earlier_ones() {
        let events = vec![
            delta("d1", "w1", "first", true),
            delta("d2", "w1", "second", true),
            final_message("f1", "w1", "second"),
        ];
        let resolved = assistant_text(&events);
        let messages = &resolved[0].messages;
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].text, "first");
        assert_eq!(messages[0].source, AssistantTextSource::Stream);
        assert_eq!(messages[1].text, "second");
        assert_eq!(messages[1].source, AssistantTextSource::Final);
    }

    #[test]
    fn a_completion_repeating_the_answer_is_reported_not_displayed_again() {
        let events = vec![
            delta("d1", "w1", "answer", true),
            completed("c1", "w1", "answer"),
        ];
        let resolved = assistant_text(&events);
        assert_eq!(resolved[0].messages.len(), 1);
        assert_eq!(resolved[0].repeated_completion_ids, vec!["c1".to_string()]);
    }

    #[test]
    fn the_whole_merged_stream_can_repeat_a_completion() {
        let events = vec![
            delta("d1", "w1", "RELAY_", true),
            delta("d2", "w1", "OK", false),
            completed("c1", "w1", "RELAY_OK"),
        ];
        let resolved = assistant_text(&events);
        assert_eq!(resolved[0].messages.len(), 1);
        assert_eq!(resolved[0].messages[0].text, "RELAY_OK");
        assert_eq!(resolved[0].repeated_completion_ids, vec!["c1".to_string()]);
    }

    #[test]
    fn a_completion_without_a_message_is_not_claimed_as_displayed() {
        let events = vec![completed("c1", "w1", "only here")];
        let resolved = assistant_text(&events);
        assert!(resolved[0].messages.is_empty());
        assert!(resolved[0].repeated_completion_ids.is_empty());
    }

    #[test]
    fn a_completion_never_repeats_another_workers_answer() {
        let events = vec![
            final_message("f1", "w1", "same text"),
            completed("c2", "w2", "same text"),
        ];
        let resolved = assistant_text(&events);
        assert_eq!(resolved.len(), 2);
        assert!(resolved
            .iter()
            .all(|worker| worker.repeated_completion_ids.is_empty()));
    }

    #[test]
    fn legacy_text_only_rows_still_stream_into_one_message() {
        let events = vec![
            event(
                "m1",
                Some("w1"),
                RelayEventType::WorkerMessage,
                serde_json::json!({ "text": "hello " }),
            ),
            event(
                "m2",
                Some("w1"),
                RelayEventType::WorkerMessage,
                serde_json::json!({ "text": "world" }),
            ),
        ];
        assert_eq!(texts(&events), vec!["hello world"]);
    }

    #[test]
    fn status_and_diagnostic_messages_are_not_assistant_text() {
        let events = vec![
            event(
                "s1",
                Some("w1"),
                RelayEventType::WorkerMessage,
                serde_json::json!({ "kind": "status", "phase": "turn_end" }),
            ),
            event(
                "s2",
                Some("w1"),
                RelayEventType::WorkerMessage,
                serde_json::json!({ "kind": "diagnostic", "message": "boom" }),
            ),
        ];
        assert!(assistant_text(&events).is_empty());
    }

    #[test]
    fn worker_text_is_grouped_per_worker_in_first_appearance_order() {
        let events = vec![
            delta("d1", "w1", "one", true),
            delta("d2", "w2", "two", true),
            delta("d3", "w1", "more", false),
        ];
        let resolved = assistant_text(&events);
        assert_eq!(resolved[0].worker_session_id.as_deref(), Some("w1"));
        assert_eq!(resolved[0].messages[0].text, "onemore");
        assert_eq!(resolved[1].worker_session_id.as_deref(), Some("w2"));
        assert_eq!(resolved[1].messages[0].text, "two");
    }
}
