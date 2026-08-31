use std::{fs, path::Path};

fn rust_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files
}

fn production(source: &str) -> &str {
    source
        .find("#[cfg(test)]\nmod tests")
        .map_or(source, |index| &source[..index])
}

#[test]
fn pi_protocol_stays_out_of_e_tui() {
    let crate_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tui_root = crate_root.parent().unwrap().join("e-tui/src");
    for path in rust_files(&tui_root) {
        let source = fs::read_to_string(&path).unwrap();
        let source = production(&source);
        for forbidden in [
            "RpcCommand",
            "RpcRecord",
            "extension_ui_request",
            "pi --mode rpc",
            "tokio::process",
            "e_pi::",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} contains Pi adapter symbol {forbidden}",
                path.display()
            );
        }
    }
}

#[test]
fn pi_adapter_does_not_reach_dsh_bridge_modules() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    for path in rust_files(&root) {
        let source = fs::read_to_string(&path).unwrap();
        let source = production(&source);
        for forbidden in [
            "e::bridge",
            "e::bridge_io",
            "e::protocol",
            "e::launcher",
            "e::setup",
            "tokio_tungstenite",
            "ClientMessage",
            "ServerMessage",
        ] {
            assert!(
                !source.contains(forbidden),
                "{} reaches DSH adapter symbol {forbidden}",
                path.display()
            );
        }
    }
}
