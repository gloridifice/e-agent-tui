use std::{env, fs, path::PathBuf};

fn main() {
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
