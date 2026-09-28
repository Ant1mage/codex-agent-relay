//! Reuse the user's network settings without touching Grok's credentials/config.

/// Grok's child-only launch environment: the shared proxy handling plus the
/// Grok-specific auto-updater switch.
pub(super) async fn launch_environment() -> Vec<(String, String)> {
    crate::environment::child_environment(&[("GROK_DISABLE_AUTOUPDATER", "1")]).await
}
