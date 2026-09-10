# Rust client documentation

> Status: Historical
> Authority: None. Former subsystem index; links are retained for historical navigation.

The Rust side contains DSH and Pi executable adapters in `crates/e-dsh` and `crates/e-pi`, plus the kernel-neutral frontend library in `crates/e-tui`.

- [Architecture](client-architecture.md) — maintained ownership boundaries, state/rendering invariants, interaction rules, and runtime discipline.
- [Generated wire protocol](../../../doco/specs/wire-protocol.md) — the adapter-facing WebSocket reference; edit the canonical JSON contract rather than this generated file.

Completed migration designs and baselines live under [history/rust-client-refactor](../rust-client-refactor/). They are context only.
