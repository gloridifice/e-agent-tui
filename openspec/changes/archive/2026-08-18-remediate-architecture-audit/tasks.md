## 1. Baseline、守卫与迁移夹具

- [x] 1.1 为 Rust 生产模块依赖编写 grouped/simple `use crate::...` edge scanner，并用合成 DAG/环图测试 SCC 报告（暂不对当前生产图启用零环断言）。
- [x] 1.2 固化当前 command catalog、内置优先级、集成命令、`/new`/`/skill` 补全和 runtime action 的 characterization tests。
- [x] 1.3 固化 Input Page 焦点 reconcile、文本编辑 `hjkl`、login/settings/model/theme/resume outcome/effect 和小终端布局测试。
- [x] 1.4 固化 copy provenance、width/generation 复用、atomic Markdown、tail splice、activity range patch 和 history anchor 的缓存/UI 回归测试。
- [x] 1.5 为所有 HostEvent 家族与现存 `Msg` 可见结果增加表驱动等价夹具，覆盖 user/assistant/reasoning/tool/file/lifecycle/retry/command/workflow/compaction/system/error。
- [x] 1.6 增加协议派生事实漂移测试并立即把 `bridge/package.json.dshCompatibility.wireProtocol` 从 3 同步到 canonical version 4。
- [x] 1.7 运行 `cargo test` 与 `cd bridge && npm test`，记录迁移前依赖图、测试数和 timing examples 基线。

## 2. 提取 Page Core 并打破页面循环

- [x] 2.1 新建 `client/src/page_core.rs`，迁移 Direction、FocusId/Node/State、TextEditor、Viewport 与通用 PageOutcome/PageEffect，不导入任何具体页面。
- [x] 2.2 让 `settings.rs` 只依赖 `page_core` 的编辑原语，并保留现有 SettingsState/Action 与配置修改语义。
- [x] 2.3 让 `login.rs` 只依赖 `page_core` 的编辑原语，并保留 provider/proxy 焦点、密文和删除确认语义。
- [x] 2.4 调整 `input_page.rs` 为上层 controller，组合 settings/login/model/theme/resume 页面并删除具体页面到 controller 的反向依赖。
- [x] 2.5 更新 `lib.rs` 导出与 page 单测，确认 `input_page ↔ login/settings` SCC 已消失并运行 `cargo test`。

## 3. 提取 Command Catalog 与 Transcript Layout 并打破交互循环

- [x] 3.1 新建 `client/src/command_catalog.rs`，迁移 `BUILTIN_COMMANDS`、动作元数据、completion context、目录合并/排名与 discovery description。
- [x] 3.2 让 `input.rs` 只消费 command catalog 查询结果，删除对 `runtime_command` 的依赖并保持所有候选刷新行为。
- [x] 3.3 让 `runtime_command.rs` 消费 command catalog 的动作标识，删除对 `InputState` 的结构性依赖；用窄 command context/effects 传递必要输入。
- [x] 3.4 新建 `client/src/transcript_layout.rs`，迁移 width-aware display rows、grapheme wrapping、活动截断、layout row id 与 copy provenance 生成。
- [x] 3.5 让 `ui` 和 `copy` 消费同一 `TranscriptLayout`/`CopyLayoutRow` 结果，删除 `copy → ui` 调用并保留 CopyRowsCache generation 语义。
- [x] 3.6 清除 `runtime_command → copy → ui → runtime_command` 的剩余反向边，启用 `client/tests/architecture.rs` 对生产图的零 SCC 和 forbidden edge 断言。
- [x] 3.7 运行 command/input/copy/ui 聚焦测试与全量 `cargo test`，确认候选、折行、复制和缓存工作量不变。

## 4. 建立可测试 Runtime Controller 与效果边界

- [x] 4.1 新建 runtime input、内部 ControllerAction 与外部 RuntimeEffect 类型；覆盖 bridge frame、terminal key/paste/mouse、deadlines，并让 Send/PersistConfig/PersistSessionId/WriteClipboard/RequestDraw/Quit/Fatal 携带完整 payload，Open/ClosePage 只在 controller 内消费。
- [x] 4.2 将 `handle_msg` 拆为可独立测试的 bridge-message handlers，处理 welcome/snapshot/history/rosters/pages/questions/errors 并返回 typed effects。
- [x] 4.3 将 PageUp/Down、mouse history paging、help、Input Page、approval/question、copy mode 和普通 input 的优先级迁入 terminal-event handlers。
- [x] 4.4 实现 RuntimeController，在单个 scoped state guard 内消费页面等内部 action，并保证 await/I/O effects 只在 guard 释放后执行且 executor 不借用 UI 状态。
- [x] 4.5 把 `main.rs::run` 收敛为 Tokio `select!`、入站预算、deadline、terminal lifecycle 和 effect executor 的组合根，删除页面/命令/复制/协议家族业务分支。
- [x] 4.6 为 terminal events、bridge transport、clipboard、config persistence/state-file 和 clock 增加生产适配器及 scripted test ports；effect executor 必须穷尽执行且不得静默忽略 effect，不引入固定 ticker 或 async-trait 依赖。
- [x] 4.7 增加 controller 顺序测试：queued dispatch、session switch、page effects、clipboard failure、bridge disconnect、animation settle 和 terminal restore。
- [x] 4.8 复跑锁纪律、调度公平性、P95 frame metrics 与全量 `cargo test`，确认 effect executor 不在锁内 await/I/O。

## 5. 为 Launcher 引入确定性 Ports

- [x] 5.1 定义 LauncherPorts/ProcessHandle/LockStore/Clock 的最小接口，并提供基于 `std`、TCP、文件和平台命令的生产实现。
- [x] 5.2 将 acquire/release、stale-lock probe、spawn timeout、terminate/reap 与 retryable `instances: 0` 状态迁到可注入的 launcher coordinator。
- [x] 5.3 用 in-memory lock store 和 scripted process/probe 覆盖正计数 stale lock、并发加入、最后实例退出、启动超时与终止失败。
- [x] 5.4 保留 Windows `taskkill /T` 真实进程树集成测试和非 Windows 终止测试，验证生产 adapter 不退化为只杀 shim。
- [x] 5.5 运行 launcher 聚焦测试和全量 `cargo test`。

## 6. 拆分协议解析、投影与 UI 家族模块

- [x] 6.1 将 `protocol.rs` 拆为 message DTO façade 与 `host_event` content/assistant/tool/lifecycle/workflow parser 模块，保持 serde wire API 和 bounded unknown envelope 不变。
- [x] 6.2 将 projection 的 surface ownership 与 pending activity correlation 状态拆入 `projection/surface.rs`、`projection/activity.rs`；assistant、tool/file、lifecycle/retry/command/workflow legacy reducers 不做临时搬迁，改由 7.2–7.5 直接迁入最终 family 模块与 TranscriptStore。
- [x] 6.3 将 `ui.rs` 拆为 transcript、input/status/accessories、overlay 以及 settings/login/model/theme/resume page renderer，保留公共 render façade。
- [x] 6.4 将 `main.rs`、HostEvent parser 和页面 renderer 中剩余的多职责超大函数收敛为家族分发 + 短处理器并补充 façade 测试；legacy `model.rs::reduce_host_event` 的 family 拆分按修订决策并入 7.2–7.5，避免双重迁移。
- [x] 6.5 运行 `cargo fmt --check`、`cargo clippy --all-targets`、聚焦测试与全量 `cargo test`，确认拆文件阶段无行为变化。

## 7. 完成单轨 Display 投影迁移

- [x] 7.1 新建只接受 ActivityRow/TranscriptBlock/ContentCard/composite 的 TranscriptStore，以稳定 DisplayId、unit、surface owner 和 cache generation 为索引。
- [x] 7.2 新建最终 assistant projection family module，并把 user、assistant assembled/streaming、reasoning 与 context/attachment 从 legacy reducer 直接迁移到 TranscriptStore，保持 reasoning 隐藏/可见模式和 tail dirty 规则。
- [x] 7.3 新建最终 tool/file projection family module，并把 Thinking、generic tool、create 与 read/view/edit/replace/insert FileGroup 直接迁移到公共 ActivityRow/composite，保持折叠、相对路径、耗时与 settle 动画。
- [x] 7.4 新建最终 lifecycle/retry/command/workflow projection family modules，并把 retry、command、Code Mode、workflow、compaction 和 turn outcome 直接迁移到公共 activity/card，保留跨页 pending result/enrichment。
- [x] 7.5 将 System、Error、unknown append fallback 与 command-result 直接迁移到 TranscriptBlock/ContentCard，删除对应 legacy 顶层 UI 分支。
- [x] 7.6 将 append/replace、shadowed seq、surface 插入位置、历史 prepend 和有效新增 display-row 计算切换到 TranscriptStore。
- [x] 7.7 将 render cache、visible-window materialization、CopyRowsCache 和 Markdown unit reuse 全部切换到共享 transcript layout 与 DisplayId ranges；Markdown `RenderLine`/atomic/raw-line provenance 由 layout registry sidecar 持有，TranscriptStore 只保存 source-only 公共表面。
- [x] 7.8 删除旧 transcript `Msg` enum、compatibility `reduce_host_event`、legacy DisplayId adapter 和 `ui.rs::msg_lines` 中事件专用旧变体转换。
- [x] 7.9 增加源码/架构守卫，若旧 `Msg` transcript、compatibility reducer 或旧 UI 变体重新出现则测试失败；校正旧 OpenSpec `unify-event-display-framework` 的 8.4 完成证据。
- [x] 7.10 运行全部 model/projection/cache/copy/ui 测试、snapshot smoke 与 timing examples，验证 tail splice、range patch、history anchor、复制来源和 redraw 指标不退化。

## 8. 扩展并同步 Canonical Wire Schema

- [x] 8.1 为 `protocol-contract.json` 设计并加入 additive message shape/record 段，覆盖全部 client messages 和全部非 HostEvent server frames 的 required/optional camelCase 字段与基本类型。
- [x] 8.2 将现有 protocol-doc 工具收敛为 contract sync 工具，校验 contract 并生成 Rust 常量/shape fixtures、Node fixtures、package wire metadata 和 `docs/protocol.md`。
- [x] 8.3 在 Rust 增加 contract-driven serde conformance tests，逐项验证 client serialization、server parsing、optional defaults、message roster 和 payload shape。
- [x] 8.4 在 Node 增加 contract-driven dispatcher/frame conformance tests，逐项验证 accepted client samples、produced server samples、roster coverage 和 bounded encoding。
- [x] 8.5 删除 `bridge/src/index.js` 中手写且已过期的消息清单，改为链接 canonical contract/generated docs。
- [x] 8.6 增加 generated-files-clean 检查，contract 改动但 fixtures/package/docs 未同步时使测试失败。
- [x] 8.7 运行 contract sync、`cargo test`、`cd bridge && npm test` 和 `node tools/smoke-bridge.mjs`。

## 9. 收敛配置 Schema 与覆盖加载

- [x] 9.1 为现有 missing field、部分覆盖、未知旧字段、非法类型、非法主题、reload 与 runtime resolved theme 增加 loader characterization tests。
- [x] 9.2 让持久化 Config 直接严格 `Deserialize`，对 `resolved_theme` 等运行时字段使用 skip/default，并保持默认值只存在于 embedded TOML。
- [x] 9.3 实现 embedded default TOML 与用户 TOML 的 schema-filtered value overlay，再对合并值执行一次 Config 反序列化和 theme resolve。
- [x] 9.4 删除 CompleteConfig、PartialConfig、into_config 和手写 apply 字段清单，保留旧文件缺字段继承及未知字段容忍。
- [x] 9.5 检查 `settings::ITEMS` 只包含用户可见元数据和显式编辑行为，不再承担完整持久化 schema 责任。
- [x] 9.6 运行 config/settings/input-page 聚焦测试与全量 `cargo test`。

## 10. 收敛 DSH Model Selection 兼容边界

- [x] 10.1 检查当前受支持 DSH package 是否公开稳定 `installModelSelection`，记录导出、签名和版本约束结论（`@deepseek-ai/dsh-agent@0.1.0-rc.6` package-root export，`(agentCtx, selection) => disposer`）。
- [x] 10.2 新建 `bridge/src/model-selection.js` adapter；稳定导出经 lazy public import 复用，`@deepseek-ai/dsh-agent` peer 精确钉为 `0.1.0-rc.6`，metadata/upgrade verifier 同步记录 tested host。
- [x] 10.3 让 `session.js` 的 create/resume setup 只调用 adapter，并从 `compose.js` 删除 model-selection waterfall 知识。
- [x] 10.4 扩展 adapter tests，覆盖 assembly variables、assembled/current 切换、request routing、reasoning effort、无 selection pass-through 和 disposer。
- [x] 10.5 将 deployed-copy smoke 纳入 `tools/verify-dsh-upgrade.mjs`；其先做 canonical contract check/host+agent version/export check，再执行 deployed helper 与 local-WS session-routing smoke。
- [x] 10.6 运行 `cd bridge && npm test`（61 tests）与 `npm run verify-dsh-upgrade`；验证 deployed `/new`、cold resume 和 `/model` 的实际 assembly/request 路由。

## 11. 文档、全量验证与审计收口

- [x] 11.1 更新 `README.md` 的功能/兼容说明，确认用户键位、启动流程和配置文件格式保持兼容（补充 strict Config overlay、canonical contract 与 DSH upgrade gate；未改变用户键位或安装流程）。
- [x] 11.2 更新 `AGENTS.md` 与 `docs/design.md`，记录新的依赖方向、runtime ports、单轨 Display、wire schema 生成流程、配置 overlay 和 model-selection adapter（并新增 `docs/architecture-audit.md` 记录收口审计）。
- [x] 11.3 更新相关旧 OpenSpec 变更的完成/归档状态，确保不再宣称存在已删除但代码仍保留的兼容层（`unify-event-display-framework`、`optimize-terminal-refresh-latency`、`unify-input-pages` 均已标注完成与后续承接关系）。
- [x] 11.4 运行 `cargo fmt --check`、`cargo clippy --all-targets`、`cargo test`、`cd bridge && npm test`、protocol generated-files-clean、snapshot/timing examples 与 live bridge smoke（fmt/protocol/diff clean；Rust、61 个 bridge tests、release snapshot/timing examples 及 deployed `npm run verify-dsh-upgrade` 全部通过；Clippy exit 0，仍有独立的非阻断 style diagnostics，未把 lint cleanup 混入本架构变更）。
- [x] 11.5 运行生产模块 SCC 守卫并重新执行 Brooks Architecture Audit；确认零循环、零 legacy Msg 路径、协议/配置无重复事实漂移，记录新的 Health Score（`cargo test --test architecture` 5/5 通过；`docs/architecture-audit.md` 记录 94/100，0 critical / 1 warning / 1 suggestion，并同步 `.brooks-lint-history.json`）。
