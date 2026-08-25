## MODIFIED Requirements

### Requirement: Wire contract 拥有所有派生协议事实
`bridge/protocol-contract.json` MUST 是 protocol version、容量、surface roster、client/server message roster 和关键 payload shape 的唯一手写事实源；package compatibility metadata、Rust 常量、conformance fixtures 与协议文档 SHALL 由其生成或在测试中强制等值。

#### Scenario: 同步当前版本
- **WHEN** canonical contract 的 `protocolVersion` 增加以支持 reasoning effort 字段（v5 → v6）
- **THEN** Rust `WIRE_PROTOCOL_VERSION`、bridge `PROTOCOL_VERSION`、`bridge/package.json` 的 `wireProtocol` 和生成文档全部等于该版本

#### Scenario: 派生文件过期
- **WHEN** contract 修改后未重新生成 package metadata、fixtures 或文档
- **THEN** 协议生成检查 MUST 失败并列出过期文件

## ADDED Requirements

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
