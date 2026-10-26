//! Correlated, non-replacing preparation of an independent native conversation.

use std::time::{Duration, Instant};

use e_tui::{
    agent::ModelSelection,
    runtime::sessions::{SidePreparationResult, SideSessionKind, SideSessionSeed},
};
use serde::Deserialize;

use crate::protocol::{extension_ui_request, RpcCommand, RpcRecord};

pub const COMMAND: &str = "__pie_side_v1";
const STATUS_KEY: &str = "pie-side-v1";
const MAX_CONTROL_BYTES: usize = 512 * 1024;
const PREPARATION_TIMEOUT: Duration = Duration::from_secs(20);

pub fn is_control_record(record: &RpcRecord) -> bool {
    if record.kind == "response" {
        return record
            .string("id")
            .is_some_and(|id| id.starts_with("pie-side-"));
    }
    record.kind == "extension_ui_request"
        && extension_ui_request(record).is_ok_and(|request| {
            request.method == "setStatus" && request.status_key.as_deref() == Some(STATUS_KEY)
        })
}

struct Pending {
    id: String,
    source: String,
    deadline: Option<Instant>,
}

#[derive(Default)]
pub struct SideSessionControl {
    pending: Option<Pending>,
    next_id: u64,
    destination: Option<std::path::PathBuf>,
}

#[derive(Deserialize)]
struct Control {
    protocol: u32,
    outcome: String,
    message: Option<String>,
    session: Option<String>,
    directory: Option<std::path::PathBuf>,
    persisted: Option<bool>,
    cwd: Option<String>,
    model: Option<Route>,
    editor: Option<String>,
    parent: Option<String>,
}

#[derive(Deserialize)]
struct Route {
    provider: String,
    model: String,
    effort: Option<String>,
}

impl SideSessionControl {
    pub fn take_destination(&mut self) -> Option<std::path::PathBuf> {
        self.destination.take()
    }

    pub fn begin(
        &mut self,
        source: &str,
        kind: SideSessionKind,
        now: Instant,
    ) -> Result<RpcCommand, String> {
        if self.pending.is_some() {
            return Err("A side session is already being prepared".into());
        }
        if source.is_empty() {
            return Err("Wait for the source session to attach".into());
        }
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or("Side operation identity exhausted")?;
        let id = format!("pie-side-{}", self.next_id);
        let kind = match kind {
            SideSessionKind::Clone => "clone",
            SideSessionKind::New => "new",
            SideSessionKind::Fork => "fork",
        };
        let message = format!(
            "/{COMMAND} {}",
            serde_json::json!({ "id": id, "kind": kind })
        );
        self.pending = Some(Pending {
            id: id.clone(),
            source: source.into(),
            deadline: Some(now + PREPARATION_TIMEOUT),
        });
        Ok(RpcCommand::Prompt {
            id: Some(id),
            message,
            streaming_behavior: None,
        })
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.pending.as_ref().and_then(|pending| pending.deadline)
    }

    pub fn expire(&mut self, now: Instant) -> Option<SidePreparationResult> {
        if !self.deadline().is_some_and(|deadline| deadline <= now) {
            return None;
        }
        self.pending = None;
        Some(SidePreparationResult::Failed(
            "Side session preparation timed out".into(),
        ))
    }

    pub fn cancel_command(&mut self) -> Option<RpcCommand> {
        let pending = self.pending.take()?;
        Some(RpcCommand::Prompt {
            id: Some(format!("{}-cancel", pending.id)),
            message: format!(
                "/{COMMAND} {}",
                serde_json::json!({"id": pending.id, "kind": "cancel", "target": pending.id})
            ),
            streaming_behavior: None,
        })
    }

    pub fn invalidate(&mut self) -> Option<SidePreparationResult> {
        self.pending
            .take()
            .map(|_| SidePreparationResult::Cancelled)
    }

    pub fn record(
        &mut self,
        record: &RpcRecord,
        current_source: &str,
        now: Instant,
    ) -> Option<SidePreparationResult> {
        let pending = self.pending.as_ref()?;
        if pending.source != current_source {
            self.pending = None;
            return Some(SidePreparationResult::Failed(
                "The source session changed during side preparation".into(),
            ));
        }
        if record.kind == "response" && record.string("id") == Some(pending.id.as_str()) {
            if record.bool("success") != Some(true) {
                let message = record
                    .string("error")
                    .unwrap_or("Pi rejected side session preparation")
                    .to_owned();
                self.pending = None;
                return Some(SidePreparationResult::Failed(message));
            }
            return None;
        }
        if record.kind != "extension_ui_request" {
            return None;
        }
        let request = extension_ui_request(record).ok()?;
        if request.method != "setStatus" || request.status_key.as_deref() != Some(STATUS_KEY) {
            return None;
        }
        let text = request.status_text.as_deref().unwrap_or_default();
        if text.len() > MAX_CONTROL_BYTES {
            self.pending = None;
            return Some(SidePreparationResult::Failed(
                "Side session control exceeds 512 KiB".into(),
            ));
        }
        let value: serde_json::Value = serde_json::from_str(text).ok()?;
        if value.get("id").and_then(serde_json::Value::as_str) != Some(pending.id.as_str()) {
            return None;
        }
        let control = match serde_json::from_value::<Control>(value) {
            Ok(control) => control,
            Err(_) => {
                self.pending = None;
                return Some(SidePreparationResult::Failed(
                    "Invalid side session control".into(),
                ));
            }
        };
        if control.protocol != 1 {
            self.pending = None;
            return Some(SidePreparationResult::Failed(
                "Unsupported side session protocol".into(),
            ));
        }
        match control.outcome.as_str() {
            "selecting" => {
                self.pending.as_mut()?.deadline = None;
                None
            }
            "preparing" => {
                self.pending.as_mut()?.deadline = Some(now + PREPARATION_TIMEOUT);
                None
            }
            _ => {
                self.pending = None;
                self.destination = control.directory.clone();
                Some(match control.outcome.as_str() {
                    "cancelled" => SidePreparationResult::Cancelled,
                    "error" => SidePreparationResult::Failed(
                        control
                            .message
                            .unwrap_or_else(|| "Side preparation failed".into()),
                    ),
                    "ready" => match seed(control) {
                        Ok(seed) => SidePreparationResult::Ready(seed),
                        Err(message) => SidePreparationResult::Failed(message),
                    },
                    _ => SidePreparationResult::Failed("Unknown side preparation outcome".into()),
                })
            }
        }
    }
}

pub struct PreparedSideFile(Option<std::path::PathBuf>);

impl PreparedSideFile {
    pub async fn materialize(
        seed: &mut SideSessionSeed,
        directory: Option<std::path::PathBuf>,
        staging_root: &std::path::Path,
    ) -> Result<Option<Self>, String> {
        let Some(source) = seed.session_id.as_ref().map(std::path::PathBuf::from) else {
            return Ok(None);
        };
        let directory = directory
            .filter(|path| path.is_absolute())
            .ok_or("Missing native side directory")?;
        let destination = directory.join(source.file_name().ok_or("Invalid staged side path")?);
        if destination == source {
            return Err("Side preparation was not staged independently".into());
        }
        let staging_root = staging_root.to_owned();
        let artifact = tokio::task::spawn_blocking(move || -> std::io::Result<Self> {
            let source = std::fs::canonicalize(source)?;
            let staging_root = std::fs::canonicalize(staging_root)?;
            if source.parent().and_then(std::path::Path::parent) != Some(staging_root.as_path()) {
                return Err(std::io::Error::other(
                    "Side artifact is outside the owned staging directory",
                ));
            }
            let result = (|| {
                if std::fs::canonicalize(&directory)?.starts_with(&staging_root) {
                    return Err(std::io::Error::other("Native side directory is temporary"));
                }
                let mut input = std::fs::File::open(&source)?;
                let mut output = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&destination)?;
                let artifact = Self(Some(destination));
                std::io::copy(&mut input, &mut output)?;
                Ok(artifact)
            })();
            let _ = std::fs::remove_file(&source);
            if let Some(parent) = source.parent() {
                let _ = std::fs::remove_dir(parent);
            }
            result
        })
        .await
        .map_err(|error| error.to_string())?
        .map_err(|error| error.to_string())?;
        seed.session_id = artifact
            .0
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned());
        Ok(Some(artifact))
    }

    pub fn commit(mut self) {
        self.0.take();
    }

    pub async fn cleanup(mut self) {
        if let Some(path) = self.0.take() {
            let _ = tokio::fs::remove_file(path).await;
        }
    }
}

struct RemovePreparedFile(std::path::PathBuf);

impl Drop for RemovePreparedFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl Drop for PreparedSideFile {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() {
            let cleanup = RemovePreparedFile(path);
            if let Ok(runtime) = tokio::runtime::Handle::try_current() {
                runtime.spawn_blocking(move || drop(cleanup));
            }
        }
    }
}

fn seed(control: Control) -> Result<SideSessionSeed, String> {
    let workspace = control
        .cwd
        .filter(|cwd| !cwd.is_empty())
        .ok_or("Missing side workspace")?;
    if !std::path::Path::new(&workspace).is_absolute() {
        return Err("Side workspace is not absolute".into());
    }
    let session_id = if control.persisted.ok_or("Missing side persistence status")? {
        let session = control
            .session
            .filter(|session| !session.is_empty())
            .ok_or("Missing side session identity")?;
        if !std::path::Path::new(&session).is_absolute() {
            return Err("Side attachment identity is not an absolute native path".into());
        }
        Some(session)
    } else {
        None
    };
    let model = control.model.map(|route| ModelSelection {
        provider: route.provider,
        model: route.model,
        reasoning_effort: route.effort,
    });
    Ok(SideSessionSeed {
        session_id,
        workspace,
        model,
        editor_text: control.editor.unwrap_or_default(),
        parent_session_id: control.parent,
    })
}
