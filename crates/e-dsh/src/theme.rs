//! Filesystem discovery and installation for `e-tui` theme values.

use std::{fs, path::Path};

pub use e_tui::theme::*;

/// Copy embedded theme TOML files as editable starting points without
/// overwriting user changes.
pub fn ensure_default_themes(dir: &Path) {
    if fs::create_dir_all(dir).is_err() {
        return;
    }
    for (name, source) in builtin_theme_sources() {
        let path = dir.join(format!("{name}.toml"));
        if !path.exists() {
            let _ = fs::write(path, source);
        }
    }
}

/// Discover every legal user theme in the directory, sorted by name.
pub fn discover_themes(dir: &Path) -> Vec<ThemeFile> {
    let mut out = Vec::new();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("toml") {
            continue;
        }
        let Ok(text) = fs::read_to_string(path) else {
            continue;
        };
        if let Some(file) = parse_theme(&text) {
            out.push(file);
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Load embedded themes plus legal user themes. A legal user file with the
/// same name overrides the embedded definition.
pub fn load_themes(dir: &Path) -> Vec<ThemeFile> {
    ensure_default_themes(dir);
    let mut themes: Vec<ThemeFile> = builtin_theme_sources()
        .into_iter()
        .filter_map(|(_, source)| parse_theme(source))
        .collect();
    for file in discover_themes(dir) {
        if let Some(index) = themes.iter().position(|theme| theme.name == file.name) {
            themes[index] = file;
        } else {
            themes.push(file);
        }
    }
    themes.sort_by(|a, b| a.name.cmp(&b.name));
    themes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_sources_are_copied_verbatim_without_overwrite() {
        let dir = std::env::temp_dir().join(format!(
            "dshe-theme-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        ensure_default_themes(&dir);
        let ferra = builtin_theme_sources()
            .into_iter()
            .find_map(|(name, source)| (name == "ferra").then_some(source))
            .unwrap();
        assert_eq!(fs::read_to_string(dir.join("ferra.toml")).unwrap(), ferra);
        fs::write(dir.join("ferra.toml"), "user edit").unwrap();
        ensure_default_themes(&dir);
        assert_eq!(
            fs::read_to_string(dir.join("ferra.toml")).unwrap(),
            "user edit"
        );
        let _ = fs::remove_dir_all(dir);
    }
}
