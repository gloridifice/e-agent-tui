# Current architecture

> Status: Current
> Authority: Implemented system boundaries and navigation. Detailed invariants belong to the linked subsystem documents; exact behavior belongs to specifications, source, tests, and generated contracts.

**e** provides two terminal executables over one provider-neutral frontend:

```text
DSH host <-> bridge/ <-> JSON WebSocket <-> crates/e-dsh (dshe)
                                                |
                                                v
                                           crates/e-tui
                                                ^
                                                |
Pi child <-> JSONL stdio <---------------> crates/e-pi (pie)
```

## Ownership and data flow

- [`bridge/`](../bridge/) composes with the DSH host and exposes session events and controls over WebSocket. Connection/session lifecycle, host integration, and bounded history projection belong to the [bridge architecture](subsystem/bridge/architecture.md).
- [`crates/e-dsh/`](../crates/e-dsh/) owns DSH wire conversion, transport, launcher/setup, and platform effects. Its embedded bridge is a generated mirror of the top-level bridge package.
- [`crates/e-pi/`](../crates/e-pi/) owns the official Pi RPC child process, JSONL framing, native session discovery, RPC conversion, and platform effects. Pi remains authoritative for backend configuration, credentials, resources, and session writes.
- [`crates/e-tui/`](../crates/e-tui/) owns provider-neutral state, interaction, projection, rendering, and shared runtime policy. Both adapters depend on this library, never on each other. Inbound adapter events become normalized `AgentEvent` values; outbound `AgentRequest` values return through the owning adapter. Detailed frontend/runtime boundaries and invariants live in the [client architecture](subsystem/client/architecture.md).
- [`tools/`](../tools/) owns repository generation and verification entry points. The DSH wire authority is [`bridge/protocol-contract.json`](../bridge/protocol-contract.json); [protocol.md](protocol.md) is generated, not a second handwritten contract.

## Task routing

- New changes and current contracts: [Doco skill](../.agents/skills/doco/SKILL.md). OpenSpec is retired and retained only as history; see the [documentation authority policy](README.md#authority).
- Build, dependencies, and release: [development](development.md).
- Bridge deployment and upgrades: [DSH integration](dsh-integration.md).
- Validation and compatibility gates: [testing](testing.md).
- Profiling and measurement: [performance methodology](subsystem/performance/methodology.md).
- User-visible keys and configuration: [key mappings](key-mapping.md).
- Operational diagnosis: [troubleshooting](troubleshooting.md).

See the [documentation index](README.md) for authority, update policy, current specifications, Doco workspace roles, and the separation of current documents from historical material.
