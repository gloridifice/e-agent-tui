//! Read-only, bounded native session metadata discovery.

use std::{
    collections::VecDeque,
    fs::File,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::SystemTime,
};

use chrono::DateTime;
use e_tui::{
    agent::SessionSummary,
    resume::{session_tree, ResumeBatch, ResumeRequest, SessionParents, MAX_RESUME_BATCH_SIZE},
};
use serde::Deserialize;
use serde_json::Value;

const HEAD_BYTES: usize = 256 * 1024;
const TAIL_BYTES: usize = 1024 * 1024;
const BLOCK_BYTES: usize = 64 * 1024;
const TITLE_CHARS: usize = 100;
const MAX_DIAGNOSTICS: usize = 3;

pub fn agent_dir() -> PathBuf {
    std::env::var_os("PI_CODING_AGENT_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .or_else(|| directories::BaseDirs::new().map(|dirs| dirs.home_dir().join(".pi/agent")))
        .unwrap_or_else(|| PathBuf::from(".pi/agent"))
}

pub fn session_root() -> PathBuf {
    std::env::var_os("PI_CODING_AGENT_SESSION_DIR")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| agent_dir().join("sessions"))
}

/// A custom session directory is exact; the default layout adds encoded cwd.
pub fn project_session_root(cwd: &Path) -> PathBuf {
    if let Some(custom) =
        std::env::var_os("PI_CODING_AGENT_SESSION_DIR").filter(|value| !value.is_empty())
    {
        return PathBuf::from(custom);
    }
    let resolved = cwd.to_string_lossy();
    let safe = resolved
        .trim_start_matches(['/', '\\'])
        .replace(['/', '\\', ':'], "-");
    session_root().join(format!("--{safe}--"))
}

pub struct SavedSession {
    pub id: String,
    pub path: PathBuf,
    pub cwd: PathBuf,
}

fn read_header(path: &Path) -> Result<Value, String> {
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut header = Vec::new();
    BufReader::new(file.take((HEAD_BYTES + 1) as u64))
        .read_until(b'\n', &mut header)
        .map_err(|error| error.to_string())?;
    if header.len() > HEAD_BYTES {
        return Err("session header exceeds metadata budget".into());
    }
    let header: Value = serde_json::from_slice(&header).map_err(|error| error.to_string())?;
    if header["type"].as_str() != Some("session") {
        return Err("first record is not a Pi session header".into());
    }
    Ok(header)
}

pub fn read_saved_session(path: &Path) -> Result<SavedSession, String> {
    let header = read_header(path)?;
    let id = header["id"]
        .as_str()
        .filter(|id| !id.is_empty())
        .ok_or("session header has no id")?;
    let cwd = header["cwd"]
        .as_str()
        .filter(|cwd| !cwd.is_empty())
        .ok_or("session header has no cwd")?;
    Ok(SavedSession {
        id: id.to_owned(),
        path: std::path::absolute(path).map_err(|error| error.to_string())?,
        cwd: PathBuf::from(cwd),
    })
}

struct Candidate {
    path: PathBuf,
    modified: Option<SystemTime>,
}

pub struct SessionIndex {
    candidates: Vec<Candidate>,
    diagnostics: Vec<String>,
    parents: SessionParents,
    ancestry_prepared: bool,
}

impl SessionIndex {
    pub fn enumerate(root: &Path) -> Self {
        let mut index = Self {
            candidates: Vec::new(),
            diagnostics: Vec::new(),
            parents: Default::default(),
            ancestry_prepared: false,
        };
        let mut pending = VecDeque::from([root.to_path_buf()]);
        while let Some(directory) = pending.pop_front() {
            let entries = match std::fs::read_dir(&directory) {
                Ok(entries) => entries,
                Err(error) if directory == root && error.kind() == std::io::ErrorKind::NotFound => {
                    break
                }
                Err(error) => {
                    index.diagnostic(format!("cannot read {}: {error}", directory.display()));
                    continue;
                }
            };
            for entry in entries {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        index.diagnostic(error.to_string());
                        continue;
                    }
                };
                let path = entry.path();
                let kind = match entry.file_type() {
                    Ok(kind) => kind,
                    Err(error) => {
                        index.diagnostic(error.to_string());
                        continue;
                    }
                };
                if kind.is_dir() {
                    pending.push_back(path);
                } else if kind.is_file() && path.extension().is_some_and(|ext| ext == "jsonl") {
                    let modified = entry.metadata().ok().and_then(|meta| meta.modified().ok());
                    index.candidates.push(Candidate { path, modified });
                }
            }
        }
        index.candidates.sort_by(|a, b| {
            b.modified
                .cmp(&a.modified)
                .then_with(|| a.path.cmp(&b.path))
        });
        index
    }

    pub fn resolve_id(mut self, id: &str) -> Result<SavedSession, String> {
        let mut found: Option<SavedSession> = None;
        for i in 0..self.candidates.len() {
            let path = &self.candidates[i].path;
            match read_saved_session(path) {
                Ok(session) if session.id == id => {
                    if let Some(previous) = found {
                        return Err(format!(
                            "Pi session ID `{id}` is ambiguous: {} and {}; use --session <file>",
                            previous.path.display(),
                            session.path.display()
                        ));
                    }
                    found = Some(session);
                }
                Ok(_) => {}
                Err(error) => self.diagnostic(format!("{}: {error}", path.display())),
            }
        }
        found.ok_or_else(|| {
            let mut message = format!("No saved Pi session found with ID `{id}`");
            if !self.diagnostics.is_empty() {
                message.push_str(&format!(": {}", self.diagnostics.join("; ")));
            }
            message
        })
    }

    fn diagnostic(&mut self, message: String) {
        if self.diagnostics.len() < MAX_DIAGNOSTICS {
            self.diagnostics.push(message);
        }
    }

    fn prepare_ancestry(&mut self) {
        if self.ancestry_prepared {
            return;
        }
        self.ancestry_prepared = true;
        let paths: std::collections::HashMap<_, _> = self
            .candidates
            .iter()
            .map(|candidate| {
                (
                    path_key(&candidate.path),
                    candidate.path.to_string_lossy().into_owned(),
                )
            })
            .collect();
        for candidate in &self.candidates {
            let parent = read_header(&candidate.path).ok().and_then(|header| {
                header
                    .get("parentSession")
                    .and_then(Value::as_str)
                    .filter(|parent| !parent.is_empty())
                    .map(str::to_owned)
            });
            if let Some(parent) = parent {
                let parent_path = Path::new(&parent);
                let parent_path = if parent_path.is_absolute() {
                    parent_path.to_path_buf()
                } else {
                    candidate
                        .path
                        .parent()
                        .unwrap_or(Path::new("."))
                        .join(parent_path)
                };
                let parent_id = paths
                    .get(&path_key(&parent_path))
                    .cloned()
                    .unwrap_or(parent);
                self.parents
                    .insert(candidate.path.to_string_lossy().into_owned(), parent_id);
            }
        }
        let ids: Vec<_> = self
            .candidates
            .iter()
            .map(|candidate| candidate.path.to_string_lossy().into_owned())
            .collect();
        let order = session_tree(
            &ids.iter().map(String::as_str).collect::<Vec<_>>(),
            &self.parents,
        );
        let mut candidates: Vec<_> = std::mem::take(&mut self.candidates)
            .into_iter()
            .map(Some)
            .collect();
        self.candidates = order
            .into_iter()
            .map(|row| {
                candidates[row.index]
                    .take()
                    .expect("unique session tree entry")
            })
            .collect();
    }

    pub fn load(&mut self, request: ResumeRequest) -> ResumeBatch {
        self.prepare_ancestry();
        let end = request
            .offset
            .saturating_add(request.limit.min(MAX_RESUME_BATCH_SIZE))
            .min(self.candidates.len());
        let mut sessions = Vec::new();
        for i in request.offset..end {
            let candidate = &self.candidates[i];
            match read_summary(candidate, Path::new(&request.workspace)) {
                Ok(Some(summary)) => sessions.push(summary),
                Ok(None) => {}
                Err(error) => self.diagnostic(format!("{}: {error}", candidate.path.display())),
            }
        }
        ResumeBatch {
            request,
            sessions,
            next_offset: end,
            has_more: end < self.candidates.len(),
            diagnostic: (!self.diagnostics.is_empty())
                .then(|| std::mem::take(&mut self.diagnostics).join("; ")),
        }
    }
}

fn read_summary(candidate: &Candidate, cwd: &Path) -> Result<Option<SessionSummary>, String> {
    let mut file = File::open(&candidate.path).map_err(|error| error.to_string())?;
    let len = file.metadata().map_err(|error| error.to_string())?.len();
    let mut head = Vec::new();
    file.by_ref()
        .take(HEAD_BYTES as u64)
        .read_to_end(&mut head)
        .map_err(|error| error.to_string())?;
    let header_end = head
        .iter()
        .position(|byte| *byte == b'\n')
        .unwrap_or(head.len());
    if header_end == HEAD_BYTES && len > HEAD_BYTES as u64 {
        return Err("session header exceeds metadata budget".into());
    }
    let header: Value =
        serde_json::from_slice(&head[..header_end]).map_err(|error| error.to_string())?;
    if header["type"].as_str() != Some("session") {
        return Err("first record is not a Pi session header".into());
    }
    let header_cwd = header["cwd"].as_str().ok_or("session header has no cwd")?;
    if !same_path(Path::new(header_cwd), cwd) {
        return Ok(None);
    }
    let name = latest_name(&mut file, len)
        .ok()
        .flatten()
        .filter(|name| !name.is_empty());
    let title = name
        .or_else(|| {
            head[header_end..]
                .split(|byte| *byte == b'\n')
                .find_map(|line| {
                    let value: Value = serde_json::from_slice(line).ok()?;
                    if value["type"].as_str() != Some("message")
                        || value["message"]["role"].as_str() != Some("user")
                    {
                        return None;
                    }
                    content_text(&value["message"]["content"]).map(clean_title)
                })
                .filter(|title| !title.is_empty())
        })
        .unwrap_or_else(|| "New session".into());
    let created_at = header["timestamp"]
        .as_str()
        .and_then(|time| DateTime::parse_from_rfc3339(time).ok())
        .and_then(|time| u64::try_from(time.timestamp_millis()).ok())
        .unwrap_or(0);
    Ok(Some(SessionSummary {
        id: candidate.path.to_string_lossy().into_owned(),
        title,
        live: false,
        created_at,
        modified_at: candidate.modified,
    }))
}

fn latest_name(file: &mut File, len: u64) -> std::io::Result<Option<String>> {
    #[derive(Deserialize)]
    struct Info {
        #[serde(rename = "type")]
        kind: String,
        name: Option<String>,
    }
    let mut position = len;
    let mut budget = TAIL_BYTES;
    let mut suffix = Vec::new();
    while position > 0 && budget > 0 {
        let count = position.min(BLOCK_BYTES.min(budget) as u64) as usize;
        position -= count as u64;
        budget -= count;
        file.seek(SeekFrom::Start(position))?;
        let mut bytes = vec![0; count];
        file.read_exact(&mut bytes)?;
        bytes.extend_from_slice(&suffix);
        let first_end = bytes
            .iter()
            .position(|byte| *byte == b'\n')
            .unwrap_or(bytes.len());
        let complete_start = if position == 0 { 0 } else { first_end };
        for line in bytes[complete_start..].rsplit(|byte| *byte == b'\n') {
            if let Ok(info) = serde_json::from_slice::<Info>(line) {
                if info.kind == "session_info" {
                    return Ok(Some(clean_title(info.name.as_deref().unwrap_or(""))));
                }
            }
        }
        suffix = bytes[..first_end].to_vec();
    }
    Ok(None)
}

fn content_text(content: &Value) -> Option<&str> {
    if let Some(text) = content.as_str() {
        return Some(text);
    }
    content.as_array()?.iter().find_map(|part| {
        (part["type"].as_str() == Some("text"))
            .then(|| part["text"].as_str())
            .flatten()
    })
}

pub(crate) fn clean_title(value: &str) -> String {
    let flattened = value.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut chars = flattened.chars();
    let title = chars.by_ref().take(TITLE_CHARS).collect::<String>();
    if chars.next().is_some() {
        format!("{title}…")
    } else {
        title
    }
}

fn path_key(path: &Path) -> String {
    let path = std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf());
    let value = path.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.to_string_lossy().replace('\\', "/");
    let right = right.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

/// One blocking job at a time; the runner remains free to service RPC, input, and frames.
#[derive(Default)]
pub struct SessionLoader {
    cached: Option<(u64, String, SessionIndex)>,
    task: Option<tokio::task::JoinHandle<(SessionIndex, ResumeBatch)>>,
    request: Option<ResumeRequest>,
}

impl SessionLoader {
    pub fn parents(&self) -> Option<&SessionParents> {
        self.cached.as_ref().map(|(_, _, index)| &index.parents)
    }

    pub fn is_idle(&self) -> bool {
        self.task.is_none()
    }

    pub fn start(&mut self, root: PathBuf, request: ResumeRequest) {
        assert!(self.is_idle());
        let cached = self.cached.take().filter(|(generation, workspace, _)| {
            *generation == request.generation && *workspace == request.workspace
        });
        self.request = Some(request.clone());
        self.task = Some(tokio::task::spawn_blocking(move || {
            let mut index = cached
                .map(|(_, _, index)| index)
                .unwrap_or_else(|| SessionIndex::enumerate(&root));
            let batch = index.load(request);
            (index, batch)
        }));
    }

    pub async fn next_batch(&mut self) -> ResumeBatch {
        let Some(task) = self.task.as_mut() else {
            return std::future::pending().await;
        };
        let result = task.await;
        self.task = None;
        let request = self.request.take().expect("active session load request");
        match result {
            Ok((index, batch)) => {
                self.cached = Some((request.generation, request.workspace, index));
                batch
            }
            Err(error) => ResumeBatch {
                next_offset: request.offset,
                request,
                sessions: Vec::new(),
                has_more: false,
                diagnostic: Some(format!("session index worker failed: {error}")),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{io::Write, time::UNIX_EPOCH};

    fn session(path: &Path, cwd: &Path, body: &str) {
        std::fs::write(
            path,
            format!(
                "{}\n{body}",
                serde_json::json!({"type":"session","cwd":cwd})
            ),
        )
        .unwrap();
    }
    fn request(cwd: &Path, offset: usize, limit: usize) -> ResumeRequest {
        ResumeRequest {
            generation: 1,
            workspace: cwd.to_string_lossy().into_owned(),
            offset,
            limit,
        }
    }
    fn summary(path: &Path, cwd: &Path) -> Option<SessionSummary> {
        read_summary(
            &Candidate {
                path: path.into(),
                modified: Some(UNIX_EPOCH),
            },
            cwd,
        )
        .unwrap()
    }

    #[test]
    fn reverse_name_crosses_blocks_and_latest_empty_name_clears() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("one.jsonl");
        let prefix =
            "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"fallback\"}}\n";
        let name = format!(
            "{{\"type\":\"session_info\",\"name\":\"Named 中文\",\"extra\":\"{}\"}}\n",
            "x".repeat(BLOCK_BYTES)
        );
        session(&path, temp.path(), &format!("{prefix}{name}"));
        assert_eq!(summary(&path, temp.path()).unwrap().title, "Named 中文");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{{\"type\":\"session_info\",\"name\":\"\"}}").unwrap();
        assert_eq!(summary(&path, temp.path()).unwrap().title, "fallback");
    }

    #[test]
    fn huge_and_damaged_records_do_not_hide_valid_sessions() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("large.jsonl");
        session(
            &path,
            temp.path(),
            "{\"type\":\"message\",\"message\":{\"role\":\"user\",\"content\":\"first\"}}\n",
        );
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        file.write_all(&vec![b'x'; 9 * 1024 * 1024]).unwrap();
        writeln!(
            file,
            "\n{{\"type\":\"session_info\",\"name\":\"latest\"}}\n{{partial"
        )
        .unwrap();
        assert_eq!(summary(&path, temp.path()).unwrap().title, "latest");
        file.write_all(&vec![b'x'; TAIL_BYTES + 1]).unwrap();
        assert_eq!(summary(&path, temp.path()).unwrap().title, "first");
        assert!(summary(&path, Path::new("other-project")).is_none());
    }

    #[test]
    fn paginates_all_candidates_in_modification_order_with_bounded_errors() {
        let temp = tempfile::tempdir().unwrap();
        for i in 0..503 {
            let path = temp.path().join(format!("{i:04}.jsonl"));
            session(&path, temp.path(), "");
            File::options()
                .write(true)
                .open(path)
                .unwrap()
                .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(i))
                .unwrap();
        }
        let mut index = SessionIndex::enumerate(temp.path());
        let first = index.load(request(temp.path(), 0, 100));
        assert_eq!(first.sessions.len(), 3);
        assert_eq!(first.next_offset, 3);
        assert!(first.sessions[0].id.ends_with("0502.jsonl"));
        assert_eq!(
            first.sessions[0].modified_at,
            Some(UNIX_EPOCH + std::time::Duration::from_secs(502))
        );
        assert!(first.has_more);
        let mut offset = first.next_offset;
        let mut total = first.sessions.len();
        loop {
            let batch = index.load(request(temp.path(), offset, 100));
            assert!(batch.sessions.len() <= 3);
            assert_eq!(batch.next_offset - offset, batch.sessions.len());
            total += batch.sessions.len();
            offset = batch.next_offset;
            if !batch.has_more {
                break;
            }
        }
        assert_eq!(total, 503);
        for i in 0..10 {
            std::fs::write(temp.path().join(format!("bad{i}.jsonl")), "not json").unwrap();
        }
        let batch = SessionIndex::enumerate(temp.path()).load(request(temp.path(), 0, 10));
        assert!(batch.sessions.is_empty());
        assert_eq!(batch.next_offset, 3);
        assert!(batch.has_more);
        assert_eq!(batch.diagnostic.unwrap().matches("; ").count(), 2);
    }

    #[test]
    fn record_count_and_tail_budget_only_affect_title_discovery() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("many.jsonl");
        session(
            &path,
            temp.path(),
            &"{\"type\":\"custom\"}\n".repeat(10_001),
        );
        assert_eq!(summary(&path, temp.path()).unwrap().title, "New session");
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "{{\"type\":\"session_info\",\"name\":\"last name\"}}").unwrap();
        let before = std::fs::read(&path).unwrap();
        assert_eq!(summary(&path, temp.path()).unwrap().title, "last name");
        assert_eq!(std::fs::read(&path).unwrap(), before);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn worker_returns_batches_and_reenumerates_for_a_new_page() {
        let temp = tempfile::tempdir().unwrap();
        session(&temp.path().join("one.jsonl"), temp.path(), "");
        session(&temp.path().join("two.jsonl"), temp.path(), "");
        let mut loader = SessionLoader::default();
        loader.start(temp.path().into(), request(temp.path(), 0, 1));
        assert!(!loader.is_idle());
        tokio::select! {
            biased;
            _ = std::future::ready(()) => {},
            _ = loader.next_batch() => panic!("ready branch has priority"),
        }
        let first = loader.next_batch().await;
        assert!(loader.is_idle());
        assert_eq!(first.sessions.len(), 1);
        assert!(first.has_more);
        loader.start(
            temp.path().into(),
            request(temp.path(), first.next_offset, 1),
        );
        let second = loader.next_batch().await;
        assert!(!second.has_more);
        assert_ne!(first.sessions[0].id, second.sessions[0].id);
        session(&temp.path().join("three.jsonl"), temp.path(), "");
        session(&temp.path().join("four.jsonl"), temp.path(), "");
        let mut reopened = request(temp.path(), 0, 100);
        reopened.generation += 1;
        loader.start(temp.path().into(), reopened.clone());
        let third = loader.next_batch().await;
        assert_eq!(third.sessions.len(), 3);
        assert_eq!(third.next_offset, 3);
        assert!(third.has_more);
        reopened.offset = third.next_offset;
        loader.start(temp.path().into(), reopened);
        let fourth = loader.next_batch().await;
        assert_eq!(fourth.sessions.len(), 1);
        assert!(!fourth.has_more);
        assert!(third
            .sessions
            .iter()
            .all(|row| row.id != fourth.sessions[0].id));
    }

    #[test]
    fn default_project_directory_uses_pi_native_cwd_encoding() {
        if std::env::var_os("PI_CODING_AGENT_SESSION_DIR").is_none() {
            assert!(project_session_root(Path::new(r"G:\work/project"))
                .ends_with("--G--work-project--"));
        }
    }
}
