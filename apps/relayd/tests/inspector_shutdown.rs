#![cfg(unix)]

use std::net::TcpListener;
use std::path::Path;
use std::process::{Child, Command, ExitStatus};
use std::time::Duration;

use relay_api::server_info::{is_process_alive, read_server_info, ServerInfo};
use tempfile::TempDir;

struct Daemon(Child);

impl Daemon {
    fn start(home: &Path, port: u16) -> Self {
        let child = Command::new(env!("CARGO_BIN_EXE_relayd"))
            .env("RELAY_HOME", home)
            .env("RELAY_PORT", port.to_string())
            .env("RELAY_TOKEN", "inspector-shutdown-test-token")
            .env("CODEX_HOME", home.join("codex"))
            .spawn()
            .expect("start relayd");
        Self(child)
    }

    fn pid(&self) -> u32 {
        self.0.id()
    }

    fn signal_term(&self) {
        let status = Command::new("kill")
            .args(["-TERM", &self.pid().to_string()])
            .status()
            .expect("send SIGTERM to relayd");
        assert!(status.success(), "SIGTERM should reach relayd");
    }

    async fn wait_for_exit(&mut self) -> ExitStatus {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if let Some(status) = self.0.try_wait().expect("check relayd status") {
                    return status;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .expect("relayd should exit promptly")
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_some() {
            return;
        }
        let _ = Command::new("kill")
            .args(["-TERM", &self.pid().to_string()])
            .status();
        let _ = self.0.wait();
    }
}

async fn wait_for_server_info(home: &Path, daemon: &mut Daemon) -> ServerInfo {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(info) = read_server_info(&home.join("server.json")) {
                return info;
            }
            if let Some(status) = daemon.0.try_wait().expect("check relayd startup") {
                panic!("relayd exited before publishing server.json: {status}");
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .expect("relayd should publish server.json")
}

#[tokio::test]
async fn inspector_connection_does_not_keep_daemon_alive_or_duplicate_it_after_restart() {
    let home = TempDir::new().expect("create isolated RELAY_HOME");
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("reserve a local port");
    let port = listener.local_addr().unwrap().port();
    drop(listener);

    let client = reqwest::Client::new();
    let mut first = Daemon::start(home.path(), port);
    let first_info = wait_for_server_info(home.path(), &mut first).await;
    assert_eq!(first_info.pid, first.pid());

    let stream = client
        .get(format!(
            "{}/api/stream?token={}",
            first_info.url, first_info.token
        ))
        .send()
        .await
        .expect("connect the Inspector event stream");
    assert!(
        stream.status().is_success(),
        "Inspector returned {}",
        stream.status()
    );

    first.signal_term();
    assert!(first.wait_for_exit().await.success());
    drop(stream);
    assert!(!is_process_alive(first_info.pid));
    assert!(read_server_info(&home.path().join("server.json")).is_none());

    let mut second = Daemon::start(home.path(), port);
    let second_info = wait_for_server_info(home.path(), &mut second).await;
    assert_ne!(first_info.pid, second_info.pid);
    assert_eq!(second_info.pid, second.pid());
    assert!(!is_process_alive(first_info.pid));
    assert_eq!(
        read_server_info(&home.path().join("server.json")).map(|info| info.pid),
        Some(second.pid())
    );

    second.signal_term();
    assert!(second.wait_for_exit().await.success());
}
