# Execution tasks

- [x] 1.1 Implement native dispatch, bounded execution and owner-scoped callbacks.
  - Acceptance: Non-blocking tool, distinct identities, no recursive tool or callbacks after retirement.
- [x] 1.2 Project task state and render compact entries.
  - Dependencies: 1.1
  - Acceptance: Display-only colored squares and work names share the composer rule with existing decorations.
- [x] 2.1 Verify native integration, Rust boundaries and update current documents.
  - Dependencies: 1.1, 1.2
  - Acceptance: Record actual checks and limits; no unrelated test changes.

## Verification
Windows PTY smoke with the built pie, native Pi 1.5.2, an isolated agent directory and a loopback model: initial request omitted the tool, enabling exposed it, Search and Build ran independently without recursive tools, both results triggered parent responses, and a subsequent disabled request omitted the tool. A separate run disabled two active children, observed no resulting parent response, re-enabled, resized to 35 columns and reloaded to off. No live hosted model, Unix process-tree or full-suite verification.

`cargo check --workspace --all-targets`, `cargo fmt --all --check`, `cargo build -p e-pi --bin pie`, `cargo clippy --workspace --all-targets` (warnings), Node syntax checks, `cargo test -p e-pi --test architecture` (2 passed) and `cargo test -p e-tui --lib ui::status` (10 passed).
