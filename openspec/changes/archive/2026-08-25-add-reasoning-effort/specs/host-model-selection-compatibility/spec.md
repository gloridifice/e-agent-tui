## MODIFIED Requirements

### Requirement: Model 切换保持现有用户语义
架构收敛 MUST 保持 `/model` 更新当前会话 selection、下一次 prompt assembly 生效、状态栏刷新且不修改历史会话既有事件组合的行为；selection MUST 是完整三元组 `{ provider, model, reasoningEffort? }`，且所有 model/effort 变更 MUST 通过 DSH `session.models` / `session.selectModel` API 与已安装的 model-selection adapter 保持单一事实源。

#### Scenario: 用户选择新模型
- **WHEN** bridge 收到合法 `model-set{provider,model}`（不带 reasoningEffort）
- **THEN** adapter 持有的 mutable current selection 和 agent options 更新为完整三元组，旧模型的 reasoningEffort 被清除，并返回反映新选择的 model frame

#### Scenario: 用户选择推理强度
- **WHEN** bridge 收到合法 `model-set{provider,model,reasoningEffort}`
- **THEN** 同一 selection 以完整三元组更新，且下一次 request 使用该推理强度

#### Scenario: API 拒绝选择
- **WHEN** `session.selectModel` 返回失败（例如不支持的 reasoningEffort）
- **THEN** bridge 不修改本地 selection 或 agent options，并向客户端返回错误帧

## ADDED Requirements

### Requirement: Reasoning effort 事实来自 session API
Effort 的显示与选择 MUST 以 `session.models` 的 `current.reasoningEffort` 和精确 provider/model 路由的 `reasoning` 元数据为事实源；bridge SHALL 在 attach/resume 时于 `welcome` 前完成一次 model/effort hydration，并将返回的 `current` 同步到本地 model-selection adapter。

#### Scenario: Attach 时 hydrate 当前选择
- **WHEN** bridge 创建或恢复一个会话
- **THEN** 在发送 `welcome` 前，bridge 通过 `session.models` 读取 `current` 并同步到 `modelSelections`，使首次 prompt 使用正确三元组

#### Scenario: 跨会话竞态隔离
- **WHEN** model/effort hydration 或 selectModel 跨越 await 后连接已切换到另一会话
- **THEN** 结果不得写入或下发到已切换的连接

#### Scenario: /new 继承完整三元组
- **WHEN** deferred `/new` 期间的 model 或 effort 选择被应用
- **THEN** 新建会话继承 provider + model + reasoningEffort 完整三元组
