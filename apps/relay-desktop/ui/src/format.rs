//! Console formatting: a line-by-line port of `apps/web/src/lib/format.ts` and
//! `apps/web/src/lib/step-events.ts`.
//!
//! The inspector's whole reading experience is these functions, so the rules are
//! copied exactly: which events are visible, how a run of reads collapses, how a
//! diff stat is summed and how a status becomes a glyph.

use std::collections::{BTreeMap, BTreeSet};

use relay_api::RunView;
use relay_core::{worker_text, RelayEvent, RelayEventType, RunStatus, StepStatus};
use serde_json::{Map, Value};

use crate::dom;
use crate::i18n::{Locale, Translator};
use leptos::prelude::Get;

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

pub fn elapsed(start: &str, end: Option<&str>, locale: Locale) -> String {
    let end_ms = end.map(dom::parse_ms).unwrap_or_else(dom::now_ms);
    let total = (end_ms - dom::parse_ms(start)).max(0.0);
    duration_text(
        if total.is_finite() {
            (total / 1000.0).floor() as u64
        } else {
            0
        },
        locale,
    )
}

fn duration_text(seconds: u64, locale: Locale) -> String {
    let (s, m, h) = match locale {
        Locale::En => ("s", "m", "h"),
        Locale::ZhCn => ("秒", "分", "小时"),
    };
    if seconds < 60 {
        format!("{seconds}{s}")
    } else if seconds < 3600 {
        format!("{}{m} {}{s}", seconds / 60, seconds % 60)
    } else {
        format!("{}{h} {}{m}", seconds / 3600, seconds / 60 % 60)
    }
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
        return format!(
            "{}{}",
            seconds as u64,
            if t.locale.get() == Locale::ZhCn {
                "秒"
            } else {
                "s"
            }
        );
    }
    let minutes = (seconds / 60.0).floor();
    if minutes < 60.0 {
        return format!(
            "{}{}",
            minutes as u64,
            if t.locale.get() == Locale::ZhCn {
                "分"
            } else {
                "m"
            }
        );
    }
    let hours = (minutes / 60.0).floor();
    if hours < 24.0 {
        return format!(
            "{}{}",
            hours as u64,
            if t.locale.get() == Locale::ZhCn {
                "小时"
            } else {
                "h"
            }
        );
    }
    date_short(timestamp, t.locale.get())
}

/// Month names localized based on user's active locale.
pub fn date_short(timestamp: &str, locale: Locale) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(timestamp));
    if date.get_time().is_nan() {
        return timestamp.to_string();
    }
    if locale == Locale::ZhCn {
        format!("{}月{}日", date.get_month() + 1, date.get_date())
    } else {
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        let month = MONTHS.get(date.get_month() as usize).copied().unwrap_or("");
        format!("{month} {}", date.get_date())
    }
}

/// `new Date(timestamp).toLocaleTimeString([], {hour12: false})`.
pub fn time_of_day(timestamp: &str) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(timestamp));
    if date.get_time().is_nan() {
        return timestamp.to_string();
    }
    format!(
        "{:02}:{:02}:{:02}",
        date.get_hours(),
        date.get_minutes(),
        date.get_seconds()
    )
}

/// `new Date(timestamp).toLocaleString()`.
pub fn date_time(timestamp: &str, locale: Locale) -> String {
    let date = js_sys::Date::new(&wasm_bindgen::JsValue::from_str(timestamp));
    if date.get_time().is_nan() {
        return timestamp.to_string();
    }
    String::from(date.to_locale_string(locale.code(), &js_sys::Object::new().into()))
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
/// child agents, relay lifecycle bookkeeping and assistant-text increments.
///
/// An increment is not a fact of its own: `worker_text::assistant_text` merges
/// them into the assistant messages, and a chunk-per-row log would drown the
/// activity that actually happened.
pub fn event_kind(event: &RelayEvent) -> Option<ConsoleKind> {
    use RelayEventType::*;
    if worker_text::delta(event).is_some() {
        return None;
    }
    match event.event_type {
        ToolRead => Some(ConsoleKind::Read),
        ToolSearch => Some(ConsoleKind::Search),
        ToolEdit => Some(ConsoleKind::Edit),
        ToolCommand => Some(ConsoleKind::Command),
        TestResult => Some(ConsoleKind::Test),
        ToolResult | WorkerCompleted => Some(ConsoleKind::Result),
        WorkerFailed | WorkerOrphaned => Some(ConsoleKind::Error),
        WorkerCancelled | WorkerInterrupted => Some(ConsoleKind::Warning),
        WorkerStarted | WorkerMessage | WorkerStatus | RunAwaitingHost | RunAccepted => {
            Some(ConsoleKind::Status)
        }
        RunCreated | StepCreated | StepIterationStarted | WorkerReasoning | ChildStarted
        | ChildCompleted => None,
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
        return if event.data.is_null() {
            event.event_type.as_str().to_string()
        } else {
            js_string(&event.data)
        };
    };
    for key in [
        "path", "file", "files", "command", "query", "summary", "text", "message", "result",
        "status", "tool",
    ] {
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

fn pick<'a>(
    diff: Option<&'a Map<String, Value>>,
    key: &str,
    data: &'a Map<String, Value>,
) -> Option<&'a Value> {
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
        if additions.is_finite() {
            additions
        } else {
            0.0
        },
        if deletions.is_finite() {
            deletions
        } else {
            0.0
        },
    ))
}

/// The one-line replacement for a completion whose final answer is already on
/// screen as the worker's assistant message.
const COMPLETION_STATUS_LABEL: &str = "Completed";

/// Ids of `worker/completed` events whose summary repeats assistant text that is
/// already displayed.
///
/// The two rows would be the same answer twice and the console must show it
/// once. The rule lives in `relay_core::worker_text::assistant_text` so every
/// surface resolves it identically; the match is deliberately narrow — the same
/// worker and byte-identical content. A different worker, a different round of
/// the same worker, or any difference in the text keeps the completion's own
/// summary, so nothing is ever lost.
fn repeated_completions(events: &[RelayEvent]) -> BTreeSet<String> {
    worker_text::assistant_text(events)
        .into_iter()
        .flat_map(|worker| worker.repeated_completion_ids)
        .collect()
}

fn to_row(event: &RelayEvent, repeated_completion: bool) -> Option<ConsoleRow> {
    // Assistant answers are rendered as message bubbles; the activity log
    // records what the worker did, never a second copy of what it said.
    if worker_text::final_text(event).is_some() {
        return None;
    }
    let kind = event_kind(event)?;
    let stat = if kind == ConsoleKind::Edit && !repeated_completion {
        diff_stat(event)
    } else {
        None
    };
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
    let mut shown: Vec<RelayEvent> = events
        .iter()
        .filter(|event| event_kind(event).is_some())
        .cloned()
        .collect();
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
        let entry = files.entry(path.clone()).or_insert(ChangedFile {
            path,
            additions: 0.0,
            deletions: 0.0,
        });
        entry.additions += additions;
        entry.deletions += deletions;
    }
    files.into_values().collect()
}

/// Events for one Step. Worker events belong to the step that owns their worker;
/// run-level events (created, awaiting Codex, accepted) have neither a step nor a
/// worker, so they stay visible on every step instead of disappearing.
pub fn events_for_step(view: &RunView, step_id: &str, events: &[RelayEvent]) -> Vec<RelayEvent> {
    let worker_ids: Vec<&str> = view
        .workers
        .iter()
        .filter(|worker| worker.step_id == step_id)
        .map(|worker| worker.id.as_str())
        .collect();
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

/// Heuristic to detect whether a text string contains markdown formatting or multiple lines.
pub fn is_markdown_content(input: &str) -> bool {
    input.contains('\n')
        || input.contains("```")
        || input.contains("**")
        || input.contains("__")
        || input.contains("##")
        || input.starts_with("# ")
        || input.starts_with("- ")
        || input.starts_with("* ")
}

pub fn html_escape(s: &str) -> String {
    let mut escaped = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(c),
        }
    }
    escaped
}

/// CommonMark/GFM, with raw HTML displayed literally and unsafe URL schemes
/// removed before the generated HTML reaches the webview.
pub fn markdown_to_html(input: &str) -> String {
    use pulldown_cmark::{html, Event, Options, Parser, Tag};
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_FOOTNOTES;
    let events = Parser::new_ext(input, options).map(|event| match event {
        Event::Html(text) | Event::InlineHtml(text) => Event::Text(text),
        Event::Start(Tag::Link {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let dest_url = if safe_markdown_url(&dest_url) {
                dest_url
            } else {
                "#".into()
            };
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                title,
                id,
            })
        }
        Event::Start(Tag::Image {
            link_type,
            dest_url,
            title,
            id,
        }) => {
            let dest_url = if safe_markdown_url(&dest_url) {
                dest_url
            } else {
                "".into()
            };
            Event::Start(Tag::Image {
                link_type,
                dest_url,
                title,
                id,
            })
        }
        event => event,
    });
    let mut out = String::new();
    html::push_html(&mut out, events);
    out
}

fn safe_markdown_url(url: &str) -> bool {
    if url.chars().any(|c| c.is_control()) {
        return false;
    }
    let normalized = url.trim().to_ascii_lowercase();
    match normalized.split_once(':') {
        Some((scheme, _)) => matches!(scheme, "http" | "https" | "mailto"),
        None => true,
    }
}

/// Translates ANSI terminal escape sequences into styled HTML spans.
pub fn ansi_to_html(input: &str) -> String {
    let mut out = String::with_capacity(input.len() * 3 / 2);
    let bytes = input.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    let mut open_spans = 0;

    while i < len {
        if bytes[i] == 0x1b && i + 1 < len && bytes[i + 1] == b'[' {
            let mut j = i + 2;
            while j < len && bytes[j] != b'm' && (bytes[j].is_ascii_digit() || bytes[j] == b';') {
                j += 1;
            }
            if j < len && bytes[j] == b'm' {
                let code_str = std::str::from_utf8(&bytes[i + 2..j]).unwrap_or("");
                let codes: Vec<u32> = code_str.split(';').filter_map(|s| s.parse().ok()).collect();
                if codes.is_empty() || codes.contains(&0) {
                    for _ in 0..open_spans {
                        out.push_str("</span>");
                    }
                    open_spans = 0;
                }
                for &c in &codes {
                    let class = match c {
                        0 => "",
                        1 => "ansi-bold",
                        2 => "ansi-dim",
                        3 => "ansi-italic",
                        4 => "ansi-underline",
                        30 => "ansi-fg-black",
                        31 => "ansi-fg-red",
                        32 => "ansi-fg-green",
                        33 => "ansi-fg-yellow",
                        34 => "ansi-fg-blue",
                        35 => "ansi-fg-magenta",
                        36 => "ansi-fg-cyan",
                        37 => "ansi-fg-white",
                        90 => "ansi-fg-bright-black",
                        91 => "ansi-fg-bright-red",
                        92 => "ansi-fg-bright-green",
                        93 => "ansi-fg-bright-yellow",
                        94 => "ansi-fg-bright-blue",
                        95 => "ansi-fg-bright-magenta",
                        96 => "ansi-fg-bright-cyan",
                        97 => "ansi-fg-bright-white",
                        _ => "",
                    };
                    if !class.is_empty() {
                        out.push_str(&format!("<span class=\"{}\">", class));
                        open_spans += 1;
                    }
                }
                i = j + 1;
                continue;
            }
        }

        match bytes[i] {
            b'&' => out.push_str("&amp;"),
            b'<' => out.push_str("&lt;"),
            b'>' => out.push_str("&gt;"),
            b'"' => out.push_str("&quot;"),
            b'\'' => out.push_str("&#39;"),
            b'\n' => out.push('\n'),
            _ => {
                let c = input[i..].chars().next().expect("character boundary");
                out.push(c);
                i += c.len_utf8();
                continue;
            }
        }
        i += 1;
    }

    for _ in 0..open_spans {
        out.push_str("</span>");
    }

    out
}

/// Formats a unified diff string into line-level HTML with red/green highlights.
pub fn diff_to_html(diff_content: &str) -> String {
    let mut out = String::with_capacity(diff_content.len() * 3 / 2);
    out.push_str("<div class=\"diff-container\">");

    for line in diff_content.lines() {
        let escaped = html_escape(line);
        if line.starts_with("+++") || line.starts_with("---") {
            out.push_str(&format!(
                "<div class=\"diff-line diff-file-header\"><code>{}</code></div>",
                escaped
            ));
        } else if line.starts_with("@@") {
            out.push_str(&format!(
                "<div class=\"diff-line diff-hunk\"><code>{}</code></div>",
                escaped
            ));
        } else if line.starts_with('+') {
            let code_part = if escaped.len() > 1 { &escaped[1..] } else { "" };
            out.push_str(&format!("<div class=\"diff-line diff-add\"><span class=\"diff-sign\">+</span><code>{}</code></div>", code_part));
        } else if line.starts_with('-') {
            let code_part = if escaped.len() > 1 { &escaped[1..] } else { "" };
            out.push_str(&format!("<div class=\"diff-line diff-del\"><span class=\"diff-sign\">-</span><code>{}</code></div>", code_part));
        } else {
            let rest = if line.starts_with(' ') && escaped.len() > 1 {
                &escaped[1..]
            } else {
                &escaped
            };
            out.push_str(&format!("<div class=\"diff-line diff-context\"><span class=\"diff-sign\">&nbsp;</span><code>{}</code></div>", rest));
        }
    }

    out.push_str("</div>");
    out
}

/// Attempts to find unified diff content for a given file path from step events.
pub fn diff_for_path(events: &[RelayEvent], file_path: &str) -> Option<String> {
    for event in events {
        if event.event_type != RelayEventType::ToolEdit {
            continue;
        }
        let Some(data) = as_record(&event.data) else {
            continue;
        };
        let path = data
            .get("path")
            .and_then(Value::as_str)
            .or_else(|| data.get("file").and_then(Value::as_str))
            .or_else(|| {
                event
                    .data
                    .pointer("/parameters/TargetFile")
                    .and_then(Value::as_str)
            });
        if path != Some(file_path) {
            continue;
        }

        if let Some(diff) = data.get("diff").and_then(Value::as_str) {
            if !diff.is_empty() {
                return Some(diff.to_string());
            }
        }
        if let Some(patch) = data.get("patch").and_then(Value::as_str) {
            if !patch.is_empty() {
                return Some(patch.to_string());
            }
        }

        let before = event
            .data
            .pointer("/parameters/TargetContent")
            .and_then(Value::as_str)
            .or_else(|| data.get("old_string").and_then(Value::as_str))
            .or_else(|| data.get("before").and_then(Value::as_str));
        let after = event
            .data
            .pointer("/parameters/ReplacementContent")
            .and_then(Value::as_str)
            .or_else(|| data.get("new_string").and_then(Value::as_str))
            .or_else(|| data.get("after").and_then(Value::as_str));
        if let (Some(before), Some(after)) = (before, after) {
            return Some(snippet_diff(before, after, file_path));
        }
    }
    None
}

/// Runtime edit snippets carry partial file contents; compute real context and
/// hunk counts rather than labelling every original line as deleted.
fn snippet_diff(before: &str, after: &str, path: &str) -> String {
    similar::TextDiff::configure()
        .algorithm(similar::Algorithm::Patience)
        .diff_lines(before, after)
        .unified_diff()
        .context_radius(3)
        .header(&format!("a/{path}"), &format!("b/{path}"))
        .to_string()
}

/// Finds command output text associated with a ToolCommand event.
pub fn command_output_for_event(events: &[RelayEvent], command_event_id: &str) -> Option<String> {
    let cmd_idx = events.iter().position(|e| e.id == command_event_id)?;
    let cmd_call_id = events[cmd_idx].data.get("callId").and_then(Value::as_str);

    for ev in &events[cmd_idx + 1..] {
        if ev.event_type == RelayEventType::ToolResult {
            if let Some(cid) = cmd_call_id {
                if ev.data.get("callId").and_then(Value::as_str) == Some(cid) {
                    return extract_output_text(&ev.data);
                }
            } else {
                return extract_output_text(&ev.data);
            }
        }
    }
    None
}

fn extract_output_text(data: &Value) -> Option<String> {
    for key in ["result", "stdout", "output", "text", "message"] {
        if let Some(val) = data.get(key) {
            if let Some(s) = val.as_str() {
                if !s.is_empty() {
                    return Some(s.to_string());
                }
            }
        }
    }
    None
}

/// Extracts the main AI assistant messages from the step events.
///
/// The merge rules are Relay's shared worker-text aggregation
/// (`relay_core::worker_text::assistant_text`): increments of one message are
/// appended in order, an authoritative `final` supersedes the stream that
/// spelled it out, and an answer is never displayed twice.
pub fn step_ai_messages(events: &[RelayEvent]) -> Vec<String> {
    worker_text::assistant_text(events)
        .into_iter()
        .flat_map(|worker| worker.messages)
        .map(|message| message.text)
        .collect()
}

/// Extracts reasoning / thinking text from step events (from WorkerReasoning events).
/// Combines continuous reasoning chunks into a single unified thinking text.
pub fn step_reasoning_text(events: &[RelayEvent]) -> Option<String> {
    let mut combined = String::new();
    for event in events {
        if event.event_type == RelayEventType::WorkerReasoning {
            if let Some(text) = event.data.get("text").and_then(|v| v.as_str()) {
                combined.push_str(text);
            }
        }
    }
    let trimmed = combined.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Extracts reasoning / thinking messages from step events (from WorkerReasoning events).
/// Returns the combined reasoning as a single message to avoid fragmented displays.
#[allow(dead_code)]
pub fn step_reasoning_messages(events: &[RelayEvent]) -> Vec<String> {
    step_reasoning_text(events).into_iter().collect()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct StepProcessSummary {
    pub total_actions: usize,
    pub reads: usize,
    pub searches: usize,
    pub edits: usize,
    pub additions: f64,
    pub deletions: f64,
    pub commands: usize,
    pub subagents: usize,
    pub active_action: Option<String>,
}

/// Extracts a compact summary of tool actions, file diffs, commands and subagents.
pub fn step_process_summary(events: &[RelayEvent]) -> StepProcessSummary {
    let mut summary = StepProcessSummary::default();
    let mut last_action = None;

    for event in events {
        match event.event_type {
            RelayEventType::ToolRead => {
                summary.reads += 1;
                summary.total_actions += 1;
                let target = event_summary(event);
                last_action = Some(format!("Reading {target}"));
            }
            RelayEventType::ToolSearch => {
                summary.searches += 1;
                summary.total_actions += 1;
                let target = event_summary(event);
                last_action = Some(format!("Searching {target}"));
            }
            RelayEventType::ToolEdit => {
                summary.edits += 1;
                summary.total_actions += 1;
                if let Some((add, del)) = diff_stat(event) {
                    summary.additions += add;
                    summary.deletions += del;
                }
                let target = event_summary(event);
                last_action = Some(format!("Editing {target}"));
            }
            RelayEventType::ToolCommand => {
                summary.commands += 1;
                summary.total_actions += 1;
                let cmd = event_summary(event);
                last_action = Some(format!("$ {cmd}"));
            }
            RelayEventType::ChildStarted => {
                summary.subagents += 1;
                summary.total_actions += 1;
                let subagent_name = event
                    .data
                    .get("role")
                    .or_else(|| event.data.get("name"))
                    .and_then(Value::as_str)
                    .unwrap_or("subagent");
                last_action = Some(format!("Subagent {subagent_name}"));
            }
            _ => {}
        }
    }
    summary.active_action = last_action;
    summary
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

    /// One increment in the unified worker-text contract.
    fn text_delta(id: &str, seq: u64, worker: &str, text: &str, starts_message: bool) -> RelayEvent {
        let data = if starts_message {
            relay_core::AssistantTextDelta::message(text)
        } else {
            relay_core::AssistantTextDelta::chunk(text)
        };
        event(
            id,
            seq,
            Some(worker),
            RelayEventType::WorkerMessage,
            data.into_data(),
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
    /// same worker. The answer is one assistant message and the completion is a
    /// short status.
    #[test]
    fn a_completion_repeating_its_final_message_is_not_expanded_twice() {
        let answer = "x".repeat(13_854);
        let events = vec![
            final_message("m1", 4, "worker-1", &answer),
            completed("c1", 5, "worker-1", &answer),
        ];
        assert_eq!(
            step_ai_messages(&events),
            vec![answer.clone()],
            "the long answer is one message"
        );
        let rows = console_rows(&events);
        assert_eq!(rows.len(), 1);
        assert_eq!(labels_for(&rows, &answer), 0);
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
        assert_eq!(labels_for(&rows, &answer), 1);
        assert!(rows.iter().all(|row| row.label != COMPLETION_STATUS_LABEL));
        assert_eq!(step_ai_messages(&events), vec![answer]);
    }

    /// Different final content is two different answers; neither is suppressed.
    #[test]
    fn a_completion_with_different_text_keeps_its_summary() {
        let events = vec![
            final_message("m1", 4, "worker-1", "the message"),
            completed("c1", 5, "worker-1", "the completion summary"),
        ];
        let rows = console_rows(&events);
        assert_eq!(labels_for(&rows, "the completion summary"), 1);
        assert!(rows.iter().all(|row| row.label != COMPLETION_STATUS_LABEL));
        assert_eq!(step_ai_messages(&events), vec!["the message".to_string()]);
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
        assert_eq!(
            step_ai_messages(&events),
            vec!["first answer".to_string(), "second answer".to_string()]
        );
        let rows = console_rows(&events);
        assert_eq!(labels_for(&rows, "first answer"), 0);
        assert_eq!(labels_for(&rows, "second answer"), 0);
        assert_eq!(labels_for(&rows, COMPLETION_STATUS_LABEL), 1);
    }

    /// A completion with no assistant message of its own is untouched.
    #[test]
    fn a_lone_completion_keeps_its_summary() {
        let events = vec![completed("c1", 4, "worker-1", "only here")];
        let rows = console_rows(&events);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, "only here");
        assert!(step_ai_messages(&events).is_empty());
    }

    /// Increments merge into one message, and none of them becomes an activity
    /// row of its own.
    #[test]
    fn streamed_chunks_merge_into_one_assistant_message() {
        let events = vec![
            text_delta("d1", 4, "worker-1", "Hello ", true),
            text_delta("d2", 5, "worker-1", "world", false),
        ];
        assert_eq!(step_ai_messages(&events), vec!["Hello world".to_string()]);
        assert!(console_rows(&events).is_empty());
        assert!(
            visible_events(&events).is_empty(),
            "an increment is not a standalone event"
        );
    }

    /// A completion that repeats the streamed answer is a status, not a second
    /// copy of it; the answer itself stays one assistant message.
    #[test]
    fn a_completion_repeating_the_streamed_answer_is_not_expanded_twice() {
        let events = vec![
            text_delta("d1", 4, "worker-1", "answer", true),
            completed("c1", 5, "worker-1", "answer"),
        ];
        assert_eq!(step_ai_messages(&events), vec!["answer".to_string()]);
        let rows = console_rows(&events);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].label, COMPLETION_STATUS_LABEL);
    }

    /// A `final` supersedes the increments that spelled it out, so the answer is
    /// displayed once even though both a stream and a final were recorded.
    #[test]
    fn a_final_is_not_repeated_by_the_stream_that_spelled_it() {
        let events = vec![
            text_delta("d1", 4, "worker-1", "Hello ", true),
            text_delta("d2", 5, "worker-1", "world", false),
            final_message("m1", 6, "worker-1", "Hello world"),
        ];
        assert_eq!(step_ai_messages(&events), vec!["Hello world".to_string()]);
        let raw = visible_events(&events);
        assert_eq!(raw.len(), 1);
        assert_eq!(raw[0].id, "m1");
    }

    #[test]
    fn markdown_parser_handles_headings_lists_code_and_formatting() {
        let md = "# Title\n\nHere is **bold** and `code`.\n\n- item 1\n- item 2\n\n```rust\nfn main() {}\n```";
        let html = markdown_to_html(md);
        assert!(html.contains("<h1>Title</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<code>code</code>"));
        assert!(html.contains("<ul>"));
        assert!(html.contains("<li>item 1</li>"));
        assert!(html.contains("<li>item 2</li>"));
        assert!(html.contains("<pre><code class=\"language-rust\">fn main() {}"));
    }

    #[test]
    fn gfm_handles_tables_tasks_nested_lists_and_quotes_safely() {
        let html = markdown_to_html("| A | B |\n| - | - |\n| 1 | 2 |\n\n- [x] done\n  - nested\n\n> quote\n\n<script>alert(1)</script>\n\n[x](javascript:alert%281%29)");
        assert!(html.contains("<table>"));
        assert!(html.contains("type=\"checkbox\""));
        assert!(html.contains("<blockquote>"));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("href=\"javascript:"));
    }

    #[test]
    fn durations_follow_the_selected_language() {
        assert_eq!(duration_text(65, Locale::ZhCn), "1分 5秒");
        assert_eq!(duration_text(3660, Locale::En), "1h 1m");
    }

    #[test]
    fn diff_engine_preserves_context_and_hunk_counts() {
        let patch = snippet_diff("first\nold\nlast\n", "first\nnew\nlast\n", "test.rs");
        assert!(patch.contains("@@ -1,3 +1,3 @@"));
        assert!(patch.contains(" first\n-old\n+new\n last"));
    }

    #[test]
    fn ansi_preserves_unicode_and_escapes_html() {
        let html = ansi_to_html("\x1b[32m成功 🦀 <ok>\x1b[0m");
        assert!(html.contains("成功 🦀 &lt;ok&gt;"));
    }

    #[test]
    fn is_markdown_heuristic_detects_formatting() {
        assert!(is_markdown_content("Hello\nworld"));
        assert!(is_markdown_content("## Heading"));
        assert!(is_markdown_content("**bold**"));
        assert!(is_markdown_content("- bullet"));
        assert!(!is_markdown_content("plain text message"));
    }

    #[test]
    fn ansi_parser_converts_colors_and_styles() {
        let raw = "\x1b[32mSuccess\x1b[0m: \x1b[1;31mError\x1b[0m";
        let html = ansi_to_html(raw);
        assert!(html.contains("<span class=\"ansi-fg-green\">Success</span>"));
        assert!(html
            .contains("<span class=\"ansi-bold\"><span class=\"ansi-fg-red\">Error</span></span>"));
    }

    #[test]
    fn diff_parser_formats_unified_diff_lines() {
        let raw = "--- a/test.rs\n+++ b/test.rs\n@@ -1,2 +1,2 @@\n-old line\n+new line";
        let html = diff_to_html(raw);
        assert!(html.contains("diff-del"));
        assert!(html.contains("diff-add"));
        assert!(html.contains("diff-hunk"));
        assert!(html.contains("new line"));
    }
}
