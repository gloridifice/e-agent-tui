# acyclic-client-architecture Specification

## Purpose
TBD - created by archiving change remediate-architecture-audit. Update Purpose after archive.
## Requirements
### Requirement: 生产模块依赖图无环
The production module dependency graphs of both Rust packages MUST contain no strongly connected component; test modules and high fan-out composition roots MUST NOT be misclassified as cycles.

#### Scenario: 架构检查通过
- **WHEN** architecture checks scan production imports under `crates/e-dsh/src` and `crates/e-tui/src`
- **THEN** they find zero multi-node strongly connected components in either package and exit successfully

#### Scenario: 禁止反向依赖被发现
- **WHEN** a leaf module adds an upward dependency that forms a cycle
- **THEN** the architecture check MUST fail and report the module paths in that cycle

#### Scenario: 包反向依赖被发现
- **WHEN** `e-tui` declares or imports a dependency on `e-dsh`
- **THEN** the architecture check MUST fail even if each package's internal module graph remains acyclic

### Requirement: 命令目录与执行解耦
Built-in command declarations, catalog merging, sorting, and argument completion MUST be owned by a command-catalog module that does not depend on composer state, `TuiApp`, Reading View, Input Pages, or asynchronous senders. Input and command-execution modules SHALL depend on that catalog in one direction only.

#### Scenario: 输入刷新命令建议
- **WHEN** the composer contains a built-in command or bridge-provided integration-command prefix
- **THEN** input obtains the same candidates, descriptions, sources, and completion forms from the catalog without the catalog reading or mutating interaction state

#### Scenario: 执行本地命令
- **WHEN** the runtime command dispatcher executes a registered built-in command
- **THEN** it uses the action identity returned by the catalog to produce a UI transition or external action, and the catalog does not depend on the dispatcher

### Requirement: 页面公共原语位于叶子层
焦点图、方向键归一化、文本编辑、viewport 和通用页面 outcome/effect MUST 位于 `page_core`；login、settings、model、theme、resume 页面 SHALL 依赖 `page_core`，而 `page_core` MUST 不导入任何具体页面。

#### Scenario: 页面焦点导航
- **WHEN** 任一具体 Input Page 使用方向键或 `hjkl` 移动焦点
- **THEN** 导航由 `page_core` 完成，并保持现有禁用项跳过和稳定 id reconcile 行为

#### Scenario: 文本编辑保留 hjkl
- **WHEN** login、settings 或 resume 页面处于文本编辑状态
- **THEN** `hjkl` 作为普通字符进入编辑器，具体页面无需反向调用 Input Page controller

### Requirement: 布局与复制共享无 UI 反向依赖
Visible-row layout, semantic Reading geometry, and copy provenance MUST be owned by independent presentation-leaf modules. Transcript rendering, Reading View, and clipboard payload selection SHALL consume the same layout and provenance results; semantic navigation and copy logic MUST NOT call the Screen, Pane, or Region renderers.

#### Scenario: 复制当前语义块
- **WHEN** Reading View copies a wrapped Markdown, table, code, Mermaid, user, reasoning, or tool Block
- **THEN** clipboard selection obtains complete original source and current semantic geometry from shared provenance/layout without a `reading/provenance -> render region` dependency

#### Scenario: 宽度变化
- **WHEN** terminal or pane width changes and causes transcript rewrapping
- **THEN** rendering, Reading geometry, and copy provenance invalidate on the same width/generation and reuse the shared layout result

### Requirement: 组合根只负责编排
`main.rs` MAY 依赖多个具体适配器，但 SHALL 只负责生命周期、事件选择、deadline 和效果执行；页面、命令、协议事件或复制行为的业务分支 MUST 位于可独立测试的处理模块。

#### Scenario: 新增页面消息处理
- **WHEN** 新增一个已有协议内的 Input Page 状态刷新分支
- **THEN** 变更落在页面或 bridge-message handler 中，而不需要向 `main.rs` 的事件循环增加页面专用业务逻辑

### Requirement: Kernel boundary is mechanically enforced
Architecture guards SHALL reject DSH protocol names, Pi RPC names, provider transport types, and raw provider event names in `e-tui`. They SHALL keep bridge/setup/launcher behavior in `e-dsh`, Pi process/RPC behavior in `e-pi`, and provider-specific path, persistence, clipboard, Preview I/O, and async effect execution in the owning executable adapter. Provider-neutral terminal coordination, controller logic, scheduling, and runtime port contracts SHALL be owned by `e_tui::runtime`.

#### Scenario: Protocol type leaks into the UI library
- **WHEN** an `e-tui` production module imports a DSH `ServerMessage`, `ClientMessage`, or bridge protocol module
- **THEN** the architecture test fails and identifies the forbidden import

#### Scenario: Protocol type leaks into the frontend library
- **WHEN** an `e-tui` production module imports a DSH `ServerMessage` or `ClientMessage`, a Pi RPC DTO, or either adapter's protocol module
- **THEN** the architecture test fails and identifies the forbidden import

#### Scenario: Provider behavior leaks into the shared runtime
- **WHEN** `e_tui::runtime` imports DSH bridge/setup/launcher behavior, Pi child-process behavior, or provider-specific persistence policy
- **THEN** the architecture test fails and identifies the provider boundary violation

#### Scenario: Provider-neutral terminal code is shared
- **WHEN** terminal routing, frame scheduling, synchronized drawing, or terminal restoration is used by both executables
- **THEN** its production implementation is reachable through `e_tui::runtime` rather than either adapter package

### Requirement: Rendering dependencies point downward
Production rendering imports SHALL follow `Screen -> Pane -> Region -> Component`; Components MUST NOT import Regions, Panes, or the Screen, and Regions MUST NOT import Panes or the Screen.

#### Scenario: Component reaches into a Region
- **WHEN** a leaf Component imports a transcript, composer, status, or Preview Region
- **THEN** the architecture check fails with the upward render dependency

### Requirement: Architecture graph covers nested production modules
The Rust architecture guard SHALL recursively discover production modules under `e-dsh`, `e-pi`, and `e-tui`, assign each source file a stable fully qualified module identity, and include dependencies expressed through `crate`, `self`, and `super` paths. Test-only modules and module-containment declarations MUST NOT create production dependency edges.

#### Scenario: Nested cycle is introduced
- **WHEN** two or more nested production modules depend on one another through a strongly connected path
- **THEN** the architecture check fails and reports the fully qualified module identities in the cycle

#### Scenario: Relative imports resolve to production modules
- **WHEN** a nested module imports another discovered module through `self`, one or more `super` prefixes, a grouped import, or an absolute `crate` path
- **THEN** the architecture graph records an edge to the same canonical target module

#### Scenario: Test-only dependency is ignored
- **WHEN** a dependency exists only inside a `#[cfg(test)]` test module
- **THEN** it does not create a production graph edge or a false cycle

#### Scenario: Nested workspace graph remains acyclic
- **WHEN** the architecture check scans the complete production trees of all three Rust packages
- **THEN** it finds no multi-node strongly connected component and preserves the existing package and rendering-layer boundary assertions

### Requirement: Adapter dependencies do not point sideways
The workspace architecture guards SHALL reject direct or transitive production dependencies from `e-pi` to `e-dsh` and from `e-dsh` to `e-pi`. Shared frontend runtime behavior MUST be reached through `e-tui` rather than through an adapter compatibility facade.

#### Scenario: Pi imports the transitional DSH library
- **WHEN** `e-pi` declares `e-dsh` in its manifest or imports the transitional `e` library name
- **THEN** the architecture check fails and identifies the forbidden adapter-to-adapter edge

#### Scenario: Both adapters use the shared runtime
- **WHEN** architecture checks inspect the completed workspace graph
- **THEN** `e-dsh` and `e-pi` each point to `e-tui`, and no shared runtime source is owned by one adapter for reuse by the other

### Requirement: Shared runtime modules remain acyclic
Production modules under `e_tui::runtime` SHALL follow one-way dependencies from adapter-facing coordination toward controller, scheduling, ports, terminal, and input leaves. Runtime modules MUST NOT introduce a strongly connected component with `TuiApp`, rendering, interaction, or provider adapters.

#### Scenario: Runtime leaf reaches into an adapter
- **WHEN** a runtime input, terminal, scheduler, or port module imports an `e-dsh` or `e-pi` module
- **THEN** the architecture check fails with the forbidden path

#### Scenario: Runtime module cycle is introduced
- **WHEN** runtime extraction creates a multi-module strongly connected component
- **THEN** the architecture check fails and reports every module in the cycle

