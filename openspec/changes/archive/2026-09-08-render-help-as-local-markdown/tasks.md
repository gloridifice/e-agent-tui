## 1. Local help content and transcript insertion

- [x] 1.1 Add a provider-neutral Markdown help generator using the built-in and current integrated command catalogs.
- [x] 1.2 Add a local Markdown transcript insertion method with Markdown-owned provenance allocation and cache invalidation.

## 2. Command behavior

- [x] 2.1 Route `/help` to the local Markdown insertion path with no outbound agent request while preserving the independent Ctrl+H overlay.

## 3. Regression coverage

- [x] 3.1 Add focused command tests for local-only output, Markdown shape, and integrated-command catalog behavior.
- [x] 3.2 Add a TestBackend regression that verifies representative Markdown help content and styling in the message pane.
- [x] 3.3 Run focused e-tui tests and `cargo fmt --all --check`.
