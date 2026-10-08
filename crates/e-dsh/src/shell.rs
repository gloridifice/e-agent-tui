//! Adapter-owned noninteractive shell execution and process lifetime.

use e_tui::{
    display::DisplayId,
    shell::{ShellRequest, ShellResult},
    EffectResult,
};
use std::{
    io::{Read, Seek, SeekFrom},
    process::Stdio,
    time::{Duration, Instant},
};
use tokio::{
    process::{Child, Command},
    sync::oneshot,
    task::JoinHandle,
};

struct Running {
    id: DisplayId,
    cancel: Option<oneshot::Sender<()>>,
    task: JoinHandle<ShellResult>,
}

#[derive(Default)]
pub struct ShellRunner {
    running: Option<Running>,
}

impl ShellRunner {
    pub fn start(&mut self, request: ShellRequest) -> Result<(), String> {
        if self.running.is_some() {
            return Err("A local shell command is still stopping".into());
        }
        let (cancel, receiver) = oneshot::channel();
        let id = request.id.clone();
        let task = tokio::spawn(async move {
            let started = Instant::now();
            let mut result = match run(request, receiver).await {
                Ok(result) => result,
                Err(error) => ShellResult {
                    error: Some(error),
                    ..Default::default()
                },
            };
            result.duration_ms = started.elapsed().as_millis() as u64;
            result
        });
        self.running = Some(Running {
            id,
            cancel: Some(cancel),
            task,
        });
        Ok(())
    }

    pub fn cancel(&mut self, id: &DisplayId) {
        if let Some(running) = self.running.as_mut().filter(|running| &running.id == id) {
            if let Some(cancel) = running.cancel.take() {
                let _ = cancel.send(());
            }
        }
    }

    pub async fn next(&mut self) -> EffectResult {
        let Some(running) = self.running.as_mut() else {
            return std::future::pending().await;
        };
        let result = (&mut running.task)
            .await
            .unwrap_or_else(|error| ShellResult {
                error: Some(format!("shell worker failed: {error}")),
                ..Default::default()
            });
        let running = self.running.take().expect("awaited shell worker");
        EffectResult::ShellFinished {
            id: running.id,
            result,
        }
    }

    pub async fn shutdown(&mut self) {
        if let Some(running) = self.running.take() {
            if let Some(cancel) = running.cancel {
                let _ = cancel.send(());
            }
            let _ = running.task.await;
        }
    }
}

impl Drop for ShellRunner {
    fn drop(&mut self) {
        if let Some(running) = self.running.as_mut() {
            if let Some(cancel) = running.cancel.take() {
                let _ = cancel.send(());
            }
        }
    }
}

async fn run(
    request: ShellRequest,
    mut cancel: oneshot::Receiver<()>,
) -> Result<ShellResult, String> {
    let mut file =
        tempfile::tempfile().map_err(|error| format!("capture shell output: {error}"))?;
    let mut command = shell_command(&request.command);
    if let Some(cwd) = request.cwd {
        command.current_dir(cwd);
    }
    command
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            file.try_clone().map_err(|error| error.to_string())?,
        ))
        .stderr(Stdio::from(
            file.try_clone().map_err(|error| error.to_string())?,
        ))
        .kill_on_drop(true);
    #[cfg(unix)]
    command.process_group(0);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    let mut child = command
        .spawn()
        .map_err(|error| format!("start shell: {error}"))?;
    let mut cancelled = false;
    let status = tokio::select! {
        result = child.wait() => result,
        _ = &mut cancel => {
            cancelled = true;
            terminate(&mut child).await;
            child.wait().await
        }
    }
    .map_err(|error| format!("wait for shell: {error}"))?;
    let output = tokio::task::spawn_blocking(move || {
        file.seek(SeekFrom::Start(0))?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        Ok::<_, std::io::Error>(String::from_utf8_lossy(&bytes).into_owned())
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| format!("read shell output: {error}"))?;
    Ok(ShellResult {
        output,
        exit_code: status.code(),
        cancelled,
        ..Default::default()
    })
}

fn shell_command(source: &str) -> Command {
    #[cfg(windows)]
    {
        let shell = if std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|directory| directory.join("pwsh.exe").is_file())
        }) {
            "pwsh.exe"
        } else {
            "powershell.exe"
        };
        let mut command = Command::new(shell);
        command.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]);
        command.arg(format!(
            "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); $OutputEncoding = [Console]::OutputEncoding; & {{ {source}\n}}; $ok = $?; if ($null -ne $LASTEXITCODE) {{ exit $LASTEXITCODE }}; if (-not $ok) {{ exit 1 }}"
        ));
        command
    }
    #[cfg(not(windows))]
    {
        let mut command = Command::new(std::env::var_os("SHELL").unwrap_or_else(|| "sh".into()));
        command.arg("-c").arg(source);
        command
    }
}

async fn terminate(child: &mut Child) {
    if let Some(pid) = child.id() {
        #[cfg(windows)]
        let mut kill = {
            let mut command = Command::new("taskkill.exe");
            command
                .args(["/PID", &pid.to_string(), "/T", "/F"])
                .creation_flags(0x0800_0000);
            command
        };
        #[cfg(not(windows))]
        let mut kill = {
            let mut command = Command::new("kill");
            command.args(["-KILL", "--", &format!("-{pid}")]);
            command
        };
        kill.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let _ = tokio::time::timeout(Duration::from_secs(3), kill.status()).await;
    }
    let _ = child.kill().await;
}
