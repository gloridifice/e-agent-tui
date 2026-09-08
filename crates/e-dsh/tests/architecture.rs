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
        if body.trim() != "#[cfg(test)]" {
            offset += line.len();
            continue;
        }
        let mut next = index + 1;
        while let Some(candidate) = lines.get(next) {
            let trimmed = candidate.trim();
            if trimmed.is_empty() || trimmed.starts_with("#[path") || trimmed.starts_with("#[cfg") {
                next += 1;
            } else {
                break;
            }
        }
        if lines
            .get(next)
            .and_then(|candidate| declared_module_name(candidate))
            .is_some()
        {
            return &source[..offset];
        }
        offset += line.len();
    }
    source
}

/// Replace comments and literals while preserving byte positions and line
/// breaks. The architecture scanner is intentionally lightweight, but it must
/// inspect Rust syntax rather than arbitrary text: paths in examples and
/// comments are not module dependencies.
fn code_only_source(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut index = 0usize;
    let mut block_depth = 0usize;
    while index < chars.len() {
        let current = chars[index];
        let next = chars.get(index + 1).copied();
        if block_depth > 0 {
            if current == '/' && next == Some('*') {
                block_depth += 1;
                out.push(' ');
                out.push(' ');
                index += 2;
            } else if current == '*' && next == Some('/') {
                block_depth = block_depth.saturating_sub(1);
                out.push(' ');
                out.push(' ');
                index += 2;
            } else {
                out.push(if current == '\n' { '\n' } else { ' ' });
                index += 1;
            }
            continue;
        }
        if current == '/' && next == Some('/') {
            out.push(' ');
            out.push(' ');
            index += 2;
            while index < chars.len() && chars[index] != '\n' {
                out.push(' ');
                index += 1;
            }
            continue;
        }
        if current == '/' && next == Some('*') {
            block_depth = 1;
            out.push(' ');
            out.push(' ');
            index += 2;
            continue;
        }
        // Raw strings, including byte raw strings, are removed as one token.
        let raw_start = current == 'r' || (current == 'b' && next == Some('r'));
        let quote_index = if raw_start {
            let mut probe = index + usize::from(current == 'b');
            if chars.get(probe) == Some(&'r') {
                probe += 1;
            }
            let hashes = chars[probe..]
                .iter()
                .take_while(|character| **character == '#')
                .count();
            (chars.get(probe + hashes) == Some(&'"')).then_some((probe + hashes, hashes))
        } else {
            None
        };
        if let Some((quote, hashes)) = quote_index {
            for character in &chars[index..=quote] {
                out.push(if *character == '\n' { '\n' } else { ' ' });
            }
            index = quote + 1;
            while index < chars.len() {
                if chars[index] == '"'
                    && chars
                        .get(index + 1..index + 1 + hashes)
                        .is_some_and(|suffix| suffix.iter().all(|character| *character == '#'))
                {
                    for _ in 0..=hashes {
                        out.push(' ');
                    }
                    index += hashes + 1;
                    break;
                }
                out.push(if chars[index] == '\n' { '\n' } else { ' ' });
                index += 1;
            }
            continue;
        }
        if current == '"' {
            out.push(' ');
            index += 1;
            while index < chars.len() {
                let character = chars[index];
                out.push(if character == '\n' { '\n' } else { ' ' });
                index += 1;
                if character == '\\' && index < chars.len() {
                    out.push(if chars[index] == '\n' { '\n' } else { ' ' });
                    index += 1;
                } else if character == '"' {
                    break;
                }
            }
            continue;
        }
        // A short quoted character is a literal; a lifetime such as `'a` is
        // deliberately left alone because it is not a path-bearing token.
        if current == '\'' && chars.get(index + 2) == Some(&'\'') {
            out.push(' ');
            out.push(' ');
            out.push(' ');
            index += 3;
            continue;
        }
        out.push(current);
        index += 1;
    }
    out
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
    file: PathBuf,
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

fn path_attribute(line: &str) -> Option<String> {
    let line = line.trim();
    let body = line.strip_prefix("#[path")?.trim_start();
    let body = body.strip_prefix('=')?.trim_start();
    let body = body.strip_prefix('"')?;
    body.split_once('"').map(|(path, _)| path.to_owned())
}

fn test_only_module_paths(root: &Path, sources: &[SourceModule]) -> BTreeSet<String> {
    let mut paths = BTreeSet::new();
    for module in sources {
        let lines: Vec<_> = module.source.lines().collect();
        for (index, line) in lines.iter().enumerate() {
            if line.trim() != "#[cfg(test)]" {
                continue;
            }
            let mut next = index + 1;
            let mut path = None;
            while let Some(candidate) = lines.get(next) {
                let trimmed = candidate.trim();
                if trimmed.is_empty() || trimmed.starts_with("#[cfg") {
                    next += 1;
                } else if let Some(attribute_path) = path_attribute(trimmed) {
                    path = Some(attribute_path);
                    next += 1;
                } else {
                    break;
                }
            }
            if let Some(name) = lines.get(next).and_then(|line| declared_module_name(line)) {
                let mut module_path = module.path.clone();
                module_path.push(name);
                paths.insert(module_path.join("::"));
                if let Some(path) = path {
                    let target = module
                        .file
                        .parent()
                        .expect("Rust source has a parent directory")
                        .join(path);
                    if target.exists() {
                        let (target_id, _) = module_name(root, &target);
                        paths.insert(target_id);
                    } else if let Some(stem) = target.file_stem().and_then(|stem| stem.to_str()) {
                        paths.insert(stem.to_owned());
                    }
                }
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

#[derive(Debug, Clone)]
struct UseImport {
    alias: String,
    path: Vec<String>,
}

fn expand_use_tree(tree: &str, prefix: &[String], imports: &mut Vec<UseImport>) {
    let tree = tree.trim().trim_end_matches(';').trim();
    let (tree, explicit_alias) = tree
        .rsplit_once(" as ")
        .map_or((tree, None), |(path, alias)| {
            (path.trim(), Some(alias.trim()))
        });
    if let Some(open) = tree.find('{') {
        let mut depth = 0usize;
        let mut close = None;
        for (relative, ch) in tree[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
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
                    expand_use_tree(child, &nested_prefix, imports);
                }
            }
            return;
        }
    }

    let local = path_segments(tree);
    let mut path = prefix.to_vec();
    if local.as_slice() == [String::from("self")] {
        // `use crate::foo::{self, Bar}` imports `foo` under its own name.
    } else {
        path.extend(
            local
                .iter()
                .filter(|segment| segment.as_str() != "*")
                .cloned(),
        );
    }
    if path.is_empty() || path.last().is_some_and(|segment| segment == "*") {
        return;
    }
    let alias = explicit_alias.map(str::to_owned).or_else(|| {
        if local.as_slice() == [String::from("self")] {
            prefix.last().cloned()
        } else {
            path.last().cloned()
        }
    });
    if let Some(alias) = alias {
        imports.push(UseImport { alias, path });
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

fn use_imports(source: &str) -> Vec<UseImport> {
    let mut imports = Vec::new();
    let mut statement = None::<String>;
    for line in source.lines() {
        if let Some(pending) = statement.as_mut() {
            pending.push(' ');
            pending.push_str(line.trim());
            if pending.contains(';') {
                expand_use_tree(pending, &[], &mut imports);
                statement = None;
            }
            continue;
        }
        if let Some(tree) = use_tree_after_prefix(line) {
            if tree.contains(';') {
                expand_use_tree(tree, &[], &mut imports);
            } else {
                statement = Some(tree.trim().to_owned());
            }
        }
    }
    imports
}

fn use_paths(source: &str) -> Vec<Vec<String>> {
    use_imports(source)
        .into_iter()
        .map(|import| import.path)
        .collect()
}

fn is_identifier_start(character: char) -> bool {
    character.is_ascii_alphabetic() || character == '_'
}

fn is_identifier_continue(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

/// Extract every qualified identifier path from code. Unlike the former
/// prefix search this handles local module calls (`screen::render`) and
/// aliases, while `code_only_source` keeps comments and literals out.
fn qualified_paths(source: &str) -> Vec<Vec<String>> {
    let chars: Vec<char> = source.chars().collect();
    let mut paths = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        if !is_identifier_start(chars[index])
            || (index > 0 && is_identifier_continue(chars[index - 1]))
        {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while end < chars.len() && is_identifier_continue(chars[end]) {
            end += 1;
        }
        let mut path = vec![chars[index..end].iter().collect::<String>()];
        let mut cursor = end;
        while chars.get(cursor) == Some(&':') && chars.get(cursor + 1) == Some(&':') {
            let segment_start = cursor + 2;
            if !chars
                .get(segment_start)
                .is_some_and(|character| is_identifier_start(*character))
            {
                break;
            }
            let mut segment_end = segment_start + 1;
            while segment_end < chars.len() && is_identifier_continue(chars[segment_end]) {
                segment_end += 1;
            }
            path.push(chars[segment_start..segment_end].iter().collect());
            cursor = segment_end;
        }
        if path.len() > 1
            && path
                .iter()
                .any(|segment| !matches!(segment.as_str(), "self" | "super" | "crate" | "e"))
        {
            paths.push(path);
        }
        index = end.max(cursor);
    }
    paths
}

fn resolve_module_path(
    path: &[String],
    current: &[String],
    modules: &BTreeMap<String, String>,
    aliases: &BTreeMap<String, Vec<String>>,
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
        Some(first) if aliases.contains_key(first) => (aliases[first].clone(), 1),
        Some(_) => (current.to_vec(), 0),
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
    let source = code_only_source(production_source(source));
    let imports = use_imports(&source);
    let mut aliases = BTreeMap::<String, Vec<String>>::new();
    // Resolve imports repeatedly so a local alias can be used by a later
    // import without making the scanner depend on declaration order details.
    for _ in 0..=imports.len() {
        let mut changed = false;
        for import in &imports {
            if let Some(module) = resolve_module_path(&import.path, current, modules, &aliases) {
                let path = path_segments(&module);
                changed |= aliases.insert(import.alias.clone(), path).is_none();
            }
        }
        if !changed {
            break;
        }
    }
    use_paths(&source)
        .into_iter()
        .chain(qualified_paths(&source))
        .filter_map(|path| resolve_module_path(&path, current, modules, &aliases))
        // Access from a child to the module that contains it is Rust's normal
        // parent-module visibility mechanism. Treat that containment edge as
        // structural so a parent re-exporting the child is not misreported as
        // an architectural cycle; sibling and ancestor feature edges remain
        // visible to the graph.
        .filter(|target| {
            current
                .get(..current.len().saturating_sub(1))
                .is_none_or(|parent| target != &parent.join("::"))
        })
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
                file: file.clone(),
                source: fs::read_to_string(&file).expect("read Rust module"),
            }
        })
        .collect();
    sources.sort_by(|left, right| left.id.cmp(&right.id));

    let test_only = test_only_module_paths(root, &sources);
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
fn scanner_resolves_local_calls_aliases_and_ignores_non_code_and_path_tests() {
    let fixture = SourceFixture::new();
    fixture.write(
        "lib.rs",
        r#"
mod ui;
mod other;
#[cfg(test)]
#[path = "test_support.rs"]
mod fixtures;
"#,
    );
    fixture.write(
        "ui.rs",
        r#"
mod screen;
pub use crate::other::Thing as OtherThing;
fn render() {
    screen::render();
    OtherThing::call();
    let _literal = "screen::ignored::path";
    // other::ignored::path must not become an edge.
}
"#,
    );
    fixture.write("ui/screen.rs", "pub fn render() {}\n");
    fixture.write("other.rs", "pub struct Thing;\n");
    fixture.write("test_support.rs", "use crate::ui::screen::Hidden;\n");

    let graph = production_graph(&fixture.root);
    assert_eq!(
        graph["ui"],
        BTreeSet::from(["other".into(), "ui::screen".into()])
    );
    assert!(!graph.contains_key("fixtures"));
    assert!(!graph.contains_key("test_support"));
}

#[test]
fn scanner_reports_an_actual_nested_cycle_but_not_parent_containment() {
    let fixture = SourceFixture::new();
    fixture.write("lib.rs", "mod parent;\n");
    fixture.write("parent.rs", "mod child;\npub use child::*;\n");
    fixture.write("parent/child.rs", "use super::ParentType;\n");
    let graph = production_graph(&fixture.root);
    assert_eq!(graph["parent"], BTreeSet::from(["parent::child".into()]));
    assert!(graph["parent::child"].is_empty());

    fixture.write("parent/cycle.rs", "use crate::parent::child::Child;\n");
    fixture.write("parent/child.rs", "use crate::parent::cycle::Cycle;\n");
    let graph = production_graph(&fixture.root);
    let cycles: Vec<_> = strongly_connected_components(&graph)
        .into_iter()
        .filter(|component| component.len() > 1)
        .collect();
    assert_eq!(
        cycles,
        vec![vec![
            String::from("parent::child"),
            String::from("parent::cycle"),
        ]],
    );
}
#[test]
fn scanner_confirms_main_pane_back_edge_is_removed() {
    let dsh_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let tui_root = dsh_root
        .parent()
        .expect("workspace crates directory")
        .join("e-tui/src");
    let graph = production_graph(&tui_root);
    assert!(graph["ui"].contains("ui::screen"));
    assert!(graph["ui::screen"].contains("ui::pane"));
    assert!(!graph["ui::pane::main"].contains("ui"));
    assert!(!strongly_connected_components(&graph)
        .into_iter()
        .any(|component| {
            component
                == vec![
                    String::from("ui"),
                    String::from("ui::pane::main"),
                    String::from("ui::screen"),
                ]
        }));
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
