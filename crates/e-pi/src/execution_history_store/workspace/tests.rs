use super::*;

fn identity(cwd: &Path) -> TraceIdentity {
    TraceIdentity {
        frontend: FRONTEND.into(),
        session_id: "session".into(),
        cwd: cwd.to_str().unwrap().into(),
    }
}

fn seed(root: &Path, cwd: &Path) -> PathBuf {
    let id = identity(cwd);
    let workspace = WorkspaceIdentity::new(&id.cwd).unwrap();
    let directory = root.join(workspace.key());
    fs::create_dir_all(&directory).unwrap();
    let path = directory.join("session.jsonl");
    fs::write(&path, format!("{}\n", header(&id).unwrap())).unwrap();
    path
}

#[test]
fn stable_normalization_is_lexical_and_does_not_merge_workspaces() {
    let temp = tempfile::tempdir().unwrap();
    let cwd = temp.path().join("missing").join("项目 space");
    let first = WorkspaceIdentity::new(cwd.to_str().unwrap()).unwrap();
    let equivalent = cwd.join("child").join("..").join(".");
    assert_eq!(
        first,
        WorkspaceIdentity::new(equivalent.to_str().unwrap()).unwrap()
    );
    assert!(!cwd.exists());
    assert_ne!(
        first.key(),
        WorkspaceIdentity::new(temp.path().to_str().unwrap())
            .unwrap()
            .key()
    );
    assert!(first
        .key()
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-'));
    assert_eq!(first.key().len(), 67);
    assert!(WorkspaceIdentity::new("relative/path").is_err());
    let differently_cased = temp.path().join("missing").join("项目 SPACE");
    assert_ne!(
        first,
        WorkspaceIdentity::new(differently_cased.to_str().unwrap()).unwrap()
    );
}

#[test]
fn missing_and_malformed_indexes_recover_all_workspaces_without_touching_traces() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("cache");
    let cwd1 = temp.path().join("one");
    let cwd2 = temp.path().join("two");
    let trace1 = seed(&root, &cwd1);
    let trace2 = seed(&root, &cwd2);
    let before1 = fs::read(&trace1).unwrap();
    let before2 = fs::read(&trace2).unwrap();
    for corrupt in [false, true] {
        if corrupt {
            fs::write(root.join("workspaces.json"), "{broken").unwrap();
        }
        let warnings = register(&root, &identity(&cwd1)).unwrap();
        assert_eq!(!warnings.is_empty(), corrupt);
        let registry: Registry =
            serde_json::from_slice(&fs::read(root.join("workspaces.json")).unwrap()).unwrap();
        assert_eq!(registry.workspaces.len(), 2);
        assert_eq!(fs::read(&trace1).unwrap(), before1);
        assert_eq!(fs::read(&trace2).unwrap(), before2);
    }
    let backups: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with("workspaces.corrupt-")
        })
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(fs::read(backups[0].path()).unwrap(), b"{broken");
    let index = root.join("workspaces.json");
    let before = fs::read(&index).unwrap();
    let modified = fs::metadata(&index).unwrap().modified().unwrap();
    register(&root, &identity(&cwd1)).unwrap();
    assert_eq!(fs::read(&index).unwrap(), before);
    assert_eq!(fs::metadata(&index).unwrap().modified().unwrap(), modified);
}

#[test]
fn unsupported_versions_and_mapping_conflicts_are_not_overwritten() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("cache");
    let cwd = temp.path().join("one");
    fs::create_dir_all(&root).unwrap();
    for text in [
        "{\"version\":99,\"workspaces\":{}}".to_owned(),
        format!("{{\"version\":1,\"workspaces\":{{\"ws-wrong\":{{\"workspace_path\":{},\"display_path\":{},\"created_at_unix_ms\":0}}}}}}",
            serde_json::to_string(&cwd).unwrap(), serde_json::to_string(&cwd).unwrap()),
    ] {
        fs::write(root.join("workspaces.json"), &text).unwrap();
        assert!(register(&root, &identity(&cwd)).is_err());
        assert_eq!(fs::read_to_string(root.join("workspaces.json")).unwrap(), text);
    }
    fs::remove_file(root.join("workspaces.json")).unwrap();
    let path = seed(&root, &cwd);
    let original = fs::read(&path).unwrap();
    let mut value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    value["workspace"]["version"] = 99.into();
    fs::write(&path, format!("{value}\n")).unwrap();
    assert!(register(&root, &identity(&cwd))
        .unwrap_err()
        .contains("unsupported"));
    fs::write(&path, original).unwrap();
    fs::rename(path.parent().unwrap(), root.join("ws-wrong")).unwrap();
    assert!(register(&root, &identity(&cwd))
        .unwrap_err()
        .contains("conflict"));
    assert!(!root.join("workspaces.json").exists());
}

#[test]
fn contended_registry_lock_fails_without_replacing_metadata_and_releases() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("cache");
    let id = identity(temp.path());
    register(&root, &id).unwrap();
    let before = fs::read(root.join("workspaces.json")).unwrap();
    let held = lock(&root).unwrap();
    assert!(register(&root, &id)
        .unwrap_err()
        .contains("lock execution-history"));
    assert_eq!(fs::read(root.join("workspaces.json")).unwrap(), before);
    drop(held);
    register(&root, &id).unwrap();
}

#[test]
fn registration_child_process() {
    let Some(root) = std::env::var_os("E_HISTORY_REGISTRY_TEST_ROOT") else {
        return;
    };
    let cwd = std::env::var_os("E_HISTORY_REGISTRY_TEST_CWD").unwrap();
    register(Path::new(&root), &identity(Path::new(&cwd))).unwrap();
}

#[test]
fn concurrent_process_registration_preserves_both_mappings() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("cache");
    let mut children = Vec::new();
    for name in ["one", "two"] {
        children.push(
            std::process::Command::new(std::env::current_exe().unwrap())
                .arg("execution_history_store::workspace::tests::registration_child_process")
                .arg("--exact")
                .env("E_HISTORY_REGISTRY_TEST_ROOT", &root)
                .env("E_HISTORY_REGISTRY_TEST_CWD", temp.path().join(name))
                .spawn()
                .unwrap(),
        );
    }
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    let registry: Registry =
        serde_json::from_slice(&fs::read(root.join("workspaces.json")).unwrap()).unwrap();
    assert_eq!(registry.workspaces.len(), 2);
}

#[cfg(windows)]
#[test]
fn windows_paths_normalize_separators_drive_and_verbatim_prefixes() {
    let expected = WorkspaceIdentity::new("C:/Repo/项目").unwrap();
    for path in [
        r"c:\Repo\项目\",
        r"\\?\C:\Repo\.\项目\child\..",
        "C:/Repo/项目",
    ] {
        assert_eq!(expected, WorkspaceIdentity::new(path).unwrap());
    }
    assert_eq!(
        WorkspaceIdentity::new(r"\\server\share\project").unwrap(),
        WorkspaceIdentity::new(r"\\?\UNC\server\share\project\child\..").unwrap()
    );
}

#[test]
fn invalid_root_and_header_identity_are_explicit() {
    let temp = tempfile::tempdir().unwrap();
    let id = identity(temp.path());
    assert_eq!(
        root_from_config(Some(temp.path().to_owned())).unwrap(),
        temp.path().join("cache").join(FRONTEND).join("history")
    );
    assert!(root_from_config(None).is_err());
    assert!(root_from_config(Some(PathBuf::from(".e"))).is_err());
    assert!(register(Path::new("relative"), &id).is_err());
    let mut value = header(&id).unwrap();
    value["workspace"]["path"] = "/different".into();
    assert!(validate_header(&value).unwrap_err().contains("identity"));
    let mut value = header(&id).unwrap();
    value["identity"]["frontend"] = "other".into();
    assert!(validate_header(&value).unwrap_err().contains("identity"));
}
