//! Console formatting: a line-by-line port of `apps/web/src/lib/format.ts` and
//! `apps/web/src/lib/step-events.ts`.
//!
//! The inspector's whole reading experience is these functions, so the rules are
//! copied exactly: which events are visible, how a run of reads collapses, how a
//! diff stat is summed and how a status becomes a glyph.

use std::collections::{BTreeMap, BTreeSet};

use relay_api::RunView;
use relay_core::{RelayEvent, RelayEventType, RunStatus, StepStatus};
use serde_json::{Map, Value};
use wasm_bindgen::JsCast;

use crate::dom;
use crate::i18n::Translator;

/// Host sessions carry their own three-state status; the inspector only ever
/// formats it as a timestamp hint (`relativeTime` treats everything else as
/// historical).
pub fn host_status(status: relay_core::HostSessionStatus) -> &'static str {
    match status {
        relay_core::HostSessionStatus::Active => "active",
        relay_core::HostSessionStatus::Offline => "offline",
        relay_core::HostSessionStatus::Ended => "ended",
    }
}

/// Runtime health as it appears on the wire, for the panel's health suffixes.
pub fn health_name(health: relay_core::RuntimeHealth) -> &'static str {
    match health {
        relay_core::RuntimeHealth::Available => "available",
        relay_core::RuntimeHealth::AuthenticationRequired => "authentication_required",
        relay_core::RuntimeHealth::Unavailable => "unavailable",
    }
}

/// The raw view is a debugging aid, not a dump: keep the tail bounded.
pub const RAW_LIMIT: usize = 400;

pub fn run_status(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Queued => "queued",
        RunStatus::Starting => "starting",
        RunStatus::Running => "running",
        RunStatus::AwaitingHost => "awaiting_host",
        RunStatus::Completed => "completed",
        RunStatus::Failed => "failed",
        RunStatus::Cancelled => "cancelled",
        RunStatus::Interrupted => "interrupted",
        RunStatus::Orphaned => "orphaned",
    }
}

pub fn step_status(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Queued => "queued",
        StepStatus::Starting => "starting",
        StepStatus::Running => "running",
        StepStatus::AwaitingHost => "awaiting_host",
        StepStatus::Completed => "completed",
        StepStatus::Failed => "failed",
        StepStatus::Cancelled => "cancelled",
        StepStatus::Interrupted => "interrupted",
        StepStatus::Orphaned => "orphaned",
    }
}

/// `elapsed`: `Ns`, `Nm Ns`, `Nh Nm`.
pub fn elapsed(start: &str, end: Option<&str>) -> String {
    let end_ms = match end {
        Some(value) => dom::parse_ms(value),
        None => dom::now_ms(),
    };
    let start_ms = dom::parse_ms(start);
    let total = (end_ms - start_ms).max(0.0);
    if !total.is_finite() {
        return "0s".to_string();
    }
    let seconds = (total / 1_000.0).floor();
    if seconds < 60.0 {
        return format!("{}s", seconds as u64);
    }
    let minutes = (seconds / 60.0).floor();
    if minutes < 60.0 {
        return format!("{}m {}s", minutes as u64, (seconds % 60.0) as u64);
    }
    let hours = (minutes / 60.0).floor();
    format!("{}h {}m", hours as u64, (minutes % 60.0) as u64)
}

/// Compact "how long ago", with a live hint while a run is still going.
pub fn relative_time(timestamp: &str, status: &str, t: &Translator) -> String {
    if status == "running" || status == "starting" {
        return t.t("common.active");
    }
    let parsed = dom::parse_ms(timestamp);
    if !parsed.is_finite() {
        return String::new();
    }
    let seconds = ((dom::now_ms() - parsed) / 1_000.0).round().max(0.0);
    if seconds < 60.0 {
        return format!("{}s", seconds as u64);
    }
    let minutes = (seconds / 60.0).floor();
    if minutes < 60.0 {
        return format!("{}m", minutes as u64);
    }
    let hours = (minutes / 60.0).floor();
    if hours < 24.0 {
        return format!("{}h", hours as u64);
    }
    date_short(timestamp)
}

/// `new Date(timestamp).toLocaleDateString([], {month: 'short', day: 'numeric'})`.
/// Month names stay English, which is also the fallback the catalogue uses.
pub fn date_short(timestamp: &str) -> String {
    const MONTHS: [&str; 12] =
        ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(timestamp));
    if date.get_time().is_nan() {
        return timestamp.to_string();
    }
    let month = MONTHS.get(date.get_month() as usize).copied().unwrap_or("");
    format!("{month} {}", date.get_date())
}

/// `new Date(timestamp).toLocaleTimeString([], {hour12: false})`.
pub fn time_of_day(timestamp: &str) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(timestamp));
    if date.get_time().is_nan() {
        return timestamp.to_string();
    }
    format!("{:02}:{:02}:{:02}", date.get_hours(), date.get_minutes(), date.get_seconds())
}

/// `new Date(timestamp).toLocaleString()`.
pub fn date_time(timestamp: &str) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(timestamp));
    if date.get_time().is_nan() {
        return timestamp.to_string();
    }
    // `toLocaleString()` — the zero-argument form, which is what the port meant.
    //
    // `Date::to_locale_string` cannot express it: its locale is a `&str`, and
    // `toLocaleString("")` is a `RangeError: Invalid language tag` because the
    // empty string is not a language tag. `Object::to_locale_string` is the
    // binding with no arguments, and on a Date it still dispatches to
    // `Date.prototype.toLocaleString`.
    //
    // This is not a cosmetic bug. A JS exception raised inside a render effect
    // escapes through wasm without running a single Rust destructor, so the
    // polling js-sys task keeps its borrow and panics with "RefCell already
    // borrowed" the next time it runs — and from then on the panel's tab branch
    // never renders again. That is the freeze the Status tab had.
    String::from(js_sys::Object::to_locale_string(date.unchecked_ref::<js_sys::Object>()))
}

/// ✓ / ● / ◆ / ✕ / ○ status glyph.
pub fn status_glyph(status: &str) -> &'static str {
    match status {
        "completed" => "✓",
        "running" | "starting" => "●",
        "failed" | "orphaned" | "cancelled" | "interrupted" => "✕",
        "awaiting_host" => "◆",
        _ => "○",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsoleKind {
    Read,
    Search,
    Edit,
    Command,
    Test,
    Result,
    Error,
    Warning,
    Status,
}

impl ConsoleKind {
    /// Translation key suffix: `console.read`, `console.search`, …
    pub fn key(self) -> &'static str {
        match self {
            ConsoleKind::Read => "console.read",
            ConsoleKind::Search => "console.search",
            ConsoleKind::Edit => "console.edit",
            ConsoleKind::Command => "console.command",
            ConsoleKind::Test => "console.test",
            ConsoleKind::Result => "console.result",
            ConsoleKind::Error => "console.error",
            ConsoleKind::Warning => "console.warning",
            ConsoleKind::Status => "console.status",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConsoleRow {
    pub id: String,
    pub kind: ConsoleKind,
    pub timestamp: String,
    pub label: String,
    pub additions: Option<f64>,
    pub deletions: Option<f64>,
    /// Repeated low-value events grouped into one row.
    pub count: Option<usize>,
}

/// Maps one observable event to a Console category, and returns `None` for
/// everything the inspector must not show: hidden reasoning, runtime-internal
/// child agents and relay lifecycle bookkeeping.
pub fn event_kind(event: &RelayEvent) -> Option<ConsoleKind> {
    use RelayEventType::*;
    match event.event_type {
        ToolRead => Some(ConsoleKind::Read),
        ToolSearch => Some(ConsoleKind::Search),
        ToolEdit => Some(ConsoleKind::Edit),
        ToolCommand => Some(ConsoleKind::Command),
        TestResult => Some(ConsoleKind::Test),
        ToolResult | WorkerCompleted => Some(ConsoleKind::Result),
        WorkerFailed | WorkerOrphaned => Some(ConsoleKind::Error),
        WorkerCancelled | WorkerInterrupted => Some(ConsoleKind::Warning),
        WorkerStarted | WorkerMessage | WorkerStatus | RunAwaitingHost | RunAccepted => Some(ConsoleKind::Status),
        RunCreated | StepCreated | StepIterationStarted | WorkerReasoning | ChildStarted | ChildCompleted => None,
    }
}

fn as_record(value: &Value) -> Option<&Map<String, Value>> {
    value.as_object()
}

/// `String(value)`: arrays join with commas, objects serialize like
/// `JSON.stringify`.
fn js_string(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => number.to_string(),
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(js_string).collect::<Vec<_>>().join(","),
        Value::Object(_) => serde_json::to_string(value).unwrap_or_default(),
    }
}

/// `data.path ?? data.file ?? … ?? data.tool`, then the fallbacks the inspector
/// has always shown for the three events that carry no text.
pub fn event_summary(event: &RelayEvent) -> String {
    let Some(data) = as_record(&event.data) else {
        return if event.data.is_null() { event.event_type.as_str().to_string() } else { js_string(&event.data) };
    };
    for key in ["path", "file", "files", "command", "query", "summary", "text", "message", "result", "status", "tool"]
    {
        if let Some(value) = data.get(key).filter(|value| !value.is_null()) {
            return match value {
                Value::Array(items) => {
                    if items.len() == 1 {
                        js_string(&items[0])
                    } else {
                        items.len().to_string()
                    }
                }
                Value::String(text) => text.clone(),
                other => serde_json::to_string(other).unwrap_or_default(),
            };
        }
    }
    match event.event_type {
        RelayEventType::WorkerStarted => data
            .get("worker")
            .and_then(as_record)
            .and_then(|worker| worker.get("runtimeId"))
            .and_then(Value::as_str)
            // `worker?.runtimeId ? … : "Worker started"`: an empty string is falsy.
            .filter(|runtime| !runtime.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| "Worker started".to_string()),
        RelayEventType::RunAwaitingHost => "Worker finished; waiting for Codex review".to_string(),
        RelayEventType::RunAccepted => "Accepted by Codex".to_string(),
        _ => event.event_type.as_str().to_string(),
    }
}

/// `Number(value)`: `null` and `""` are 0, anything unparseable is NaN.
fn as_number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(flag)) => {
            if *flag {
                1.0
            } else {
                0.0
            }
        }
        Some(Value::Number(number)) => number.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(text)) => {
            let trimmed = text.trim();
            if trimmed.is_empty() {
                0.0
            } else {
                trimmed.parse::<f64>().unwrap_or(f64::NAN)
            }
        }
        Some(Value::Array(items)) => {
            if items.is_empty() {
                0.0
            } else {
                f64::NAN
            }
        }
        Some(Value::Object(_)) => f64::NAN,
    }
}

fn pick<'a>(diff: Option<&'a Map<String, Value>>, key: &str, data: &'a Map<String, Value>) -> Option<&'a Value> {
    diff.and_then(|map| map.get(key))
        .filter(|value| !value.is_null())
        .or_else(|| data.get(key).filter(|value| !value.is_null()))
}

/// Diff stat for an edit, when the CLI reported one ("+42 -12").
fn diff_stat(event: &RelayEvent) -> Option<(f64, f64)> {
    let data = as_record(&event.data)?;
    let diff = data.get("diff").and_then(as_record);
    let additions = as_number(pick(diff, "additions", data));
    let deletions = as_number(pick(diff, "deletions", data));
    if !additions.is_finite() && !deletions.is_finite() {
        return None;
    }
    Some((
        if additions.is_finite() { additions } else { 0.0 },
        if deletions.is_finite() { deletions } else { 0.0 },
    ))
}

/// The one-line replacement for a completion whose final answer is already on
/// screen as the worker's final message.
const COMPLETION_STATUS_LABEL: &str = "Completed";

/// `(worker, text)` for a final assistant message: `worker/message` with
/// `kind == "final"`. The worker session id is what keeps two iterations from
/// collapsing into each other, because every iteration runs as its own worker.
fn final_message(event: &RelayEvent) -> Option<(&str, &str)> {
    if event.event_type != RelayEventType::WorkerMessage {
        return None;
    }
    let data = event.data.as_object()?;
    if data.get("kind").and_then(Value::as_str) != Some("final") {
        return None;
    }
    let text = data.get("text").and_then(Value::as_str)?;
    Some((event.worker_session_id.as_deref()?, text))
}

/// `(worker, summary)` for a completion event, when it carries one.
fn completion_summary(event: &RelayEvent) -> Option<(&str, &str)> {
    if event.event_type != RelayEventType::WorkerCompleted {
        return None;
    }
    let data = event.data.as_object()?;
    let summary = data.get("summary").and_then(Value::as_str)?;
    Some((event.worker_session_id.as_deref()?, summary))
}

/// Ids of `worker/completed` events whose summary repeats, verbatim, the final
/// message of the same worker.
///
/// The two rows are the same answer twice and the console must show it once.
/// The match is deliberately narrow: same worker and byte-identical content.
/// A different worker, a different round of the same worker, or any difference
/// in the text keeps the completion's own summary, so nothing is ever lost.
fn repeated_completions(events: &[RelayEvent]) -> BTreeSet<String> {
    let mut finals: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    let mut repeated = BTreeSet::new();
    for event in events {
        if let Some((worker, text)) = final_message(event) {
            if !text.is_empty() {
                finals.entry(worker).or_default().push(text);
            }
        }
        if let Some((worker, summary)) = completion_summary(event) {
            if summary.is_empty() {
                continue;
            }
            if let Some(texts) = finals.get_mut(worker) {
                if let Some(position) = texts.iter().position(|text| *text == summary) {
                    // Pair a completion with one final message at most: a second
                    // completion is its own fact and keeps its full summary.
                    texts.remove(position);
                    repeated.insert(event.id.clone());
                }
            }
        }
    }
    repeated
}

fn to_row(event: &RelayEvent, repeated_completion: bool) -> Option<ConsoleRow> {
    let kind = event_kind(event)?;
    let stat =
        if kind == ConsoleKind::Edit && !repeated_completion { diff_stat(event) } else { None };
    Some(ConsoleRow {
        id: event.id.clone(),
        kind,
        timestamp: event.timestamp.clone(),
        label: if repeated_completion {
            COMPLETION_STATUS_LABEL.to_string()
        } else {
            event_summary(event)
        },
        additions: stat.map(|(additions, _)| additions),
        deletions: stat.map(|(_, deletions)| deletions),
        count: None,
    })
}

/// Builds Console rows, collapsing runs of three or more consecutive Read/Search
/// events into one summary row so repetitive low-value work stays quiet.
///
/// A completion that repeats its worker's final message is shown as a short
/// status instead of the same long answer twice. Both events stay in the log and
/// in the Raw view; only the Console row is shortened.
pub fn console_rows(events: &[RelayEvent]) -> Vec<ConsoleRow> {
    let repeated = repeated_completions(events);
    let mut rows: Vec<ConsoleRow> = Vec::new();
    let mut index = 0usize;
    while index < events.len() {
        let Some(row) = to_row(&events[index], repeated.contains(&events[index].id)) else {
            index += 1;
            continue;
        };
        if row.kind != ConsoleKind::Read && row.kind != ConsoleKind::Search {
            rows.push(row);
            index += 1;
            continue;
        }
        let mut end = index;
        while end + 1 < events.len() {
            match to_row(&events[end + 1], repeated.contains(&events[end + 1].id)) {
                Some(next) if next.kind == row.kind => end += 1,
                _ => break,
            }
        }
        let run = &events[index..=end];
        if run.len() >= 3 {
            rows.push(ConsoleRow {
                id: format!("{}:group", row.id),
                kind: row.kind,
                timestamp: row.timestamp,
                label: row.label,
                additions: None,
                deletions: None,
                count: Some(run.len()),
            });
        } else {
            for item in run {
                if let Some(single) = to_row(item, repeated.contains(&item.id)) {
                    rows.push(single);
                }
            }
        }
        index = end + 1;
    }
    rows
}

/// Every event the inspector shows, capped at the tail of 400 and newest first —
/// the Raw view's projection.
pub fn visible_events(events: &[RelayEvent]) -> Vec<RelayEvent> {
    let mut shown: Vec<RelayEvent> =
        events.iter().filter(|event| event_kind(event).is_some()).cloned().collect();
    if shown.len() > RAW_LIMIT {
        shown.drain(..shown.len() - RAW_LIMIT);
    }
    shown.reverse();
    shown
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChangedFile {
    pub path: String,
    pub additions: f64,
    pub deletions: f64,
}

/// File changes reported by edit events, for the Changes view.
pub fn changed_files(events: &[RelayEvent]) -> Vec<ChangedFile> {
    let mut files: BTreeMap<String, ChangedFile> = BTreeMap::new();
    for event in events {
        if event_kind(event) != Some(ConsoleKind::Edit) {
            continue;
        }
        let Some(data) = as_record(&event.data) else {
            continue;
        };
        let path = data
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| data.get("file").and_then(Value::as_str))
            .map(str::to_string);
        let Some(path) = path else {
            continue;
        };
        let (additions, deletions) = diff_stat(event).unwrap_or((0.0, 0.0));
        let entry = files.entry(path.clone()).or_insert(ChangedFile { path, additions: 0.0, deletions: 0.0 });
        entry.additions += additions;
        entry.deletions += deletions;
    }
    files.into_values().collect()
}

/// Events for one Step. Worker events belong to the step that owns their worker;
/// run-level events (created, awaiting Codex, accepted) have neither a step nor a
/// worker, so they stay visible on every step instead of disappearing.
pub fn events_for_step(view: &RunView, step_id: &str, events: &[RelayEvent]) -> Vec<RelayEvent> {
    let worker_ids: Vec<&str> =
        view.workers.iter().filter(|worker| worker.step_id == step_id).map(|worker| worker.id.as_str()).collect();
    events
        .iter()
        .filter(|event| {
            if let Some(event_step) = event.step_id.as_deref() {
                return event_step == step_id;
            }
            if let Some(worker) = event.worker_session_id.as_deref() {
                return worker_ids.contains(&worker);
            }
            true
        })
        .cloned()
        .collect()
}

/// `+42` for a diff stat; whole numbers print without a decimal point, like
/// JavaScript numbers do.
pub fn number(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{}", value as i64)
    } else {
        format!("{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(
        id: &str,
        seq: u64,
        worker: Option<&str>,
        event_type: RelayEventType,
        data: Value,
    ) -> RelayEvent {
        RelayEvent {
            id: id.to_string(),
            run_id: "run-1".to_string(),
            step_id: Some("step-1".to_string()),
            worker_session_id: worker.map(str::to_string),
            seq,
            timestamp: "2026-09-28T00:00:00.000Z".to_string(),
            event_type,
            data,
            native_event: None,
        }
    }

    fn final_message(id: &str, seq: u64, worker: &str, text: &str) -> RelayEvent {
        event(
            id,
            seq,
            Some(worker),
            RelayEventType::WorkerMessage,
            serde_json::json!({ "kind": "final", "text": text }),
        )
    }

    fn completed(id: &str, seq: u64, worker: &str, summary: &str) -> RelayEvent {
        event(
            id,
            seq,
            Some(worker),
            RelayEventType::WorkerCompleted,
            serde_json::json!({ "summary": summary, "exitCode": 0 }),
        )
    }

    fn labels_for(rows: &[ConsoleRow], text: &str) -> usize {
        rows.iter().filter(|row| row.label == text).count()
    }

    /// The reported case: `worker/message(kind=final,text)` and
    /// `worker/completed(summary)` carry the same 13,854-character answer for the
    /// same worker. The Console must show the answer once and the completion as a
    /// short status.
    #[test]
    fn a_completion_repeating_its_final_message_is_not_expanded_twice() {
        let answer = "x".repeat(13_854);
        let events = vec![
            final_message("m1", 4, "worker-1", &answer),
            completed("c1", 5, "worker-1", &answer),
        ];
        let rows = console_rows(&events);
        assert_eq!(rows.len(), 2);
        assert_eq!(labels_for(&rows, &answer), 1, "the long answer must appear once");
        let completion = rows.iter().find(|row| row.id == "c1").unwrap();
        assert_eq!(completion.label, COMPLETION_STATUS_LABEL);
        assert_eq!(completion.kind, ConsoleKind::Result);

        // Raw output keeps the original events: nothing is dropped from the log.
        let raw = visible_events(&events);
        assert_eq!(raw.len(), 2);
        assert!(raw.iter().any(|event| event.id == "m1"));
        assert!(raw.iter().any(|event| event.id == "c1"));
    }

    /// The counter-example the fix must not break: a different worker's
    /// completion is never collapsed by another worker's identical message.
    #[test]
    fn a_completion_from_another_worker_keeps_its_summary() {
        let answer = "same text".to_string();
        let events = vec![
            final_message("m1", 4, "worker-1", &answer),
            completed("c2", 5, "worker-2", &answer),
        ];
        let rows = console_rows(&events);
        assert_eq!(labels_for(&rows, &answer), 2);
        assert!(rows.iter().all(|row| row.label != COMPLETION_STATUS_LABEL));
    }

    /// Different final content is two different answers; neither is suppressed.
    #[test]
    fn a_completion_with_different_text_keeps_its_summary() {
        let events = vec![
            final_message("m1", 4, "worker-1", "the message"),
            completed("c1", 5, "worker-1", "the completion summary"),
        ];
        let rows = console_rows(&events);
        assert_eq!(labels_for(&rows, "the message"), 1);
        assert_eq!(labels_for(&rows, "the completion summary"), 1);
        assert!(rows.iter().all(|row| row.label != COMPLETION_STATUS_LABEL));
    }

    /// Multiple rounds of the same worker keep every message; only the completion
    /// that repeats the latest one is shortened.
    #[test]
    fn multiple_rounds_of_one_worker_keep_every_message() {
        let events = vec![
            final_message("m1", 4, "worker-1", "first answer"),
            final_message("m2", 5, "worker-1", "second answer"),
            completed("c1", 6, "worker-1", "second answer"),
        ];
        let rows = console_rows(&events);
        assert_eq!(labels_for(&rows, "first answer"), 1);
        assert_eq!(labels_for(&rows, "second answer"), 1);
        assert_eq!(labels_for(&rows, COMPLETION_STATUS_LABEL), 1);
    }

    /// A completion with no matching final message is untouched.
    #[test]
    fn a_lone_completion_keeps_its_summary() {
        let events = vec![completed("c1", 4, "worker-1", "only here")];
        let rows = console_rows(&events);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "only here");
    }

    /// A delta is progress, not the final answer, so it never pairs with a
    /// completion — even when the text happens to be identical.
    #[test]
    fn a_delta_message_never_replaces_a_completion() {
        let events = vec![
            event(
                "m1",
                4,
                Some("worker-1"),
                RelayEventType::WorkerMessage,
                serde_json::json!({ "kind": "delta", "text": "answer" }),
            ),
            completed("c1", 5, "worker-1", "answer"),
        ];
        let rows = console_rows(&events);
        assert_eq!(labels_for(&rows, "answer"), 2);
    }
}
