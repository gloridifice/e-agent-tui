## MODIFIED Requirements

### Requirement: 消息 shape 可由机器验证
Contract MUST describe required/optional fields, wire names, and basic container/value types for every client message, including atomic new-session first input `new-input` and the ordered text/image content carried by `input`, `new-input`, and image-capable `command` messages; it MUST also describe every non-HostEvent server frame. Rust and Node tests MUST verify that their serialization, parsing, or shaping matches those descriptions.

#### Scenario: Client message 字段一致
- **WHEN** a conformance sample is generated for `input`, `new-input`, `command`, `login-proxy-create`, `model-set`, `history`, or another client message
- **THEN** Rust serialization uses the contract's camelCase names, required fields, ordered prompt-part records, and value types, and the Node dispatcher accepts that sample

#### Scenario: Mixed prompt content
- **WHEN** a Rust client serializes text and encoded image prompt parts
- **THEN** the wire preserves part order and carries each image's `mediaType`, canonical Base64 `data`, and optional bounded `name`

#### Scenario: Server frame 字段一致
- **WHEN** Node shape/send logic produces a welcome, sessions, commands, login, model, or error frame
- **THEN** Rust parses it according to the contract and applies backward-compatible defaults for absent optional fields

#### Scenario: Roster 与实现不一致
- **WHEN** a Rust enum or Node dispatcher adds a message type not present in the contract, or a contract roster entry has no implementation
- **THEN** protocol coverage tests MUST fail

### Requirement: 协议边界继续有界且 typed
The extended shape contract MUST NOT allow arbitrary payloads across the boundary. Prompt content SHALL be a closed union of typed text and encoded-image records, HostEvent SHALL still parse into typed `HostEventKind` at the Rust wire boundary, and bridge frames MUST continue to obey the canonical maximum bytes and trimming policy.

#### Scenario: 超大 snapshot 或 history
- **WHEN** an encoded event array exceeds `maxFrameBytes`
- **THEN** the bridge sends only the newest fitting suffix and sets `truncated` or `hasMore`

#### Scenario: 超大单体 frame
- **WHEN** a non-pageable server frame exceeds the limit
- **THEN** the bridge sends a bounded `frame-too-large` error instead of the oversized frame

#### Scenario: 超大 image input
- **WHEN** an encoded image makes a client input frame exceed `maxFrameBytes`
- **THEN** the client rejects the request before WebSocket transmission and retains a visible error rather than sending a partial prompt
