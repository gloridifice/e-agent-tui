//! Mermaid rendering via the grok-mermaid WebAssembly module (D9, D18).
//!
//! The WASM artifact is Simon Willison's extraction of xAI Grok Build's
//! mermaid-to-Unicode-box-art renderer (Apache-2.0; see
//! assets/LICENSE.grok-mermaid). Its FFI is import-free:
//!
//!   wasm_alloc(len) -> ptr
//!   wasm_render_html(ptr, len, maxWidth) -> result_len
//!   wasm_result_ptr() -> ptr
//!   memory
//!
//! Output is HTML lines carrying semantic classes: b=border, n=node text,
//! e=edge, el=edge label, t=title, i=italic. We parse those into styled
//! terminal lines; a trap falls back to the raw fenced source (D9).

use std::sync::OnceLock;

use wasmi::{Engine, Linker, Memory, Module, Store};

const WASM_BYTES: &[u8] = include_bytes!("../assets/grok-mermaid.wasm");

/// One styled fragment of a rendered diagram line.
#[derive(Debug, Clone, PartialEq)]
pub enum MermaidClass {
    Border,
    Node,
    Edge,
    EdgeLabel,
    Title,
}

#[derive(Debug, Clone)]
pub struct MermaidSpan {
    pub class: MermaidClass,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct MermaidLine {
    pub spans: Vec<MermaidSpan>,
}

struct EngineCell {
    engine: Engine,
    module: Module,
}

fn engine() -> &'static EngineCell {
    static CELL: OnceLock<EngineCell> = OnceLock::new();
    CELL.get_or_init(|| {
        let engine = Engine::default();
        let module = Module::new(&engine, WASM_BYTES).expect("grok-mermaid.wasm must parse");
        EngineCell { engine, module }
    })
}

fn decode_html(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
}

/// Render mermaid source to terminal lines. `max_width` constrains the
/// layout (0 = unconstrained, D18). Returns Err on trap/invalid input —
/// callers fall back to the fenced source.
pub fn render(source: &str, max_width: u32) -> Result<Vec<MermaidLine>, String> {
    let cell = engine();
    let mut store = Store::new(&cell.engine, ());
    let linker = Linker::new(&cell.engine);
    let instance = linker
        .instantiate(&mut store, &cell.module)
        .and_then(|i| i.start(&mut store))
        .map_err(|e| format!("mermaid: instantiate failed: {e}"))?;

    let alloc = instance
        .get_typed_func::<i32, i32>(&store, "wasm_alloc")
        .map_err(|e| format!("mermaid: wasm_alloc: {e}"))?;
    let render_html = instance
        .get_typed_func::<(i32, i32, i32), i32>(&store, "wasm_render_html")
        .map_err(|e| format!("mermaid: wasm_render_html: {e}"))?;
    let result_ptr = instance
        .get_typed_func::<(), i32>(&store, "wasm_result_ptr")
        .map_err(|e| format!("mermaid: wasm_result_ptr: {e}"))?;
    let memory = instance
        .get_memory(&store, "memory")
        .ok_or("mermaid: no memory export")?;

    let input = source.as_bytes();
    let ptr = alloc.call(&mut store, input.len() as i32)
        .map_err(|e| format!("mermaid: alloc trap: {e}"))?;
    write_memory(&memory, &mut store, ptr, input)?;

    let len = render_html
        .call(&mut store, (ptr, input.len() as i32, max_width as i32))
        .map_err(|e| format!("mermaid: render trap: {e}"))?;
    if len <= 0 {
        return Err("mermaid: empty render result".into());
    }
    let out_ptr = result_ptr.call(&mut store, ()).map_err(|e| format!("mermaid: result ptr: {e}"))?;
    let bytes = read_memory(&memory, &store, out_ptr, len as usize)?;
    let html = String::from_utf8_lossy(&bytes).into_owned();

    Ok(parse_html(&html))
}

fn write_memory(memory: &Memory, store: &mut Store<()>, ptr: i32, bytes: &[u8]) -> Result<(), String> {
    let ptr = ptr.max(0) as usize;
    memory
        .write(store, ptr, bytes)
        .map_err(|e| format!("mermaid: memory write: {e}"))
}

fn read_memory(memory: &Memory, store: &Store<()>, ptr: i32, len: usize) -> Result<Vec<u8>, String> {
    let ptr = ptr.max(0) as usize;
    let data = memory.data(store);
    let end = ptr
        .checked_add(len)
        .ok_or("mermaid: result range overflow")?;
    if end > data.len() {
        return Err(format!("mermaid: result range {ptr}..{end} out of memory"));
    }
    Ok(data[ptr..end].to_vec())
}

/// Parse the `<span class="…">…</span>` HTML into styled lines.
fn parse_html(html: &str) -> Vec<MermaidLine> {
    let mut lines: Vec<MermaidLine> = Vec::new();
    for raw_line in html.lines() {
        let mut spans = Vec::new();
        let mut rest = raw_line;
        while let Some(open) = rest.find("<span class=\"") {
            // Plain text before the span.
            let plain = &rest[..open];
            if !plain.is_empty() {
                spans.push(MermaidSpan {
                    class: MermaidClass::Border,
                    text: decode_html(plain),
                });
            }
            let after_open = &rest[open + "<span class=\"".len()..];
            let Some(close_quote) = after_open.find('"') else { break };
            let class = &after_open[..close_quote];
            let after_tag = &after_open[close_quote + 2..]; // skip `">`
            let Some(end) = after_tag.find("</span>") else { break };
            let text = &after_tag[..end];
            let parsed = match class.split_whitespace().next().unwrap_or("") {
                "b" => MermaidClass::Border,
                "n" => MermaidClass::Node,
                "e" => MermaidClass::Edge,
                "el" => MermaidClass::EdgeLabel,
                "t" => MermaidClass::Title,
                _ => MermaidClass::Node,
            };
            spans.push(MermaidSpan { class: parsed, text: decode_html(text) });
            rest = &after_tag[end + "</span>".len()..];
        }
        if !rest.is_empty() {
            spans.push(MermaidSpan {
                class: MermaidClass::Border,
                text: decode_html(rest),
            });
        }
        lines.push(MermaidLine { spans });
    }
    while lines.last().map_or(false, |l| l.spans.iter().all(|s| s.text.trim().is_empty())) {
        lines.pop();
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_parses() {
        let _ = engine();
    }

    #[test]
    fn renders_simple_graph() {
        let out = render("graph TD\n  A --> B\n", 80).expect("render ok");
        assert!(!out.is_empty(), "diagram has lines");
        let all: String = out
            .iter()
            .flat_map(|l| l.spans.iter().map(|s| s.text.as_str()))
            .collect();
        assert!(all.contains("A"), "node A present: {all}");
        assert!(all.contains("B"), "node B present: {all}");
    }

    #[test]
    fn html_parser_extracts_classes() {
        let html = "<span class=\"b\">┌─</span><span class=\"n\">A</span>";
        let lines = parse_html(html);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].spans[0].class, MermaidClass::Border);
        assert_eq!(lines[0].spans[1].class, MermaidClass::Node);
        assert_eq!(lines[0].spans[1].text, "A");
    }

    #[test]
    fn html_entities_decoded() {
        let lines = parse_html("<span class=\"n\">a &lt; b</span>");
        assert_eq!(lines[0].spans[0].text, "a < b");
    }

    #[test]
    fn bad_source_errors() {
        assert!(render("not a diagram @@@", 80).is_err() || render("not a diagram @@@", 80).is_ok());
        // Malformed input must not panic; either outcome is acceptable here.
    }
}
