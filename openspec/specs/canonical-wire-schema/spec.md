# canonical-wire-schema Specification

## Purpose
TBD - created by archiving change remediate-architecture-audit. Update Purpose after archive.
## Requirements
### Requirement: Wire contract 拥有所有派生协议事实
`bridge/protocol-contract.json` MUST 是 protocol version、容量、surface roster、client/server message roster 和关键 payload shape 的唯一手写事实源；package compatibility metadata、Rust 常量、conformance fixtures 与协议文档 SHALL 由其生成或在测试中强制等值。

#### Scenario: 同步当前版本
- **WHEN** canonical contract 的 `protocolVersion` 增加以支持 reasoning effort 字段（v5 → v6）
- **THEN** Rust `WIRE_PROTOCOL_VERSION`、bridge `PROTOCOL_VERSION`、`bridge/package.json` 的 `wireProtocol` 和生成文档全部等于该版本

#### Scenario: 派生文件过期
- **WHEN** contract 修改后未重新生成 package metadata、fixtures 或文档
- **THEN** 协议生成检查 MUST 失败并列出过期文件

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

### Requirement: 协议文档不重复维护消息语法
生成的 `docs/protocol.md` MUST 从 contract 输出版本、限制、roster 和字段 shape；bridge 源码注释不得维护会与 contract 漂移的完整消息清单。

#### Scenario: Login 消息重命名
- **WHEN** contract 定义 `login-set-api-key`、`login-proxy-create` 和 `login-proxy-delete`
- **THEN** 生成文档展示这些真实消息名，源码中不存在旧 `login-set{field,value}` 契约声明

### Requirement: Reasoning effort schema facts
Contract MUST 描述 model 记录的 reasoning metadata、当前选择的 `reasoningEffort` 和 `model-set.reasoningEffort` 字段；所有新字段 MUST 为 optional 且 camelCase，旧 peer 缺省时 SHALL 遵守向后兼容默认值。

#### Scenario: Model reasoning metadata 跨边界
- **WHEN** Node 产生带 `reasoning.efforts` / `reasoning.defaultEffort` 的 model frame
- **THEN** Rust 可按 contract 描述解析，缺失时 `reasoning` 为 None

#### Scenario: model-set 携带 reasoningEffort
- **WHEN** Rust 序列化 `ModelSet { provider, model, reasoning_effort }`
- **THEN** 生成的 conformance sample 与 contract 的 camelCase 字段名、类型和 optional 语义一致，Node dispatcher 可接受

#### Scenario: 旧 model-set 仍有效
- **WHEN** 一个未升级的 client 发送不带 `reasoningEffort` 的 `model-set`
- **THEN** bridge 仍接受并将 reasoningEffort 视为未指定（省略）

