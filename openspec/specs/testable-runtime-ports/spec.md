# testable-runtime-ports Specification

## Purpose
TBD - created by archiving change remediate-architecture-audit. Update Purpose after archive.
## Requirements
### Requirement: 主循环通过 typed input 与 effect 驱动
客户端运行时 MUST 提供可独立测试的 controller：接收 bridge frame、terminal event、animation deadline 和 frame deadline 等 typed input。页面开关等纯状态变化 MUST 作为 controller 内部 action 在 scoped state guard 内消费；只有发送消息、持久化完整配置/状态、写剪贴板、请求绘制或退出等锁外工作才能返回为 typed effect，且每个 effect MUST 携带执行所需的完整 payload。

#### Scenario: 脚本化终端与 bridge 事件
- **WHEN** 测试按顺序向 controller 输入 bridge welcome、用户按键和 session switch
- **THEN** controller 确定性地产生预期状态变化和 outbound effects，无需创建真实终端或 WebSocket

#### Scenario: Effect 在锁外执行
- **WHEN** controller 产生需要 await、剪贴板或文件 I/O 的 effect
- **THEN** 状态 guard 已在 effect executor 执行该 effect 前释放，executor 无需持有或重新借用 controller 的可变 UI 状态

#### Scenario: 页面状态动作不泄漏到 executor
- **WHEN** terminal handler 请求打开或关闭 Input Page
- **THEN** controller 在 guard 内消费该内部 action，外部 effect executor 不接收页面 session，也不存在被静默忽略的 effect

#### Scenario: 配置持久化携带快照
- **WHEN**页面修改产生配置保存请求
- **THEN** `PersistConfig` effect 携带完整配置快照，配置 port 可在锁外直接保存而无需读取 controller 状态

### Requirement: 生产 runner 保持事件驱动与公平预算
抽取 controller 后，生产 runner MUST 继续使用 EventStream 直接唤醒、独立交互/内容/动画 deadline，以及入站数量和时间双预算；不得恢复固定 ticker 或每事件绘帧。

#### Scenario: 空闲运行
- **WHEN** 没有输入、bridge 消息、动画或到期绘制请求
- **THEN** runner 不周期性唤醒或绘制

#### Scenario: Bridge backlog 与输入竞争
- **WHEN** bridge 持续积压消息且终端输入到达
- **THEN** runner 在既定 batch 条数或时间预算后让出执行权，并处理输入及到期帧

### Requirement: 外部运行时能力具有窄 seam
终端事件/绘制、bridge transport、剪贴板、配置持久化和 clock MUST 可由生产适配器与脚本化测试实现替换，业务 handler MUST 不直接构造这些基础设施。

#### Scenario: 剪贴板失败
- **WHEN** 测试 clipboard port 返回失败
- **THEN** controller 产生与生产行为一致的可见错误并退出 copy mode，无需访问系统剪贴板

#### Scenario: Bridge 断开
- **WHEN** scripted transport 报告 inbound closed
- **THEN** runtime 进入 fatal shutdown 路径、恢复终端并返回断开原因

### Requirement: Launcher 生命周期通过 ports 验证
Launcher MUST 通过可替换的 probe、process、clock/sleep 和 lock-store ports 实现 acquire/release；Windows 生产适配器 MUST 继续按进程树终止 `cmd /C` shim 及 Node 子进程。

#### Scenario: 正计数 stale lock
- **WHEN** lock store 含 `instances > 0` 但 probe 返回服务不可达
- **THEN** launcher 删除 stale lock、重新启动服务并写入新的所有权记录

#### Scenario: 最后实例退出
- **WHEN** 最后一个受管 TUI release 且 process port 成功终止并回收服务树
- **THEN** launcher 删除 lock 并返回 `true`

#### Scenario: 终止失败可重试
- **WHEN** 最后实例 release 但 process port 无法确认服务树已停止
- **THEN** launcher 保留 `instances: 0` 的 lock、返回 `false`，供下一次 acquire 重新 probe

### Requirement: 关键调度路径具有确定性回归测试
测试套件 MUST 覆盖 queued prompt、copy movement/expand、Input Page effect、approval/question 优先级、animation deadline、session switch 和 terminal restore，并验证不会在 scrutinee 或 effect 执行期间持有可重入状态锁。

#### Scenario: 队列自动派发
- **WHEN** agent 从 running 变为 idle 且队列非空
- **THEN** controller 原子取出一个 prompt、标记 Thinking、释放状态锁后产生一个 Input send effect
