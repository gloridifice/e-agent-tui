## 1. Mapping foundation

- [x] 1.1 Add the complete embedded default TOML and typed scope/action/chord parser with overlay, disable, conflict validation and display tests.
- [x] 1.2 Load user mappings in both adapters, retain runtime-only values, and implement startup diagnostics and atomic reload.

## 2. Semantic input migration

- [x] 2.1 Route globals, clipboard, approvals and Reading through effective actions; protect modal state and preserve Windows transport.
- [x] 2.2 Migrate composer editing, idle/working submission, suggestion and history search without legacy fallback.
- [x] 2.3 Migrate page browse/edit/choice, login, settings, resume and question keyboard handling with protected text contexts.

## 3. Presentation and validation

- [x] 3.1 Render effective bindings in help, local help Markdown, status and page hints; update README and client architecture/workflow documentation.
- [x] 3.2 Add and run scoped regression tests for overrides, disabled keys, routing, queues, page state, Reading, reload and terminal input; update the binding gate.
- [x] 3.3 Run OpenSpec validation, workspace formatting, Clippy and architecture checks; resolve introduced failures and record remaining environment limitations.

## 4. Reading cursor fast movement

- [x] 4.1 Add configurable Reading PageUp/PageDown actions equivalent to 15 up/down cursor steps, with Item-mode transitions, viewport following, no global-paging fallback, updated help and scoped regression tests.

## Validation notes

- Reading fast-movement follow-up passed the key_mapping (14), runtime::input (35), and help (5) scoped checks, formatting, strict OpenSpec validation and diff checks. Tests compare fast movement with 15 ordinary cursor steps, including viewport state, Item-to-Block transitions, boundary clamping, remapping, disable, and ordinary/page-owned paging.

- Scoped e-tui checks passed: key_mapping (14), input (124), runtime (90), UI (94), config (11), i18n (4), render (36), settings (16), login (9), and runtime::input (35). Filters overlap; these are not unique-test totals.
- dshe binary tests (13), pie binary tests (2), and recursive architecture tests (10) passed.
- `cargo fmt --all --check`, `git diff --check`, and strict OpenSpec validation passed. `cargo clippy --workspace --all-targets` completed successfully with repository lint warnings; the stricter `-D warnings` run is not clean. New key-mapping modules produce no Clippy diagnostics in the final run.
- No real terminal/OS shortcut-delivery matrix was run. The current terminal binding gate documents the manual checks and macOS Command interception limits; no historical measurements are claimed for the new defaults.
- Stale nonfunctional approval-details and Markdown Enter-expand key hints were removed rather than introducing undocumented actions. Question free-text input has its own `page.question.edit` scope to retain arrow question switching while keeping letters editable.
