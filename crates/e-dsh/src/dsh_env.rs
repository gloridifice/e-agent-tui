//! Shared DSH environment resolution: home directory, the dedicated profile,
//! and the argv used to boot or drive DSH.
//!
//! This is a leaf module so both the launcher and the setup installer depend
//! on it in one direction (no `launcher`/`setup` cycle).

use std::path::PathBuf;
use std::process::Command;

/// The dedicated profile `dshe` uses for its bridge and launcher.
pub const PROFILE_NAME: &str = "dshe";

/// Resolve the DSH home directory from raw environment values.
///
/// An unset, empty, or whitespace-only `DSH_HOME` uses the platform user home
/// joined with `.dsh`; a non-empty value is preserved verbatim.
pub fn resolve_dsh_home(
    dsh_home: Option<&str>,
    user_profile: Option<&str>,
    home: Option<&str>,
) -> PathBuf {
    if let Some(value) = dsh_home {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }
    let user = user_profile
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .or_else(|| home.map(str::trim).filter(|value| !value.is_empty()))
        .unwrap_or(".");
    PathBuf::from(user).join(".dsh")
}

/// Resolve the DSH home from the current process environment.
pub fn current_dsh_home() -> PathBuf {
    resolve_dsh_home(
        std::env::var("DSH_HOME").ok().as_deref(),
        std::env::var("USERPROFILE").ok().as_deref(),
        std::env::var("HOME").ok().as_deref(),
    )
}

pub(crate) fn command_exists(cmd: &str) -> bool {
    #[cfg(windows)]
    let out = Command::new("where").arg(cmd).output();
    #[cfg(not(windows))]
    let out = Command::new("which").arg(cmd).output();
    out.map(|o| o.status.success()).unwrap_or(false)
}

/// The argv prefix that boots or drives DSH: the global `dsh` executable when
/// installed, else `npx -y @deepseek-ai/dsh` (downloads on first run). Returns
/// `None` when neither executable is available on PATH, so callers can emit a
/// specific "install DSH" diagnostic instead of a generic non-zero exit.
pub fn dsh_launcher_argv() -> Option<Vec<String>> {
    if command_exists("dsh") {
        Some(vec!["dsh".into()])
    } else if command_exists("npx") {
        Some(vec!["npx".into(), "-y".into(), "@deepseek-ai/dsh".into()])
    } else {
        None
    }
}

/// The argv that boots the dedicated `dshe` profile, or `None` when DSH is
/// not installed.
pub fn dsh_command() -> Option<Vec<String>> {
    let mut argv = dsh_launcher_argv()?;
    argv.push("--profile".into());
    argv.push(PROFILE_NAME.into());
    Some(argv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_dsh_home_falls_back_and_preserves_custom() {
        let custom = resolve_dsh_home(Some("/custom/dsh"), Some("C:\\Users\\me"), None);
        assert_eq!(custom, PathBuf::from("/custom/dsh"));

        let unset = resolve_dsh_home(None, Some("C:\\Users\\me"), None);
        assert_eq!(unset, PathBuf::from("C:\\Users\\me").join(".dsh"));

        let empty = resolve_dsh_home(Some("   "), Some("C:\\Users\\me"), None);
        assert_eq!(empty, PathBuf::from("C:\\Users\\me").join(".dsh"));

        let whitespace_custom = resolve_dsh_home(Some("\t/dsh\t"), None, Some("/home/u"));
        assert_eq!(whitespace_custom, PathBuf::from("/dsh"));
    }

    #[test]
    fn launcher_argv_uses_dedicated_profile() {
        let Some(command) = dsh_command() else {
            return; // dsh/npx unavailable in this environment.
        };
        assert!(command
            .windows(2)
            .any(|args| args[0] == "--profile" && args[1] == PROFILE_NAME));
    }
}
