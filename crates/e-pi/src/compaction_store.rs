//! Shared user-wide compaction route; no session or project state.
use std::{io::Write, path::Path};

use e_tui::config::CompactionModel;

pub fn load(path: &Path) -> Result<Option<CompactionModel>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.to_string()),
    };
    let value: Option<CompactionModel> = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    if value.as_ref().is_some_and(|value| !value.is_valid()) {
        return Err(format!(
            "Invalid compaction model configuration: {}",
            path.display()
        ));
    }
    Ok(value)
}

pub fn save(path: &Path, value: Option<&CompactionModel>) -> Result<(), String> {
    let parent = path.parent().ok_or("Compaction model path has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let mut file = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    serde_json::to_writer(&mut file, &value).map_err(|e| e.to_string())?;
    file.write_all(b"\n").map_err(|e| e.to_string())?;
    file.as_file().sync_all().map_err(|e| e.to_string())?;
    file.persist(path).map_err(|e| e.to_string())?;
    Ok(())
}
