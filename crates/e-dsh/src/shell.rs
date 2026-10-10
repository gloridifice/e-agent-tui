//! Adapter-owned noninteractive shell execution and process lifetime.

use e_tui::{
    display::DisplayId,
    shell::{bound_output, ShellRequest, ShellResult, OUTPUT_CACHE_BYTES},
    EffectResult,
};
use std::{
    ffi::OsString,
    io::{Read, Seek, SeekFrom},
    path::Path,
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

pub struct ShellRunner {
    program: OsString,
    running: Option<Running>,
}

impl Default for ShellRunner {
    fn default() -> Self {
        Self {
            program: shell_program(),
            running: None,
        }
    }
}

impl ShellRunner {
    pub fn name(&self) -> String {
        let name = Path::new(&self.program)
            .file_stem()
            .unwrap_or(&self.program)
            .to_string_lossy();
        match name.as_ref() {
            "pwsh" | "powershell" => "powershell".into(),
            _ => name.into_owned(),
        }
    }

    pub fn start(&mut self, request: ShellRequest) -> Result<(), String> {
        if self.running.is_some() {
            return Err("A local shell command is still stopping".into());
        }
        let (cancel, receiver) = oneshot::channel();
        let id = request.id.clone();
        let program = self.program.clone();
        let task = tokio::spawn(async move {
            let started = Instant::now();
            let mut result = match run(program, request, receiver).await {
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
    program: OsString,
    request: ShellRequest,
    mut cancel: oneshot::Receiver<()>,
) -> Result<ShellResult, String> {
    let mut file =
        tempfile::tempfile().map_err(|error| format!("capture shell output: {error}"))?;
    let mut command = shell_command(program, &request.command);
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
    let (output, output_truncated) = tokio::task::spawn_blocking(move || {
        let offset = file
            .metadata()?
            .len()
            .saturating_sub(OUTPUT_CACHE_BYTES as u64);
        file.seek(SeekFrom::Start(offset.saturating_sub(1)))?;
        let mut bytes = Vec::new();
        file.take(OUTPUT_CACHE_BYTES as u64 + 1)
            .read_to_end(&mut bytes)?;
        if offset > 0 {
            let start = bytes
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(bytes.len(), |i| i + 1);
            bytes.drain(..start);
        }
        let mut output = String::from_utf8_lossy(&bytes).into_owned();
        let truncated = bound_output(&mut output) || offset > 0;
        Ok::<_, std::io::Error>((output, truncated))
    })
    .await
    .map_err(|error| error.to_string())?
    .map_err(|error| format!("read shell output: {error}"))?;
    Ok(ShellResult {
        output,
        output_truncated,
        exit_code: status.code(),
        cancelled,
        ..Default::default()
    })
}

fn shell_program() -> OsString {
    #[cfg(windows)]
    {
        if std::env::var_os("PATH").is_some_and(|path| {
            std::env::split_paths(&path).any(|directory| directory.join("pwsh.exe").is_file())
        }) {
            "pwsh.exe"
        } else {
            "powershell.exe"
        }
        .into()
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("SHELL").unwrap_or_else(|| "sh".into())
    }
}

fn shell_command(program: OsString, source: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        command.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command"]);
        command.arg(format!(
            "[Console]::OutputEncoding = [System.Text.UTF8Encoding]::new(); $OutputEncoding = [Console]::OutputEncoding; & {{ {source}\n}}; $ok = $?; if ($null -ne $LASTEXITCODE) {{ exit $LASTEXITCODE }}; if (-not $ok) {{ exit 1 }}"
        ));
    }
    #[cfg(not(windows))]
    command.arg("-c").arg(source);
    command
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
