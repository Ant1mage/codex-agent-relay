//! Process identity, as the kernel reports it.
//!
//! Relay owns worker processes, so a daemon that starts after a hard crash finds
//! rows whose workers may still be running. A pid is not enough to act on: the OS
//! reuses pids, and killing whatever now holds a recorded pid would be worse than
//! leaving a stray worker behind. This module records what the kernel says about a
//! process — its start time, its process group, its parent and its real executable
//! image — and only ever signals a process whose whole recorded identity still
//! matches.
//!
//! macOS and Linux answer these questions differently; both answers are put in the
//! same shape, and a value is only ever compared with another capture of the same
//! machine.

use std::time::Duration;

use crate::domain::ProcessIdentity;

/// How often a terminating process is re-checked.
const POLL: Duration = Duration::from_millis(50);

/// What happened when Relay tried to end a surviving worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Termination {
    /// The process was already gone; nothing was signalled.
    AlreadyGone,
    /// The process ended after SIGTERM to its process group.
    Terminated,
    /// SIGTERM was not enough, so the process group was killed.
    Killed,
    /// The pid does not describe the recorded process (or the kernel would not
    /// say), so no signal was sent.
    Unverified,
    /// The identity matched, the signal was sent, and the process is still there.
    Survived,
}

impl Termination {
    /// One short phrase for a diagnostic or an event payload.
    pub fn summary(self) -> &'static str {
        match self {
            Termination::AlreadyGone => "the worker process was already gone",
            Termination::Terminated => "the surviving worker was terminated",
            Termination::Killed => "the surviving worker ignored SIGTERM and was killed",
            Termination::Unverified => {
                "could not safely identify surviving worker process; it was not signalled"
            }
            Termination::Survived => "the surviving worker did not exit after SIGTERM and SIGKILL",
        }
    }
}

/// Reads the kernel's current answer for one pid.
pub fn capture(pid: u32) -> Option<ProcessIdentity> {
    imp::capture(pid)
}

/// True when the pid exists, including when it exists but belongs to another user.
pub fn alive(pid: u32) -> bool {
    imp::alive(pid)
}

/// True when the kernel still describes exactly the process Relay recorded.
pub fn still_matches(recorded: &ProcessIdentity) -> bool {
    match capture(recorded.pid) {
        Some(live) => recorded.same_process(&live),
        None => false,
    }
}

/// Ends a surviving worker, but only after confirming it is that worker.
///
/// SIGTERM goes to the process group so the CLI's own children go with it; if the
/// group ignores it, SIGKILL follows. Nothing is ever signalled on a pid whose
/// identity Relay cannot confirm.
pub fn terminate_verified(recorded: &ProcessIdentity, grace: Duration) -> Termination {
    if !alive(recorded.pid) {
        return Termination::AlreadyGone;
    }
    if !still_matches(recorded) {
        return Termination::Unverified;
    }
    signal(recorded, libc::SIGTERM);
    if wait_for_exit(recorded.pid, grace) {
        return Termination::Terminated;
    }
    // The pid may have been recycled while the worker was dying: never SIGKILL a
    // process Relay can no longer identify.
    if !still_matches(recorded) {
        return Termination::Unverified;
    }
    signal(recorded, libc::SIGKILL);
    if wait_for_exit(recorded.pid, grace) {
        return Termination::Killed;
    }
    Termination::Survived
}

fn signal(recorded: &ProcessIdentity, number: libc::c_int) {
    let pid = recorded.pid as i32;
    // A worker leads its own process group, so the negative id reaches it and
    // everything it spawned. A group Relay cannot be sure about — unknown, zero,
    // or Relay's own — is never signalled as a group: the daemon must not be able
    // to take itself down with a stray worker.
    let own_group = unsafe { libc::getpgrp() };
    let target = match recorded.pgid {
        Some(pgid) if pgid != 0 && pgid as i32 != own_group => -(pgid as i32),
        _ => pid,
    };
    unsafe {
        libc::kill(target, number);
    }
}

fn wait_for_exit(pid: u32, grace: Duration) -> bool {
    let deadline = std::time::Instant::now() + grace;
    loop {
        if !alive(pid) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(POLL);
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::ProcessIdentity;
    use std::ffi::CStr;

    pub fn alive(pid: u32) -> bool {
        let result = unsafe { libc::kill(pid as i32, 0) };
        if result == 0 {
            return true;
        }
        // EPERM means the process exists and belongs to somebody else.
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    pub fn capture(pid: u32) -> Option<ProcessIdentity> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
        let read = unsafe {
            libc::proc_pidinfo(
                pid as libc::c_int,
                libc::PROC_PIDTBSDINFO,
                0,
                &mut info as *mut _ as *mut libc::c_void,
                size,
            )
        };
        if read != size {
            return None;
        }
        Some(ProcessIdentity {
            pid,
            pgid: Some(info.pbi_pgid),
            parent_pid: Some(info.pbi_ppid),
            start_time_seconds: i64::try_from(info.pbi_start_tvsec).ok(),
            start_time_micros: Some(info.pbi_start_tvusec as u32),
            executable: executable(pid),
        })
    }

    fn executable(pid: u32) -> Option<String> {
        let mut buffer = [0i8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let written = unsafe {
            libc::proc_pidpath(
                pid as libc::c_int,
                buffer.as_mut_ptr() as *mut libc::c_void,
                buffer.len() as u32,
            )
        };
        if written <= 0 {
            return None;
        }
        let path = unsafe { CStr::from_ptr(buffer.as_ptr()) };
        path.to_str().ok().map(str::to_string)
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::ProcessIdentity;

    pub fn alive(pid: u32) -> bool {
        let result = unsafe { libc::kill(pid as i32, 0) };
        if result == 0 {
            return true;
        }
        std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }

    pub fn capture(pid: u32) -> Option<ProcessIdentity> {
        let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
        // Field 2 is the command in parentheses and may contain spaces or
        // parentheses itself, so the fields after it are read from the last
        // closing parenthesis.
        let rest = stat.rsplit_once(')')?.1;
        let fields: Vec<&str> = rest.split_whitespace().collect();
        // 1 = state, 2 = ppid, 3 = pgrp, … 20 = starttime, all 1-based there.
        let parent_pid = fields.get(1).and_then(|value| value.parse().ok());
        let pgid = fields.get(2).and_then(|value| value.parse().ok());
        let ticks: i64 = fields.get(19).and_then(|value| value.parse().ok())?;
        let per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
        let per_second = if per_second > 0 { per_second } else { 100 };
        Some(ProcessIdentity {
            pid,
            pgid,
            parent_pid,
            start_time_seconds: Some(ticks / per_second),
            start_time_micros: Some(((ticks % per_second) * 1_000_000 / per_second) as u32),
            executable: std::fs::read_link(format!("/proc/{pid}/exe"))
                .ok()
                .map(|path| path.to_string_lossy().to_string()),
        })
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod imp {
    use super::ProcessIdentity;

    pub fn alive(_pid: u32) -> bool {
        false
    }

    pub fn capture(_pid: u32) -> Option<ProcessIdentity> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recorded(start_time: i64) -> ProcessIdentity {
        ProcessIdentity {
            pid: 4242,
            pgid: Some(4242),
            parent_pid: Some(1),
            start_time_seconds: Some(start_time),
            start_time_micros: Some(0),
            executable: Some("/usr/local/bin/node".to_string()),
        }
    }

    fn live(start_time: i64, executable: &str) -> ProcessIdentity {
        ProcessIdentity {
            executable: Some(executable.to_string()),
            ..recorded(start_time)
        }
    }

    /// A child in its own process group, so a test can signal it and only it.
    #[cfg(unix)]
    fn spawn_child() -> std::process::Child {
        use std::os::unix::process::CommandExt;
        std::process::Command::new("/bin/sleep")
            .arg("30")
            .process_group(0)
            .spawn()
            .expect("the test needs a child process")
    }

    /// The whole point: a reused pid must never be mistaken for the worker.
    #[test]
    fn a_reused_pid_does_not_match() {
        let identity = recorded(1_000);
        assert!(identity.same_process(&live(1_000, "/usr/local/bin/node")));
        assert!(!identity.same_process(&live(2_000, "/usr/local/bin/node")));
        assert!(!identity.same_process(&live(1_000, "/usr/bin/python3")));
        let other_group = ProcessIdentity {
            pgid: Some(7),
            ..live(1_000, "/usr/local/bin/node")
        };
        assert!(!identity.same_process(&other_group));
        let other_pid = ProcessIdentity {
            pid: 4243,
            ..live(1_000, "/usr/local/bin/node")
        };
        assert!(!identity.same_process(&other_pid));
    }

    /// A fact the kernel would not answer can never confirm a process.
    #[test]
    fn a_missing_fact_is_never_a_match() {
        let unknown_start = ProcessIdentity {
            start_time_seconds: None,
            ..recorded(1_000)
        };
        assert!(!unknown_start.same_process(&live(1_000, "/usr/local/bin/node")));
        let now_unknown = ProcessIdentity {
            start_time_seconds: None,
            ..live(1_000, "/usr/local/bin/node")
        };
        assert!(!recorded(1_000).same_process(&now_unknown));
    }

    /// Our own process is the one identity this test can capture for real.
    #[test]
    fn a_captured_identity_matches_its_own_process() {
        let me = std::process::id();
        let Some(identity) = capture(me) else {
            // A platform that will not answer simply never confirms anything.
            return;
        };
        assert_eq!(identity.pid, me);
        assert!(still_matches(&identity));
        assert!(identity.start_time_seconds.is_some());
        assert!(identity.executable.is_some(), "{identity:?}");
        assert!(alive(me));
    }

    /// A pid whose identity does not match is reported, never signalled.
    #[test]
    #[cfg(unix)]
    fn an_identity_that_does_not_match_is_never_signalled() {
        let mut child = spawn_child();
        let pid = child.id();
        let mut identity = capture(pid).expect("the child is running");
        identity.start_time_seconds = identity.start_time_seconds.map(|value| value + 1);
        assert_eq!(
            terminate_verified(&identity, Duration::from_millis(100)),
            Termination::Unverified,
            "a pid whose identity cannot be confirmed must never be signalled"
        );
        assert!(alive(pid), "the child must still be running");
        let _ = child.kill();
        let _ = child.wait();
    }

    /// A matching identity ends the process group the worker leads.
    ///
    /// The child is reaped on another thread while it is being terminated: a
    /// killed child stays a zombie — and answers signal 0 — until its parent
    /// reaps it, which is not how a worker orphaned by a daemon crash behaves.
    #[test]
    #[cfg(unix)]
    fn a_matching_identity_terminates_its_process_group() {
        let mut child = spawn_child();
        let pid = child.id();
        let identity = capture(pid).expect("the child is running");
        assert_eq!(identity.pgid, Some(pid), "the child leads its own group");
        let reaper = std::thread::spawn(move || {
            let _ = child.wait();
        });
        assert_eq!(
            terminate_verified(&identity, Duration::from_secs(5)),
            Termination::Terminated
        );
        reaper.join().unwrap();
        assert!(!alive(pid));
    }

    #[test]
    fn a_pid_that_is_already_gone_reports_so() {
        let identity = ProcessIdentity {
            pid: 4_000_000,
            ..recorded(1)
        };
        assert_eq!(
            terminate_verified(&identity, Duration::from_millis(10)),
            Termination::AlreadyGone
        );
    }
}
