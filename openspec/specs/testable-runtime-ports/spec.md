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
The shared runtime port model SHALL execute deferred Preview work outside frontend state guards and return completion facts carrying request ID, key, revision, and result. Scripted tests MUST be able to order target changes and completions arbitrarily, and each executable adapter MUST be able to supply the external resolver without changing controller behavior.

#### Scenario: Late completion is scripted
- **WHEN** a test requests Preview A, selects B, then delivers completion A
- **THEN** A may enter cache but the visible state remains targeted at B without any real asynchronous task

#### Scenario: Completion requests a frame
- **WHEN** a matching Preview completion changes visible state
- **THEN** the returned dirty/action state schedules drawing through the shared frame scheduler rather than drawing directly

### Requirement: Main loop remains the async and terminal composition root
`e_tui::runtime` SHALL own provider-neutral terminal lifecycle, terminal event routing, synchronized frame submission, frame scheduling, and controller mechanics without creating an async executor or agent process. Each executable adapter SHALL remain the async composition root for its transport, bounded channels, inbound fairness budget, external effect implementations, and provider lifecycle. Calling `TuiApp` state, input, and render APIs in isolation MUST NOT initialize a terminal, executor, filesystem, clipboard, or provider process.

#### Scenario: UI library is updated in isolation
- **WHEN** a unit test calls `TuiApp::update`, `handle_input`, and render methods
- **THEN** no Tokio runtime, DSH process, filesystem, clipboard, or terminal lifecycle is required

#### Scenario: Frontend core is updated in isolation
- **WHEN** a unit test calls `TuiApp::update`, `handle_input`, and render methods without constructing `e_tui::runtime` terminal facilities
- **THEN** no Tokio runtime, terminal lifecycle, filesystem, clipboard, DSH service, or Pi process is required

#### Scenario: Construct a production runner
- **WHEN** either executable starts interactive operation
- **THEN** it creates the shared terminal/runtime facilities and composes them with its own transport and external effect ports

#### Scenario: Restore after an adapter failure
- **WHEN** either provider transport or effect executor terminates with a fatal error
- **THEN** the shared terminal owner performs the same idempotent restoration path before the executable exits

### Requirement: Runner scheduling policy has one shared owner
`e_tui::runtime` SHALL own the normalized animation minimum, inbound item and time budgets, deadline helpers, budget admission, and streaming-delta classification used by every executable adapter. Adapter runners MUST retain control of their provider inbound streams but MUST NOT maintain independent copies of these frontend scheduling decisions.

#### Scenario: Scheduling policy changes
- **WHEN** a frontend fairness limit, animation minimum, or normalized streaming admission rule changes
- **THEN** both DSH and Pi runners consume the changed value or behavior from the same `e_tui::runtime` implementation without parallel adapter edits

#### Scenario: Provider inbound streams remain independent
- **WHEN** DSH receives WebSocket frames and Pi receives RPC process records
- **THEN** each executable keeps its own provider `tokio::select!` and normalization path while applying the shared budget and deadline policy

#### Scenario: Idle and backlog behavior stays equivalent
- **WHEN** either adapter is idle or its provider continuously supplies inbound events
- **THEN** it preserves zero periodic idle wakeups and yields at the same shared count or time budget for terminal input and expired deadlines

### Requirement: Common frontend actions use one ordered executor
`e_tui::runtime` SHALL provide one provider-neutral executor for ordered `UiAction` sequences. The executor MUST use adapter-supplied ports for agent transport, configuration, clipboard, Preview, and clock work; it MUST return normalized completion facts, quit state, and fatal transport errors without borrowing frontend state or constructing provider infrastructure.

#### Scenario: Equivalent action is executed by either adapter
- **WHEN** DSH and Pi receive the same persistence, clipboard, Preview, draw, or quit action
- **THEN** both use the shared executor and produce the same normalized completion and scheduling semantics while their own port performs external work

#### Scenario: Agent requests preserve order
- **WHEN** an action sequence interleaves agent requests with local external effects
- **THEN** the shared executor invokes the supplied agent port and other effect ports in original sequence order

#### Scenario: External work occurs after state release
- **WHEN** the controller returns an action that requires await, filesystem, clipboard, Preview, or provider transport work
- **THEN** the adapter calls the shared executor only after releasing frontend state guards

#### Scenario: Provider infrastructure stays outside the frontend package
- **WHEN** the shared executor dispatches an agent or external-effect action
- **THEN** DSH/Pi transport types, platform paths, clipboard implementations, Windows FFI, and process or service lifecycle remain implemented in the owning executable adapter

### Requirement: Shared runtime ports serve every adapter
`e_tui::runtime` SHALL define provider-neutral terminal, effect, and clock seams that can be implemented by each executable adapter and replaced by scripted tests. Shared handlers MUST consume normalized frontend values and MUST NOT construct DSH, Pi, filesystem, clipboard, or process implementations directly.

#### Scenario: Execute the same frontend action through either adapter
- **WHEN** DSH and Pi runners receive the same owned clipboard, persistence, Preview, draw, or exit action
- **THEN** both runners dispatch it through the shared runtime contract while their adapter-owned port performs the external operation

#### Scenario: Script an external failure
- **WHEN** a scripted adapter port returns a clipboard, persistence, or Preview failure
- **THEN** the shared runtime produces the same normalized visible completion behavior without accessing a production service

### Requirement: Adapter runners preserve equivalent scheduling policy
Both executable runners MUST use the shared frame scheduler and the same interactive, content, animation, and idle admission rules. Each runner SHALL retain control of its provider inbound stream and MUST yield after the configured item or time budget so terminal input and expired deadlines are not starved.

#### Scenario: Idle operation
- **WHEN** no terminal input, provider event, animation, or frame deadline is pending
- **THEN** neither runner periodically wakes or draws through a fixed ticker

#### Scenario: Provider backlog competes with input
- **WHEN** either provider continuously supplies inbound events while terminal input arrives
- **THEN** the runner yields at the shared count or time budget and processes input and expired frames

