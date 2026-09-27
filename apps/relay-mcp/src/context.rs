//! Codex request context: which session is delegating.
//!
//! Codex supplies the thread id through the environment (hooks) or through the
//! request metadata. Relay accepts either, and never invents one.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodexInvocationContext {
    pub thread_id: String,
    pub turn_id: Option<String>,
}

fn find_string(value: &serde_json::Value, keys: &[&str], depth: usize) -> Option<String> {
    if depth > 5 {
        return None;
    }
    match value {
        serde_json::Value::Object(object) => {
            for (key, item) in object {
                if keys.contains(&key.as_str()) {
                    if let Some(text) = item.as_str() {
                        if !text.trim().is_empty() {
                            return Some(text.to_string());
                        }
                    }
                }
                if let Some(text) = item.as_str() {
                    // Some clients encode metadata as a JSON string.
                    if text.starts_with('{') {
                        if let Ok(nested) = serde_json::from_str::<serde_json::Value>(text) {
                            if let Some(found) = find_string(&nested, keys, depth + 1) {
                                return Some(found);
                            }
                        }
                    }
                }
                if let Some(found) = find_string(item, keys, depth + 1) {
                    return Some(found);
                }
            }
            None
        }
        serde_json::Value::Array(items) => {
            for item in items {
                if let Some(found) = find_string(item, keys, depth + 1) {
                    return Some(found);
                }
            }
            None
        }
        _ => None,
    }
}

/// Resolves the Codex thread identity for one MCP call.
pub fn invocation_context(
    request_context: Option<&serde_json::Value>,
    environment: &[(String, String)],
) -> Result<CodexInvocationContext, String> {
    let env = |key: &str| {
        environment
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.clone())
            .filter(|value| !value.trim().is_empty())
    };
    let thread_id = env("CODEX_THREAD_ID")
        .or_else(|| env("CODEX_SESSION_ID"))
        .or_else(|| {
            request_context.and_then(|value| {
                find_string(
                    value,
                    &["thread_id", "threadId", "session_id", "sessionId"],
                    0,
                )
            })
        })
        .ok_or_else(|| "Codex thread identity is unavailable".to_string())?;
    let turn_id = request_context.and_then(|value| find_string(value, &["turn_id", "turnId"], 0));
    Ok(CodexInvocationContext { thread_id, turn_id })
}

/// The session id carried by a `SessionEnd` hook payload.
pub fn session_id_from_hook(payload: &serde_json::Value) -> Option<String> {
    find_string(
        payload,
        &["session_id", "thread_id", "threadId", "sessionId"],
        0,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_wins_over_request_metadata() {
        let context = invocation_context(
            Some(&serde_json::json!({ "thread_id": "from-request" })),
            &[("CODEX_THREAD_ID".to_string(), "from-env".to_string())],
        )
        .unwrap();
        assert_eq!(context.thread_id, "from-env");
    }

    #[test]
    fn nested_request_metadata_is_found() {
        let context = invocation_context(
            Some(&serde_json::json!({ "meta": { "sessionId": "nested-1" }, "turnId": "turn-1" })),
            &[],
        )
        .unwrap();
        assert_eq!(context.thread_id, "nested-1");
        assert_eq!(context.turn_id.as_deref(), Some("turn-1"));
    }

    #[test]
    fn a_missing_identity_is_an_error() {
        assert!(invocation_context(Some(&serde_json::json!({})), &[]).is_err());
        assert!(invocation_context(None, &[]).is_err());
    }

    #[test]
    fn session_end_payloads_are_understood() {
        assert_eq!(
            session_id_from_hook(&serde_json::json!({ "session_id": "abc" })).as_deref(),
            Some("abc")
        );
        assert_eq!(
            session_id_from_hook(&serde_json::json!({ "payload": { "thread_id": "t-1" } }))
                .as_deref(),
            Some("t-1")
        );
    }
}
