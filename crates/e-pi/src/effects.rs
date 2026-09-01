//! Pi-owned production implementations for frontend runtime effects.

use std::{fs, path::Path};

use e_tui::{PreviewContent, PreviewRequest, ThemeFile};

const MAX_PREVIEW_BYTES: usize = 256 * 1024;
const MAX_PREVIEW_LINES: usize = 2_000;

pub fn load_themes(dir: &Path) -> Vec<ThemeFile> {
    ensure_default_themes(dir);
    let mut themes: Vec<ThemeFile> = e_tui::theme::builtin_theme_sources()
        .into_iter()
        .filter_map(|(_, source)| e_tui::theme::parse_theme(source))
        .collect();
    for file in discover_themes(dir) {
        if let Some(index) = themes.iter().position(|theme| theme.name == file.name) {
            themes[index] = file;
        } else {
            themes.push(file);
        }
    }
    themes.sort_by(|left, right| left.name.cmp(&right.name));
    themes
}

fn ensure_default_themes(dir: &Path) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    for (name, source) in e_tui::theme::builtin_theme_sources() {
        let path = dir.join(format!("{name}.toml"));
        if !path.exists() {
            let _ = fs::write(path, source);
        }
    }
}

fn discover_themes(dir: &Path) -> Vec<ThemeFile> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut themes = entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            (path.extension().and_then(|extension| extension.to_str()) == Some("toml"))
                .then(|| fs::read_to_string(path).ok())
                .flatten()
                .and_then(|source| e_tui::theme::parse_theme(&source))
        })
        .collect::<Vec<_>>();
    themes.sort_by(|left, right| left.name.cmp(&right.name));
    themes
}

pub fn read_clipboard() -> Result<String, String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_text())
        .map_err(|error| error.to_string())
}

pub fn write_clipboard(text: String) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.set_text(text))
        .map_err(|error| error.to_string())
}

pub async fn resolve_preview(request: PreviewRequest) -> Result<PreviewContent, String> {
    let key = request.key.0;
    if let Some(path) = key.strip_prefix("file:") {
        return Ok(PreviewContent::PlainText(read_bounded(path).await?));
    }
    if let Some(spec) = key.strip_prefix("lines:") {
        let (path, start) = spec
            .rsplit_once(':')
            .and_then(|(path, start)| start.parse::<usize>().ok().map(|start| (path, start)))
            .ok_or_else(|| "invalid line Preview key".to_owned())?;
        let lines = read_bounded(path)
            .await?
            .lines()
            .skip(start.saturating_sub(1))
            .take(MAX_PREVIEW_LINES)
            .map(str::to_owned)
            .collect();
        return Ok(PreviewContent::Lines {
            path: path.to_owned(),
            start,
            lines,
        });
    }
    Err("unsupported deferred Preview reference".into())
}

async fn read_bounded(path: &str) -> Result<String, String> {
    let metadata = tokio::fs::metadata(path)
        .await
        .map_err(|error| bounded_error("inspect Preview file", error))?;
    if metadata.len() > MAX_PREVIEW_BYTES as u64 {
        return Err(format!(
            "Preview file exceeds the {} KiB limit",
            MAX_PREVIEW_BYTES / 1024
        ));
    }
    let bytes = tokio::fs::read(path)
        .await
        .map_err(|error| bounded_error("read Preview file", error))?;
    let mut source = String::from_utf8_lossy(&bytes).into_owned();
    if source.lines().count() > MAX_PREVIEW_LINES {
        source = source
            .lines()
            .take(MAX_PREVIEW_LINES)
            .collect::<Vec<_>>()
            .join("\n");
    }
    Ok(source)
}

fn bounded_error(context: &str, error: impl std::fmt::Display) -> String {
    let detail = error.to_string().chars().take(240).collect::<String>();
    format!("{context}: {detail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use e_tui::{PreviewKey, PreviewRequestId, PreviewRevision};

    fn request(key: String) -> PreviewRequest {
        PreviewRequest {
            request_id: PreviewRequestId(1),
            key: PreviewKey(key),
            revision: PreviewRevision(1),
        }
    }

    #[tokio::test]
    async fn line_preview_keeps_the_requested_start_and_bounds_output() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("sample.txt");
        fs::write(&path, "one\ntwo\nthree\n").expect("write fixture");

        let content = resolve_preview(request(format!("lines:{}:2", path.display())))
            .await
            .expect("resolve line preview");

        assert_eq!(
            content,
            PreviewContent::Lines {
                path: path.display().to_string(),
                start: 2,
                lines: vec!["two".into(), "three".into()],
            }
        );
    }

    #[tokio::test]
    async fn oversized_preview_is_rejected_before_reading() {
        let dir = tempfile::tempdir().expect("temporary directory");
        let path = dir.path().join("large.txt");
        fs::write(&path, vec![b'x'; MAX_PREVIEW_BYTES + 1]).expect("write fixture");

        let error = resolve_preview(request(format!("file:{}", path.display())))
            .await
            .expect_err("oversized preview must fail");

        assert!(error.contains("256 KiB limit"), "unexpected error: {error}");
    }
}
