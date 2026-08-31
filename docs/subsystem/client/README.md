# Rust client documentation

> Status: Current

The Rust side contains DSH and Pi executable adapters in `crates/e-dsh` and `crates/e-pi`, plus the kernel-neutral frontend library in `crates/e-tui`.

- [Architecture](architecture.md) — maintained ownership boundaries, state/rendering invariants, interaction rules, and runtime discipline.
- [Generated wire protocol](../../protocol.md) — the adapter-facing WebSocket reference; edit the canonical JSON contract rather than this generated file.

Completed migration designs and baselines live under [history/rust-client-refactor](../../history/rust-client-refactor/). They are context only.
