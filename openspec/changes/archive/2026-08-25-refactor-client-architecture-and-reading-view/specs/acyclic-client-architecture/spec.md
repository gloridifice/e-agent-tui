## MODIFIED Requirements

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

### Requirement: 布局与复制共享无 UI 反向依赖
Visible-row layout, semantic Reading geometry, and copy provenance MUST be owned by independent presentation-leaf modules. Transcript rendering, Reading View, and clipboard payload selection SHALL consume the same layout and provenance results; semantic navigation and copy logic MUST NOT call the Screen, Pane, or Region renderers.

#### Scenario: 复制当前语义块
- **WHEN** Reading View copies a wrapped Markdown, table, code, Mermaid, user, reasoning, or tool Block
- **THEN** clipboard selection obtains complete original source and current semantic geometry from shared provenance/layout without a `reading/provenance -> render region` dependency

#### Scenario: 宽度变化
- **WHEN** terminal or pane width changes and causes transcript rewrapping
- **THEN** rendering, Reading geometry, and copy provenance invalidate on the same width/generation and reuse the shared layout result

## ADDED Requirements

### Requirement: Kernel boundary is mechanically enforced
Architecture guards SHALL reject DSH protocol imports and raw DSH event names in `e-tui`, and SHALL keep bridge, setup, launcher, process, persistence, clipboard implementation, and async effect execution owned by `e-dsh`.

#### Scenario: Protocol type leaks into the UI library
- **WHEN** an `e-tui` production module imports a DSH `ServerMessage`, `ClientMessage`, or bridge protocol module
- **THEN** the architecture test fails and identifies the forbidden import

### Requirement: Rendering dependencies point downward
Production rendering imports SHALL follow `Screen -> Pane -> Region -> Component`; Components MUST NOT import Regions, Panes, or the Screen, and Regions MUST NOT import Panes or the Screen.

#### Scenario: Component reaches into a Region
- **WHEN** a leaf Component imports a transcript, composer, status, or Preview Region
- **THEN** the architecture check fails with the upward render dependency
