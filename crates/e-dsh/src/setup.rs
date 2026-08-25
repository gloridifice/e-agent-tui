//! Self-contained `dshe setup` and the startup readiness gate.
//!
//! The bridge runtime (package manifest, canonical protocol contract, and
//! production `bridge/src/*.js` modules) is embedded in this executable at
//! build time. `dshe setup` materializes that runtime into the dedicated
//! `dshe` DSH profile, registers the bridge idempotently, installs its
//! dependencies through `dsh plugin`, and only then records a successful
//! setup. Normal startup refuses to acquire DSH or touch the terminal until
//! that record is present and current.
//!
//! All user-facing failures produced here are English and actionable: they
//! name the failed condition and the exact next command or repair step.

use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::dsh_env::PROFILE_NAME;

include!(concat!(env!("OUT_DIR"), "/embedded_bridge.rs"));

/// The embedded bridge runtime, ordered deterministically by relative path.
pub fn embedded_files() -> &'static [EmbeddedFile] {
    BRIDGE_FILES
}

/// Content identity of the embedded bridge runtime for this executable.
pub fn bridge_digest() -> &'static str {
    BRIDGE_DIGEST
}

const BRIDGE_PACKAGE: &str = "dsh-tui-bridge";
const SETUP_RECORD_FILE: &str = ".dshe-setup.json";
const SETUP_SCHEMA_VERSION: u32 = 1;
/// The platform runtime pinned for a freshly created dedicated profile,
/// matching the deployed compatibility target. Must stay in sync with the
/// same version in `tools/mount-bridge.ps1` (development-only mount script).
const DSH_WIN32_VERSION: &str = "0.13.0";

const PATCH_HEADER: &str =
    "# Your patch layer for this dsh profile, applied after every bundle layer:\n\
# a top-level YAML array of loader patch entries.\n";
const PATCH_INSERT_BLOCK: &str = "- insert:\n    - id: tui-bridge\n      name: dsh-tui-bridge\n";

/// The dedicated profile directory below a resolved DSH home.
pub fn profile_dir(home: &Path) -> PathBuf {
    home.join("profiles").join(PROFILE_NAME)
}

/// Legacy dedicated-profile location used by pre-rename `dshe` builds.
fn legacy_profile_dir(home: &Path) -> PathBuf {
    home.join("profiles").join("dshe")
}

fn packages_dir(profile: &Path) -> PathBuf {
    profile.join("packages")
}

fn bridge_dir(profile: &Path) -> PathBuf {
    packages_dir(profile).join(BRIDGE_PACKAGE)
}

fn setup_record_path(profile: &Path) -> PathBuf {
    profile.join(SETUP_RECORD_FILE)
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// A setup or readiness failure whose message is a complete English
/// instruction: what failed, and what to do next.
#[derive(Debug)]
pub struct SetupError {
    message: String,
}

impl SetupError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for SetupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for SetupError {}

fn read_error(path: &Path, error: std::io::Error) -> SetupError {
    SetupError::new(format!(
        "Cannot read `{}`: {error}. Check that the file exists and is accessible, then run `dshe setup` again.",
        path.display()
    ))
}

fn write_error(path: &Path, error: std::io::Error) -> SetupError {
    SetupError::new(format!(
        "Cannot write `{}`: {error}. Check the directory permissions and available disk space, then run `dshe setup` again.",
        path.display()
    ))
}

fn parse_error(path: &Path, detail: impl AsRef<str>) -> SetupError {
    SetupError::new(format!(
        "Cannot update `{}` because it is not in the expected format: {}. Repair or restore that file, then run `dshe setup` again.",
        path.display(),
        detail.as_ref()
    ))
}

fn validation_error(profile: &Path, detail: impl AsRef<str>) -> SetupError {
    SetupError::new(format!(
        "DSH bridge installation is incomplete at {}: {}. Run `dshe setup` again to repair it, then restart any running DSH service before running `dshe`.",
        profile.display(),
        detail.as_ref()
    ))
}

// ---------------------------------------------------------------------------
// Setup record
// ---------------------------------------------------------------------------

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
struct SetupRecord {
    #[serde(rename = "schemaVersion")]
    schema_version: u32,
    profile: String,
    #[serde(rename = "bridgeDigest")]
    bridge_digest: String,
    #[serde(rename = "wireProtocol")]
    wire_protocol: u64,
}

fn read_setup_record(path: &Path) -> Option<SetupRecord> {
    let raw = fs::read_to_string(path).ok()?;
    serde_json::from_str(raw.trim_start_matches('\u{feff}').trim()).ok()
}

/// Write a file atomically through a sibling temporary file, replacing any
/// existing content only after the new bytes are fully on disk.
fn write_text_atomic(path: &Path, text: &str) -> Result<(), SetupError> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent).map_err(|error| write_error(path, error))?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "file".to_string());
    let tmp = parent.join(format!(".{name}.tmp-{}", std::process::id()));
    fs::write(&tmp, text.as_bytes()).map_err(|error| write_error(path, error))?;
    fs::rename(&tmp, path).map_err(|error| {
        let _ = fs::remove_file(&tmp);
        write_error(path, error)
    })
}

fn write_setup_record(profile: &Path) -> Result<(), SetupError> {
    let record = SetupRecord {
        schema_version: SETUP_SCHEMA_VERSION,
        profile: PROFILE_NAME.to_string(),
        bridge_digest: BRIDGE_DIGEST.to_string(),
        wire_protocol: crate::protocol::WIRE_PROTOCOL_VERSION,
    };
    let path = setup_record_path(profile);
    let text = serde_json::to_string_pretty(&record)
        .map_err(|error| parse_error(&path, format!("cannot serialize setup record: {error}")))?;
    write_text_atomic(&path, &format!("{text}\n"))
}

// ---------------------------------------------------------------------------
// Readiness classification
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupStatus {
    /// A current, structurally complete setup exists.
    Ready,
    /// No successful setup record exists.
    Missing,
    /// The record belongs to a different embedded bridge or schema.
    Outdated,
    /// The record matches this executable but required structure is gone.
    Damaged,
}

/// Classify the current setup state below a resolved DSH home.
pub fn check_setup_status(home: &Path) -> SetupStatus {
    let profile = profile_dir(home);
    let record_path = setup_record_path(&profile);
    let Some(record) = read_setup_record(&record_path) else {
        // Distinguish "never set up" from "record present but unreadable": a
        // corrupt record is structural damage, not a fresh install.
        return if record_path.exists() {
            SetupStatus::Damaged
        } else {
            SetupStatus::Missing
        };
    };
    if record.schema_version != SETUP_SCHEMA_VERSION
        || record.profile != PROFILE_NAME
        || record.bridge_digest != BRIDGE_DIGEST
    {
        return SetupStatus::Outdated;
    }
    if structure_valid(&profile) {
        SetupStatus::Ready
    } else {
        SetupStatus::Damaged
    }
}

/// Fail with an actionable message unless setup is ready.
pub fn require_ready(home: &Path) -> Result<(), SetupError> {
    match check_setup_status(home) {
        SetupStatus::Ready => Ok(()),
        SetupStatus::Missing => Err(SetupError::new(
            "DSH bridge setup is missing. Run `dshe setup`, then run `dshe` again.",
        )),
        SetupStatus::Outdated => Err(SetupError::new(
            "The installed DSH bridge does not match this `dshe` build. Run `dshe setup` to update it, restart any running DSH service, then run `dshe` again.",
        )),
        SetupStatus::Damaged => Err(SetupError::new(format!(
            "DSH bridge setup is incomplete at {}. Run `dshe setup` to repair it, restart any running DSH service, then run `dshe` again.",
            profile_dir(home).display()
        ))),
    }
}

/// The structural checks shared by setup validation and startup readiness.
/// They are cheap filesystem/registration checks and never start DSH or Node.
fn structure_valid(profile: &Path) -> bool {
    if !bridge_dir(profile).join("package.json").is_file() {
        return false;
    }
    if !bridge_dir(profile).join("protocol-contract.json").is_file() {
        return false;
    }
    // Profile manifest must register the workspace bridge dependency.
    if let Ok(raw) = fs::read_to_string(profile.join("package.json")) {
        let Ok(value) =
            serde_json::from_str::<serde_json::Value>(raw.trim_start_matches('\u{feff}').trim())
        else {
            return false;
        };
        if value["dependencies"][BRIDGE_PACKAGE].as_str() != Some("workspace:*") {
            return false;
        }
    } else {
        return false;
    }
    // Workspace must list the packages/* glob.
    let workspace_ok = fs::read_to_string(profile.join("pnpm-workspace.yaml"))
        .map(|raw| raw.lines().any(|line| line.trim() == "- packages/*"))
        .unwrap_or(false);
    if !workspace_ok {
        return false;
    }
    // Patch layer must register the bridge insert.
    let patch_ok = fs::read_to_string(profile.join("cordis.patch.yml"))
        .map(|raw| raw.contains("tui-bridge"))
        .unwrap_or(false);
    if !patch_ok {
        return false;
    }
    // pnpm must have materialized the workspace package as a link or a real
    // directory (not an arbitrary regular file).
    match fs::symlink_metadata(profile.join("node_modules").join(BRIDGE_PACKAGE)) {
        Ok(metadata) => {
            let file_type = metadata.file_type();
            file_type.is_symlink() || file_type.is_dir()
        }
        Err(_) => false,
    }
}

// ---------------------------------------------------------------------------
// Profile provisioning
// ---------------------------------------------------------------------------

/// Move the pre-rename dedicated profile into the current profile location
/// only when that location does not exist. A rename preserves arbitrary valid
/// user dependencies, patches, packages, and lockfiles without trying to
/// merge two independent profile trees.
fn migrate_legacy_profile(home: &Path) -> Result<bool, SetupError> {
    let profile = profile_dir(home);
    if profile.exists() {
        return Ok(false);
    }
    let legacy = legacy_profile_dir(home);
    if !legacy.exists() {
        return Ok(false);
    }
    if !legacy.is_dir() {
        return Err(SetupError::new(format!(
            "Cannot migrate the legacy DSH profile at {} because it is not a directory. Repair or remove it, then run `dshe setup` again.",
            legacy.display()
        )));
    }
    fs::rename(&legacy, &profile).map_err(|error| {
        SetupError::new(format!(
            "Cannot migrate the legacy DSH profile from {} to {}: {error}. Stop any DSH service using the profile, check filesystem permissions, then run `dshe setup` again.",
            legacy.display(),
            profile.display()
        ))
    })?;
    Ok(true)
}

fn ensure_profile_files(profile: &Path) -> Result<(), SetupError> {
    fs::create_dir_all(profile).map_err(|error| write_error(profile, error))?;
    ensure_package_json(profile)?;
    ensure_workspace_yaml(profile)?;
    ensure_cordis_yml(profile)?;
    ensure_cordis_patch(profile)
}

fn ensure_package_json(profile: &Path) -> Result<(), SetupError> {
    let path = profile.join("package.json");
    if !path.exists() {
        let fresh = serde_json::json!({
            "name": format!("dsh-profile-{PROFILE_NAME}"),
            "private": true,
            "dependencies": {
                "dsh-win32": DSH_WIN32_VERSION,
                BRIDGE_PACKAGE: "workspace:*"
            },
            "dsh": {
                "profile": {
                    "bundles": ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app", "dsh-win32"]
                }
            }
        });
        let text = serde_json::to_string_pretty(&fresh)
            .map_err(|error| parse_error(&path, format!("cannot serialize manifest: {error}")))?;
        return write_text_atomic(&path, &format!("{text}\n"));
    }

    let raw = fs::read_to_string(&path).map_err(|error| read_error(&path, error))?;
    let mut value: serde_json::Value =
        serde_json::from_str(raw.trim_start_matches('\u{feff}').trim())
            .map_err(|error| parse_error(&path, format!("invalid JSON: {error}")))?;
    let Some(object) = value.as_object_mut() else {
        return Err(parse_error(&path, "expected a JSON object"));
    };
    let dependencies = object
        .entry("dependencies")
        .or_insert_with(|| serde_json::json!({}));
    let Some(deps) = dependencies.as_object_mut() else {
        return Err(parse_error(
            &path,
            "the `dependencies` field must be an object",
        ));
    };
    // Already registered correctly: leave the user's manifest untouched.
    if deps.get(BRIDGE_PACKAGE).and_then(serde_json::Value::as_str) == Some("workspace:*") {
        return Ok(());
    }
    deps.insert(BRIDGE_PACKAGE.to_string(), serde_json::json!("workspace:*"));
    let text = serde_json::to_string_pretty(&value)
        .map_err(|error| parse_error(&path, format!("cannot reserialize manifest: {error}")))?;
    write_text_atomic(&path, &format!("{text}\n"))
}

fn ensure_workspace_yaml(profile: &Path) -> Result<(), SetupError> {
    let path = profile.join("pnpm-workspace.yaml");
    if !path.exists() {
        return write_text_atomic(&path, "packages:\n  - .\n  - packages/*\n");
    }
    let raw = fs::read_to_string(&path).map_err(|error| read_error(&path, error))?;
    if raw.lines().any(|line| line.trim() == "- packages/*") {
        return Ok(());
    }
    // Insert `- packages/*` right after the first package list item under the
    // `packages:` key, preserving every other line.
    let mut out = String::new();
    let mut inserted = false;
    let mut in_packages = false;
    for line in raw.lines() {
        let trimmed = line.trim();
        let indent = &line[..line.len() - line.trim_start().len()];
        out.push_str(line);
        out.push('\n');
        if trimmed == "packages:" {
            in_packages = true;
        } else if in_packages && trimmed.starts_with("- ") {
            if !inserted {
                out.push_str(&format!("{indent}- packages/*\n"));
                inserted = true;
            }
            in_packages = false;
        } else if in_packages && !trimmed.is_empty() && !trimmed.starts_with('-') {
            in_packages = false;
        }
    }
    if !inserted {
        return Err(parse_error(
            &path,
            "could not locate the `packages:` list to add the `- packages/*` entry",
        ));
    }
    write_text_atomic(&path, &out)
}

fn ensure_cordis_yml(profile: &Path) -> Result<(), SetupError> {
    let path = profile.join("cordis.yml");
    if path.exists() {
        return Ok(());
    }
    write_text_atomic(&path, "[]\n")
}

fn ensure_cordis_patch(profile: &Path) -> Result<(), SetupError> {
    let path = profile.join("cordis.patch.yml");
    if !path.exists() {
        return write_text_atomic(&path, &format!("{PATCH_HEADER}{PATCH_INSERT_BLOCK}"));
    }
    let raw = fs::read_to_string(&path).map_err(|error| read_error(&path, error))?;
    if raw.contains("tui-bridge") {
        return Ok(());
    }
    let updated = if raw.lines().any(|line| line.trim() == "[]") {
        let mut replaced = false;
        let mut out = String::new();
        for line in raw.lines() {
            if !replaced && line.trim() == "[]" {
                out.push_str(PATCH_INSERT_BLOCK);
                replaced = true;
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        if !replaced {
            return Err(parse_error(
                &path,
                "could not replace the empty `[]` patch array",
            ));
        }
        out
    } else {
        let mut out = raw.clone();
        if !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(PATCH_INSERT_BLOCK);
        out
    };
    write_text_atomic(&path, &updated)
}

/// Best-effort recursive removal of a file, directory, or symlink/junction.
fn remove_tree(path: &Path) {
    if path.is_dir() {
        let _ = fs::remove_dir_all(path);
    } else {
        let _ = fs::remove_file(path);
    }
}

/// A relative path is only safe to extract when it is a forward-slash path
/// with no parent traversal. Generated paths satisfy this by construction;
/// this guard makes the invariant explicit.
fn is_safe_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.starts_with('\\')
        && !path.contains('\\')
        && !path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..")
}

/// Materialize the embedded bridge into `packages/dsh-tui-bridge`, staging to
/// a sibling directory first so a failed extraction never leaves a partially
/// written package in place.
fn install_embedded_bridge(profile: &Path) -> Result<(), SetupError> {
    let packages = packages_dir(profile);
    fs::create_dir_all(&packages).map_err(|error| write_error(&packages, error))?;
    let target = bridge_dir(profile);
    let staging = packages.join(".dsh-tui-bridge.staging");

    remove_tree(&staging);
    fs::create_dir(&staging).map_err(|error| write_error(&staging, error))?;

    for file in embedded_files() {
        if !is_safe_relative_path(file.path) {
            return Err(SetupError::new(format!(
                "Refusing to extract the embedded bridge: `{}` is not a safe relative path. Rebuild `dshe` from an unmodified source tree and run `dshe setup` again.",
                file.path
            )));
        }
        let dest = staging.join(file.path);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|error| write_error(parent, error))?;
        }
        fs::write(&dest, file.bytes).map_err(|error| write_error(&dest, error))?;
    }

    if target.exists() {
        // Fixed backup name (no PID): a prior crashed run's leftover is removed
        // here before the swap, so repeated updates cannot accumulate `.old`
        // directories.
        let backup = packages.join(".dsh-tui-bridge.old");
        remove_tree(&backup);
        fs::rename(&target, &backup).map_err(|error| write_error(&target, error))?;
        match fs::rename(&staging, &target) {
            Ok(()) => {
                remove_tree(&backup);
                Ok(())
            }
            Err(error) => {
                let _ = fs::rename(&backup, &target);
                Err(write_error(&target, error))
            }
        }
    } else {
        fs::rename(&staging, &target).map_err(|error| write_error(&target, error))
    }
}

// ---------------------------------------------------------------------------
// Setup prerequisites and DSH plugin installation
// ---------------------------------------------------------------------------

fn pnpm_error(detail: &str) -> SetupError {
    SetupError::new(format!(
        "pnpm {detail}. Install or repair pnpm with `npm install --global pnpm`, verify it with `pnpm --version`, then run `dshe setup` again."
    ))
}

/// Verify the pnpm executable before setup mutates the dedicated profile.
fn require_pnpm() -> Result<String, SetupError> {
    let available = crate::dsh_env::command_exists("pnpm");
    require_pnpm_with(available, || {
        #[cfg(windows)]
        let output = Command::new("cmd")
            .arg("/D")
            .arg("/C")
            .arg("pnpm")
            .arg("--version")
            .output()?;
        #[cfg(not(windows))]
        let output = Command::new("pnpm").arg("--version").output()?;
        Ok((output.status.code(), output.stdout))
    })
}

/// Prerequisite check with an injected process seam for deterministic tests.
fn require_pnpm_with<F>(available: bool, run_version: F) -> Result<String, SetupError>
where
    F: FnOnce() -> std::io::Result<(Option<i32>, Vec<u8>)>,
{
    if !available {
        return Err(pnpm_error(
            "is required to set up the DSH bridge, but it was not found on PATH",
        ));
    }
    let (exit, stdout) = run_version()
        .map_err(|_| pnpm_error("was found on PATH but could not be executed successfully"))?;
    if exit != Some(0) {
        return Err(pnpm_error(
            "was found on PATH but could not be executed successfully",
        ));
    }
    let version = String::from_utf8_lossy(&stdout).trim().to_string();
    if version.is_empty() {
        return Err(pnpm_error("was found on PATH but did not report a version"));
    }
    Ok(version)
}

/// The argv for `dsh plugin --profile e install`, using the global `dsh`
/// when available and the `npx` fallback otherwise. `None` when neither
/// executable is on PATH.
fn plugin_install_argv() -> Option<Vec<String>> {
    let mut argv = crate::dsh_env::dsh_launcher_argv()?;
    argv.extend([
        "plugin".to_string(),
        "--profile".to_string(),
        PROFILE_NAME.to_string(),
        "install".to_string(),
    ]);
    Some(argv)
}

fn missing_prerequisite() -> SetupError {
    SetupError::new(
        "DSH is required to install the bridge, but neither `dsh` nor `npx` was found on PATH. Install DSH with `npm install --global @deepseek-ai/dsh`, then run `dshe setup` again.",
    )
}

/// Run the DSH plugin installer with inherited stdio and the resolved home
/// propagated, returning an actionable error for missing tools, spawn
/// failures, and non-zero exits.
fn run_plugin_install(home: &Path) -> Result<(), SetupError> {
    let Some(argv) = plugin_install_argv() else {
        return Err(missing_prerequisite());
    };
    println!("      Running: {}", argv.join(" "));
    let _ = std::io::stdout().flush();
    run_plugin_install_with(home, &argv, |argv, home| spawn_inherited(argv, home))
}

/// Installer execution with an injected spawn seam for scripted tests.
fn run_plugin_install_with<F>(_home: &Path, argv: &[String], spawn: F) -> Result<(), SetupError>
where
    F: FnOnce(&[String], &Path) -> std::io::Result<Option<i32>>,
{
    let command_display = argv.join(" ");
    let exit = match spawn(argv, _home) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(missing_prerequisite());
        }
        Err(error) => {
            return Err(SetupError::new(format!(
                "Failed to start the DSH installer `{command_display}`: {error}. Ensure DSH is installed and available, then run `dshe setup` again."
            )));
        }
        Ok(code) => code,
    };
    match exit {
        Some(0) => Ok(()),
        Some(code) => Err(SetupError::new(format!(
            "DSH bridge installation failed: `{command_display}` exited with exit code {code}. Review the package-manager output above, fix the reported issue, then run `dshe setup` again."
        ))),
        None => Err(SetupError::new(format!(
            "DSH bridge installation failed: `{command_display}` was terminated by a signal. Fix the reported issue, then run `dshe setup` again."
        ))),
    }
}

fn spawn_inherited(argv: &[String], home: &Path) -> std::io::Result<Option<i32>> {
    #[cfg(windows)]
    let status = {
        // `dsh`/`npx` are `.cmd` shims on Windows and need `cmd /C`.
        Command::new("cmd")
            .arg("/D")
            .arg("/C")
            .args(argv)
            .env("DSH_HOME", home)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit())
            .status()?
    };
    #[cfg(not(windows))]
    let status = {
        let mut command = Command::new(&argv[0]);
        command
            .args(&argv[1..])
            .env("DSH_HOME", home)
            .stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        command.status()?
    };
    Ok(status.code())
}

// ---------------------------------------------------------------------------
// Validation and the top-level setup operation
// ---------------------------------------------------------------------------

fn validate_installed(profile: &Path) -> Result<(), SetupError> {
    if !bridge_dir(profile).join("package.json").is_file() {
        return Err(validation_error(
            profile,
            "the bridge package manifest is missing",
        ));
    }
    if !bridge_dir(profile).join("protocol-contract.json").is_file() {
        return Err(validation_error(
            profile,
            "the bridge protocol contract is missing",
        ));
    }
    if !structure_valid(profile) {
        return Err(validation_error(
            profile,
            "a required profile registration or installed package is missing",
        ));
    }
    Ok(())
}

/// Run setup with an injected installer for testability.
pub fn run_setup_with<F>(home: &Path, install: F) -> Result<(), SetupError>
where
    F: FnOnce(&Path) -> Result<(), SetupError>,
{
    let migrated = migrate_legacy_profile(home)?;
    let profile = profile_dir(home);
    if migrated {
        println!("[2/6] Migrated the legacy `dshe` profile to `{PROFILE_NAME}`...");
    } else {
        println!("[2/6] Preparing the `{PROFILE_NAME}` profile...");
    }
    ensure_profile_files(&profile)?;
    println!("[3/6] Installing the embedded bridge...");
    install_embedded_bridge(&profile)?;
    println!("[4/6] Installing plugin dependencies...");
    let _ = std::io::stdout().flush();
    install(home)?;
    println!("[5/6] Validating the installation...");
    validate_installed(&profile)?;
    println!("[6/6] Recording the completed setup...");
    write_setup_record(&profile)?;
    println!();
    println!("Setup completed successfully.");
    println!();
    println!("You can now run:");
    println!("  dshe");
    println!();
    println!(
        "If a DSH service is already running, restart it first so the updated bridge is loaded."
    );
    Ok(())
}

/// Run `dshe setup` against the current environment.
pub fn run_setup(home: &Path) -> Result<(), SetupError> {
    let profile = profile_dir(home);
    println!("Setting up the DSH bridge...");
    println!();
    println!("DSH home: {}", home.display());
    println!("Profile:  {}", profile.display());
    println!();
    println!("[1/6] Checking prerequisites...");
    let pnpm_version = require_pnpm()?;
    println!("      pnpm {pnpm_version}");
    println!();
    run_setup_with(home, run_plugin_install)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_home(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("dshe-setup-{name}-{}", std::process::id()));
        remove_tree(&dir);
        dir
    }

    fn write_manifest(profile: &Path, json: &str) {
        let path = profile.join("package.json");
        fs::create_dir_all(profile).unwrap();
        fs::write(path, json).unwrap();
    }

    #[test]
    fn pnpm_preflight_reports_version_and_actionable_failures() {
        let version = require_pnpm_with(true, || Ok((Some(0), b"10.6.2\n".to_vec()))).unwrap();
        assert_eq!(version, "10.6.2");

        let missing = require_pnpm_with(false, || Ok((Some(0), Vec::new()))).unwrap_err();
        assert!(missing.to_string().contains("not found on PATH"));
        assert!(missing.to_string().contains("npm install --global pnpm"));

        let broken = require_pnpm_with(true, || Ok((Some(1), Vec::new()))).unwrap_err();
        assert!(broken.to_string().contains("could not be executed"));
        assert!(broken.to_string().contains("pnpm --version"));
    }

    #[test]
    fn embedded_bundle_is_normalized_and_unique() {
        let files = embedded_files();
        assert!(!files.is_empty());
        let mut seen = std::collections::HashSet::new();
        for file in files {
            assert!(
                is_safe_relative_path(file.path),
                "unsafe path: {}",
                file.path
            );
            assert!(!file.bytes.is_empty(), "empty bytes: {}", file.path);
            assert!(seen.insert(file.path), "duplicate path: {}", file.path);
        }
        let paths: Vec<_> = files.iter().map(|file| file.path).collect();
        assert!(paths.contains(&"package.json"));
        assert!(paths.contains(&"protocol-contract.json"));
        assert!(paths
            .iter()
            .any(|path| path.starts_with("src/") && path.ends_with(".js")));
    }

    #[test]
    fn bridge_digest_is_hex_content_identity() {
        let digest = bridge_digest();
        assert_eq!(digest.len(), 64);
        assert!(digest.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn fresh_profile_creates_expected_skeleton() {
        let home = temp_home("fresh-skeleton");
        let profile = profile_dir(&home);
        ensure_profile_files(&profile).unwrap();

        let manifest = fs::read_to_string(profile.join("package.json")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(value["name"], "dsh-profile-e");
        assert_eq!(value["dependencies"]["dsh-tui-bridge"], "workspace:*");
        assert_eq!(value["dependencies"]["dsh-win32"], DSH_WIN32_VERSION);
        assert_eq!(
            value["dsh"]["profile"]["bundles"][0],
            "@deepseek-ai/dsh-base"
        );

        let workspace = fs::read_to_string(profile.join("pnpm-workspace.yaml")).unwrap();
        assert!(workspace.contains("- packages/*"));

        let patch = fs::read_to_string(profile.join("cordis.patch.yml")).unwrap();
        assert!(patch.contains("tui-bridge"));

        assert_eq!(
            fs::read_to_string(profile.join("cordis.yml")).unwrap(),
            "[]\n"
        );
        remove_tree(&home);
    }

    #[test]
    fn legacy_profile_migrates_and_preserves_unrelated_configuration() {
        let home = temp_home("migrate-legacy");
        let legacy = legacy_profile_dir(&home);
        fs::create_dir_all(&legacy).unwrap();
        fs::write(
            legacy.join("package.json"),
            r#"{"name":"dsh-profile-dshe","dependencies":{"extra-plugin":"1.0.0"}}"#,
        )
        .unwrap();
        fs::write(
            legacy.join("cordis.patch.yml"),
            "- insert:\n    - id: extra\n      name: extra-plugin\n",
        )
        .unwrap();

        let profile = profile_dir(&home);
        run_setup_with(&home, |_home| {
            fs::create_dir_all(profile.join("node_modules").join(BRIDGE_PACKAGE)).unwrap();
            Ok(())
        })
        .unwrap();

        assert!(!legacy.exists());
        let manifest = fs::read_to_string(profile.join("package.json")).unwrap();
        let value: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(value["dependencies"]["extra-plugin"], "1.0.0");
        assert_eq!(value["dependencies"][BRIDGE_PACKAGE], "workspace:*");
        assert!(fs::read_to_string(profile.join("cordis.patch.yml"))
            .unwrap()
            .contains("extra-plugin"));
        assert_eq!(
            read_setup_record(&setup_record_path(&profile))
                .unwrap()
                .profile,
            PROFILE_NAME
        );
        remove_tree(&home);
    }

    #[test]
    fn legacy_profile_is_untouched_when_current_profile_exists() {
        let home = temp_home("keep-legacy");
        let legacy = legacy_profile_dir(&home);
        fs::create_dir_all(&legacy).unwrap();
        fs::write(legacy.join("keep.txt"), "legacy").unwrap();
        let profile = profile_dir(&home);
        fs::create_dir_all(&profile).unwrap();

        run_setup_with(&home, |_home| {
            fs::create_dir_all(profile.join("node_modules").join(BRIDGE_PACKAGE)).unwrap();
            Ok(())
        })
        .unwrap();

        assert_eq!(
            fs::read_to_string(legacy.join("keep.txt")).unwrap(),
            "legacy"
        );
        assert!(profile.join("package.json").is_file());
        remove_tree(&home);
    }

    #[test]
    fn invalid_legacy_profile_path_has_actionable_error() {
        let home = temp_home("invalid-legacy");
        fs::create_dir_all(home.join("profiles")).unwrap();
        fs::write(legacy_profile_dir(&home), "not a directory").unwrap();

        let error = migrate_legacy_profile(&home).unwrap_err().to_string();
        assert!(error.contains("not a directory"), "{error}");
        assert!(error.contains("dshe setup"), "{error}");
        remove_tree(&home);
    }

    #[test]
    fn existing_manifest_preserves_unrelated_fields() {
        let home = temp_home("preserve-manifest");
        let profile = profile_dir(&home);
        write_manifest(
            &profile,
            r#"{"name":"dsh-profile-e","private":true,"dependencies":{"dsh-win32":"0.13.0","extra-plugin":"1.0.0"},"dsh":{"profile":{"bundles":["@deepseek-ai/dsh-base","@deepseek-ai/dsh-web-app","dsh-win32"]}}}"#,
        );
        ensure_package_json(&profile).unwrap();

        let value: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(profile.join("package.json")).unwrap())
                .unwrap();
        assert_eq!(value["dependencies"]["dsh-tui-bridge"], "workspace:*");
        assert_eq!(value["dependencies"]["extra-plugin"], "1.0.0");
        assert_eq!(value["dependencies"]["dsh-win32"], "0.13.0");
        assert_eq!(value["name"], "dsh-profile-e");
        remove_tree(&home);
    }

    #[test]
    fn workspace_insert_appends_packages_glob_once() {
        for (name, before, keep_extra) in [
            (
                "single-entry",
                "packages:\n  - .\n",
                false,
            ),
            (
                "existing-glob",
                "packages:\n  - .\n  - packages/*\nminimumReleaseAgeExclude:\n  - dsh-win32@0.13.0\n",
                true,
            ),
        ] {
            let home = temp_home(&format!("workspace-{name}"));
            let profile = profile_dir(&home);
            fs::create_dir_all(&profile).unwrap();
            fs::write(profile.join("pnpm-workspace.yaml"), before).unwrap();
            ensure_workspace_yaml(&profile).unwrap();
            let raw = fs::read_to_string(profile.join("pnpm-workspace.yaml")).unwrap();
            assert_eq!(raw.matches("- packages/*").count(), 1);
            if keep_extra {
                assert!(raw.contains("minimumReleaseAgeExclude"));
            }
            remove_tree(&home);
        }
    }

    #[test]
    fn patch_handles_empty_array_and_existing_entries() {
        let home = temp_home("patch-cases");
        let profile = profile_dir(&home);
        fs::create_dir_all(&profile).unwrap();

        // Existing empty array (DSH init format).
        fs::write(profile.join("cordis.patch.yml"), "# comment header\n[]\n").unwrap();
        ensure_cordis_patch(&profile).unwrap();
        let raw = fs::read_to_string(profile.join("cordis.patch.yml")).unwrap();
        assert!(raw.contains("tui-bridge"));
        assert!(raw.contains("# comment header"));

        // Already registered is a no-op.
        let before = raw.clone();
        ensure_cordis_patch(&profile).unwrap();
        assert_eq!(
            fs::read_to_string(profile.join("cordis.patch.yml")).unwrap(),
            before
        );

        // Existing unrelated entry gets the bridge appended once.
        fs::write(
            profile.join("cordis.patch.yml"),
            "- insert:\n    - id: other\n      name: other-plugin\n",
        )
        .unwrap();
        ensure_cordis_patch(&profile).unwrap();
        let raw = fs::read_to_string(profile.join("cordis.patch.yml")).unwrap();
        assert_eq!(raw.matches("id: tui-bridge").count(), 1);
        assert!(raw.contains("other-plugin"));
        remove_tree(&home);
    }

    #[test]
    fn malformed_manifest_is_refused_with_actionable_error() {
        let home = temp_home("malformed-manifest");
        let profile = profile_dir(&home);
        write_manifest(&profile, "{ not valid json");
        let error = ensure_package_json(&profile).unwrap_err();
        let message = error.to_string();
        assert!(message.contains("package.json"));
        assert!(message.contains("dshe setup"));
        assert!(message.contains("Repair or restore"));
        remove_tree(&home);
    }

    #[test]
    fn extraction_replaces_bridge_and_is_idempotent() {
        let home = temp_home("extraction");
        let profile = profile_dir(&home);
        fs::create_dir_all(&profile).unwrap();
        install_embedded_bridge(&profile).unwrap();
        let target = bridge_dir(&profile);
        assert!(target.join("package.json").is_file());
        assert!(target.join("protocol-contract.json").is_file());
        let src_count = embedded_files()
            .iter()
            .filter(|f| f.path.starts_with("src/"))
            .count();
        assert!(src_count > 0);

        // A second extraction replaces cleanly and leaves no staging dirs.
        install_embedded_bridge(&profile).unwrap();
        assert!(target.join("package.json").is_file());
        let leftovers: Vec<_> = fs::read_dir(packages_dir(&profile))
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with('.'))
            .collect();
        assert!(
            leftovers.is_empty(),
            "staging/backup dirs were not cleaned: {leftovers:?}"
        );
        remove_tree(&home);
    }

    #[test]
    fn failed_install_never_records_success() {
        let home = temp_home("failed-install");
        let profile = profile_dir(&home);
        let error = run_setup_with(&home, |_home| {
            Err(SetupError::new(
                "DSH bridge installation failed: `dsh plugin --profile e install` exited with exit code 1. Review the package-manager output above, fix the reported issue, then run `dshe setup` again.",
            ))
        })
        .unwrap_err();
        assert!(error.to_string().contains("dshe setup"));
        assert!(!setup_record_path(&profile).exists());
        assert_eq!(check_setup_status(&home), SetupStatus::Missing);
        remove_tree(&home);
    }

    #[test]
    fn successful_setup_records_current_state() {
        let home = temp_home("successful-setup");
        let profile = profile_dir(&home);
        run_setup_with(&home, |_home| {
            // Simulate pnpm materializing the workspace package as a link.
            fs::create_dir_all(profile.join("node_modules")).unwrap();
            let link = profile.join("node_modules").join(BRIDGE_PACKAGE);
            fs::create_dir_all(&link).unwrap();
            Ok(())
        })
        .unwrap();

        assert!(setup_record_path(&profile).is_file());
        assert_eq!(check_setup_status(&home), SetupStatus::Ready);
        let record = read_setup_record(&setup_record_path(&profile)).unwrap();
        assert_eq!(record.bridge_digest, BRIDGE_DIGEST);
        assert_eq!(record.wire_protocol, crate::protocol::WIRE_PROTOCOL_VERSION);
        remove_tree(&home);
    }

    #[test]
    fn damaged_setup_is_classified_when_structure_is_removed() {
        let home = temp_home("damaged-setup");
        let profile = profile_dir(&home);
        run_setup_with(&home, |_home| {
            fs::create_dir_all(profile.join("node_modules")).unwrap();
            fs::create_dir_all(profile.join("node_modules").join(BRIDGE_PACKAGE)).unwrap();
            Ok(())
        })
        .unwrap();
        assert_eq!(check_setup_status(&home), SetupStatus::Ready);

        // Removing the patch registration makes the setup structurally damaged.
        fs::remove_file(profile.join("cordis.patch.yml")).unwrap();
        assert_eq!(check_setup_status(&home), SetupStatus::Damaged);
        remove_tree(&home);
    }

    #[test]
    fn record_mismatch_is_classified_as_outdated() {
        let home = temp_home("outdated-setup");
        let profile = profile_dir(&home);
        run_setup_with(&home, |_home| {
            fs::create_dir_all(profile.join("node_modules")).unwrap();
            fs::create_dir_all(profile.join("node_modules").join(BRIDGE_PACKAGE)).unwrap();
            Ok(())
        })
        .unwrap();

        let path = setup_record_path(&profile);
        let mut record = read_setup_record(&path).unwrap();
        record.bridge_digest = "0".repeat(64);
        write_text_atomic(
            &path,
            &format!("{}\n", serde_json::to_string(&record).unwrap()),
        )
        .unwrap();
        assert_eq!(check_setup_status(&home), SetupStatus::Outdated);
        remove_tree(&home);
    }

    #[test]
    fn require_ready_messages_are_english_and_actionable() {
        // Missing: no profile at all.
        let missing = temp_home("require-missing");
        let error = require_ready(&missing).unwrap_err().to_string();
        assert!(error.contains("missing"), "{error}");
        assert!(error.contains("`dshe setup`"), "{error}");
        assert!(error.contains("`dshe`"), "{error}");
        remove_tree(&missing);

        // Outdated: a valid structure but a foreign digest.
        let outdated = temp_home("require-outdated");
        let profile = profile_dir(&outdated);
        run_setup_with(&outdated, |_home| {
            fs::create_dir_all(profile.join("node_modules")).unwrap();
            fs::create_dir_all(profile.join("node_modules").join(BRIDGE_PACKAGE)).unwrap();
            Ok(())
        })
        .unwrap();
        let path = setup_record_path(&profile);
        let mut record = read_setup_record(&path).unwrap();
        record.bridge_digest = "f".repeat(64);
        write_text_atomic(
            &path,
            &format!("{}\n", serde_json::to_string(&record).unwrap()),
        )
        .unwrap();
        let error = require_ready(&outdated).unwrap_err().to_string();
        assert!(error.contains("`dshe setup`"), "{error}");
        assert!(error.contains("restart"), "{error}");
        remove_tree(&outdated);

        // Damaged: matching digest but removed structure.
        let damaged = temp_home("require-damaged");
        let profile = profile_dir(&damaged);
        run_setup_with(&damaged, |_home| {
            fs::create_dir_all(profile.join("node_modules")).unwrap();
            fs::create_dir_all(profile.join("node_modules").join(BRIDGE_PACKAGE)).unwrap();
            Ok(())
        })
        .unwrap();
        fs::remove_file(profile.join("cordis.patch.yml")).unwrap();
        let error = require_ready(&damaged).unwrap_err().to_string();
        assert!(error.contains("incomplete"), "{error}");
        assert!(error.contains(&profile.display().to_string()), "{error}");
        assert!(error.contains("`dshe setup`"), "{error}");
        assert!(error.contains("restart"), "{error}");
        remove_tree(&damaged);
    }

    fn install_argv() -> Vec<String> {
        vec![
            "dsh".to_string(),
            "plugin".to_string(),
            "--profile".to_string(),
            "e".to_string(),
            "install".to_string(),
        ]
    }

    #[test]
    fn plugin_install_argv_uses_dedicated_profile() {
        let Some(argv) = plugin_install_argv() else {
            return; // dsh/npx unavailable in this environment.
        };
        assert_eq!(
            &argv[argv.len() - 4..],
            ["plugin", "--profile", "e", "install"]
        );
        assert!(argv[0] == "dsh" || (argv[0] == "npx" && argv.len() >= 3 && argv[1] == "-y"));
    }

    #[test]
    fn plugin_install_missing_tool_is_actionable() {
        let argv = install_argv();
        let error = run_plugin_install_with(Path::new("home"), &argv, |_, _| {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "boom"))
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("Install DSH"), "{error}");
        assert!(error.contains("`dshe setup`"), "{error}");
    }

    #[test]
    fn plugin_install_spawn_failure_is_actionable() {
        let argv = install_argv();
        let error = run_plugin_install_with(Path::new("home"), &argv, |_, _| {
            Err(std::io::Error::other("spawn boom"))
        })
        .unwrap_err()
        .to_string();
        assert!(error.contains("Failed to start"), "{error}");
        assert!(error.contains("`dshe setup`"), "{error}");
    }

    #[test]
    fn plugin_install_nonzero_exit_is_actionable() {
        let argv = install_argv();
        let error = run_plugin_install_with(Path::new("home"), &argv, |_, _| Ok(Some(3)))
            .unwrap_err()
            .to_string();
        assert!(error.contains("exit code 3"), "{error}");
        assert!(error.contains("`dshe setup`"), "{error}");
    }

    #[test]
    fn plugin_install_zero_exit_succeeds() {
        let argv = install_argv();
        assert!(run_plugin_install_with(Path::new("home"), &argv, |_, _| Ok(Some(0))).is_ok());
    }

    #[test]
    fn corrupt_record_is_damaged_not_missing() {
        let home = temp_home("corrupt-record");
        let profile = profile_dir(&home);
        run_setup_with(&home, |_home| {
            fs::create_dir_all(profile.join("node_modules")).unwrap();
            fs::create_dir_all(profile.join("node_modules").join(BRIDGE_PACKAGE)).unwrap();
            Ok(())
        })
        .unwrap();
        assert_eq!(check_setup_status(&home), SetupStatus::Ready);

        fs::write(setup_record_path(&profile), "{ not json").unwrap();
        assert_eq!(check_setup_status(&home), SetupStatus::Damaged);
        remove_tree(&home);
    }

    #[test]
    fn unchanged_manifest_is_not_rewritten() {
        let home = temp_home("manifest-noop");
        let profile = profile_dir(&home);
        let original =
            "{\"name\":\"dsh-profile-e\",\"dependencies\":{\"dsh-tui-bridge\":\"workspace:*\"}}\n";
        write_manifest(&profile, original);
        ensure_package_json(&profile).unwrap();
        assert_eq!(
            fs::read_to_string(profile.join("package.json")).unwrap(),
            original,
            "an already-correct manifest must not be reformatted"
        );
        remove_tree(&home);
    }
}
