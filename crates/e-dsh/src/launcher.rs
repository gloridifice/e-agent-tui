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
//! The "last TUI" bookkeeping uses a small lock file (`%DSH_HOME%\e.lock`)
//! recording the spawned dsh pid and the number of attached TUI processes, so
//! concurrent `dshe` instances share one spawned service.

use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};
use std::{error::Error, fmt};

use serde::{Deserialize, Serialize};

const PROBE_TIMEOUT_MS: u64 = 400;
const SPAWN_WAIT_TIMEOUT_SECS: u64 = 90;
const CHILD_REAP_TIMEOUT_MS: u64 = 2_000;
const CHILD_REAP_POLL_MS: u64 = 20;

pub trait ProcessHandle {
    fn id(&self) -> u32;
    fn has_exited(&mut self) -> bool;
    fn terminate_and_reap(&mut self, timeout: Duration) -> bool;
}

pub trait LockStore {
    fn read(&self, path: &Path) -> Option<InstanceLock>;
    fn write(&mut self, path: &Path, lock: &InstanceLock);
    fn remove(&mut self, path: &Path);
}

pub trait LauncherClock {
    fn now(&self) -> Instant;
    fn sleep(&mut self, duration: Duration);
}

pub trait LauncherPorts {
    type Process: ProcessHandle;
    type Locks: LockStore;
    type Clock: LauncherClock;

    fn probe(&mut self, url: &str) -> bool;
    fn spawn(&mut self, argv: &[String]) -> std::io::Result<Self::Process>;
    fn terminate_pid(&mut self, pid: u32) -> bool;
    fn locks(&mut self) -> &mut Self::Locks;
    fn clock(&mut self) -> &mut Self::Clock;
}

pub struct StdProcessHandle {
    child: Option<Child>,
}

impl StdProcessHandle {
    fn new(child: Child) -> Self {
        Self { child: Some(child) }
    }
}

impl ProcessHandle for StdProcessHandle {
    fn id(&self) -> u32 {
        self.child.as_ref().map_or(0, Child::id)
    }

    fn has_exited(&mut self) -> bool {
        match self.child.as_mut() {
            Some(child) => matches!(child.try_wait(), Ok(Some(_))),
            None => true,
        }
    }

    fn terminate_and_reap(&mut self, _timeout: Duration) -> bool {
        self.child.take().is_some_and(kill_child_service)
    }
}

#[derive(Default)]
pub struct FileLockStore;

impl LockStore for FileLockStore {
    fn read(&self, path: &Path) -> Option<InstanceLock> {
        read_lock(path)
    }

    fn write(&mut self, path: &Path, lock: &InstanceLock) {
        write_lock(path, lock);
    }

    fn remove(&mut self, path: &Path) {
        remove_lock(path);
    }
}

#[derive(Default)]
pub struct SystemLauncherClock;

impl LauncherClock for SystemLauncherClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

#[derive(Default)]
pub struct ProductionLauncherPorts {
    locks: FileLockStore,
    clock: SystemLauncherClock,
}

impl LauncherPorts for ProductionLauncherPorts {
    type Process = StdProcessHandle;
    type Locks = FileLockStore;
    type Clock = SystemLauncherClock;

    fn probe(&mut self, url: &str) -> bool {
        probe(url)
    }

    fn spawn(&mut self, argv: &[String]) -> std::io::Result<Self::Process> {
        spawn_dsh(argv).map(StdProcessHandle::new)
    }

    fn terminate_pid(&mut self, pid: u32) -> bool {
        kill_process(pid)
    }

    fn locks(&mut self) -> &mut Self::Locks {
        &mut self.locks
    }

    fn clock(&mut self) -> &mut Self::Clock {
        &mut self.clock
    }
}

/// DSH home (the bridge token and the instance lock live here), matching the
/// bridge's own `dshHome()` resolution. Empty or whitespace-only `DSH_HOME`
/// values fall back to the platform default just like an unset variable.
pub use crate::dsh_env::{current_dsh_home as dsh_home, dsh_command};

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
    dsh_home.join("e.lock")
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CleanOutcome {
    NothingToClean,
    RemovedStaleLock { pid: Option<u32> },
    StoppedManagedService { pid: u32 },
}

fn remove_lock_checked(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(windows)]
fn process_exists(pid: u32) -> Option<bool> {
    let filter = format!("PID eq {pid}");
    let output = Command::new("tasklist")
        .args(["/FI", &filter, "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let expected = pid.to_string();
    Some(String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        line.split(',')
            .nth(1)
            .is_some_and(|field| field.trim().trim_matches('"') == expected)
    }))
}

#[cfg(not(windows))]
fn process_exists(pid: u32) -> Option<bool> {
    let output = Command::new("ps")
        .args(["-p", &pid.to_string(), "-o", "pid="])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .split_whitespace()
            .any(|field| field == pid.to_string()),
    )
}

/// Force-stop the DSH service recorded by this project's lock and remove the
/// lock. Missing, malformed, zero-pid, and dead-process locks are stale and can
/// be removed without trying to terminate anything. A lock is retained when a
/// live process cannot be terminated, so a later launcher does not orphan it.
pub fn clean(dsh_home: &Path) -> std::io::Result<CleanOutcome> {
    let path = lock_path(dsh_home);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(CleanOutcome::NothingToClean);
        }
        Err(error) => return Err(error),
    };
    let lock = match serde_json::from_str::<InstanceLock>(&text) {
        Ok(lock) => lock,
        Err(_) => {
            remove_lock_checked(&path)?;
            return Ok(CleanOutcome::RemovedStaleLock { pid: None });
        }
    };
    if lock.dsh_pid == 0 || process_exists(lock.dsh_pid) == Some(false) {
        remove_lock_checked(&path)?;
        return Ok(CleanOutcome::RemovedStaleLock {
            pid: (lock.dsh_pid != 0).then_some(lock.dsh_pid),
        });
    }
    if kill_process(lock.dsh_pid) {
        remove_lock_checked(&path)?;
        return Ok(CleanOutcome::StoppedManagedService { pid: lock.dsh_pid });
    }
    if process_exists(lock.dsh_pid) == Some(false) {
        remove_lock_checked(&path)?;
        return Ok(CleanOutcome::RemovedStaleLock {
            pid: Some(lock.dsh_pid),
        });
    }
    Err(std::io::Error::other(format!(
        "could not terminate managed DSH process {} — lock retained at {}",
        lock.dsh_pid,
        path.display()
    )))
}

/// An instance count records ownership, not service liveness. Even a positive
/// count can survive an abruptly terminated TUI, so the endpoint must still be
/// reachable before the lock is reused.
fn reusable_lock(lock: &InstanceLock, service_reachable: bool) -> bool {
    lock.dsh_pid != 0 && service_reachable
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

/// Generic launcher bookkeeping owned by one TUI instance.
pub struct ManagedDshSession<H: ProcessHandle> {
    pub child: Option<H>,
    pub in_lock: bool,
    pub path: PathBuf,
}

pub type DshSession = ManagedDshSession<StdProcessHandle>;

pub struct LauncherCoordinator<P: LauncherPorts> {
    ports: P,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LauncherError {
    Spawn {
        command: String,
        message: String,
    },
    Exited {
        command: String,
        url: String,
    },
    Timeout {
        command: String,
        url: String,
        seconds: u64,
    },
}

impl fmt::Display for LauncherError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Spawn { command, message } => write!(
                f,
                "cannot launch DSH with `{command}`: {message}. Install `@deepseek-ai/dsh`, then retry"
            ),
            Self::Exited { command, url } => write!(
                f,
                "DSH exited before its bridge became available at {url}. Run `{command}` directly to inspect its startup error; then run `dshe setup` to repair the bridge and restart any running DSH service"
            ),
            Self::Timeout {
                command,
                url,
                seconds,
            } => write!(
                f,
                "DSH bridge did not become available at {url} within {seconds}s after starting `{command}`. Run that command directly to inspect startup output; then run `dshe setup` to repair the bridge and restart any running DSH service"
            ),
        }
    }
}

impl Error for LauncherError {}

enum ServiceWait {
    Ready,
    Exited,
    TimedOut,
}

impl<P: LauncherPorts> LauncherCoordinator<P> {
    pub fn new(ports: P) -> Self {
        Self { ports }
    }

    fn wait_for_service(
        &mut self,
        url: &str,
        timeout: Duration,
        child: &mut P::Process,
    ) -> ServiceWait {
        let deadline = self.ports.clock().now() + timeout;
        while self.ports.clock().now() < deadline {
            if self.ports.probe(url) {
                return ServiceWait::Ready;
            }
            if child.has_exited() {
                return ServiceWait::Exited;
            }
            self.ports.clock().sleep(Duration::from_millis(300));
        }
        if self.ports.probe(url) {
            ServiceWait::Ready
        } else {
            ServiceWait::TimedOut
        }
    }

    pub fn acquire(
        &mut self,
        url: &str,
        dsh_home: &Path,
    ) -> Result<ManagedDshSession<P::Process>, LauncherError> {
        let path = lock_path(dsh_home);
        if let Some(mut lock) = self.ports.locks().read(&path) {
            if reusable_lock(&lock, self.ports.probe(url)) {
                lock.instances = lock.instances.saturating_add(1);
                self.ports.locks().write(&path, &lock);
                return Ok(ManagedDshSession {
                    child: None,
                    in_lock: true,
                    path,
                });
            }
            self.ports.locks().remove(&path);
        }
        if self.ports.probe(url) {
            return Ok(ManagedDshSession {
                child: None,
                in_lock: false,
                path,
            });
        }

        let argv = dsh_command().ok_or_else(|| LauncherError::Spawn {
            command: "dsh --profile dshe".to_string(),
            message: "neither `dsh` nor `npx` is available on PATH".to_string(),
        })?;
        let command = argv.join(" ");
        let mut child = self
            .ports
            .spawn(&argv)
            .map_err(|error| LauncherError::Spawn {
                command: command.clone(),
                message: error.to_string(),
            })?;
        let pid = child.id();
        match self.wait_for_service(
            url,
            Duration::from_secs(SPAWN_WAIT_TIMEOUT_SECS),
            &mut child,
        ) {
            ServiceWait::Ready => {}
            ServiceWait::Exited => {
                child.terminate_and_reap(Duration::from_millis(CHILD_REAP_TIMEOUT_MS));
                return Err(LauncherError::Exited {
                    command,
                    url: url.to_owned(),
                });
            }
            ServiceWait::TimedOut => {
                child.terminate_and_reap(Duration::from_millis(CHILD_REAP_TIMEOUT_MS));
                return Err(LauncherError::Timeout {
                    command,
                    url: url.to_owned(),
                    seconds: SPAWN_WAIT_TIMEOUT_SECS,
                });
            }
        }
        self.ports.locks().write(
            &path,
            &InstanceLock {
                dsh_pid: pid,
                instances: 1,
            },
        );
        Ok(ManagedDshSession {
            child: Some(child),
            in_lock: true,
            path,
        })
    }

    pub fn release(&mut self, session: &mut ManagedDshSession<P::Process>) -> bool {
        if !session.in_lock {
            if let Some(mut child) = session.child.take() {
                child.terminate_and_reap(Duration::from_millis(CHILD_REAP_TIMEOUT_MS));
            }
            return false;
        }
        let Some(mut lock) = self.ports.locks().read(&session.path) else {
            return false;
        };
        lock.instances = lock.instances.saturating_sub(1);
        if lock.instances != 0 {
            self.ports.locks().write(&session.path, &lock);
            return false;
        }

        let stopped = if let Some(mut child) = session.child.take() {
            child.terminate_and_reap(Duration::from_millis(CHILD_REAP_TIMEOUT_MS))
        } else {
            self.ports.terminate_pid(lock.dsh_pid)
        };
        if stopped {
            self.ports.locks().remove(&session.path);
        } else {
            lock.instances = 0;
            self.ports.locks().write(&session.path, &lock);
        }
        stopped
    }
}

/// Ensure a bridge is available using production launcher adapters.
pub fn acquire(url: &str, dsh_home: &Path) -> Result<DshSession, LauncherError> {
    LauncherCoordinator::new(ProductionLauncherPorts::default()).acquire(url, dsh_home)
}

/// Release production launcher bookkeeping.
pub fn release(session: &mut DshSession) -> bool {
    LauncherCoordinator::new(ProductionLauncherPorts::default()).release(session)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        collections::{HashMap, VecDeque},
        net::TcpListener,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
    };

    /// Reaps a spawned child on drop so an assertion failure (or an early
    /// `panic!`) cannot leak a live process past the test harness.
    struct ChildGuard(Option<Child>);

    impl ChildGuard {
        fn child_mut(&mut self) -> &mut Child {
            self.0.as_mut().expect("child present")
        }
    }

    impl Drop for ChildGuard {
        fn drop(&mut self) {
            if let Some(mut child) = self.0.take() {
                let _ = child.kill();
                let _ = reap_child_with_timeout(
                    &mut child,
                    Duration::from_millis(CHILD_REAP_TIMEOUT_MS),
                );
            }
        }
    }

    /// Force-stops a process tree by pid on drop; covers panic/early-return
    /// paths in tests that spawn a real Windows `cmd` wrapper.
    #[cfg(windows)]
    struct PidGuard(u32);

    #[cfg(windows)]
    impl Drop for PidGuard {
        fn drop(&mut self) {
            kill_process(self.0);
        }
    }

    struct FakeProcess {
        id: u32,
        exited: bool,
        stopped: Arc<AtomicBool>,
        stop_result: bool,
    }

    impl ProcessHandle for FakeProcess {
        fn id(&self) -> u32 {
            self.id
        }

        fn has_exited(&mut self) -> bool {
            self.exited
        }

        fn terminate_and_reap(&mut self, _timeout: Duration) -> bool {
            self.stopped.store(true, Ordering::SeqCst);
            self.stop_result
        }
    }

    #[derive(Default)]
    struct MemoryLocks(HashMap<PathBuf, InstanceLock>);

    impl LockStore for MemoryLocks {
        fn read(&self, path: &Path) -> Option<InstanceLock> {
            self.0.get(path).cloned()
        }

        fn write(&mut self, path: &Path, lock: &InstanceLock) {
            self.0.insert(path.to_owned(), lock.clone());
        }

        fn remove(&mut self, path: &Path) {
            self.0.remove(path);
        }
    }

    struct FakeClock(Instant);

    impl Default for FakeClock {
        fn default() -> Self {
            Self(Instant::now())
        }
    }

    impl LauncherClock for FakeClock {
        fn now(&self) -> Instant {
            self.0
        }

        fn sleep(&mut self, duration: Duration) {
            self.0 += duration;
        }
    }

    struct FakePorts {
        probes: VecDeque<bool>,
        default_probe: bool,
        locks: MemoryLocks,
        clock: FakeClock,
        spawned: usize,
        spawn_fails: bool,
        child_stopped: Arc<AtomicBool>,
        child_exited: bool,
        child_stop_result: bool,
        terminate_result: bool,
    }

    impl Default for FakePorts {
        fn default() -> Self {
            Self {
                probes: VecDeque::new(),
                default_probe: false,
                locks: MemoryLocks::default(),
                clock: FakeClock::default(),
                spawned: 0,
                spawn_fails: false,
                child_stopped: Arc::new(AtomicBool::new(false)),
                child_exited: false,
                child_stop_result: true,
                terminate_result: true,
            }
        }
    }

    impl LauncherPorts for FakePorts {
        type Process = FakeProcess;
        type Locks = MemoryLocks;
        type Clock = FakeClock;

        fn probe(&mut self, _url: &str) -> bool {
            self.probes.pop_front().unwrap_or(self.default_probe)
        }

        fn spawn(&mut self, _argv: &[String]) -> std::io::Result<Self::Process> {
            if self.spawn_fails {
                return Err(std::io::Error::other("spawn failed"));
            }
            self.spawned += 1;
            Ok(FakeProcess {
                id: 42,
                exited: self.child_exited,
                stopped: self.child_stopped.clone(),
                stop_result: self.child_stop_result,
            })
        }

        fn terminate_pid(&mut self, _pid: u32) -> bool {
            self.terminate_result
        }

        fn locks(&mut self) -> &mut Self::Locks {
            &mut self.locks
        }

        fn clock(&mut self) -> &mut Self::Clock {
            &mut self.clock
        }
    }

    #[test]
    fn coordinator_recovers_stale_lock_and_spawns_once() {
        let home = PathBuf::from("fake-home");
        let path = lock_path(&home);
        let mut ports = FakePorts::default();
        ports.locks.0.insert(
            path.clone(),
            InstanceLock {
                dsh_pid: 9,
                instances: 3,
            },
        );
        ports.probes = [false, false, true].into_iter().collect();
        let mut coordinator = LauncherCoordinator::new(ports);
        let session = coordinator.acquire("ws://fake", &home).unwrap();
        assert!(session.in_lock);
        assert_eq!(coordinator.ports.spawned, 1);
        assert_eq!(coordinator.ports.locks.0[&path].instances, 1);
    }

    #[test]
    fn coordinator_joins_live_lock_without_spawning() {
        let home = PathBuf::from("fake-home");
        let path = lock_path(&home);
        let mut ports = FakePorts::default();
        ports.locks.0.insert(
            path.clone(),
            InstanceLock {
                dsh_pid: 9,
                instances: 1,
            },
        );
        ports.probes.push_back(true);
        let mut coordinator = LauncherCoordinator::new(ports);
        let session = coordinator.acquire("ws://fake", &home).unwrap();
        assert!(session.in_lock && session.child.is_none());
        assert_eq!(coordinator.ports.spawned, 0);
        assert_eq!(coordinator.ports.locks.0[&path].instances, 2);
    }

    #[test]
    fn coordinator_timeout_stops_spawned_process() {
        let ports = FakePorts::default();
        let stopped = ports.child_stopped.clone();
        let mut coordinator = LauncherCoordinator::new(ports);
        let error = match coordinator.acquire("ws://fake", Path::new("fake-home")) {
            Err(error) => error,
            Ok(_) => panic!("unreachable bridge must time out"),
        };
        assert!(matches!(error, LauncherError::Timeout { .. }));
        assert!(stopped.load(Ordering::SeqCst));
        assert_eq!(coordinator.ports.spawned, 1);
    }

    #[test]
    fn coordinator_reports_spawn_and_early_exit_failures() {
        let mut spawn_ports = FakePorts::default();
        spawn_ports.spawn_fails = true;
        let mut coordinator = LauncherCoordinator::new(spawn_ports);
        let spawn_error = match coordinator.acquire("ws://fake", Path::new("fake-home")) {
            Err(error) => error,
            Ok(_) => panic!("failed spawn must be reported"),
        };
        assert!(matches!(spawn_error, LauncherError::Spawn { .. }));
        assert!(spawn_error.to_string().contains("cannot launch DSH"));

        let mut exit_ports = FakePorts::default();
        exit_ports.child_exited = true;
        let stopped = exit_ports.child_stopped.clone();
        let mut coordinator = LauncherCoordinator::new(exit_ports);
        let exit_error = match coordinator.acquire("ws://fake", Path::new("fake-home")) {
            Err(error) => error,
            Ok(_) => panic!("early exit must be reported"),
        };
        assert!(matches!(exit_error, LauncherError::Exited { .. }));
        assert!(exit_error.to_string().contains("dshe setup"));
        assert!(stopped.load(Ordering::SeqCst));
    }

    #[test]
    fn coordinator_last_release_removes_or_retains_retry_lock() {
        let home = PathBuf::from("fake-home");
        let path = lock_path(&home);
        let mut ports = FakePorts::default();
        ports.locks.0.insert(
            path.clone(),
            InstanceLock {
                dsh_pid: 9,
                instances: 1,
            },
        );
        ports.terminate_result = false;
        let mut coordinator = LauncherCoordinator::new(ports);
        let mut session = ManagedDshSession::<FakeProcess> {
            child: None,
            in_lock: true,
            path: path.clone(),
        };
        assert!(!coordinator.release(&mut session));
        assert_eq!(coordinator.ports.locks.0[&path].instances, 0);

        coordinator.ports.terminate_result = true;
        coordinator.ports.locks.0.get_mut(&path).unwrap().instances = 1;
        assert!(coordinator.release(&mut session));
        assert!(!coordinator.ports.locks.0.contains_key(&path));
    }

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
        let Some(command) = dsh_command() else {
            return; // dsh/npx unavailable in this environment.
        };
        assert!(command
            .windows(2)
            .any(|args| args[0] == "--profile" && args[1] == crate::dsh_env::PROFILE_NAME));
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
        assert_eq!(
            path.file_name().and_then(|name| name.to_str()),
            Some("e.lock")
        );
        let lock = InstanceLock {
            dsh_pid: 42,
            instances: 2,
        };
        write_lock(&path, &lock);
        assert_eq!(read_lock(&path), Some(lock));
        remove_lock(&path);
        assert_eq!(read_lock(&path), None);
    }

    #[test]
    fn clean_removes_missing_malformed_and_zero_pid_locks() {
        let dir = std::env::temp_dir().join(format!("dshe-clean-test-{}", std::process::id()));
        let path = lock_path(&dir);
        remove_lock(&path);

        assert_eq!(clean(&dir).unwrap(), CleanOutcome::NothingToClean);

        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(
            clean(&dir).unwrap(),
            CleanOutcome::RemovedStaleLock { pid: None }
        );
        assert!(!path.exists());

        write_lock(
            &path,
            &InstanceLock {
                dsh_pid: 0,
                instances: 1,
            },
        );
        assert_eq!(
            clean(&dir).unwrap(),
            CleanOutcome::RemovedStaleLock { pid: None }
        );
        assert!(!path.exists());
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn positive_instance_count_does_not_make_an_unreachable_lock_reusable() {
        let stale = InstanceLock {
            dsh_pid: 42,
            instances: 1,
        };
        assert!(!reusable_lock(&stale, false));
        assert!(reusable_lock(&stale, true));
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
        let mut guard = ChildGuard(Some(
            Command::new(test_exe)
                .args([
                    "--exact",
                    "launcher::tests::reap_timeout_child",
                    "--nocapture",
                ])
                .env("DSHE_LAUNCHER_REAP_TEST_CHILD", "1")
                .spawn()
                .expect("spawn reap timeout child"),
        ));
        std::thread::sleep(Duration::from_millis(50));

        let started = std::time::Instant::now();
        let exited = reap_child_with_timeout(guard.child_mut(), Duration::from_millis(40));
        let elapsed = started.elapsed();
        let _ = guard.child_mut().kill();
        let cleaned = reap_child_with_timeout(guard.child_mut(), Duration::from_secs(2));

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
    #[ignore = "spawns a real Windows cmd process tree; run with `cargo test -- --ignored`"]
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
        let _cleanup = PidGuard(wrapper_pid);
        let url = format!("ws://127.0.0.1:{port}");
        if !wait_for_dsh(&url, Duration::from_secs(5)) {
            panic!("cmd-wrapped child server (pid {wrapper_pid}) did not start");
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
            child: Some(StdProcessHandle::new(child)),
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

        let session = acquire(&url, &dir).unwrap();

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
