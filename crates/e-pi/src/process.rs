//! Pi RPC child-process lifecycle and bounded stdio channels.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
    sync::mpsc,
};

use crate::{
    framing::{JsonlDecoder, DEFAULT_MAX_RECORD_BYTES},
    protocol::{RpcCommand, RpcRecord},
};

const CHANNEL_CAPACITY: usize = 256;
const STDERR_TAIL_BYTES: usize = 32 * 1024;
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectTrust {
    Native,
    Approve,
    Reject,
}

#[derive(Debug, Clone)]
pub struct PiLaunchOptions {
    pub cwd: PathBuf,
    pub session: Option<String>,
    pub trust: ProjectTrust,
    pub executable: String,
    /// Optional executable prefix arguments, primarily for wrappers.
    pub executable_args: Vec<String>,
}

impl PiLaunchOptions {
    pub fn for_cwd(cwd: impl Into<PathBuf>) -> Self {
        Self {
            cwd: cwd.into(),
            session: None,
            trust: ProjectTrust::Native,
            executable: std::env::var("PIE_PI_COMMAND").unwrap_or_else(|_| "pi".into()),
            executable_args: Vec::new(),
        }
    }

    pub fn args(&self) -> Vec<String> {
        let mut args = vec!["--mode".into(), "rpc".into()];
        match self.trust {
            ProjectTrust::Native => {}
            ProjectTrust::Approve => args.push("--approve".into()),
            ProjectTrust::Reject => args.push("--no-approve".into()),
        }
        if let Some(session) = &self.session {
            args.extend(["--session".into(), session.clone()]);
        }
        args
    }
}

#[derive(Debug)]
pub enum PiProcessEvent {
    Record(RpcRecord),
    Fatal(String),
    Eof,
}

pub struct PiProcess {
    child: Child,
    outbound: mpsc::Sender<RpcCommand>,
    inbound: mpsc::Receiver<PiProcessEvent>,
    stderr_tail: Arc<Mutex<VecDeque<u8>>>,
}

impl PiProcess {
    pub async fn spawn(options: &PiLaunchOptions) -> anyhow::Result<Self> {
        let mut command = Command::new(&options.executable);
        command
            .args(&options.executable_args)
            .args(options.args())
            .current_dir(&options.cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|error| {
            anyhow::anyhow!(
                "cannot start Pi RPC with `{}`: {error}. Install @earendil-works/pi-coding-agent and ensure `pi` is on PATH",
                options.executable
            )
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Pi RPC stdin was not captured"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Pi RPC stdout was not captured"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("Pi RPC stderr was not captured"))?;

        let (outbound, outbound_rx) = mpsc::channel(CHANNEL_CAPACITY);
        let (inbound_tx, inbound) = mpsc::channel(CHANNEL_CAPACITY);
        let stderr_tail = Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_TAIL_BYTES)));

        tokio::spawn(write_loop(stdin, outbound_rx, inbound_tx.clone()));
        tokio::spawn(read_loop(stdout, inbound_tx));
        tokio::spawn(stderr_loop(stderr, Arc::clone(&stderr_tail)));

        Ok(Self {
            child,
            outbound,
            inbound,
            stderr_tail,
        })
    }

    pub fn sender(&self) -> mpsc::Sender<RpcCommand> {
        self.outbound.clone()
    }

    pub async fn send(&self, command: RpcCommand) -> Result<(), String> {
        self.outbound
            .send(command)
            .await
            .map_err(|_| "Pi RPC stdin channel closed".into())
    }

    pub async fn recv(&mut self) -> Option<PiProcessEvent> {
        self.inbound.recv().await
    }

    pub fn stderr_tail(&self) -> String {
        let bytes = self
            .stderr_tail
            .lock()
            .unwrap()
            .iter()
            .copied()
            .collect::<Vec<u8>>();
        String::from_utf8_lossy(&bytes).into_owned()
    }

    pub fn try_exit(&mut self) -> Result<Option<std::process::ExitStatus>, std::io::Error> {
        self.child.try_wait()
    }

    pub async fn shutdown(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.start_kill();
        }
        let _ = tokio::time::timeout(SHUTDOWN_TIMEOUT, self.child.wait()).await;
    }
}

impl Drop for PiProcess {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.start_kill();
        }
    }
}

async fn write_loop(
    mut stdin: tokio::process::ChildStdin,
    mut commands: mpsc::Receiver<RpcCommand>,
    events: mpsc::Sender<PiProcessEvent>,
) {
    while let Some(command) = commands.recv().await {
        let mut wire = match serde_json::to_vec(&command) {
            Ok(wire) => wire,
            Err(error) => {
                let _ = events
                    .send(PiProcessEvent::Fatal(format!(
                        "cannot encode Pi RPC command: {error}"
                    )))
                    .await;
                return;
            }
        };
        wire.push(b'\n');
        if let Err(error) = stdin.write_all(&wire).await {
            let _ = events
                .send(PiProcessEvent::Fatal(format!(
                    "cannot write Pi RPC stdin: {error}"
                )))
                .await;
            return;
        }
        if let Err(error) = stdin.flush().await {
            let _ = events
                .send(PiProcessEvent::Fatal(format!(
                    "cannot flush Pi RPC stdin: {error}"
                )))
                .await;
            return;
        }
    }
    let _ = stdin.shutdown().await;
}

async fn read_loop(mut stdout: tokio::process::ChildStdout, events: mpsc::Sender<PiProcessEvent>) {
    let mut decoder = JsonlDecoder::new(DEFAULT_MAX_RECORD_BYTES);
    let mut buffer = [0_u8; 8192];
    loop {
        match stdout.read(&mut buffer).await {
            Ok(0) => {
                if let Some(Err(error)) = decoder.finish() {
                    let _ = events.send(PiProcessEvent::Fatal(error.to_string())).await;
                } else {
                    let _ = events.send(PiProcessEvent::Eof).await;
                }
                return;
            }
            Ok(read) => {
                for record in decoder.push(&buffer[..read]) {
                    let record = match record
                        .map_err(|error| error.to_string())
                        .and_then(|value| RpcRecord::from_value(value).map_err(|e| e.to_string()))
                    {
                        Ok(record) => record,
                        Err(error) => {
                            let _ = events.send(PiProcessEvent::Fatal(error)).await;
                            return;
                        }
                    };
                    if events.send(PiProcessEvent::Record(record)).await.is_err() {
                        return;
                    }
                }
            }
            Err(error) => {
                let _ = events
                    .send(PiProcessEvent::Fatal(format!(
                        "cannot read Pi RPC stdout: {error}"
                    )))
                    .await;
                return;
            }
        }
    }
}

async fn stderr_loop(mut stderr: tokio::process::ChildStderr, tail: Arc<Mutex<VecDeque<u8>>>) {
    let mut buffer = [0_u8; 2048];
    loop {
        let Ok(read) = stderr.read(&mut buffer).await else {
            return;
        };
        if read == 0 {
            return;
        }
        let mut tail = tail.lock().unwrap();
        for byte in &buffer[..read] {
            if tail.len() == STDERR_TAIL_BYTES {
                tail.pop_front();
            }
            tail.push_back(*byte);
        }
    }
}

pub fn session_arg_is_path(value: &str) -> bool {
    Path::new(value)
        .extension()
        .is_some_and(|ext| ext == "jsonl")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_arguments_preserve_native_trust_default() {
        let mut options = PiLaunchOptions::for_cwd(".");
        assert_eq!(options.args(), ["--mode", "rpc"]);
        options.trust = ProjectTrust::Approve;
        options.session = Some("s.jsonl".into());
        assert_eq!(
            options.args(),
            ["--mode", "rpc", "--approve", "--session", "s.jsonl"]
        );
    }

    #[tokio::test]
    async fn child_lifecycle_writes_and_reads_jsonl() {
        let temp = tempfile::tempdir().unwrap();
        let mut options = PiLaunchOptions::for_cwd(".");
        #[cfg(windows)]
        let script = {
            let path = temp.path().join("fake-pi.cmd");
            std::fs::write(
                &path,
                "@echo off\r\nset /p line=\r\necho {\"type\":\"response\",\"id\":\"p1\",\"command\":\"prompt\",\"success\":true}\r\necho diagnostic 1>&2\r\n",
            )
            .unwrap();
            path
        };
        #[cfg(not(windows))]
        let script = {
            use std::os::unix::fs::PermissionsExt;
            let path = temp.path().join("fake-pi.sh");
            std::fs::write(
                &path,
                "#!/bin/sh\nread line\nprintf diagnostic >&2\nprintf '%s\\n' '{\"type\":\"response\",\"id\":\"p1\",\"command\":\"prompt\",\"success\":true}'\n",
            )
            .unwrap();
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
            path
        };
        options.executable = script.to_string_lossy().into_owned();
        let mut process = PiProcess::spawn(&options).await.unwrap();
        process
            .send(RpcCommand::Prompt {
                id: Some("p1".into()),
                message: "hello".into(),
                streaming_behavior: None,
            })
            .await
            .unwrap();
        let event = tokio::time::timeout(Duration::from_secs(5), process.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(
            matches!(
                event,
                PiProcessEvent::Record(RpcRecord { ref kind, .. }) if kind == "response"
            ),
            "unexpected child event: {event:?}"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(process.stderr_tail().contains("diagnostic"));
        process.shutdown().await;
    }
}
