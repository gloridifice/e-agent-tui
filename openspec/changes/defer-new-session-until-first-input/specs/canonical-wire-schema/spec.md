## MODIFIED Requirements

### Requirement: Wire contract 拥有所有派生协议事实
`bridge/protocol-contract.json` MUST 是 protocol version、容量、surface roster、client/server message roster 和关键 payload shape 的唯一手写事实源；package compatibility metadata、Rust 常量、conformance fixtures 与协议文档 SHALL 由其生成或在测试中强制等值。

#### Scenario: 同步当前版本
- **WHEN** canonical contract 的 `protocolVersion` 增加以支持 `new-input`
- **THEN** Rust `WIRE_PROTOCOL_VERSION`、bridge `PROTOCOL_VERSION`、`bridge/package.json` 的 `wireProtocol` 和生成文档全部等于该版本

#### Scenario: 派生文件过期
- **WHEN** contract 修改后未重新生成 package metadata、fixtures 或文档
- **THEN** 协议生成检查 MUST 失败并列出过期文件

### Requirement: 消息 shape 可由机器验证
Contract MUST 为所有 client messages（包括原子新会话首条输入 `new-input`）和所有非 HostEvent server frames 描述 required/optional 字段、wire 字段名和基本容器/值类型；Rust 与 Node 测试 MUST 验证各自序列化、解析或 shaping 与这些描述一致。

#### Scenario: Client message 字段一致
- **WHEN** 对 `new-input`、`login-proxy-create`、`model-set`、`history` 或其他 client message 生成 conformance sample
- **THEN** Rust 序列化字段与 contract 的 camelCase 名称、required 字段和类型一致，Node dispatcher 可接受该 sample

#### Scenario: Server frame 字段一致
- **WHEN** Node shape/send 逻辑产生 welcome、sessions、commands、login、model 或 error frame
- **THEN** Rust 可按 contract 描述解析该 frame，optional 字段缺失时遵守向后兼容默认值

#### Scenario: Roster 与实现不一致
- **WHEN** Rust enum 或 Node dispatcher 新增消息类型但未加入 contract，或 contract roster 中的类型没有实现
- **THEN** 双方协议覆盖测试 MUST 失败
