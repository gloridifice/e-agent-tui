# Current architecture

This document records implemented ownership and data flow. Exact behavior belongs to the linked contracts, source, tests, generated artifacts, and command help.

## System

```text
DSH host <-> bridge/ <-> WebSocket <-> crates/e-dsh ─┐
                                                     ├─> crates/e-tui
Pi child <-------- JSONL stdio ------> crates/e-pi ──┘
   ^ public companion extension          |
   └── native-auth refresh/context       └── JSONL stdio ──> Pi SDK auth helper
```

## Rust workspace

- `crates/e-tui` owns provider-neutral state, event projection, interaction, rendering, and runtime policy, including optional parent-child session ordering and resume presentation.
- `crates/e-dsh` owns DSH wire conversion, WebSocket transport, launcher/setup, and external effects.
- `crates/e-pi` owns Pi RPC DTOs, bounded JSONL framing, child lifecycle, native session discovery and ancestry normalization, native fork/clone orchestration, Pi-native authentication integration, and external effects. Authentication uses an embedded public-API companion extension for runtime context/refresh and a lazily started SDK helper for native provider interactions; Pi remains the credential and OAuth owner. The companion also exposes native resource reload; the adapter refreshes catalogs only after that operation settles.
- `e-pi` also owns optional Herdr status reporting through a bounded background CLI worker. It observes runtime and interaction state without changing Pi extensions or claiming native session-restore authority.
- Both adapters depend on `e-tui`; they never depend on each other. Provider payloads do not cross adapter boundaries.
- Runners execute owned frontend effects only after releasing UI state guards. Shared scheduling and input policy remain in `e-tui`.

See [runtime and adapter contracts](specs/runtime-and-adapters.md), [interaction and session contracts](specs/interaction-and-sessions.md), and [presentation contracts](specs/presentation.md).

## Bridge

`bridge/` is the authoritative Node.js DSH plugin. It composes host services, owns socket/session lifecycles, projects bounded history, and routes client controls. `index.js` is composition and socket wiring; domain behavior stays in focused modules. The package copied into `crates/e-dsh/assets/bridge` is generated from this directory.

The hand-written DSH wire authority is `bridge/protocol-contract.json`. [Wire protocol](specs/wire-protocol.md), fixtures, package metadata, and adapter constants are derived from it.

See [bridge contracts](specs/dsh-bridge.md).

## State and effects

Frontend semantic state is provider-neutral. Presentation caches and reveal progress are derived state. Adapters own filesystem, process, clipboard, transport, and provider persistence effects. Execution-history records are normalized before entering shared code and exclude model text, file contents, patches, and raw unknown payloads.

Compaction selection uses one user-wide route file shared by the Pi adapter and DSH bridge; each compaction captures the latest route, independently of session cwd. Filesystem persistence remains adapter-owned and its data schema belongs to `e-tui`.

See [configuration and storage contracts](specs/configuration-and-storage.md).

## Repository operations

Generation and verification entry points live in `tools/`. Build, deployment, testing, troubleshooting, key mapping, and performance procedures live in `readme/`; they are operational guidance rather than architecture or requirement stores.

Doco owns current architecture, contracts, durable decisions, and change packages. Document ownership is defined by [ADR-0001](decisions/0001-document-ownership.md).
