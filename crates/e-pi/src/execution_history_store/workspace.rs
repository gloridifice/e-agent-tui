use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions, TryLockError},
    io::{BufRead, BufReader, Read, Write},
    path::{Component, Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use e_tui::execution_history::{TraceIdentity, TraceLine, TRACE_VERSION};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const FRONTEND: &str = "e-pi";
const VERSION: u32 = 1;
const HEADER_LIMIT: u64 = 64 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct WorkspaceIdentity {
    version: u32,
    path: String,
}

impl WorkspaceIdentity {
    pub(super) fn new(cwd: &str) -> Result<Self, String> {
        #[cfg(windows)]
        let cwd = {
            let path = cwd.replace('\\', "/");
            if let Some(unc) = path.strip_prefix("//?/UNC/") {
                format!("//{unc}")
            } else {
                path.strip_prefix("//?/").unwrap_or(&path).to_owned()
            }
        };
        let path = Path::new(&cwd);
        if !path.is_absolute() {
            return Err("execution-history workspace must be an absolute path".into());
        }
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    normalized.pop();
                }
                _ => normalized.push(component.as_os_str()),
            }
        }
        let path = normalized
            .to_str()
            .ok_or_else(|| "execution-history workspace is not UTF-8".to_owned())?
            .to_owned();
        #[cfg(windows)]
        let path = {
            let mut path = path.replace('\\', "/");
            if path.as_bytes().get(1) == Some(&b':') {
                path[..1].make_ascii_uppercase();
            }
            path
        };
        Ok(Self {
            version: VERSION,
            path,
        })
    }

    pub(super) fn key(&self) -> String {
        let digest = Sha256::digest(format!("workspace-v{}\0{}", self.version, self.path));
        let hex: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
        format!("ws-{hex}")
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct WorkspaceEntry {
    workspace_path: String,
    display_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at_unix_ms: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
struct Registry {
    version: u32,
    workspaces: BTreeMap<String, WorkspaceEntry>,
}

pub(super) fn root() -> Result<PathBuf, String> {
    root_from_config(crate::config::user_config_dir())
}

fn root_from_config(config: Option<PathBuf>) -> Result<PathBuf, String> {
    config
        .filter(|path| path.is_absolute())
        .map(|path| path.join("cache").join(FRONTEND).join("history"))
        .ok_or_else(|| {
            "execution history unavailable: user configuration directory is unresolved".into()
        })
}

pub(super) fn header(identity: &TraceIdentity) -> Result<serde_json::Value, String> {
    let mut header = serde_json::to_value(TraceLine::Header {
        version: TRACE_VERSION,
        identity: identity.clone(),
    })
    .map_err(|error| error.to_string())?;
    header["workspace"] = serde_json::to_value(WorkspaceIdentity::new(&identity.cwd)?)
        .map_err(|error| error.to_string())?;
    if serde_json::to_vec(&header)
        .map_err(|error| error.to_string())?
        .len() as u64
        + 1
        > HEADER_LIMIT
    {
        return Err("execution-history header is too large".into());
    }
    Ok(header)
}

pub(super) fn validate_header(value: &serde_json::Value) -> Result<TraceIdentity, String> {
    let mut trace = value.clone();
    if let Some(object) = trace.as_object_mut() {
        object.remove("workspace");
    }
    let line: TraceLine = serde_json::from_value(trace)
        .map_err(|error| format!("malformed execution-history header: {error}"))?;
    let TraceLine::Header { version, identity } = line else {
        return Err("execution-history header is missing".into());
    };
    if version != TRACE_VERSION {
        return Err(format!("unsupported execution-history version {version}"));
    }
    let workspace: WorkspaceIdentity = serde_json::from_value(value["workspace"].clone())
        .map_err(|error| format!("malformed execution-history workspace header: {error}"))?;
    if workspace.version != VERSION {
        return Err(format!(
            "unsupported workspace version {}",
            workspace.version
        ));
    }
    if identity.frontend != FRONTEND || workspace != WorkspaceIdentity::new(&identity.cwd)? {
        return Err("execution-history workspace identity mismatch".into());
    }
    Ok(identity)
}

pub(super) fn read_header(path: &Path) -> Result<serde_json::Value, String> {
    let file = File::open(path).map_err(|error| format!("read {}: {error}", path.display()))?;
    let mut line = String::new();
    BufReader::new(file.take(HEADER_LIMIT + 1))
        .read_line(&mut line)
        .map_err(|error| format!("read execution-history header: {error}"))?;
    if line.len() as u64 > HEADER_LIMIT || !line.ends_with('\n') {
        return Err("execution-history header is incomplete or too large".into());
    }
    serde_json::from_str(&line)
        .map_err(|error| format!("malformed execution-history header: {error}"))
}

fn lock(root: &Path) -> Result<File, String> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(root.join(".workspaces.lock"))
        .map_err(|error| format!("open workspace lock: {error}"))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(error) => return Err(format!("lock execution-history workspaces: {error}")),
        }
    }
}

pub(super) fn register(root: &Path, identity: &TraceIdentity) -> Result<Vec<String>, String> {
    if !root.is_absolute() || identity.frontend != FRONTEND {
        return Err("execution-history root or frontend identity is invalid".into());
    }
    let workspace = WorkspaceIdentity::new(&identity.cwd)?;
    fs::create_dir_all(root)
        .map_err(|error| format!("create history root {}: {error}", root.display()))?;
    let _lock = lock(root)?;
    let path = root.join("workspaces.json");
    let bytes = match fs::read(&path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(format!("read workspace registry: {error}")),
    };
    let parsed = bytes
        .as_deref()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(bytes).ok());
    if let Some(version) = parsed.as_ref().and_then(|value| value.get("version")) {
        if version != VERSION {
            return Err(format!("unsupported workspace registry version {version}"));
        }
    }
    let existing = parsed.and_then(|value| serde_json::from_value::<Registry>(value).ok());
    let rebuilding = existing.is_none();
    let mut registry = match existing {
        Some(registry) => registry,
        None => recover(root)?,
    };
    for (key, entry) in &registry.workspaces {
        let stored = WorkspaceIdentity::new(&entry.workspace_path)?;
        if stored.path != entry.workspace_path
            || stored.key() != *key
            || WorkspaceIdentity::new(&entry.display_path)? != stored
        {
            return Err("execution-history workspace registry identity conflict".into());
        }
    }
    let key = workspace.key();
    if registry
        .workspaces
        .get(&key)
        .is_some_and(|entry| entry.workspace_path != workspace.path)
    {
        return Err("execution-history workspace key collision".into());
    }
    let new_workspace = !registry.workspaces.contains_key(&key);
    registry
        .workspaces
        .entry(key)
        .or_insert_with(|| WorkspaceEntry {
            workspace_path: workspace.path,
            display_path: identity.cwd.clone(),
            created_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64),
        });
    let mut warnings = Vec::new();
    if rebuilding || new_workspace {
        if rebuilding {
            if let Some(bytes) = bytes {
                let mut backup = tempfile::Builder::new()
                    .prefix("workspaces.corrupt-")
                    .suffix(".json")
                    .tempfile_in(root)
                    .map_err(|error| format!("preserve workspace registry: {error}"))?;
                backup
                    .write_all(&bytes)
                    .map_err(|error| error.to_string())?;
                let (_, backup_path) = backup.keep().map_err(|error| error.to_string())?;
                warnings.push(format!(
                    "workspace registry rebuilt; malformed original preserved at {}",
                    backup_path.display()
                ));
            }
        }
        let mut temporary = tempfile::NamedTempFile::new_in(root)
            .map_err(|error| format!("create workspace registry: {error}"))?;
        serde_json::to_writer_pretty(&mut temporary, &registry)
            .map_err(|error| error.to_string())?;
        temporary
            .write_all(b"\n")
            .map_err(|error| error.to_string())?;
        temporary.flush().map_err(|error| error.to_string())?;
        temporary
            .persist(&path)
            .map_err(|error| format!("replace workspace registry: {error}"))?;
    }
    Ok(warnings)
}

fn recover(root: &Path) -> Result<Registry, String> {
    let mut registry = Registry {
        version: VERSION,
        workspaces: BTreeMap::new(),
    };
    for directory in fs::read_dir(root).map_err(|error| error.to_string())? {
        let directory = directory.map_err(|error| error.to_string())?;
        if !directory
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            continue;
        }
        let key = directory.file_name().to_string_lossy().into_owned();
        if !key.starts_with("ws-") {
            continue;
        }
        for trace in fs::read_dir(directory.path()).map_err(|error| error.to_string())? {
            let trace = trace.map_err(|error| error.to_string())?;
            if !trace
                .file_type()
                .map_err(|error| error.to_string())?
                .is_file()
                || trace
                    .path()
                    .extension()
                    .is_none_or(|extension| extension != "jsonl")
            {
                continue;
            }
            let identity = validate_header(&read_header(&trace.path())?)?;
            let workspace = WorkspaceIdentity::new(&identity.cwd)?;
            if workspace.key() != key {
                return Err(
                    "execution-history directory/header workspace identity conflict".into(),
                );
            }
            registry
                .workspaces
                .entry(key.clone())
                .or_insert(WorkspaceEntry {
                    workspace_path: workspace.path,
                    display_path: identity.cwd,
                    created_at_unix_ms: None,
                });
        }
    }
    Ok(registry)
}

#[cfg(test)]
mod tests;
