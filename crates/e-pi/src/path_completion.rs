use std::path::{Component, Path};

use e_tui::path_completion::{PathCandidate, PathCompletionRequest};

pub fn complete(request: &PathCompletionRequest) -> Vec<PathCandidate> {
    let root = Path::new(&request.cwd);
    let query = Path::new(&request.query);
    if query
        .components()
        .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
    {
        return Vec::new();
    }
    let target = root.join(query);
    let (directory, prefix) = if target.is_dir() {
        (query, "")
    } else if request.query.ends_with('/') {
        return Vec::new();
    } else {
        (
            query.parent().unwrap_or(Path::new("")),
            query.file_name().and_then(|s| s.to_str()).unwrap_or(""),
        )
    };
    let Ok(entries) = root.join(directory).read_dir() else {
        return Vec::new();
    };
    let prefix = prefix.to_lowercase();
    let mut candidates = entries
        .take(4096)
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if !name.to_lowercase().starts_with(&prefix)
                || name.chars().any(|c| c.is_control() || c == '"')
            {
                return None;
            }
            let kind = entry.file_type().ok()?;
            let is_directory = kind.is_dir() || (kind.is_symlink() && entry.path().is_dir());
            let suffix = if is_directory { "/" } else { "" };
            let path = directory.join(&name).to_str()?.replace('\\', "/");
            Some(PathCandidate {
                path: format!("{path}{suffix}"),
                label: format!("{name}{suffix}"),
            })
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|a, b| {
        b.label
            .ends_with('/')
            .cmp(&a.label.ends_with('/'))
            .then_with(|| a.label.cmp(&b.label))
    });
    candidates.truncate(100);
    candidates
}

pub fn validate_links(
    request: &e_tui::link_copy::LinkValidationRequest,
) -> Vec<e_tui::link_copy::PathValidation> {
    use e_tui::link_copy::PathValidation::{Exists, Missing, Rejected};
    let root = request
        .candidates
        .iter()
        .any(|candidate| candidate.relative.is_some())
        .then(|| Path::new(&request.cwd).canonicalize().ok())
        .flatten();
    request
        .candidates
        .iter()
        .map(|candidate| {
            let Some(relative) = &candidate.relative else {
                return Exists;
            };
            let Some(root) = &root else {
                return Rejected;
            };
            let relative = Path::new(relative);
            if relative
                .components()
                .any(|part| !matches!(part, Component::Normal(_) | Component::CurDir))
            {
                return Rejected;
            }
            let mut path = root.join(relative);
            let mut missing = false;
            loop {
                match path.canonicalize() {
                    Ok(resolved) => {
                        return if !resolved.starts_with(root) {
                            Rejected
                        } else if missing {
                            Missing
                        } else {
                            Exists
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        if path
                            .symlink_metadata()
                            .is_ok_and(|metadata| metadata.file_type().is_symlink())
                        {
                            return Rejected;
                        }
                        missing = true;
                        if !path.pop() {
                            return Rejected;
                        }
                    }
                    Err(_) => return Rejected,
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_links_validate_workspace_and_reject_symlink_escape() {
        use e_tui::link_copy::{
            discover, LinkValidationRequest,
            PathValidation::{Exists, Missing, Rejected},
        };
        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("src")).unwrap();
        std::fs::write(root.path().join("src/main"), "").unwrap();
        std::fs::write(root.path().join("README"), "").unwrap();
        let link = root.path().join("escape");
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), &link).unwrap();
        #[cfg(windows)]
        assert!(std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(outside.path())
            .output()
            .unwrap()
            .status
            .success());
        let request = LinkValidationRequest {
            generation: 1,
            cwd: root.path().to_str().unwrap().into(),
            candidates: discover(
                "src/main README ./missing.rs escape/ escape/missing.rs https://example.com",
            ),
        };
        assert_eq!(
            validate_links(&request),
            [Exists, Exists, Missing, Rejected, Rejected, Exists]
        );
    }

    #[test]
    fn hierarchy_completion_uses_the_requested_workspace() {
        let root = tempfile::tempdir().unwrap();
        for directory in ["foo", "bar", "hello", "中文 空格"] {
            std::fs::create_dir(root.path().join(directory)).unwrap();
        }
        for file in ["foo/a.rs", "foo/b.rs", "bar/c.rs", "中文 空格/文件.rs"] {
            std::fs::write(root.path().join(file), "").unwrap();
        }
        let candidates = |query: &str| {
            let buffer = format!("@\"{query}\"");
            let request = PathCompletionRequest::new(
                root.path().to_str().unwrap(),
                &buffer,
                buffer.chars().count(),
            )
            .unwrap();
            complete(&request)
        };
        assert_eq!(
            candidates("")
                .iter()
                .map(|c| c.label.as_str())
                .collect::<Vec<_>>(),
            ["bar/", "foo/", "hello/", "中文 空格/"]
        );
        assert_eq!(candidates("foo"), candidates("foo/"));
        assert_eq!(
            candidates("foo")
                .iter()
                .map(|c| c.path.as_str())
                .collect::<Vec<_>>(),
            ["foo/a.rs", "foo/b.rs"]
        );
        assert_eq!(candidates("foo/a")[0].label, "a.rs");
        assert_eq!(candidates("中文 空格/")[0].path, "中文 空格/文件.rs");
        assert!(candidates("hello/").is_empty());
        assert!(candidates("missing/").is_empty());
        assert!(candidates("../").is_empty());
        assert!(candidates("/absolute").is_empty());
    }
}
