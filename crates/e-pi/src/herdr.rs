//! Best-effort, pane-local Herdr lifecycle reporting owned by the Pi host.

use std::{
    ffi::OsString,
    process::Stdio,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use e_tui::{
    agent::AuthNoticeKind, input_page::InputPage, login::Page as LoginPage, runtime::RuntimeState,
    SessionStatus,
};
use tokio::{process::Command, sync::watch, task::JoinHandle};

const SOURCE: &str = "custom:pie";
const COMMAND_TIMEOUT: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Unknown,
    Idle,
    Working,
    Blocked(&'static str),
}

impl Status {
    pub fn from_runtime(app: &RuntimeState) -> Self {
        if app.interaction.approval.is_some() {
            return Self::Blocked("Waiting for approval");
        }
        if let Some(page) = &app.interaction.input_page {
            match &page.page {
                InputPage::Question(_) => return Self::Blocked("Waiting for an answer"),
                InputPage::Login(login) => match &login.page {
                    LoginPage::NativePrompt(_) | LoginPage::NativeLogout { .. } => {
                        return Self::Blocked("Waiting for authentication");
                    }
                    LoginPage::NativeWaiting { .. } => {
                        let notice = login
                            .auth_notices
                            .iter()
                            .rev()
                            .find(|notice| notice.kind != AuthNoticeKind::Information);
                        return if notice.is_some_and(|notice| {
                            matches!(
                                notice.kind,
                                AuthNoticeKind::AuthorizationUrl | AuthNoticeKind::DeviceCode
                            )
                        }) {
                            Self::Blocked("Waiting for authentication")
                        } else {
                            Self::Working
                        };
                    }
                    _ => {}
                },
                _ => {}
            }
        }
        if app.session.status == SessionStatus::Running
            || app.session.working
            || app.has_active_command()
            || app
                .session
                .new_conversation
                .as_ref()
                .is_some_and(|draft| draft.pending_input.is_some())
        {
            Self::Working
        } else if app.session.session_id.is_some() {
            Self::Idle
        } else {
            Self::Unknown
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Idle => "idle",
            Self::Working => "working",
            Self::Blocked(_) => "blocked",
        }
    }
}

pub struct Reporter {
    status: watch::Sender<Status>,
    worker: JoinHandle<()>,
}

impl Reporter {
    pub fn from_env() -> Option<Self> {
        if std::env::var("HERDR_ENV").as_deref() != Ok("1") {
            return None;
        }
        let endpoint = Endpoint {
            executable: nonempty_env("HERDR_BIN_PATH")?,
            socket: nonempty_env("HERDR_SOCKET_PATH")?,
            pane: nonempty_env("HERDR_PANE_ID")?,
        };
        let (status, updates) = watch::channel(Status::Unknown);
        let worker = tokio::spawn(endpoint.run(updates));
        Some(Self { status, worker })
    }

    pub fn report(&self, status: Status) {
        self.status.send_if_modified(|current| {
            if *current == status {
                return false;
            }
            *current = status;
            true
        });
    }

    pub async fn shutdown(self) {
        let Self { status, worker } = self;
        drop(status);
        let _ = worker.await;
    }
}

fn nonempty_env(name: &str) -> Option<OsString> {
    std::env::var_os(name).filter(|value| !value.is_empty())
}

struct Endpoint {
    executable: OsString,
    socket: OsString,
    pane: OsString,
}

impl Endpoint {
    async fn run(self, mut updates: watch::Receiver<Status>) {
        let mut seq = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| u64::try_from(elapsed.as_micros()).ok())
            .unwrap_or_default();
        loop {
            if updates.has_changed().is_err() {
                break;
            }
            let status = *updates.borrow_and_update();
            for _ in 0..2 {
                seq = seq.saturating_add(1);
                if self.send(Some(status), seq).await || updates.has_changed().unwrap_or(true) {
                    break;
                }
            }
            if updates.changed().await.is_err() {
                break;
            }
        }
        let _ = self.send(None, seq.saturating_add(1)).await;
    }

    async fn send(&self, status: Option<Status>, seq: u64) -> bool {
        let mut command = Command::new(&self.executable);
        command
            .arg("pane")
            .arg(if status.is_some() {
                "report-agent"
            } else {
                "release-agent"
            })
            .arg(&self.pane)
            .args(["--source", SOURCE, "--agent", "pie", "--seq"])
            .arg(seq.to_string())
            .env("HERDR_SOCKET_PATH", &self.socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        if let Some(status) = status {
            command.args(["--state", status.label()]);
            if let Status::Blocked(message) = status {
                command.args(["--message", message]);
            }
        }
        #[cfg(windows)]
        command.creation_flags(winapi::um::winbase::CREATE_NO_WINDOW);
        matches!(
            tokio::time::timeout(COMMAND_TIMEOUT, command.status()).await,
            Ok(Ok(exit)) if exit.success()
        )
    }
}
