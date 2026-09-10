# Bridge documentation

> Status: Historical
> Authority: None. Former subsystem index; links are retained for historical navigation.

The bridge is a Node.js ESM host-composition plugin that exposes DSH session events and controls to `dshe` over the `/dsh-tui` WebSocket route.

- [Architecture](bridge-architecture.md) — maintained composition, connection, session, trimming, and DSH integration invariants.
- [Generated wire protocol](../../../doco/specs/wire-protocol.md) — readable derivative of [`bridge/protocol-contract.json`](../../../bridge/protocol-contract.json).

Exact module exports, payload shapes, and compatibility versions are governed by source, tests, package metadata, and the canonical contract.
