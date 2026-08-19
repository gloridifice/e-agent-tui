//! Bounded deferred Preview resolution outside the frontend state guard.

use e_tui::{PreviewContent, PreviewRequest};

const MAX_PREVIEW_BYTES: usize = 256 * 1024;
const MAX_PREVIEW_LINES: usize = 2_000;

pub async fn resolve(request: PreviewRequest) -> Result<PreviewContent, String> {
    let key = request.key.0;
    if let Some(path) = key.strip_prefix("file:") {
        let source = read_bounded(path).await?;
        return Ok(PreviewContent::PlainText(source));
    }
    if let Some(spec) = key.strip_prefix("lines:") {
        let (path, start) = spec
            .rsplit_once(':')
            .and_then(|(path, start)| start.parse::<usize>().ok().map(|start| (path, start)))
            .ok_or_else(|| "invalid line Preview key".to_owned())?;
        let source = read_bounded(path).await?;
        let lines = source
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
    let detail = error.to_string();
    let detail = detail.chars().take(240).collect::<String>();
    format!("{context}: {detail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use e_tui::{PreviewKey, PreviewRequestId, PreviewRevision};

    #[tokio::test]
    async fn file_and_line_resolution_are_bounded_and_typed() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("dshe-preview-{}-{nonce}.txt", std::process::id(),));
        std::fs::write(&path, "one\ntwo\nthree\n").unwrap();
        let path_text = path.to_string_lossy();
        let file = resolve(PreviewRequest {
            request_id: PreviewRequestId(1),
            key: PreviewKey(format!("file:{path_text}")),
            revision: PreviewRevision(1),
        })
        .await
        .unwrap();
        assert!(matches!(file, PreviewContent::PlainText(text) if text.contains("two")));
        let lines = resolve(PreviewRequest {
            request_id: PreviewRequestId(2),
            key: PreviewKey(format!("lines:{path_text}:2")),
            revision: PreviewRevision(1),
        })
        .await
        .unwrap();
        assert!(matches!(
            lines,
            PreviewContent::Lines { start: 2, lines, .. } if lines.first().is_some_and(|line| line == "two")
        ));
        std::fs::remove_file(path).ok();
    }
}
