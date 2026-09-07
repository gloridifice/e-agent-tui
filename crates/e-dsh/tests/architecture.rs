use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

type Graph = BTreeMap<String, BTreeSet<String>>;

fn production_source(source: &str) -> &str {
    // Chunks keep their line terminators so `offset` always advances by the
    // exact byte length (CRLF `\r` must count too, or the slice drifts into
    // the middle of a multi-byte UTF-8 character and panics).
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut offset = 0usize;
    for (index, line) in lines.iter().enumerate() {
        let body = line.trim_end_matches(['\r', '\n']);
        if body.trim() == "#[cfg(test)]"
            && lines
                .iter()
                .skip(index + 1)
                .find(|line| !line.trim().is_empty())
                .is_some_and(|line| line.trim_start().starts_with("mod tests"))
        {
            return &source[..offset];
        }
        offset += line.len();
    }
    source
}

#[test]
fn runners_process_admitted_input_before_claiming_queued_prompts() {
    for (name, source) in [
        ("dshe", include_str!("../src/main.rs")),
        ("pie", include_str!("../../e-pi/src/main.rs")),
    ] {
        let source = production_source(source);
        let input = source
            .find("RuntimeController::apply_terminal_route(")
            .unwrap();
        let dispatch = source
            .find("RuntimeController::dispatch_next_queued(")
            .unwrap();
        assert!(
            input < dispatch,
            "{name} dispatch must not outrun admitted Escape"
        );
    }
}

#[test]
fn runners_share_selection_policy_and_publish_only_after_submission() {
    for (name, source) in [
        ("dshe", include_str!("../src/main.rs")),
        ("pie", include_str!("../../e-pi/src/main.rs")),
    ] {
        let source = production_source(source);
        let reconcile = source
            .find("RuntimeController::reconcile_presentation(")
            .unwrap();
        let wait = source
            .find("let frame_deadline = scheduler.deadline()")
            .unwrap();
        assert!(
            reconcile < wait,
            "{name} must reconcile capture before sleeping"
        );
        assert_eq!(
            source.matches("scheduler.presentation_deadline(").count(),
            2,
            "{name} must gate both animation and notice deadlines"
        );
        let draw = source.find("let transaction = terminal.draw(").unwrap();
        let submitted = source[draw..]
            .find("let transaction = transaction?;")
            .unwrap();
        let publish = source[draw..]
            .find("committed_presentation.commit(")
            .unwrap();
        assert!(
            submitted < publish,
            "{name} must not publish a failed frame"
        );
        assert!(
            !source.contains("same_geometry("),
            "{name} must not duplicate snapshot policy"
        );
    }
}

#[derive(Debug)]
struct SourceModule {
    id: String,
    path: Vec<String>,
    source: String,
}

fn module_name(root: &Path, path: &Path) -> (String, Vec<String>) {
    let relative = path
        .strip_prefix(root)
        .unwrap_or_else(|_| panic!("{} is outside {}", path.display(), root.display()));
    let mut segments: Vec<String> = relative
        .parent()
        .into_iter()
        .flat_map(|parent| parent.components())
        .filter_map(|component| component.as_os_str().to_str())
        .map(str::to_owned)
        .collect();
    let stem = relative
        .file_stem()
        .and_then(|stem| stem.to_str())
        .unwrap_or_else(|| panic!("Rust source has no UTF-8 stem: {}", path.display()));
    if stem != "mod" && !(segments.is_empty() && matches!(stem, "lib" | "main")) {
        segments.push(stem.to_owned());
    }

    let id = if segments.is_empty() {
        stem.to_owned()
    } else {
        segments.join("::")
    };
    (id, segments)
}

fn declared_module_name(line: &str) -> Option<String> {
    let line = line.trim_start();
    let line = line
        .strip_prefix("pub(")
        .and_then(|line| line.find(')').map(|end| &line[end + 1..]))
        .or_else(|| line.strip_prefix("pub "))
        .unwrap_or(line)
        .trim_start();
    let name = line.strip_prefix("mod ")?;
    let name = name
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || *ch == '_')
        .collect::<String>();
    (!name.is_empty()).then_some(name)
}

fn test_only_module_paths(sources: &[SourceModule]) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for module in sources {
        let lines: Vec<_> = module.source.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if line.trim() != "#[cfg(test)]" {
                continue;
            }
            if let Some(name) = lines
                .iter()
                .skip(index + 1)
                .find_map(|line| (!line.trim().is_empty()).then(|| declared_module_name(line)))
                .flatten()
            {
                let mut path = module.path.clone();
                path.push(name);
                paths.insert(path.join("::"));
            }
        }
    }
    paths
}

fn split_top_level(source: &str, separator: char) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut depth = 0usize;
    let mut start = 0usize;
    for (index, ch) in source.char_indices() {
        match ch {
            '{' => depth += 1,
            '}' => depth = depth.saturating_sub(1),
            _ if ch == separator && depth == 0 => {
                parts.push(source[start..index].trim());
                start = index + ch.len_utf8();
            }
            _ => {}
        }
    }
    parts.push(source[start..].trim());
    parts
}

fn path_segments(source: &str) -> Vec<String> {
    source
        .split("::")
        .map(str::trim)
        .filter(|segment| !segment.is_empty() && *segment != "*")
        .map(str::to_owned)
        .collect()
}

fn expand_use_tree(tree: &str, prefix: &[String], paths: &mut Vec<Vec<String>>) {
    let tree = tree.trim().trim_end_matches(';').trim();
    let tree = tree
        .split_once(" as ")
        .map_or(tree, |(path, _)| path)
        .trim();
    if let Some(open) = tree.find('{') {
        let mut depth = 0usize;
        let mut close = None;
        for (relative, ch) in tree[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = Some(open + relative);
                        break;
                    }
                }
                _ => {}
            }
        }
        if let Some(close) = close {
            let mut nested_prefix = prefix.to_vec();
            nested_prefix.extend(path_segments(tree[..open].trim_end_matches(':')));
            for child in split_top_level(&tree[open + 1..close], ',') {
                if !child.is_empty() {
                    expand_use_tree(child, &nested_prefix, paths);
                }
            }
            return;
        }
    }

    let mut path = prefix.to_vec();
    path.extend(path_segments(tree));
    if !path.is_empty() {
        paths.push(path);
    }
}

fn use_tree_after_prefix(line: &str) -> Option<&str> {
    let line = line.trim_start();
    if let Some(tree) = line.strip_prefix("use ") {
        return Some(tree);
    }
    if let Some(tree) = line.strip_prefix("pub use ") {
        return Some(tree);
    }
    let line = line.strip_prefix("pub(")?;
    let end = line.find(')')?;
    line[end + 1..].trim_start().strip_prefix("use ")
}

fn use_paths(source: &str) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    let mut statement = None::<String>;
    for line in source.lines() {
        if let Some(pending) = statement.as_mut() {
            pending.push(' ');
            pending.push_str(line.trim());
            if pending.contains(';') {
                expand_use_tree(pending, &[], &mut paths);
                statement = None;
            }
            continue;
        }
        if let Some(tree) = use_tree_after_prefix(line) {
            if tree.contains(';') {
                expand_use_tree(tree, &[], &mut paths);
            } else {
                statement = Some(tree.trim().to_owned());
            }
        }
    }
    paths
}

fn qualified_paths(source: &str) -> Vec<Vec<String>> {
    let mut paths = Vec::new();
    for prefix in ["crate::", "self::", "super::", "e_dsh::"] {
        let mut search = 0usize;
        while let Some(relative) = source[search..].find(prefix) {
            let start = search + relative;
            let boundary = source[..start]
                .chars()
                .next_back()
                .is_none_or(|ch| !(ch.is_ascii_alphanumeric() || ch == '_'));
            if !boundary {
                search = start + prefix.len();
                continue;
            }
            let end = source[start..]
                .char_indices()
                .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || *ch == '_' || *ch == ':')
                .last()
                .map_or(start + prefix.len(), |(index, ch)| {
                    start + index + ch.len_utf8()
                });
            paths.push(path_segments(&source[start..end]));
            search = end;
        }
    }
    paths
}

fn resolve_module_path(
    path: &[String],
    current: &[String],
    modules: &BTreeMap<String, String>,
) -> Option<String> {
    let (mut candidate, index) = match path.first().map(String::as_str) {
        Some("crate" | "e") => (Vec::new(), 1),
        Some("self") => (current.to_vec(), 1),
        Some("super") => {
            let mut parent = current.to_vec();
            let mut index = 0;
            while path.get(index).is_some_and(|segment| segment == "super") {
                parent.pop()?;
                index += 1;
            }
            (parent, index)
        }
        Some(_) => return None,
        None => return None,
    };
    candidate.extend(
        path[index..]
            .iter()
            .filter(|segment| segment.as_str() != "self")
            .cloned(),
    );

    for length in (1..=candidate.len()).rev() {
        if let Some(module) = modules.get(&candidate[..length].join("::")) {
            return Some(module.clone());
        }
    }
    None
}

fn module_references(
    source: &str,
    current: &[String],
    modules: &BTreeMap<String, String>,
) -> BTreeSet<String> {
    let source = production_source(source);
    use_paths(source)
        .into_iter()
        .chain(qualified_paths(source))
        .filter_map(|path| resolve_module_path(&path, current, modules))
        .collect()
}

fn source_files(root: &Path) -> Vec<PathBuf> {
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

fn rust_files_recursive(root: &Path) -> Vec<PathBuf> {
    source_files(root)
}

fn production_graph(root: &Path) -> Graph {
    let mut sources: Vec<_> = source_files(root)
        .into_iter()
        .map(|file| {
            let (id, path) = module_name(root, &file);
            SourceModule {
                id,
                path,
                source: fs::read_to_string(&file).expect("read Rust module"),
            }
        })
        .collect();
    sources.sort_by(|left, right| left.id.cmp(&right.id));

    let test_only = test_only_module_paths(&sources);
    sources.retain(|module| {
        !test_only.iter().any(|path| {
            module.id == *path
                || module
                    .id
                    .strip_prefix(path)
                    .is_some_and(|suffix| suffix.starts_with("::"))
        })
    });

    let mut modules = BTreeMap::new();
    for module in &sources {
        if module.path.is_empty() {
            continue;
        }
        let path = module.path.join("::");
        assert!(
            modules.insert(path.clone(), module.id.clone()).is_none(),
            "duplicate Rust module path {path}"
        );
    }

    sources
        .iter()
        .map(|module| {
            let mut edges = module_references(&module.source, &module.path, &modules);
            edges.remove(&module.id);
            (module.id.clone(), edges)
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

struct SourceFixture {
    root: PathBuf,
}

impl SourceFixture {
    fn new() -> Self {
        use std::sync::atomic::{AtomicUsize, Ordering};

        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "e-architecture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).expect("create scanner fixture root");
        Self { root }
    }

    fn write(&self, relative: &str, source: &str) {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().expect("fixture source parent"))
            .expect("create scanner fixture directory");
        fs::write(path, source).expect("write scanner fixture source");
    }
}

impl Drop for SourceFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn scanner_discovers_nested_modules_and_resolves_relative_grouped_paths() {
    let fixture = SourceFixture::new();
    fixture.write(
        "lib.rs",
        r#"
pub mod flat;
pub mod nested;
#[cfg(test)]
mod test_support;
"#,
    );
    fixture.write(
        "flat.rs",
        r#"
use crate::{
    nested::{
        cycle_a::CycleA,
        sibling::{self, child::Child},
    },
};
"#,
    );
    fixture.write("nested/mod.rs", "pub mod sibling;\n");
    fixture.write(
        "nested/sibling.rs",
        "use self::child::Child;\nuse super::cycle_a::CycleA;\n",
    );
    fixture.write(
        "nested/sibling/child.rs",
        "use super::super::cycle_b::CycleB;\n",
    );
    fixture.write("nested/cycle_a.rs", "use super::cycle_b::CycleB;\n");
    fixture.write("nested/cycle_b.rs", "use super::{cycle_c::CycleC};\n");
    fixture.write("nested/cycle_c.rs", "use crate::nested::cycle_a::CycleA;\n");
    fixture.write("test_support.rs", "use crate::nested::cycle_a::CycleA;\n");

    let graph = production_graph(&fixture.root);
    assert!(graph.contains_key("flat"));
    assert!(graph.contains_key("nested"));
    assert!(graph.contains_key("nested::sibling"));
    assert!(graph.contains_key("nested::sibling::child"));
    assert!(!graph.contains_key("test_support"));
    assert_eq!(
        graph["flat"],
        BTreeSet::from([
            "nested::cycle_a".into(),
            "nested::sibling".into(),
            "nested::sibling::child".into(),
        ])
    );
    assert_eq!(
        graph["nested::sibling"],
        BTreeSet::from(["nested::cycle_a".into(), "nested::sibling::child".into()])
    );
    assert_eq!(
        graph["nested::sibling::child"],
        BTreeSet::from(["nested::cycle_b".into()])
    );

    let cycles: Vec<_> = strongly_connected_components(&graph)
        .into_iter()
        .filter(|component| component.len() > 1)
        .collect();
    assert_eq!(
        cycles,
        vec![vec![
            String::from("nested::cycle_a"),
            String::from("nested::cycle_b"),
            String::from("nested::cycle_c"),
        ]]
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
    let tui_root = dsh_root
        .parent()
        .expect("workspace crates directory")
        .join("e-tui/src");
    let model = fs::read_to_string(tui_root.join("runtime/state/mod.rs"))
        .expect("read shared runtime state");
    let app = fs::read_to_string(tui_root.join("app.rs")).expect("read app module");
    let projection =
        fs::read_to_string(tui_root.join("projection/mod.rs")).expect("read projection module");
    let surface =
        fs::read_to_string(tui_root.join("projection/surface.rs")).expect("read surface.rs");
    let transcript =
        fs::read_to_string(tui_root.join("ui/transcript.rs")).expect("read transcript renderer");

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

    assert!(
        !production_source(&transcript).contains("Msg::"),
        "production UI must render DisplayItem directly"
    );
    assert!(
        model.contains("pub tui: TuiApp")
            && app.contains("pub timeline: TimelineModel")
            && !model.contains("pub transcript: TranscriptStore"),
        "RuntimeState must forward through TuiApp to the sole TimelineModel owner"
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
fn lifecycle_models_are_sole_production_owners() {
    let dsh_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tui_root = dsh_root
        .parent()
        .expect("workspace crates directory")
        .join("e-tui/src");
    let runtime_state = fs::read_to_string(tui_root.join("runtime/state/mod.rs"))
        .expect("read shared runtime state");
    let main = fs::read_to_string(dsh_root.join("src/main.rs")).expect("read composition root");
    let app = fs::read_to_string(tui_root.join("app.rs")).expect("read app root");

    for owner in [
        "pub session: SessionModel",
        "pub timeline: TimelineModel",
        "pub catalogs: CatalogModel",
        "pub interaction: InteractionModel",
        "pub render: RenderState",
    ] {
        assert!(
            app.contains(owner),
            "TuiApp missing lifecycle owner: {owner}"
        );
    }

    for forbidden in [
        "pub session_id:",
        "pub new_conversation:",
        "pub todos:",
        "pub approval:",
        "pub question:",
        "pub queue:",
        "pub transcript_cache:",
        "pub units:",
        "pub expanded:",
    ] {
        assert!(
            !production_source(&runtime_state).contains(forbidden),
            "RuntimeState retained migrated mirror field: {forbidden}"
        );
    }
    for forbidden in [
        "let mut input = InputState::new",
        "let mut scroll = ScrollState::default",
        "let mut input_page:",
        "let mut help_visible =",
        "let mut copy_mode:",
    ] {
        assert!(
            !production_source(&main).contains(forbidden),
            "composition root retained migrated interaction mirror: {forbidden}"
        );
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
fn row_copy_mode_is_absent_while_semantic_provenance_remains() {
    let dsh_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tui_root = dsh_root
        .parent()
        .expect("workspace crates directory")
        .join("e-tui/src");
    for root in [dsh_root.join("src"), tui_root.clone()] {
        for path in rust_files_recursive(&root) {
            let source = fs::read_to_string(&path).expect("read Rust source");
            let production = production_source(&source);
            for forbidden in [
                "CopyMode",
                "CopyRowsCache",
                "CopyOverlay",
                "CopyAction",
                "copy_mode_open",
                "InputAction::CopyMode",
            ] {
                assert!(
                    !production.contains(forbidden),
                    "{} retains old row Copy Mode symbol {forbidden}",
                    path.display()
                );
            }
        }
    }
    let layout =
        fs::read_to_string(tui_root.join("transcript_layout.rs")).expect("read provenance layout");
    let reading = fs::read_to_string(tui_root.join("reading.rs")).expect("read Reading model");
    assert!(layout.contains("ProvenanceLayoutRow"));
    assert!(reading.contains("ReadingCopyPayload"));
}

#[test]
fn preview_resolution_returns_events_without_ui_lock_access() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let resolver = fs::read_to_string(root.join("preview_resolver.rs")).expect("read resolver");
    let ports = fs::read_to_string(root.join("runtime_ports.rs")).expect("read ports");
    let executor = fs::read_to_string(
        root.parent()
            .and_then(Path::parent)
            .expect("workspace crates directory")
            .join("e-tui/src/runtime/executor.rs"),
    )
    .expect("read shared executor");
    for forbidden in ["TuiApp", "AppState", "Mutex", ".lock()"] {
        assert!(
            !production_source(&resolver).contains(forbidden),
            "Preview resolver reaches UI state through {forbidden}"
        );
    }
    assert!(ports.contains("fn resolve_preview("));
    assert!(executor.contains("EffectResult::PreviewResolved"));
    assert!(executor.contains("ports.resolve_preview(request.clone()).await"));
}

#[test]
fn rendering_dependencies_point_screen_to_pane_to_region_to_component() {
    let dsh_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let ui_root = dsh_root
        .parent()
        .expect("workspace crates directory")
        .join("e-tui/src/ui");

    for (layer, forbidden) in [
        ("component", vec!["ui::region", "ui::pane", "ui::screen"]),
        ("region", vec!["ui::pane", "ui::screen"]),
        ("pane", vec!["ui::screen"]),
    ] {
        for path in rust_files_recursive(&ui_root.join(layer)) {
            let source = fs::read_to_string(&path).expect("read render layer");
            let source = production_source(&source);
            for upward in &forbidden {
                assert!(
                    !source.contains(upward),
                    "{} has upward rendering import {upward}",
                    path.display()
                );
            }
        }
    }

    let screen = fs::read_to_string(ui_root.join("screen.rs")).expect("read Screen");
    let main_pane = fs::read_to_string(ui_root.join("pane/main.rs")).expect("read main Pane");
    let transcript =
        fs::read_to_string(ui_root.join("region/transcript.rs")).expect("read transcript Region");
    assert!(screen.contains("pane::main::render_with_cursor"));
    assert!(main_pane.contains("render_main_pane_with_cursor"));
    assert!(transcript.contains("render_transcript"));
}

#[test]
fn production_crate_graphs_are_acyclic_and_keep_leaf_boundaries() {
    let dsh_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tui_root = dsh_root
        .parent()
        .expect("workspace crates directory")
        .join("e-tui");
    let pi_root = dsh_root
        .parent()
        .expect("workspace crates directory")
        .join("e-pi");
    let dsh_graph = production_graph(&dsh_root.join("src"));
    let pi_graph = production_graph(&pi_root.join("src"));
    let tui_graph = production_graph(&tui_root.join("src"));

    for (name, graph) in [
        ("e-dsh", &dsh_graph),
        ("e-pi", &pi_graph),
        ("e-tui", &tui_graph),
    ] {
        let cycles: Vec<_> = strongly_connected_components(graph)
            .into_iter()
            .filter(|component| component.len() > 1)
            .collect();
        assert!(
            cycles.is_empty(),
            "{name} production module cycles: {cycles:?}"
        );
    }

    assert!(
        !tui_graph["copy"].contains("ui"),
        "semantic copy selection must remain independent of rendering"
    );
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
        tui_graph["transcript_layout"]
            .iter()
            .all(|module| matches!(module.as_str(), "config" | "display" | "render" | "wrap")),
        "transcript_layout may only depend on presentation leaf services: {:?}",
        tui_graph["transcript_layout"]
    );
    assert!(
        tui_graph["command_catalog"].is_empty(),
        "command catalog must remain a leaf among top-level frontend modules"
    );

    let workspace_manifest = fs::read_to_string(
        dsh_root
            .parent()
            .expect("workspace crates directory")
            .parent()
            .expect("workspace root")
            .join("Cargo.toml"),
    )
    .expect("read workspace manifest");
    let dsh_manifest =
        fs::read_to_string(dsh_root.join("Cargo.toml")).expect("read e-dsh manifest");
    let pi_manifest = fs::read_to_string(pi_root.join("Cargo.toml")).expect("read e-pi manifest");
    let tui_manifest =
        fs::read_to_string(tui_root.join("Cargo.toml")).expect("read e-tui manifest");
    let workspace: toml::Value =
        toml::from_str(&workspace_manifest).expect("parse workspace manifest");
    let workspace_version = workspace["workspace"]["package"]["version"]
        .as_str()
        .expect("workspace package version");
    let tui_dependency = &workspace["workspace"]["dependencies"]["e-tui"];
    let synchronized_requirement = format!("={workspace_version}");
    assert!(
        tui_dependency["path"].as_str() == Some("crates/e-tui")
            && tui_dependency["version"].as_str() == Some(synchronized_requirement.as_str())
            && dsh_manifest.contains("e-tui.workspace = true"),
        "e-dsh must inherit the synchronized, versioned local e-tui package"
    );
    assert!(
        pi_manifest.contains("e-tui.workspace = true") && !pi_manifest.contains("e-dsh"),
        "e-pi must depend directly on the inherited e-tui package and not on e-dsh"
    );
    assert!(
        !tui_manifest.contains("e-dsh")
            && !tui_manifest.contains("../e-dsh")
            && !tui_manifest.contains("e-pi")
            && !tui_manifest.contains("../e-pi"),
        "e-tui must not depend on an executable adapter"
    );
}
