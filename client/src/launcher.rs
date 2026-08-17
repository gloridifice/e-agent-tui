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
pub fn kill_process(pid: u32) {
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
    #[cfg(not(windows))]
    {
        unsafe {
            // SAFETY: best-effort SIGTERM by pid.
            let _ = std::process::Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .output();
        }
    }
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
        // Another `dshe` already spawned (or is spawning) the service — join
        // its count and bridge to it.
        lock.instances += 1;
        write_lock(&path, &lock);
        return DshSession {
            child: None,
            in_lock: true,
            path,
        };
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
    let mut child = match spawn_dsh(&argv) {
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
        // The service never came up — abandon the child so it isn't orphaned
        // and fall through to the normal connect (which will surface the
        // connection error to the user).
        let _ = child.kill();
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
pub fn release(session: &mut DshSession) {
    if !session.in_lock {
        // External dsh — never touch it. If we spawned a child but didn't
        // lock (spawn failed to come up), make sure it's reaped.
        if let Some(mut child) = session.child.take() {
            let _ = child.kill();
        }
        return;
    }
    let Some(mut lock) = read_lock(&session.path) else {
        return;
    };
    lock.instances = lock.instances.saturating_sub(1);
    if lock.instances == 0 {
        // Last TUI: shut the spawned service down (via our child handle when
        // we own it, else by pid).
        if let Some(mut child) = session.child.take() {
            let _ = child.kill();
        } else {
            kill_process(lock.dsh_pid);
        }
        remove_lock(&session.path);
    } else {
        write_lock(&session.path, &lock);
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
}
