## Context

Preview currently stores one of several content-shaped values (`Path`, `Command`, `PlainText`, `Diff`, and others). A `tool/call` installs a call-derived value, but a non-empty `tool/result` later overwrites it with `PlainText(output)`. This loses the tool name and primary input exactly when output becomes available. It also forces DSH-specific argument heuristics to manufacture strings such as `- old\n+ new` before the Preview renderer knows whether the event supplied a real mutation presentation.

The desired visual grammar is:

```text
<tool name>              # Umber

<primary content>        # tool-specific semantic colors

<optional secondary>     # tool-specific semantic colors
```

DSH research establishes the following constraints:

- Durable `tool/call` records contain `name` and a JSON-encoded `arguments` string. Durable `tool/result` records contain bounded content and optional opaque `meta`.
- The newer DSH `edit` tool stores DSH-computed contextual hunks in result `meta.diffs`, each as `{ path, oldText, newText }`. Those hunks can be presented without reading a file or running a client diff algorithm.
- `str_replace_editor` exposes call-time `old_str`/`new_str` for `str_replace`, but has no `presentResult`; therefore replace is feasible as the requested call-time hunk, not as a reconstructed applied contextual diff.
- `str_replace_editor` insert exposes `new_str` and `insert_line`, but DSH supplies no before-image. It can only be an addition-only hunk anchored to the supplied line.
- DSH's API proxy can transiently attach a host-computed `ToolEventView` to mux/history entries, but the current bridge forwards durable raw session events and does not forward that view. Depending on the transient view would require a wire and snapshot/history migration.
- Bridge payload trimming already removes read output and bounds other tool output. Result metadata is also bounded; malformed or trimmed metadata must degrade safely.

## Goals / Non-Goals

**Goals:**

- Preserve a tool's name, primary input, settled metrics, and optional output in one correlated Preview value.
- Keep DSH parsing in `e-dsh` and expose only typed, provider-neutral preview facts to `e-tui`.
- Match the requested Ferra palette semantics using existing theme roles: Umber from `activity.label`, Bark from `activity.detail`/`surface.muted_text`, Mist from `surface.primary_text`, and Coral from `input.prompt`/the palette alias.
- Render terminal output safely as two-tone styled text while preserving bold and italic.
- Present mutation data supplied by the event without filesystem reads or an LCS/diff computation.
- Produce equivalent Preview values for live events, snapshot replay, backward history, and out-of-order tool call/result halves.

**Non-Goals:**

- Changing transcript ActivityRow layout, file-call folding, reasoning/thinking rendering, Preview selection policy, or Preview pane geometry.
- Reconstructing file contents from disk, expanding trimmed bridge payloads, or claiming a retained output tail is complete.
- Forwarding DSH's transient `ToolEventView` in this change.
- Adding syntax highlighting for command output or arbitrary JSON beyond semantic two-tone ANSI and stable pretty printing.
- Making `str_replace_editor` insert look like a contextual applied diff when DSH did not provide a before-image.

## Decisions

### 1. Add one semantic `Tool` Preview content variant

Add `PreviewContent::Tool(ToolPreview)` while retaining specialized non-tool variants such as `Diff`, `Reasoning`, `Markdown`, and links. `ToolPreview` owns a display name, a typed primary body, optional secondary body, and bounded/truncation metadata. Suggested provider-neutral bodies are:

```rust
pub struct ToolPreview {
    pub name: String,
    pub primary: ToolPreviewPrimary,
    pub secondary: Option<ToolPreviewSecondary>,
}

pub enum ToolPreviewPrimary {
    Location { path: String, lines: Option<LineSelection> },
    Command { command: String, metrics: ToolMetrics },
    Search { query: String, path: Option<String> },
    Json { source: String, truncated: bool },
}

pub enum ToolPreviewSecondary {
    Terminal { output: String, truncated: bool },
}
```

The model carries semantic roles and source text, never Ratatui `Style`, terminal-width wrapping, or DSH raw JSON. The renderer maps semantic roles to the active theme. This keeps tool structure stable across themes and allows wrapping to remain Preview-region-owned.

Alternatives considered:

- **Embed pre-styled Ratatui lines in `PreviewContent`:** rejected because width/theme-dependent render products would enter domain/cache state.
- **Concatenate one Markdown/plain-text string:** rejected because it cannot safely preserve ANSI modifiers or express exact theme roles without inventing a private markup language.
- **Replace all existing Preview variants with a universal document AST:** rejected as unnecessary scope; diff, reasoning, and ordinary Markdown already have correct specialized behavior.

### 2. Normalize a preview seed at the DSH adapter boundary

`normalize_tool_call` will build a provider-neutral preview seed in addition to the existing capability/reference used by transcript projection. The seed retains a Preview-only display name so transcript labels do not change:

- exact `pwsh`/`powershell` -> the original tool name (`pwsh` stays `pwsh`, `powershell` stays `powershell`); `bash`/`cmd`/`sh`/`shell` likewise keep their names, and `command` is the fallback for any other Command-capability name;
- exact `bash` -> `bash`;
- exact `command` -> `command`;
- read/view/create/search capabilities -> their canonical capability names;
- unsupported tools -> the original tool name.

Known argument schemas become typed primary values. Unsupported arguments are parsed and pretty-printed in `e-dsh`, bounded by a fixed character/byte budget, and passed to `e-tui` as inert JSON source. Invalid JSON is shown as bounded raw text in the same JSON body. `e-tui` never inspects DSH tool names or argument keys.

Read/view line notation is normalized as follows:

- no known range: `path`;
- one line: `path:N`;
- closed range: `path:N-M`;
- open range: `path:N-`.

For `read`, derive this from `offset`/`limit`; for `str_replace_editor.view`, derive it from `view_range`. Paths use the existing workspace-relative normalization.

Search uses `pattern` or `query` as the quoted first row and `path`/workspace as an optional `at "…"` second row. If the minimum known schema is absent, the call uses the JSON fallback instead of displaying misleading empty labels.

### 3. Correlate call and result into one revisioned Preview

Tool projection will retain bounded preview state keyed by call ID alongside the existing call-to-row correlation. Applying a call creates the seed. Applying a result enriches the same seed rather than replacing it with plain output:

- command/bash/pwsh: add/update final line count, truncation marker, duration, and terminal secondary output;
- read/view/create/search/generic: retain the primary and no secondary output, per the requested table;
- edit/replace/insert: install or update the specialized diff Preview;
- ignored interaction tools continue to create no transcript or Preview target.

The existing `PreviewKey("tool:<call-id>")` remains stable. Each event uses its sequence as `PreviewRevision`, so same-target settlement refreshes content while preserving Preview scroll. Pending result halves continue to stage until the call arrives, and history reconstruction produces the same settled value as live processing.

Metrics are sourced from the canonical `ActivityRow` settlement fields, not recomputed in the Preview renderer. A trimmed result displays a qualified line count (for example `lines 18+`) and never claims completeness. This change guarantees exact settled metrics; it does not add a new high-frequency Preview revision for every live duration animation tick.

### 4. Use event-provided mutation facts; never compute file diffs

Replace the current synthesized `ToolReference::Diff(format!("- {old}\n+ {new}"))` path with typed mutation payloads:

1. Prefer an event-provided unified diff/patch string when a supported event field contains one; render it verbatim.
2. For DSH `edit` result metadata, parse `meta.diffs` into ordered hunk pairs and use those DSH-computed contextual fragments.
3. For call-time edit/replace schemas, preserve the event's old/new fragments as one requested hunk.
4. For insert, preserve `new_str` as an addition-only hunk and `insert_line` as its anchor; do not invent removed/context lines.
5. If required mutation fields are missing, keep the path or generic tool Preview instead of manufacturing a diff.

The diff renderer may prefix already-separated old lines with `-` and new lines with `+` and apply existing removed/added styles. This is linear presentation formatting, not a diff algorithm: it performs no file read, before/after comparison, hunk search, or line matching.

`str_replace_editor.str_replace` is therefore supported, with one limitation: after success the call-time requested hunk remains because DSH does not persist an applied result-time contextual hunk for that tool. The newer DSH `edit` tool can replace its pending hunk with the applied `meta.diffs` hunks.

Create intentionally remains the common-format `create` + path Preview even when DSH could describe it as an all-added file diff.

### 5. Parse ANSI through a bounded terminal parser and map to two tones

Use a small dedicated Preview component backed by the `vte` parser rather than forwarding escape bytes or implementing a partial escape scanner by hand. The performer emits inert styled spans:

- default/uncolored text -> Umber;
- text with an explicit ANSI foreground color -> Bark;
- SGR bold and italic -> corresponding Ratatui modifiers;
- reset codes -> reset color/modifier state;
- backgrounds, cursor movement, OSC, hyperlinks, title changes, erasure, and unsupported controls -> ignored/stripped;
- CR/LF are normalized into safe Preview lines.

Parsing runs only over the already bounded result output. Wrapped rows continue through `wrap_line`, preserving span modifiers. Tests include standard, bright, 256-color, RGB, reset, nested bold/italic, malformed/truncated escape sequences, OSC, and control-sequence injection.

Alternative: `ansi-to-tui` was rejected because it retains the source palette instead of enforcing the requested two-color semantic map.

### 6. Render exact common-format spacing and existing theme semantics

`ui/region/preview.rs` delegates `ToolPreview` to a component that emits:

1. tool name in `theme.activity.label` (Umber in Ferra);
2. primary body directly on the following row, with no blank row between name and primary;
3. only when secondary exists, one empty row followed by secondary.

Per-tool primary styling:

- read/view location: Mist, with a Bark line/range suffix if split into spans;
- command/bash/pwsh: `$` Coral, command Mist, metrics Bark on the next row;
- create path: Mist;
- search: quoted query Mist; `at` Bark and quoted path Mist on the next row;
- generic JSON: stable indentation in Mist with punctuation/metadata allowed to use Bark, but no DSH-specific key interpretation.

No empty secondary section or trailing separator is rendered.

### 7. Add a distinct muted-Markdown semantic for injected context

Add `PreviewContent::MutedMarkdown` (or an equivalently named semantic variant) and reuse the existing full Markdown materializer while forcing foregrounds to Bark and preserving Markdown modifiers. Context/prompt-injection `ContentCard` fallback chooses this variant. `PreviewContent::Reasoning` and its current gray Markdown path remain unchanged; the new variant prevents context from being mislabeled as reasoning in the model.

## Risks / Trade-offs

- **[DSH metadata shape changes]** Typed `meta.diffs` parsing may stop matching after a DSH upgrade. -> Fail soft to call-time/path/JSON Preview, keep the parser bounded, and add fixtures plus the deployed DSH upgrade gate.
- **[Replace is requested, not applied]** `str_replace_editor` has no result presenter/meta. -> Clearly retain the call-time requested hunk after settlement; never imply contextual applied data that was not supplied.
- **[Insert lacks a before-image]** An addition-only preview has less context than a full diff. -> Show the supplied anchor and only `+` lines; do not read the file or synthesize context.
- **[ANSI parser attack surface]** Tool output may contain hostile terminal controls. -> Use `vte`, accept only text/newline plus selected SGR state, strip every other control, and test OSC/cursor injection.
- **[Generic arguments can be large]** Pretty JSON can inflate a call payload. -> Bound before storing, mark truncation visibly, and retain existing frame/payload caps.
- **[Preview cache staleness]** Reusing the call key with a cached revision could show old content. -> Revision every call/result update by event sequence and preserve current request-id/key/revision race checks.
- **[History halves arrive out of order]** Result metadata may be observed before its call. -> Stage the bounded typed result preview with the existing pending-result mechanism and finalize once the call arrives.
- **[Theme drift]** Hard-coded palette colors would break custom themes. -> Reference existing semantic theme roles; add no required theme schema fields.

## Migration Plan

1. Add characterized Preview renderer tests for current reasoning, path, command, diff, and fallback behavior.
2. Introduce the provider-neutral tool Preview and mutation types without selecting them yet.
3. Extend DSH parsing/adapter normalization, including bounded generic JSON, line ranges, shell-name mapping, and typed result `meta.diffs`.
4. Correlate call/result Preview state and switch known tools to the structured variant; retain safe fallback for malformed/unknown data.
5. Add the two-tone ANSI component and wire command secondary output.
6. Switch mutation tools to event-provided diff fragments and context cards to muted Markdown.
7. Add TestBackend regression coverage for content, spacing, colors, modifiers, wrapping, trimming, settlement, and history replay; run scoped Rust tests and the protocol sync check if the parser fixtures touch generated wire artifacts.
8. Update `docs/client.md` and `docs/design.md`; no README change is expected.

Rollback is a source revert: no persisted client data or config schema changes. Existing durable events remain readable because the new typed preview fields are derived during replay.

## Open Questions

- Whether a future follow-up should forward DSH's ephemeral `ToolEventView` through the bridge to gain richer provider-authored read/search/web cards. It is deliberately not required for this change.
- Whether running command Preview metrics should receive animation-tick revisions. The initial design guarantees final metrics and avoids a new Preview cache churn path.
