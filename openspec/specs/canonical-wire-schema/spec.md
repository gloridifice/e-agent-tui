# canonical-wire-schema Specification

## Purpose
TBD - created by archiving change remediate-architecture-audit. Update Purpose after archive.
## Requirements
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

### Requirement: 协议边界继续有界且 typed
扩展 shape contract MUST 不允许任意 payload 越过边界；HostEvent 仍 SHALL 在 Rust wire boundary 解析为 typed `HostEventKind`，bridge frames 仍 MUST 遵守 canonical maximum bytes 和 trimming policy。

#### Scenario: 超大 snapshot 或 history
- **WHEN** 编码后的事件数组超过 `maxFrameBytes`
- **THEN** bridge 只发送可容纳的最新后缀并设置 `truncated` 或 `hasMore`

#### Scenario: 超大单体 frame
- **WHEN** 一个不可分页的 server frame 超过上限
- **THEN** bridge 发送有界 `frame-too-large` error，而不是超限原帧

### Requirement: 协议文档不重复维护消息语法
生成的 `docs/protocol.md` MUST 从 contract 输出版本、限制、roster 和字段 shape；bridge 源码注释不得维护会与 contract 漂移的完整消息清单。

#### Scenario: Login 消息重命名
- **WHEN** contract 定义 `login-set-api-key`、`login-proxy-create` 和 `login-proxy-delete`
- **THEN** 生成文档展示这些真实消息名，源码中不存在旧 `login-set{field,value}` 契约声明

