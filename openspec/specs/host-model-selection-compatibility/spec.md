# host-model-selection-compatibility Specification

## Purpose
TBD - created by archiving change remediate-architecture-audit. Update Purpose after archive.
## Requirements
### Requirement: Model selection 安装逻辑只有一个 bridge adapter
Bridge session 创建和冷恢复 MUST 只通过专用 model-selection adapter 安装 provider/model 选择；`compose.js`、`session.js` 或其他模块 MUST 不各自复制 DSH waterfall 实现。

#### Scenario: 创建新会话
- **WHEN** bridge 创建带当前或默认 model selection 的 agent
- **THEN** session setup 通过 adapter 安装 selection，并在 preset mount 前后保持既定组合语义

#### Scenario: 恢复持久化会话
- **WHEN** bridge cold-resume 一个 agent
- **THEN** 同一 adapter 在 resume setup 中安装 selection，避免 persona `{{model}}` 变量缺失

### Requirement: 优先复用稳定的上游实现
若受支持 DSH 版本公开稳定的 `installModelSelection`，adapter MUST 复用该导出；若不存在稳定导出，adapter MAY 保留兼容副本，但 MUST 精确声明已验证 host 版本而不得使用覆盖未验证实现的宽版本范围。

#### Scenario: 上游提供稳定导出
- **WHEN** 当前支持的 DSH package 暴露兼容 helper
- **THEN** adapter 调用该 helper，仓库中不存在本地 waterfall 副本

#### Scenario: 必须保留兼容副本
- **WHEN** 当前 DSH 没有可用的稳定导出
- **THEN** 副本只存在于 adapter 中，package compatibility 精确钉住测试版本并记录升级验证入口

### Requirement: Assembly 与 request 路由契约自动验证
Adapter 测试 MUST 验证 `system-prompt/assemble` 注入 provider/model、`agent/request` 使用该次 assembly 的 selection、reasoning effort 覆盖和 disposer 行为；部署副本 smoke MUST 纳入升级验证命令。

#### Scenario: 中途切换 model
- **WHEN** assembly 捕获 selection A 后，用户在 request 前把 current 改为 selection B
- **THEN** 当前 request 仍使用 assembled selection A，下一次 assembly/request 才使用 B

#### Scenario: Reasoning effort
- **WHEN** selection 指定 reasoning effort 而 host resolved request 含继承值
- **THEN** adapter 使用 selection 值；selection 未指定时不错误保留不适用的继承值

#### Scenario: DSH 升级
- **WHEN** package compatibility 或已测试 DSH 版本发生变化
- **THEN** 自动测试运行 adapter contract 和 deployed bridge smoke；任一 waterfall 签名或行为不兼容都会阻止升级完成

### Requirement: Model 切换保持现有用户语义
架构收敛 MUST 保持 `/model` 更新当前会话 selection、下一次 prompt assembly 生效、状态栏刷新且不修改历史会话既有事件组合的行为。

#### Scenario: 用户选择新模型
- **WHEN** bridge 收到合法 `model-set{provider,model}`
- **THEN** adapter 持有的 mutable current selection 和 agent options 更新，并返回反映新选择的 model frame
