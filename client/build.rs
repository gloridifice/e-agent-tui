use std::{env, fs, path::PathBuf};

fn main() {
    generate_wire_contract();
    generate_embedded_bridge();
}

fn generate_wire_contract() {
    let contract_path = PathBuf::from("../bridge/protocol-contract.json");
    println!("cargo:rerun-if-changed={}", contract_path.display());
    let raw = fs::read_to_string(&contract_path).expect("read bridge/protocol-contract.json");
    let contract: serde_json::Value = serde_json::from_str(&raw).expect("parse protocol contract");
    let number = |path: &[&str]| -> u64 {
        let mut value = &contract;
        for key in path {
            value = &value[*key];
        }
        value
            .as_u64()
            .unwrap_or_else(|| panic!("missing numeric contract field {path:?}"))
    };
    let strings = |key: &str| -> Vec<String> {
        contract[key]
            .as_array()
            .unwrap_or_else(|| panic!("missing contract array {key}"))
            .iter()
            .map(|v| v.as_str().expect("contract string").to_owned())
            .collect()
    };
    let array = |name: &str, values: Vec<String>| -> String {
        let values = values
            .into_iter()
            .map(|v| format!("{v:?}"))
            .collect::<Vec<_>>()
            .join(", ");
        format!("pub const {name}: &[&str] = &[{values}];\n")
    };
    let records_json = serde_json::to_string(&contract["records"]).expect("serialize records");
    let shapes_json =
        serde_json::to_string(&contract["messageShapes"]).expect("serialize message shapes");
    let generated = format!(
        "// Generated from bridge/protocol-contract.json; do not edit.\n\
         pub const WIRE_PROTOCOL_VERSION: u64 = {};\n\
         pub const SNAPSHOT_EVENT_CAP: usize = {};\n\
         pub const HISTORY_EVENT_CAP: usize = {};\n\
         pub const MAX_WIRE_FRAME_BYTES: usize = {};\n\
         pub const CLIENT_REPLAY_EVENT_CAP: usize = {};\n\
         pub const WIRE_RECORD_SHAPES_JSON: &str = {:?};\n\
         pub const WIRE_MESSAGE_SHAPES_JSON: &str = {:?};\n{}{}{}",
        number(&["protocolVersion"]),
        number(&["limits", "snapshotEvents"]),
        number(&["limits", "historyEvents"]),
        number(&["limits", "maxFrameBytes"]),
        number(&["limits", "clientReplayEvents"]),
        records_json,
        shapes_json,
        array("SURFACE_EVENT_TYPES", strings("surfaceEvents")),
        array("CLIENT_MESSAGE_TYPES", strings("clientMessages")),
        array("SERVER_MESSAGE_TYPES", strings("serverMessages")),
    );
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("wire_contract.rs");
    fs::write(out, generated).expect("write generated wire contract");
}

fn generate_embedded_bridge() {
    use sha2::{Digest, Sha256};

    let bridge = PathBuf::from("../bridge");

    // The runtime bridge package: manifest, canonical contract, and every
    // production module directly under bridge/src. Tests, tools, caches, and
    // node_modules are intentionally excluded; their absence also makes the
    // bundle deterministic across environments.
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    for rel in ["package.json", "protocol-contract.json"] {
        files.push((rel.to_string(), bridge.join(rel)));
    }
    let src_dir = bridge.join("src");
    let mut modules: Vec<PathBuf> = fs::read_dir(&src_dir)
        .expect("read bridge/src")
        .map(|entry| entry.expect("bridge/src entry").path())
        .filter(|path| path.is_file() && path.extension().is_some_and(|ext| ext == "js"))
        .collect();
    modules.sort();
    for path in modules {
        let name = path
            .file_name()
            .expect("bridge module file name")
            .to_string_lossy()
            .into_owned();
        files.push((format!("src/{name}"), path));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hasher = Sha256::new();
    let mut entries = String::new();
    for (rel, path) in &files {
        println!("cargo:rerun-if-changed={}", path.display());
        let bytes =
            fs::read(path).unwrap_or_else(|e| panic!("read embedded bridge file {rel}: {e}"));
        // Path is part of the digest so renames and additions are observable.
        hasher.update((rel.len() as u64).to_le_bytes());
        hasher.update(rel.as_bytes());
        hasher.update(&bytes);
        let absolute = path
            .canonicalize()
            .unwrap_or_else(|e| panic!("canonicalize embedded bridge file {rel}: {e}"));
        entries.push_str(&format!(
            "    EmbeddedFile {{ path: {rel:?}, bytes: include_bytes!({:?}) }},\n",
            absolute.display().to_string()
        ));
    }
    let digest = hasher.finalize();
    let digest_hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();

    let generated = format!(
        "// Generated from bridge/ by build.rs; do not edit.\n\
         pub struct EmbeddedFile {{\n\
             pub path: &'static str,\n\
             pub bytes: &'static [u8],\n\
         }}\n\n\
         pub const BRIDGE_FILES: &[EmbeddedFile] = &[\n\
         {entries}\
         ];\n\n\
         pub const BRIDGE_DIGEST: &str = \"{digest_hex}\";\n"
    );
    let out = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR")).join("embedded_bridge.rs");
    fs::write(out, generated).expect("write generated embedded bridge");
}
