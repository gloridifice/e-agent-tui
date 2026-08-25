# testable-runtime-ports Specification

## Purpose
TBD - created by archiving change remediate-architecture-audit. Update Purpose after archive.
## Requirements
### Requirement: 主循环通过 typed input 与 effect 驱动
The client runtime MUST provide a testable controller in which `e-tui` receives normalized `AgentEvent` and `InputEvent` values and returns complete owned `UiAction` values. Pure page, focus, Reading, Preview-selection, and other state changes MUST be consumed synchronously inside the scoped state guard; only agent I/O, persistence, clipboard writes, Preview resolution, draw requests, exit, and other external work may cross to the `e-dsh` executor. Every action MUST carry the complete payload required for execution.

#### Scenario: 脚本化终端与 bridge 事件
- **WHEN** a test supplies a normalized welcome event, user key, and session switch in sequence
- **THEN** `TuiApp` deterministically produces the expected state changes and outbound actions without a real terminal, WebSocket, or DSH protocol value

#### Scenario: Effect 在锁外执行
- **WHEN** an update produces an action requiring await, clipboard, filesystem, or agent I/O
- **THEN** the UI state guard is released before `e-dsh` executes the action and the executor does not borrow mutable `TuiApp`

#### Scenario: 页面状态动作不泄漏到 executor
- **WHEN** input requests opening or closing an Input Page, entering Reading View, moving a cursor, or selecting an inline Preview
- **THEN** `e-tui` consumes the transition synchronously and the external executor receives no internal state object

#### Scenario: 配置持久化携带快照
- **WHEN** a page modification requests configuration persistence
- **THEN** the persistence action carries a complete configuration snapshot that can be saved without reading controller state

### Requirement: 生产 runner 保持事件驱动与公平预算
抽取 controller 后，生产 runner MUST 继续使用 EventStream 直接唤醒、独立交互/内容/动画 deadline，以及入站数量和时间双预算；不得恢复固定 ticker 或每事件绘帧。

#### Scenario: 空闲运行
- **WHEN** 没有输入、bridge 消息、动画或到期绘制请求
- **THEN** runner 不周期性唤醒或绘制

#### Scenario: Bridge backlog 与输入竞争
- **WHEN** bridge 持续积压消息且终端输入到达
- **THEN** runner 在既定 batch 条数或时间预算后让出执行权，并处理输入及到期帧

### Requirement: 外部运行时能力具有窄 seam
Terminal input/drawing, agent transport, clipboard access, configuration persistence, Preview resolution, and clock behavior MUST be replaceable by production adapters and scripted tests. UI handlers MUST NOT construct these infrastructure implementations directly.

#### Scenario: 剪贴板失败
- **WHEN** a scripted clipboard port returns failure for a Reading View copy action
- **THEN** the controller produces the same visible error as production and exits Reading View without accessing the system clipboard

#### Scenario: Bridge 断开
- **WHEN** scripted transport reports that inbound communication closed
- **THEN** the runtime enters fatal shutdown, restores the terminal, and returns the disconnection reason

#### Scenario: Preview resolver fails
- **WHEN** a scripted Preview resolver returns a bounded error for the current request
- **THEN** `e-tui` displays the matching Preview error through a completion event without direct filesystem or kernel access

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
The test suite MUST cover queued prompts, Reading Block/Item movement, semantic copy, Input Page actions, approval/question precedence, Preview completion races, animation deadlines, session switching, and terminal restoration. It MUST also verify that no reentrant state lock is held in a scrutinee, resolver, or action executor.

#### Scenario: 队列自动派发
- **WHEN** the agent transitions from running to idle while the prompt queue is non-empty
- **THEN** the controller atomically removes one prompt, marks Thinking, releases the state lock, and produces one agent-input action

#### Scenario: Reading 与 Preview 调度
- **WHEN** scripted input moves the Reading cursor and a deferred Preview result arrives afterward
- **THEN** updates occur deterministically in event order and any external work or draw request executes after the state guard is released

### Requirement: Preview resolution races are deterministic through ports
The runtime port model SHALL execute deferred Preview work outside `e-tui` and return completion facts carrying request ID, key, revision, and result. Scripted tests MUST be able to order target changes and completions arbitrarily.

#### Scenario: Late completion is scripted
- **WHEN** a test requests Preview A, selects B, then delivers completion A
- **THEN** A may enter cache but the visible state remains targeted at B without any real asynchronous task

#### Scenario: Completion requests a frame
- **WHEN** a matching Preview completion changes visible state
- **THEN** the returned dirty/action state schedules drawing through the existing frame scheduler rather than drawing directly

### Requirement: Main loop remains the async and terminal composition root
`e-dsh` SHALL retain the Tokio runtime, bounded channels, fair inbound budgets, independent deadlines, terminal setup/restoration, DSH reader/writer tasks, and Preview resolver tasks. `e-tui` MUST NOT create an executor or own terminal lifecycle.

#### Scenario: UI library is updated in isolation
- **WHEN** a unit test calls `TuiApp::update`, `handle_input`, and render methods
- **THEN** no Tokio runtime, DSH process, filesystem, clipboard, or terminal lifecycle is required

