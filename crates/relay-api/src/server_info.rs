//! How the tray and scripts reach a running daemon.
//!
//! The record carries a per-process nonce. Liveness cannot be proven by a PID
//! (they get reused), so both the startup guard and the tray's restart path make
//! the daemon confirm its nonce before anything is trusted or killed.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const HOST: &str = "127.0.0.1";
/// "RELA" on a phone keypad; IANA has no registration for it.
pub const DEFAULT_PORT: u16 = 7352;
/// Consecutive ports the daemon tries before giving up.
pub const PORT_ATTEMPTS: u16 = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    pub pid: u32,
    pub port: u16,
    /// Loopback base URL, without the token.
    pub url: String,
    /// Per-start secret; required by `/api/*` and by the stream.
    pub token: String,
    /// Random per process start; `/api/health` echoes it so identity is provable.
    #[serde(default)]
    pub nonce: String,
    #[serde(default)]
    pub started_at: String,
    #[serde(default = "unknown_version")]
    pub version: String,
    #[serde(default)]
    pub database: String,
}

fn unknown_version() -> String {
    "0.0.0".to_string()
}

impl ServerInfo {
    pub fn inspector_url(&self, session_id: Option<&str>, run_id: Option<&str>) -> String {
        format!("{}{}#t={}", self.url, inspector_path(session_id, run_id), self.token)
    }
}

/// Inspector route for a session or a run. The tray deep-links through this.
pub fn inspector_path(session_id: Option<&str>, run_id: Option<&str>) -> String {
    let Some(session_id) = session_id else {
        return "/".to_string();
    };
    let base = format!("/s/{}", encode(session_id));
    match run_id {
        Some(run_id) => format!("{base}/r/{}", encode(run_id)),
        None => base,
    }
}

fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (byte as char).to_string(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

pub fn read_server_info(path: &Path) -> Option<ServerInfo> {
    let contents = std::fs::read_to_string(path).ok()?;
    // A half-written or hand-edited file is treated as "no daemon".
    serde_json::from_str::<ServerInfo>(&contents).ok().filter(|info| info.token.len() > 0 && info.port > 0)
}

pub fn write_server_info(path: &Path, info: &ServerInfo) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(info).unwrap_or_default();
    std::fs::write(path, format!("{body}\n"))?;
    // 0600: the token in this file is the only credential the daemon has.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Removes the file only when it still describes this process.
pub fn clear_server_info(path: &Path, pid: u32) {
    if let Some(current) = read_server_info(path) {
        if current.pid != pid {
            return;
        }
    }
    let _ = std::fs::remove_file(path);
}

pub fn is_process_alive(pid: u32) -> bool {
    libc_kill(pid as i32)
}

#[cfg(unix)]
fn libc_kill(pid: i32) -> bool {
    // Signal 0 only performs the permission/existence check.
    unsafe { libc_kill_raw(pid, 0) == 0 }
}

#[cfg(unix)]
unsafe fn libc_kill_raw(pid: i32, signal: i32) -> i32 {
    unsafe extern "C" {
        fn kill(pid: i32, sig: i32) -> i32;
    }
    unsafe { kill(pid, signal) }
}

#[cfg(not(unix))]
fn libc_kill(_pid: i32) -> bool {
    false
}

pub fn server_info_path() -> PathBuf {
    relay_config::server_info_path()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info() -> ServerInfo {
        ServerInfo {
            pid: 42,
            port: 7352,
            url: "http://127.0.0.1:7352".into(),
            token: "secret".into(),
            nonce: "nonce".into(),
            started_at: "2026-01-01T00:00:00.000Z".into(),
            version: "0.2.0".into(),
            database: "/tmp/relay.sqlite".into(),
        }
    }

    #[test]
    fn server_info_round_trips_with_owner_only_permissions() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.json");
        write_server_info(&path, &info()).unwrap();
        assert_eq!(read_server_info(&path).unwrap(), info());
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
        }
    }

    #[test]
    fn a_broken_record_reads_as_no_daemon() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.json");
        std::fs::write(&path, "{ half written").unwrap();
        assert!(read_server_info(&path).is_none());
        std::fs::write(&path, r#"{"pid":1,"port":0,"url":"","token":""}"#).unwrap();
        assert!(read_server_info(&path).is_none());
    }

    #[test]
    fn clearing_only_removes_our_own_record() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("server.json");
        write_server_info(&path, &info()).unwrap();
        clear_server_info(&path, 7);
        assert!(read_server_info(&path).is_some());
        clear_server_info(&path, 42);
        assert!(read_server_info(&path).is_none());
    }

    #[test]
    fn inspector_urls_carry_the_token_in_the_fragment() {
        let info = info();
        assert_eq!(info.inspector_url(None, None), "http://127.0.0.1:7352/#t=secret");
        assert_eq!(
            info.inspector_url(Some("codex:thread 1"), Some("run/1")),
            "http://127.0.0.1:7352/s/codex%3Athread%201/r/run%2F1#t=secret"
        );
    }
}
