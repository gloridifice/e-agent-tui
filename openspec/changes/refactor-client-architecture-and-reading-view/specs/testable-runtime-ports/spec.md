## MODIFIED Requirements

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

### Requirement: 关键调度路径具有确定性回归测试
The test suite MUST cover queued prompts, Reading Block/Item movement, semantic copy, Input Page actions, approval/question precedence, Preview completion races, animation deadlines, session switching, and terminal restoration. It MUST also verify that no reentrant state lock is held in a scrutinee, resolver, or action executor.

#### Scenario: 队列自动派发
- **WHEN** the agent transitions from running to idle while the prompt queue is non-empty
- **THEN** the controller atomically removes one prompt, marks Thinking, releases the state lock, and produces one agent-input action

#### Scenario: Reading 与 Preview 调度
- **WHEN** scripted input moves the Reading cursor and a deferred Preview result arrives afterward
- **THEN** updates occur deterministically in event order and any external work or draw request executes after the state guard is released

## ADDED Requirements

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
