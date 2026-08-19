use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

type Graph = BTreeMap<String, BTreeSet<String>>;

fn production_source(source: &str) -> &str {
    let lines: Vec<&str> = source.lines().collect();
    let mut offset = 0usize;
    for (index, line) in lines.iter().enumerate() {
        if line.trim() == "#[cfg(test)]"
            && lines
                .iter()
                .skip(index + 1)
                .find(|line| !line.trim().is_empty())
                .is_some_and(|line| line.trim_start().starts_with("mod tests"))
        {
            return &source[..offset];
        }
        offset += line.len() + 1;
    }
    source
}

fn identifier_after(source: &str, prefix: &str, mut offset: usize) -> Option<(String, usize)> {
    offset += prefix.len();
    let tail = &source[offset..];
    let len = tail
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || *ch == '_')
        .last()
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    (len > 0).then(|| (tail[..len].to_owned(), offset + len))
}

fn grouped_roots(source: &str, prefix: &str) -> BTreeSet<String> {
    let mut roots = BTreeSet::new();
    let mut search = 0usize;
    while let Some(relative) = source[search..].find(prefix) {
        let body_start = search + relative + prefix.len();
        let mut depth = 0usize;
        let mut segment_start = body_start;
        for (relative_index, ch) in source[body_start..].char_indices() {
            let index = body_start + relative_index;
            match ch {
                '{' => depth += 1,
                '}' if depth == 0 => {
                    let segment = source[segment_start..index].trim();
                    if let Some(root) = segment
                        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                        .find(|part| !part.is_empty())
                    {
                        roots.insert(root.to_owned());
                    }
                    search = index + 1;
                    break;
                }
                '}' => depth -= 1,
                ',' if depth == 0 => {
                    let segment = source[segment_start..index].trim();
                    if let Some(root) = segment
                        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                        .find(|part| !part.is_empty())
                    {
                        roots.insert(root.to_owned());
                    }
                    segment_start = index + 1;
                }
                _ => {}
            }
        }
    }
    roots
}

fn module_references(source: &str, modules: &BTreeSet<String>) -> BTreeSet<String> {
    let source = production_source(source);
    let mut references = BTreeSet::new();
    for prefix in ["crate::", "e::"] {
        let mut search = 0usize;
        while let Some(relative) = source[search..].find(prefix) {
            let offset = search + relative;
            if let Some((name, end)) = identifier_after(source, prefix, offset) {
                if modules.contains(&name) {
                    references.insert(name);
                }
                search = end;
            } else {
                search = offset + prefix.len();
            }
        }
    }
    for prefix in ["use crate::{", "use e::{"] {
        references.extend(
            grouped_roots(source, prefix)
                .into_iter()
                .filter(|name| modules.contains(name)),
        );
    }
    references
}

fn source_files(root: &Path) -> Vec<PathBuf> {
    let mut files: Vec<_> = fs::read_dir(root)
        .unwrap_or_else(|error| panic!("read crate source root {}: {error}", root.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "rs"))
        .collect();
    files.sort();
    files
}

fn rust_files_recursive(root: &Path) -> Vec<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(&directory)
            .unwrap_or_else(|error| panic!("read {}: {error}", directory.display()))
            .filter_map(Result::ok)
        {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|extension| extension == "rs") {
                files.push(path);
            }
        }
    }
    files.sort();
    files
}

fn production_graph(root: &Path) -> Graph {
    let files = source_files(root);
    let modules: BTreeSet<_> = files
        .iter()
        .filter_map(|path| path.file_stem()?.to_str().map(str::to_owned))
        .collect();
    files
        .into_iter()
        .map(|path| {
            let name = path.file_stem().unwrap().to_str().unwrap().to_owned();
            let source = fs::read_to_string(&path).expect("read Rust module");
            let mut edges = module_references(&source, &modules);
            edges.remove(&name);
            (name, edges)
        })
        .collect()
}

fn strongly_connected_components(graph: &Graph) -> Vec<Vec<String>> {
    struct Tarjan<'a> {
        graph: &'a Graph,
        next: usize,
        indices: BTreeMap<String, usize>,
        low: BTreeMap<String, usize>,
        stack: Vec<String>,
        on_stack: BTreeSet<String>,
        components: Vec<Vec<String>>,
    }

    impl Tarjan<'_> {
        fn visit(&mut self, node: &str) {
            let index = self.next;
            self.next += 1;
            self.indices.insert(node.to_owned(), index);
            self.low.insert(node.to_owned(), index);
            self.stack.push(node.to_owned());
            self.on_stack.insert(node.to_owned());

            for dependency in self.graph.get(node).into_iter().flatten() {
                if !self.indices.contains_key(dependency) {
                    self.visit(dependency);
                    let dependency_low = self.low[dependency];
                    self.low
                        .entry(node.to_owned())
                        .and_modify(|low| *low = (*low).min(dependency_low));
                } else if self.on_stack.contains(dependency) {
                    let dependency_index = self.indices[dependency];
                    self.low
                        .entry(node.to_owned())
                        .and_modify(|low| *low = (*low).min(dependency_index));
                }
            }

            if self.low[node] == self.indices[node] {
                let mut component = Vec::new();
                loop {
                    let member = self.stack.pop().expect("SCC stack member");
                    self.on_stack.remove(&member);
                    component.push(member.clone());
                    if member == node {
                        break;
                    }
                }
                component.sort();
                self.components.push(component);
            }
        }
    }

    let mut tarjan = Tarjan {
        graph,
        next: 0,
        indices: BTreeMap::new(),
        low: BTreeMap::new(),
        stack: Vec::new(),
        on_stack: BTreeSet::new(),
        components: Vec::new(),
    };
    for node in graph.keys() {
        if !tarjan.indices.contains_key(node) {
            tarjan.visit(node);
        }
    }
    tarjan.components.sort();
    tarjan.components
}

#[test]
fn scanner_reads_simple_grouped_and_explicit_paths_but_not_test_modules() {
    let modules = ["config", "model", "protocol", "ui"]
        .into_iter()
        .map(str::to_owned)
        .collect();
    let source = r#"
use crate::config::Config;
use crate::{
    model::{AppState, Msg},
    protocol::ClientMessage,
};
fn layout() { let _ = crate::ui::copy_layout_rows; }
#[cfg(test)]
mod tests { use crate::ignored::OnlyInTests; }
"#;
    assert_eq!(
        module_references(source, &modules),
        ["config", "model", "protocol", "ui"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    );
}

#[test]
fn tarjan_reports_cycles_and_leaves_dag_nodes_single() {
    let graph = BTreeMap::from([
        ("a".into(), BTreeSet::from(["b".into()])),
        ("b".into(), BTreeSet::from(["a".into()])),
        ("root".into(), BTreeSet::from(["a".into(), "leaf".into()])),
        ("leaf".into(), BTreeSet::new()),
    ]);
    let components = strongly_connected_components(&graph);
    assert!(components.contains(&vec!["a".into(), "b".into()]));
    assert!(components.contains(&vec!["leaf".into()]));
    assert!(components.contains(&vec!["root".into()]));
}

#[test]
fn single_track_transcript_has_no_legacy_production_path() {
    let dsh_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = dsh_root.join("src");
    let tui_root = dsh_root
        .parent()
        .expect("workspace crates directory")
        .join("e-tui/src");
    let model = fs::read_to_string(root.join("model.rs")).expect("read model.rs");
    let projection =
        fs::read_to_string(tui_root.join("projection/mod.rs")).expect("read projection module");
    let surface =
        fs::read_to_string(tui_root.join("projection/surface.rs")).expect("read surface.rs");
    let transcript =
        fs::read_to_string(root.join("ui/transcript.rs")).expect("read transcript renderer");

    for (source, forbidden) in [
        (&model, "enum Msg"),
        (&model, "fn reduce_host_event"),
        (&model, "pub transcript: TranscriptStore"),
        (&model, "pub projector: EventProjector"),
        (&projection, "display_positions"),
        (&surface, "record_display_position"),
        (&surface, "display_position("),
        (&transcript, "fn msg_lines"),
        (&transcript, "fn styled_msg_lines"),
    ] {
        assert!(
            !source.contains(forbidden),
            "legacy transcript symbol reintroduced: {forbidden}"
        );
    }

    assert!(
        projection.contains("pub struct TimelineModel")
            && projection.contains("pub transcript: TranscriptStore")
            && projection.contains("pub projector: EventProjector"),
        "e-tui TimelineModel must remain the sole transcript/projector owner"
    );

    let legacy_test_renderer = transcript
        .find("pub(super) fn legacy_test_lines")
        .expect("test-only legacy renderer marker");
    let production_renderer = &transcript[..legacy_test_renderer];
    assert!(
        !production_renderer.contains("Msg::"),
        "production UI must render DisplayItem directly"
    );
    assert!(
        model.contains("pub timeline: TimelineModel")
            && !model.contains("pub transcript: TranscriptStore"),
        "AppState must forward to the sole e-tui TimelineModel owner"
    );
    let lines = model.lines().collect::<Vec<_>>();
    for (index, line) in lines.iter().enumerate() {
        if line.contains("pub msgs:") {
            assert_eq!(
                lines.get(index.wrapping_sub(1)).map(|line| line.trim()),
                Some("#[cfg(test)]"),
                "legacy characterization storage must never enter production"
            );
        }
    }
}

#[test]
fn persisted_config_has_one_strict_schema_and_one_default_source() {
    let dsh_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tui_root = dsh_root
        .parent()
        .expect("workspace crates directory")
        .join("e-tui");
    let config = fs::read_to_string(tui_root.join("src/config.rs")).expect("read UI config");
    let store = fs::read_to_string(dsh_root.join("src/config.rs")).expect("read config store");
    for forbidden in [
        "struct CompleteConfig",
        "struct PartialConfig",
        "impl<'de> Deserialize<'de> for Config",
        "fn into_config",
    ] {
        assert!(
            !config.contains(forbidden),
            "parallel config schema reintroduced: {forbidden}"
        );
    }
    assert!(config.contains("#[serde(deny_unknown_fields)]"));
    assert!(config.contains("toml::from_str(DEFAULT_CONFIG_SOURCE)"));
    assert!(config.contains("overlay_known(&mut merged, user)"));
    assert!(
        store.contains("pub use e_tui::config::{Config"),
        "e-dsh must re-export the one UI config schema"
    );
    assert!(
        !config.contains("std::fs") && !config.contains("directories::"),
        "e-tui config values must not own filesystem persistence"
    );
}

#[test]
fn e_tui_has_no_dsh_or_infrastructure_imports() {
    let dsh_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tui_root = dsh_root
        .parent()
        .expect("workspace crates directory")
        .join("e-tui");
    for path in rust_files_recursive(&tui_root.join("src")) {
        let source = fs::read_to_string(&path).expect("read e-tui source");
        let production = production_source(&source);
        for forbidden in [
            "e_dsh::",
            "ServerMessage",
            "ClientMessage",
            "HostEvent",
            "tokio_tungstenite",
            "tool/call",
            "assistant/chunk",
            "user/message",
            "surfaceOp",
        ] {
            assert!(
                !production.contains(forbidden),
                "{} contains forbidden DSH/infrastructure symbol {forbidden}",
                path.display()
            );
        }
    }
    let manifest = fs::read_to_string(tui_root.join("Cargo.toml")).expect("read e-tui manifest");
    for forbidden in ["e-dsh", "tokio-tungstenite", "directories", "arboard"] {
        assert!(
            !manifest.contains(forbidden),
            "e-tui manifest contains infrastructure dependency {forbidden}"
        );
    }
}

#[test]
fn production_crate_graphs_are_acyclic_and_keep_leaf_boundaries() {
    let dsh_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tui_root = dsh_root
        .parent()
        .expect("workspace crates directory")
        .join("e-tui");
    let dsh_graph = production_graph(&dsh_root.join("src"));
    let tui_graph = production_graph(&tui_root.join("src"));

    for (name, graph) in [("e-dsh", &dsh_graph), ("e-tui", &tui_graph)] {
        let cycles: Vec<_> = strongly_connected_components(graph)
            .into_iter()
            .filter(|component| component.len() > 1)
            .collect();
        assert!(
            cycles.is_empty(),
            "{name} production module cycles: {cycles:?}"
        );
    }

    for (from, forbidden) in [("copy", "ui"), ("ui", "runtime_command")] {
        assert!(
            !dsh_graph[from].contains(forbidden),
            "forbidden reverse edge {from} -> {forbidden}"
        );
    }
    for (from, forbidden) in [
        ("input", "input_page"),
        ("command_catalog", "input"),
        ("login", "input_page"),
        ("settings", "input_page"),
    ] {
        assert!(
            !tui_graph[from].contains(forbidden),
            "forbidden frontend reverse edge {from} -> {forbidden}"
        );
    }
    assert!(
        dsh_graph["transcript_layout"]
            .iter()
            .all(|module| matches!(module.as_str(), "config" | "display" | "render")),
        "transcript_layout may only depend on presentation leaf services: {:?}",
        dsh_graph["transcript_layout"]
    );
    assert!(dsh_graph["command_catalog"].is_empty());
    assert!(
        tui_graph["command_catalog"].is_empty(),
        "command catalog must remain a leaf among top-level frontend modules"
    );

    let dsh_manifest =
        fs::read_to_string(dsh_root.join("Cargo.toml")).expect("read e-dsh manifest");
    let tui_manifest =
        fs::read_to_string(tui_root.join("Cargo.toml")).expect("read e-tui manifest");
    assert!(
        dsh_manifest.contains("e-tui = { path = \"../e-tui\" }"),
        "e-dsh must depend on the local e-tui package"
    );
    assert!(
        !tui_manifest.contains("e-dsh") && !tui_manifest.contains("../e-dsh"),
        "e-tui must not depend on e-dsh"
    );
}
