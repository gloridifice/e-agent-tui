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

#[cfg(test)]
mod tests {
    use super::*;

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
