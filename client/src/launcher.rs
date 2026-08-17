//! `dshe` launcher: probe for a running DSH bridge, spawn `dsh --profile dshe`
//! when absent (global `dsh` first, `npx @deepseek-ai/dsh` fallback), and shut
//! the spawned service down when the last attached TUI exits.
//!
//! Startup modes:
//!   1. `dshe` (no running dsh) → spawn `dsh --profile dshe`, run the TUI, and
//!      kill the spawned service when the last TUI closes.
//!   2. `dsh --profile dshe` → the user starts dsh themselves; a later `dshe`
//!      bridges to it.
//!   3. dsh already running (any profile) → `dshe` bridges and never touches
//!      the existing service.
//!
//! The "last TUI" bookkeeping uses a small lock file (`%DSH_HOME%\dsh-tui.lock`)
//! recording the spawned dsh pid and the number of attached TUI processes, so
//! concurrent `dshe` instances share one spawned service.

use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::Duration;

use serde::{Deserialize, Serialize};

const PROBE_TIMEOUT_MS: u64 = 400;
const SPAWN_WAIT_TIMEOUT_SECS: u64 = 45;
const CHILD_REAP_TIMEOUT_MS: u64 = 2_000;
const CHILD_REAP_POLL_MS: u64 = 20;
const DSH_PROFILE: &str = "dshe";

/// DSH home (the bridge token and the instance lock live here), matching the
/// bridge's own `dshHome()` resolution.
pub fn dsh_home() -> PathBuf {
    if let Ok(home) = std::env::var("DSH_HOME") {
        return PathBuf::from(home);
    }
    let user = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_else(|_| ".".into());
    PathBuf::from(user).join(".dsh")
}

// ---------- URL / probe (pure) ----------

/// Split a `ws://host:port/path` URL into (host, port). Default port 3080.
pub fn parse_host_port(url: &str) -> Option<(String, u16)> {
    let rest = url
        .strip_prefix("ws://")
        .or_else(|| url.strip_prefix("wss://"))?;
    let host_port = rest.split('/').next().unwrap_or(rest);
    let (host, port) = match host_port.rsplit_once(':') {
        Some((h, p)) => (h, p.parse().ok()?),
        None => (host_port, 3080),
    };
    if host.is_empty() {
        return None;
    }
    Some((host.to_string(), port))
}

/// Whether a DSH bridge is accepting TCP connections at `url`.
pub fn probe(url: &str) -> bool {
    let Some((host, port)) = parse_host_port(url) else {
        return false;
    };
    match (host.as_str(), port).to_socket_addrs() {
        Ok(mut addrs) => addrs.any(|a| {
            TcpStream::connect_timeout(&a, Duration::from_millis(PROBE_TIMEOUT_MS)).is_ok()
        }),
        Err(_) => false,
    }
}

// ---------- dsh command resolution ----------

fn command_exists(cmd: &str) -> bool {
    #[cfg(windows)]
    let out = Command::new("where").arg(cmd).output();
    #[cfg(not(windows))]
    let out = Command::new("which").arg(cmd).output();
    out.map(|o| o.status.success()).unwrap_or(false)
}

/// The argv that boots the dedicated `dshe` profile: global `dsh` when
/// installed, else `npx @deepseek-ai/dsh` (downloads on first run).
pub fn dsh_command() -> Vec<String> {
    if command_exists("dsh") {
        vec!["dsh".into(), "--profile".into(), DSH_PROFILE.into()]
    } else {
        vec![
            "npx".into(),
            "-y".into(),
            "@deepseek-ai/dsh".into(),
            "--profile".into(),
            DSH_PROFILE.into(),
        ]
    }
}

/// Spawn the dsh command. On Windows the `dsh`/`npx` shims are `.cmd`
/// files, which need `cmd /C` to be created as a child process.
#[cfg(windows)]
fn spawn_dsh(argv: &[String]) -> std::io::Result<Child> {
    Command::new("cmd").arg("/C").args(argv).spawn()
}

#[cfg(not(windows))]
fn spawn_dsh(argv: &[String]) -> std::io::Result<Child> {
    let mut cmd = Command::new(&argv[0]);
    cmd.args(&argv[1..]);
    cmd.spawn()
}

/// Poll `url` until the bridge accepts connections or the timeout elapses.
pub fn wait_for_dsh(url: &str, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    while std::time::Instant::now() < deadline {
        if probe(url) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(300));
    }
    false
}

// ---------- instance lock (last TUI closes dsh) ----------

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct InstanceLock {
    pub dsh_pid: u32,
    pub instances: u32,
}

pub fn lock_path(dsh_home: &Path) -> PathBuf {
    dsh_home.join("dsh-tui.lock")
}

pub fn read_lock(path: &Path) -> Option<InstanceLock> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

pub fn write_lock(path: &Path, lock: &InstanceLock) {
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = serde_json::to_string(lock) {
        let _ = std::fs::write(path, text);
    }
}

pub fn remove_lock(path: &Path) {
    let _ = std::fs::remove_file(path);
}

/// Kill a process by pid (the last attached TUI may not own the child handle).
/// Returns whether the operating-system command accepted the termination.
pub fn kill_process(pid: u32) -> bool {
    #[cfg(windows)]
    {
        Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
    #[cfg(not(windows))]
    {
        Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false)
    }
}

/// Reap an exited child without allowing launcher shutdown to block forever.
fn reap_child_with_timeout(child: &mut Child, timeout: Duration) -> bool {
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return true,
            Ok(None) if std::time::Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(CHILD_REAP_POLL_MS));
            }
            Ok(None) | Err(_) => return false,
        }
    }
}

/// Stop a service through the child handle retained by the TUI.
///
/// On Windows that handle belongs to the `cmd /C` shim, not to the Node DSH
/// process. `Child::kill` would terminate only `cmd.exe` and orphan Node, so
/// terminate the complete wrapper process tree instead. Reaping is bounded so
/// a failed OS termination cannot hang TUI shutdown indefinitely.
fn kill_child_service(mut child: Child) -> bool {
    #[cfg(windows)]
    let (service_stopped, wrapper_stopped) = {
        let tree_stopped = kill_process(child.id());
        let wrapper_stopped = tree_stopped || child.kill().is_ok();
        // Killing only the shim is cleanup, not proof that Node stopped.
        (tree_stopped, wrapper_stopped)
    };
    #[cfg(not(windows))]
    let (service_stopped, wrapper_stopped) = {
        let stopped = child.kill().is_ok();
        (stopped, stopped)
    };
    let reaped = if wrapper_stopped {
        reap_child_with_timeout(&mut child, Duration::from_millis(CHILD_REAP_TIMEOUT_MS))
    } else {
        matches!(child.try_wait(), Ok(Some(_)))
    };
    service_stopped && reaped
}

// ---------- orchestration ----------

/// Outcome of the launcher preamble: whether this process spawned dsh (and
/// must therefore own the shutdown when it is the last TUI).
pub struct DshSession {
    /// The spawned dsh child, when this process started it.
    pub child: Option<Child>,
    /// Whether this process joined the instance lock (a dshe-spawned service).
    pub in_lock: bool,
    /// The lock file, so `release` can decrement it.
    pub path: PathBuf,
}

/// Ensure a DSH bridge is listening at `url`, spawning one if absent.
/// Returns the session bookkeeping to pass back to [`release`] on TUI exit.
pub fn acquire(url: &str, dsh_home: &Path) -> DshSession {
    let path = lock_path(dsh_home);
    if let Some(mut lock) = read_lock(&path) {
        // A zero-instance lock records a previous shutdown failure. Reuse it
        // only while the service is still reachable; otherwise discard the
        // stale retry record and start a fresh managed service below.
        if lock.instances > 0 || probe(url) {
            lock.instances = lock.instances.saturating_add(1);
            write_lock(&path, &lock);
            return DshSession {
                child: None,
                in_lock: true,
                path,
            };
        }
        remove_lock(&path);
    }
    if probe(url) {
        // A dsh is already running out-of-band: bridge without lifecycle.
        return DshSession {
            child: None,
            in_lock: false,
            path,
        };
    }
    // Spawn `dsh --profile dshe` and own its shutdown.
    let argv = dsh_command();
    let child = match spawn_dsh(&argv) {
        Ok(child) => child,
        Err(_) => {
            return DshSession {
                child: None,
                in_lock: false,
                path,
            };
        }
    };
    let pid = child.id();
    let ready = wait_for_dsh(url, Duration::from_secs(SPAWN_WAIT_TIMEOUT_SECS));
    if !ready {
        // The service never came up. Use the same process-tree cleanup as the
        // normal last-TUI shutdown so a Windows cmd shim cannot orphan Node.
        kill_child_service(child);
        return DshSession {
            child: None,
            in_lock: false,
            path,
        };
    }
    write_lock(
        &path,
        &InstanceLock {
            dsh_pid: pid,
            instances: 1,
        },
    );
    DshSession {
        child: Some(child),
        in_lock: true,
        path,
    }
}

/// Release the launcher bookkeeping on TUI exit: decrement the instance count
/// and, when this is the last TUI of a dshe-spawned service, shut it down.
/// Returns `true` only when this release successfully stopped that service.
pub fn release(session: &mut DshSession) -> bool {
    if !session.in_lock {
        // External dsh — never touch it. If we spawned a child but didn't
        // lock (spawn failed to come up), make sure its complete shim tree is
        // stopped and reaped. This is startup-failure cleanup, not a managed
        // service shutdown to announce to the user.
        if let Some(child) = session.child.take() {
            kill_child_service(child);
        }
        return false;
    }
    let Some(mut lock) = read_lock(&session.path) else {
        return false;
    };
    lock.instances = lock.instances.saturating_sub(1);
    if lock.instances == 0 {
        // Last TUI: shut the spawned service down (via our child handle when
        // we own it, else by pid).
        let stopped = if let Some(child) = session.child.take() {
            kill_child_service(child)
        } else {
            kill_process(lock.dsh_pid)
        };
        if stopped {
            remove_lock(&session.path);
        } else {
            // Keep a retryable ownership record. A later acquire joins it if
            // the service is alive, or removes it as stale before respawning.
            lock.instances = 0;
            write_lock(&session.path, &lock);
        }
        stopped
    } else {
        write_lock(&session.path, &lock);
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn parse_host_port_handles_default_and_path() {
        assert_eq!(
            parse_host_port("ws://127.0.0.1:3080/dsh-tui"),
            Some(("127.0.0.1".into(), 3080))
        );
        assert_eq!(
            parse_host_port("ws://localhost:4000"),
            Some(("localhost".into(), 4000))
        );
        assert_eq!(
            parse_host_port("ws://127.0.0.1"),
            Some(("127.0.0.1".into(), 3080))
        );
        assert_eq!(parse_host_port("http://x"), None);
        assert_eq!(parse_host_port("ws://"), None);
    }

    #[test]
    fn dsh_command_uses_the_dedicated_dshe_profile() {
        let command = dsh_command();
        assert!(command
            .windows(2)
            .any(|args| args[0] == "--profile" && args[1] == DSH_PROFILE));
    }

    #[test]
    fn probe_detects_a_listening_socket() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let url = format!("ws://127.0.0.1:{}", addr.port());
        assert!(probe(&url));
        drop(listener);
    }

    #[test]
    fn probe_rejects_a_closed_port() {
        // Bind then drop so the port is almost certainly closed.
        let port = {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let p = l.local_addr().unwrap().port();
            drop(l);
            p
        };
        assert!(!probe(&format!("ws://127.0.0.1:{port}")));
    }

    #[test]
    fn instance_lock_roundtrips() {
        let dir = std::env::temp_dir().join(format!("dshe-launcher-test-{}", std::process::id()));
        let path = lock_path(&dir);
        let lock = InstanceLock {
            dsh_pid: 42,
            instances: 2,
        };
        write_lock(&path, &lock);
        assert_eq!(read_lock(&path), Some(lock));
        remove_lock(&path);
        assert_eq!(read_lock(&path), None);
    }

    /// Child mode used by `reap_child_timeout_is_bounded`. In an ordinary
    /// test run the environment variable is absent and this exits.
    #[test]
    fn reap_timeout_child() {
        if std::env::var_os("DSHE_LAUNCHER_REAP_TEST_CHILD").is_none() {
            return;
        }
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }

    #[test]
    fn reap_child_timeout_is_bounded() {
        let test_exe = std::env::current_exe().unwrap();
        let mut child = Command::new(test_exe)
            .args([
                "--exact",
                "launcher::tests::reap_timeout_child",
                "--nocapture",
            ])
            .env("DSHE_LAUNCHER_REAP_TEST_CHILD", "1")
            .spawn()
            .expect("spawn reap timeout child");
        std::thread::sleep(Duration::from_millis(50));

        let started = std::time::Instant::now();
        let exited = reap_child_with_timeout(&mut child, Duration::from_millis(40));
        let elapsed = started.elapsed();
        let _ = child.kill();
        let cleaned = reap_child_with_timeout(&mut child, Duration::from_secs(2));

        assert!(!exited, "running child must hit the reap deadline");
        assert!(
            elapsed < Duration::from_secs(1),
            "reap deadline blocked for {elapsed:?}"
        );
        assert!(cleaned, "test child was not reaped after cleanup");
    }

    /// Child mode used by `release_kills_windows_cmd_process_tree`. In an
    /// ordinary test run the environment variable is absent and this exits.
    #[cfg(windows)]
    #[test]
    fn windows_process_tree_server_child() {
        let Ok(port) = std::env::var("DSHE_LAUNCHER_TEST_PORT") else {
            return;
        };
        let port: u16 = port.parse().expect("test port");
        let _listener = TcpListener::bind(("127.0.0.1", port)).expect("bind child server");
        loop {
            std::thread::sleep(Duration::from_secs(60));
        }
    }

    #[cfg(windows)]
    #[test]
    fn release_kills_windows_cmd_process_tree() {
        let port = {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let port = listener.local_addr().unwrap().port();
            drop(listener);
            port
        };
        let test_exe = std::env::current_exe().unwrap();
        let child = Command::new("cmd")
            .args(["/D", "/C"])
            .arg(test_exe)
            .args([
                "--exact",
                "launcher::tests::windows_process_tree_server_child",
                "--nocapture",
            ])
            .env("DSHE_LAUNCHER_TEST_PORT", port.to_string())
            .spawn()
            .expect("spawn cmd-wrapped child server");
        let wrapper_pid = child.id();
        let url = format!("ws://127.0.0.1:{port}");
        if !wait_for_dsh(&url, Duration::from_secs(5)) {
            kill_process(wrapper_pid);
            panic!("cmd-wrapped child server did not start");
        }

        let dir = std::env::temp_dir().join(format!(
            "dshe-launcher-tree-test-{}-{port}",
            std::process::id()
        ));
        let path = lock_path(&dir);
        write_lock(
            &path,
            &InstanceLock {
                dsh_pid: wrapper_pid,
                instances: 1,
            },
        );
        let mut session = DshSession {
            child: Some(child),
            in_lock: true,
            path: path.clone(),
        };

        assert!(release(&mut session));

        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while probe(&url) && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!probe(&url), "Node-like descendant was left running");
        assert!(!path.exists(), "instance lock was not removed");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn zero_instance_lock_rejoins_a_still_running_service() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let dir =
            std::env::temp_dir().join(format!("dshe-zero-lock-rejoin-test-{}", std::process::id()));
        let path = lock_path(&dir);
        write_lock(
            &path,
            &InstanceLock {
                dsh_pid: u32::MAX,
                instances: 0,
            },
        );

        let session = acquire(&url, &dir);

        assert!(session.in_lock, "live failed-shutdown service is rejoined");
        assert!(session.child.is_none(), "rejoin must not spawn another dsh");
        assert_eq!(read_lock(&path).unwrap().instances, 1);
        remove_lock(&path);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn release_retains_retry_lock_when_shutdown_fails() {
        let dir =
            std::env::temp_dir().join(format!("dshe-release-failure-test-{}", std::process::id()));
        let path = lock_path(&dir);
        write_lock(
            &path,
            &InstanceLock {
                dsh_pid: u32::MAX,
                instances: 1,
            },
        );
        let mut session = DshSession {
            child: None,
            in_lock: true,
            path: path.clone(),
        };

        assert!(!release(&mut session));
        assert_eq!(
            read_lock(&path),
            Some(InstanceLock {
                dsh_pid: u32::MAX,
                instances: 0,
            }),
            "failed shutdown remains retryable"
        );
        remove_lock(&path);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn release_does_not_report_shutdown_for_external_or_remaining_instances() {
        let dir =
            std::env::temp_dir().join(format!("dshe-release-outcome-test-{}", std::process::id()));
        let path = lock_path(&dir);
        let mut external = DshSession {
            child: None,
            in_lock: false,
            path: path.clone(),
        };
        assert!(!release(&mut external));

        write_lock(
            &path,
            &InstanceLock {
                dsh_pid: u32::MAX,
                instances: 2,
            },
        );
        let mut joined = DshSession {
            child: None,
            in_lock: true,
            path: path.clone(),
        };
        assert!(!release(&mut joined));
        assert_eq!(read_lock(&path).unwrap().instances, 1);
        remove_lock(&path);
        let _ = std::fs::remove_dir_all(dir);
    }
}
