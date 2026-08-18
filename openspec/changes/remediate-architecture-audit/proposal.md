## Why

最新架构审计得分为 59/100：Rust 客户端的输入、命令、复制、渲染与 Input Page 形成两个循环依赖簇，核心事件循环、协议解析、事件归约和页面渲染仍集中在超大函数中；已宣称完成的 Display 迁移还保留旧 `Msg` 双轨模型。协议版本元数据、配置字段声明和 DSH model-selection 兼容策略也存在重复事实，已经出现 `protocolVersion: 4` 与 `wireProtocol: 3` 的实际漂移，因此需要一次有边界、可验证的架构收敛，而不是继续局部打补丁。

## What Changes

- 打破 `input ↔ runtime_command`、`runtime_command → copy → ui → runtime_command`、`input_page ↔ login/settings` 循环，建立只能向下依赖的 `command_catalog`、`page_core`、`transcript_layout` 等稳定内核。
- 完成共享 Display 框架迁移：应用状态只保存公共显示表面，删除旧 `Msg` 兼容归约和 `ui.rs` 中事件专用顶层适配，同时保持流式 tail splice、copy provenance、历史锚点和动画 patch 语义。
- 将 `main.rs::run`、HostEvent 解析、事件家族归约和 Input Page 渲染拆成职责单一、可独立测试的模块；组合根可保持高 fan-out，但不再承载具体业务分支。
- 为终端事件、bridge transport、剪贴板、进程、时钟和 launcher lock store 建立窄端口，使主循环与启动器竞态可使用脚本化测试替身确定性验证。
- 扩展 wire contract 的机器可读覆盖面，使版本、容量、消息 roster、关键 payload shape、package compatibility 元数据和生成文档保持同源；修正现有 wire version 漂移并增加 Rust/Node 双向一致性测试。
- 将配置加载改为“嵌入默认 TOML + 用户 TOML 值级覆盖 + 单一严格 `Config` 反序列化”，删除 `CompleteConfig`/`PartialConfig` 平行字段清单，并收敛设置页元数据。
- 消除或严格封装内联的 `installModelSelection` 上游知识：优先使用官方导出；若宿主版本不提供稳定导出，则精确钉住兼容版本并把 live smoke compatibility 纳入自动验证。
- 为依赖方向、协议生成物、配置继承、Display 单轨存储、主循环端口和 launcher 生命周期增加架构/回归测试。
- 同步更新 `README.md`、`AGENTS.md`、`docs/design.md`、生成的 `docs/protocol.md` 与相关 OpenSpec 完成状态。

## Capabilities

### New Capabilities

- `acyclic-client-architecture`: 定义客户端模块依赖方向、交互内核边界、组合根职责以及可自动验证的无环约束。
- `single-display-projection`: 定义 HostEvent 到公共显示表面的单轨投影、状态存储、缓存、复制、历史和动画兼容要求。
- `testable-runtime-ports`: 定义主循环与 launcher 的终端、传输、剪贴板、进程、时钟及锁存储测试 seam。
- `canonical-wire-schema`: 定义跨 Rust/Node 的协议版本、roster、payload shape、package metadata、文档生成与兼容验证单一事实源。
- `declarative-config-overlay`: 定义嵌入默认配置、用户值级覆盖、严格反序列化和设置元数据的单一字段所有权。
- `host-model-selection-compatibility`: 定义 DSH model-selection 安装逻辑的上游复用、版本钉住和自动兼容验证策略。

### Modified Capabilities

无。

## Impact

- Rust 客户端核心：`client/src/main.rs`、`lib.rs`、`input.rs`、`runtime_command.rs`、`copy.rs`、`ui.rs`、`input_page.rs`、`login.rs`、`settings.rs`、`model.rs`、`projection.rs`、`display.rs`、`protocol.rs`、`config.rs`、`launcher.rs`、`bridge_io.rs`、`terminal_runtime.rs`，并新增稳定内核、事件处理器和 runtime port 模块。
- Node bridge 与契约：`bridge/protocol-contract.json`、`bridge/package.json`、`bridge/src/protocol.js`、`dispatcher.js`、`frame.js`、`compose.js` 及对应测试和生成工具。
- 内部 Rust 模块 API 会发生较大调整，但用户键位、可见 TUI 行为、配置文件兼容、WebSocket 消息语义和历史重建结果必须保持向后兼容。
- 不恢复 `client/vendor/`，新增依赖仅在确有必要时通过 crates.io/ npm 锁文件引入；优先使用现有 serde、tokio 和测试基础设施。
- 变更规模较大，实施必须按“先加约束与 characterization tests，再切依赖与状态所有权，最后删除兼容层”的顺序分阶段完成，每阶段保持 `cargo test` 与 bridge 测试通过。
