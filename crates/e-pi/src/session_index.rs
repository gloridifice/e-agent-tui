//! Bounded, read-only indexing of documented native Pi session metadata.

use std::{
    collections::VecDeque,
    fs::File,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use e_tui::agent::SessionSummary;
use serde_json::Value;

const MAX_SESSION_FILES: usize = 500;
const MAX_SESSION_BYTES: u64 = 8 * 1024 * 1024;
const MAX_SESSION_LINES: usize = 10_000;
const TITLE_CHARS: usize = 100;

#[derive(Debug, Clone)]
pub struct SessionIndex {
    pub sessions: Vec<SessionSummary>,
    pub diagnostics: Vec<String>,
}

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

/// Return Pi's native directory for this project. A custom session directory
/// is already an exact directory; the default layout adds encoded cwd.
pub fn project_session_root(cwd: &Path) -> PathBuf {
    if let Some(custom) =
        std::env::var_os("PI_CODING_AGENT_SESSION_DIR").filter(|value| !value.is_empty())
    {
        return PathBuf::from(custom);
    }
    let resolved = cwd.to_string_lossy();
    let trimmed = resolved.trim_start_matches(['/', '\\']);
    let safe = trimmed.replace(['/', '\\', ':'], "-");
    session_root().join(format!("--{safe}--"))
}

pub fn list_current_project(root: &Path, cwd: &Path) -> SessionIndex {
    let mut diagnostics = Vec::new();
    let mut sessions = Vec::new();
    let mut pending = VecDeque::from([root.to_path_buf()]);
    let mut seen = 0usize;
    while let Some(directory) = pending.pop_front() {
        let entries = match std::fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(error) => {
                if directory == root && error.kind() == std::io::ErrorKind::NotFound {
                    break;
                }
                diagnostics.push(format!("cannot read {}: {error}", directory.display()));
                continue;
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push_back(path);
                continue;
            }
            if path
                .extension()
                .is_none_or(|extension| extension != "jsonl")
            {
                continue;
            }
            if seen == MAX_SESSION_FILES {
                diagnostics.push(format!(
                    "Pi session index capped at {MAX_SESSION_FILES} files"
                ));
                pending.clear();
                break;
            }
            seen += 1;
            match read_summary(&path, cwd) {
                Ok(Some(summary)) => sessions.push(summary),
                Ok(None) => {}
                Err(error) => diagnostics.push(format!("{}: {error}", path.display())),
            }
        }
    }
    sessions.sort_by_key(|session| std::cmp::Reverse(session.created_at));
    SessionIndex {
        sessions,
        diagnostics,
    }
}

fn read_summary(path: &Path, cwd: &Path) -> Result<Option<SessionSummary>, String> {
    let metadata = std::fs::metadata(path).map_err(|error| error.to_string())?;
    if metadata.len() > MAX_SESSION_BYTES {
        return Err(format!("session exceeds {MAX_SESSION_BYTES} bytes"));
    }
    let file = File::open(path).map_err(|error| error.to_string())?;
    let mut lines = BufReader::new(file).lines();
    let header = lines
        .next()
        .transpose()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "empty session file".to_owned())?;
    let header: Value = serde_json::from_str(&header).map_err(|error| error.to_string())?;
    if header.get("type").and_then(Value::as_str) != Some("session") {
        return Err("first record is not a Pi session header".into());
    }
    let header_cwd = header
        .get("cwd")
        .and_then(Value::as_str)
        .ok_or_else(|| "session header has no cwd".to_owned())?;
    if !same_path(Path::new(header_cwd), cwd) {
        return Ok(None);
    }

    let mut native_name = None;
    let mut first_user = None;
    for (index, line) in lines.enumerate() {
        if index >= MAX_SESSION_LINES {
            return Err(format!(
                "session metadata exceeds {MAX_SESSION_LINES} lines"
            ));
        }
        let line = line.map_err(|error| error.to_string())?;
        let value: Value = serde_json::from_str(&line).map_err(|error| error.to_string())?;
        match value.get("type").and_then(Value::as_str) {
            Some("session_info") => {
                native_name = value.get("name").and_then(Value::as_str).map(clean_title);
            }
            Some("message") if first_user.is_none() => {
                let message = &value["message"];
                if message.get("role").and_then(Value::as_str) == Some("user") {
                    first_user = message
                        .get("content")
                        .and_then(content_text)
                        .map(clean_title);
                }
            }
            _ => {}
        }
    }
    let title = native_name
        .filter(|name| !name.is_empty())
        .or(first_user.filter(|name| !name.is_empty()))
        .unwrap_or_else(|| "New session".into());
    let created_at = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |duration| duration.as_millis() as u64);
    Ok(Some(SessionSummary {
        id: path.to_string_lossy().into_owned(),
        title,
        live: false,
        created_at,
    }))
}

fn content_text(content: &Value) -> Option<&str> {
    if let Some(text) = content.as_str() {
        return Some(text);
    }
    content.as_array()?.iter().find_map(|part| {
        (part.get("type").and_then(Value::as_str) == Some("text"))
            .then(|| part.get("text").and_then(Value::as_str))
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

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.to_string_lossy().replace('\\', "/");
    let right = right.to_string_lossy().replace('\\', "/");
    if cfg!(windows) {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn default_project_directory_uses_pi_native_cwd_encoding() {
        if std::env::var_os("PI_CODING_AGENT_SESSION_DIR").is_none() {
            let path = project_session_root(Path::new(r"G:\work/project"));
            assert!(path.ends_with("--G--work-project--"));
        }
    }

    #[test]
    fn indexes_matching_sessions_with_native_name_precedence() {
        let temp = tempfile::tempdir().unwrap();
        let cwd = temp.path().join("project");
        std::fs::create_dir_all(&cwd).unwrap();
        let sessions = temp.path().join("sessions/project");
        std::fs::create_dir_all(&sessions).unwrap();
        let path = sessions.join("one.jsonl");
        let mut file = File::create(&path).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"session","version":3,"id":"s1","cwd":cwd})
        )
        .unwrap();
        writeln!(file, "{}", serde_json::json!({"type":"message","id":"1","parentId":null,"message":{"role":"user","content":"first prompt"}})).unwrap();
        writeln!(
            file,
            "{}",
            serde_json::json!({"type":"session_info","id":"2","parentId":"1","name":"Named work"})
        )
        .unwrap();

        let index = list_current_project(&temp.path().join("sessions"), &cwd);
        assert!(index.diagnostics.is_empty(), "{:?}", index.diagnostics);
        assert_eq!(index.sessions.len(), 1);
        assert_eq!(index.sessions[0].title, "Named work");
        assert_eq!(PathBuf::from(&index.sessions[0].id), path);
    }

    #[test]
    fn skips_other_projects_and_reports_bad_files() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("sessions");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("other.jsonl"),
            "{\"type\":\"session\",\"cwd\":\"elsewhere\"}\n",
        )
        .unwrap();
        std::fs::write(root.join("bad.jsonl"), "not json\n").unwrap();
        let index = list_current_project(&root, temp.path());
        assert!(index.sessions.is_empty());
        assert_eq!(index.diagnostics.len(), 1);
    }
}
