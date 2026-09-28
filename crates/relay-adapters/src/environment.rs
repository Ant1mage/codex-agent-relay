//! Child-only network settings shared by the CLI adapters.
//!
//! The proxy is applied to the CLI process (and its model-discovery probes),
//! never to Relay itself: Relay's local HTTP clients connect directly and its
//! updater keeps its own proxy handling. Both `NO_PROXY` spellings receive the
//! same merged list so the CLI's own loopback traffic bypasses the proxy.

#[cfg(any(target_os = "macos", test))]
use std::collections::HashMap;
#[cfg(target_os = "macos")]
use std::time::Duration;

const PROXY_KEYS: [&str; 6] = [
    "HTTPS_PROXY",
    "https_proxy",
    "HTTP_PROXY",
    "http_proxy",
    "ALL_PROXY",
    "all_proxy",
];

/// The environment a CLI subprocess is launched with: the caller's own
/// child-only overrides (auto-update off, for instance), a merged loopback
/// bypass for both `NO_PROXY` spellings, and — on macOS — the system proxies
/// when the parent has no explicit proxy override at all.
///
/// Nothing here mutates the parent's environment.
pub(crate) async fn child_environment(extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let bypass = loopback_bypass(
        std::env::var("NO_PROXY").ok().as_deref(),
        std::env::var("no_proxy").ok().as_deref(),
    );
    let mut env: Vec<(String, String)> = extra
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    env.push(("NO_PROXY".into(), bypass.clone()));
    env.push(("no_proxy".into(), bypass));
    // Explicit process settings, including a deliberately empty override, win.
    if PROXY_KEYS.iter().any(|key| std::env::var_os(key).is_some()) {
        return env;
    }
    #[cfg(target_os = "macos")]
    {
        if let Some((0, stdout, _)) = crate::probe::capture_with(
            "/usr/sbin/scutil",
            &["--proxy".into()],
            &[],
            Duration::from_secs(2),
        )
        .await
        {
            env.extend(system_proxies(&stdout));
        }
    }
    env
}

/// The loopback exclusions every child gets, merged with the two exclusion
/// lists the parent already had. Deduplicated case-insensitively, order kept.
pub(crate) fn loopback_bypass(upper: Option<&str>, lower: Option<&str>) -> String {
    let mut entries: Vec<&str> = Vec::new();
    for value in upper
        .into_iter()
        .chain(lower)
        .chain(["localhost,127.0.0.1,127.0.0.0/8,::1,[::1],.localhost"])
    {
        for entry in value
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
        {
            if !entries
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(entry))
            {
                entries.push(entry);
            }
        }
    }
    entries.join(",")
}

#[cfg(any(target_os = "macos", test))]
fn system_proxies(text: &str) -> Vec<(String, String)> {
    let fields: HashMap<_, _> = text
        .lines()
        .filter_map(|line| line.trim().split_once(" : "))
        .collect();
    let mut env = Vec::new();
    for (prefix, key, scheme) in [
        ("HTTPS", "HTTPS_PROXY", "http"),
        ("HTTP", "HTTP_PROXY", "http"),
        ("SOCKS", "ALL_PROXY", "socks5h"),
    ] {
        if fields.get(format!("{prefix}Enable").as_str()) != Some(&"1") {
            continue;
        }
        let Some(host) = fields.get(format!("{prefix}Proxy").as_str()) else {
            continue;
        };
        let Some(port) = fields
            .get(format!("{prefix}Port").as_str())
            .and_then(|port| port.parse::<u16>().ok())
            .filter(|port| *port != 0)
        else {
            continue;
        };
        // Only a hostname or IP address, never URL userinfo or arbitrary syntax.
        if host.is_empty()
            || !host
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ".-:".contains(ch))
        {
            continue;
        }
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host.to_string()
        };
        env.push((key.into(), format!("{scheme}://{host}:{port}")));
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn child_loopback_bypass_preserves_both_inherited_exclusion_lists() {
        let bypass = loopback_bypass(
            Some("internal.example,LOCALHOST"),
            Some("internal.example,custom.local"),
        );
        assert_eq!(
            bypass,
            "internal.example,LOCALHOST,custom.local,127.0.0.1,127.0.0.0/8,::1,[::1],.localhost"
        );
        assert!(loopback_bypass(Some("*"), None).starts_with("*,"));
    }

    #[test]
    fn enabled_system_proxies_are_translated_without_credentials() {
        let env = system_proxies("<dictionary> {\n HTTPSEnable : 1\n HTTPSProxy : 127.0.0.1\n HTTPSPort : 7890\n HTTPEnable : 0\n SOCKSEnable : 1\n SOCKSProxy : ::1\n SOCKSPort : 7891\n}");
        assert_eq!(
            env,
            vec![
                ("HTTPS_PROXY".into(), "http://127.0.0.1:7890".into()),
                ("ALL_PROXY".into(), "socks5h://[::1]:7891".into())
            ]
        );
        assert!(
            system_proxies("HTTPSEnable : 1\nHTTPSProxy : user:secret@host\nHTTPSPort : 443")
                .is_empty()
        );
    }
}
