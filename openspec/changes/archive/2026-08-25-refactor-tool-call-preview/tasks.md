## 1. Characterize and Extend the Preview Model

- [x] 1.1 Add focused characterization tests for current tool-call/result Preview selection, same-target revision behavior, reasoning Preview, and context fallback before changing the model.
- [x] 1.2 Add provider-neutral structured tool Preview, line-selection, metrics, terminal-secondary, and mutation-hunk types in `e-tui`, keeping Ratatui styles and DSH field names out of the model.
- [x] 1.3 Add a distinct muted-Markdown Preview content kind for injected context while preserving the existing reasoning content kind and cache semantics.
- [x] 1.4 Add the chosen bounded ANSI parser dependency to `crates/e-tui/Cargo.toml` and the workspace lockfile.

## 2. Normalize DSH Tool Preview Facts

- [x] 2.1 Extend the typed host-event tool parser to narrow supported result `meta.diffs` entries and fail soft on absent, malformed, or bridge-trimmed metadata.
- [x] 2.2 Normalize read/view locations and ranges, create paths, filesystem-search query/path values, and Preview-only shell names (`command`, `bash`, `powershell`) in `bridge/adapter.rs`.
- [x] 2.3 Normalize unsupported tool arguments into stable bounded pretty JSON with invalid-JSON and truncation fallbacks.
- [x] 2.4 Normalize unified diff payloads when present, DSH edit result hunks, str-replace requested hunks, and addition-only insert hunks without reading files or invoking a diff algorithm.
- [x] 2.5 Add adapter/parser regression tests covering native `edit`, `str_replace_editor` view/str_replace/insert/create, bash, pwsh, search, unknown tools, malformed metadata, and bounded payloads.

## 3. Correlate Call and Result Preview State

- [x] 3.1 Store a bounded Preview seed with each projected tool call and stage typed result Preview facts when a result arrives before its call.
- [x] 3.2 Merge command result output and canonical ActivityRow metrics into the same `tool:<call-id>` Preview target, retaining primary content and qualifying trimmed line counts.
- [x] 3.3 Keep read/view/create/search/generic calls primary-only, let applied edit metadata replace pending mutation hunks, and retain requested str-replace hunks when no applied result hunk exists.
- [x] 3.4 Preserve stable Preview keys, event-sequence revisions, same-target scroll, surface replacement cleanup, and live/snapshot/history equivalence.
- [x] 3.5 Route context/prompt-injection cards to muted Markdown without changing normal assistant Markdown or reasoning/thinking projection.

## 4. Render Structured Tool Previews

- [x] 4.1 Implement the exact header/blank/primary/optional-blank-secondary layout using existing activity-label, primary-text, muted-detail, and prompt-accent theme semantics.
- [x] 4.2 Render read/view `path[:lines]`, create path, quoted search query/path, command prompt plus `lines x, duration y`, and bounded generic JSON with width-aware wrapping.
- [x] 4.3 Implement the safe two-tone ANSI component: colored runs to Bark-equivalent, uncolored runs to Umber-equivalent, bold/italic preserved, and all other controls stripped.
- [x] 4.4 Render provider-supplied unified diffs verbatim and structured old/new or insertion fragments as linear removed/added rows without computing hunks.
- [x] 4.5 Add TestBackend UI regression tests asserting common-format spacing, names, line ranges, command metrics/output, Ferra colors, ANSI modifiers/control stripping, JSON truncation, diff colors/content, wrapping, and muted Markdown.

## 5. Documentation and Validation

- [x] 5.1 Update `docs/client.md` with the structured Preview contract, correlation/bounding rules, ANSI semantics, and adapter ownership.
- [x] 5.2 Update `docs/design.md` with the per-tool Preview table and the researched DSH edit/str-replace/insert mutation-data limitations; keep README unchanged unless implementation changes user-facing basics beyond this design.
- [x] 5.3 Run the scoped parser, adapter, projection, Preview-state, and UI regression tests added or affected by this change.
- [x] 5.4 Run `cargo fmt --all`, `cargo fmt --all --check`, and `cargo clippy --all-targets`; run the protocol-contract sync check only if implementation changes generated wire fixtures or the contract.
