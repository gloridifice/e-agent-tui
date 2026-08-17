# AGENTS.md

面向编码 agent 的项目说明。人类读者请看 [README.md](README.md)；设计决策看
[docs/design.md](docs/design.md)（D1–D30、协议、里程碑）。

## 项目是什么

DeepSeek Harness（DSH）的终端客户端（项目名 **e**，可执行文件 **`dshe`**），两部分：

- `bridge/` — Node.js（ESM）DSH **host-composition 插件**。注册一条 WS 升级路由
  （`/dsh-tui`），把会话事件转发给 TUI，并接受输入/命令/中断/审批应答/会话切换/
  历史分页，以及 `/login` `/model` `/skill:<名称>` 桥接。注入依赖只有 `webServer`。
- `client/` — Rust（ratatui + crossterm）单 exe 客户端（crate `e`，产物
  `dshe.exe`）。含启动器（`launcher.rs`：探测/spawn `dsh --profile dshe`/桥接）与
  主题系统（`theme.rs` + `config.rs`）。无 TLS/网络依赖（除 WebSocket 本体）。

两个进程通过 JSON WebSocket 通信；唯一机器可读契约是
`bridge/protocol-contract.json`（`client/build.rs` 生成 Rust 常量，`bridge/src/protocol.js`
运行时读取，`tools/generate-protocol-doc.mjs` 生成 `docs/protocol.md`）。token 认证，token
在 `%DSH_HOME%\dsh-tui.token`。
客户端配置在 `%APPDATA%\dshe\config.toml`，默认配置源是
`client/assets/default_config.toml`（`include_str!` 嵌入并解析，用户文件作为缺字段可继承的覆盖层）；
主题在 `%APPDATA%\dshe\themes\`。

## 常用命令（Windows / PowerShell）

面向用户的源码安装流程记录在 README「Quick Start」：全局安装
`@deepseek-ai/dsh`，设置/沿用 `DSH_HOME`，挂载并安装专属 `dshe` profile 桥接，最后用
`cargo install --path client --locked` 把 `dshe.exe` 安装到 Cargo bin 目录。

```powershell
# 首次安装
npm install --global @deepseek-ai/dsh
# DSH_HOME 未设置/空值时 mount 脚本自动用 $HOME\.dsh；自定义 home 才需先设置环境变量
.\tools\mount-bridge.ps1 -Profile dshe
dsh plugin --profile dshe install
cargo install --path client --locked

# 客户端（根目录是 Cargo workspace，默认成员 client，crate 名 e，产物 dshe.exe）
cargo run                                # 根目录编译并启动 dshe
cargo build --release                    # 产物 target\release\dshe.exe
cargo build --release --features tracy   # Tracy profiling 版（DSH_TUI_TRACY=1 激活）
cargo test                               # 全量单测（约 170 个）

# 桥接同步（改 bridge/ 后必做；重启 dsh 后生效）
.\tools\mount-bridge.ps1 -Profile web    # 或 -Profile dshe（dshe 启动器的专属 profile）
# 等价手动：robocopy bridge\src "$env:DSH_HOME\profiles\<p>\packages\dsh-tui-bridge\src" /MIR
# 脚本须兼容 Windows PowerShell 5.1：空 DSH_HOME 回退 $HOME\.dsh；变量名不区分大小写；Node JSON 必须 UTF-8 无 BOM

# 桥接测试（node:test；trim/compose/login/skill/model 五个模块）
cd bridge; npm test                      # = node --test --test-isolation=none "test/*.test.js"
node tools/smoke-bridge.mjs        # DSH 升级后跑：对部署副本跑契约冒烟

# 联调
node tools/probe-online.mjs       # 桥接是否在线
node tools/hello-test.mjs         # 发 hello 打印全部帧（验证启动路径）
node tools/probe-startup.mjs      # attach 延迟/快照大小
node tools/dump-snapshot.mjs      # 抓快照样本 → tools/cache/snapshot-sample.json
cargo run --release --example timing_snapshot -- tools/cache/snapshot-sample.json
cargo run --release --example timing_frames # 1002 消息持续滚动/流式/动画帧基准
cargo run --example smoke_snapshot -- tools/cache/snapshot-sample.json
```

cargo 走 crates.io 官方源（本机网络已修复）。`client/vendor/` 与
`tools/vendor-crates.mjs` 是历史遗留的离线兜底，已停用，勿再依赖；新依赖直接加
`client/Cargo.toml` 并提交根目录 `Cargo.lock`（workspace 锁文件）。

## 关键架构约定

### client（Rust）

- **事件显示模型**（`display.rs` + `projection.rs` + `model.rs`）：所有可见事件归入四类公共表面：
  `ActivityRow`（带 Waiting/Running/Success/Failure/Cancelled 状态，可带 parent/depth）、
  `TranscriptBlock`（plain/markdown/reasoning/unknown fallback）、`ContentCard`（统一 padding/
  背景/copy source）与 `InputAccessory`（输入栏上方）。`EventProjector` 先产出 display/
  surface mutation/page state/accessory/ignore effect，再由状态层应用；禁止在 `ui.rs` 新增绕过
  公共表面的事件专用顶层渲染。旧 `Msg` 域状态只可作为迁移/关联载体，渲染前必须适配到公共表面。
- **思考输出（reasoning）折叠**：`TranscriptFormat::Reasoning` 块不渲染到屏幕、也不进 copy
  provenance（`ui.rs::is_hidden_msg` 让缓存构建与 `copy_layout_rows` 都跳过它，且不产生行间 gap）；
  活动行邻接必须查找下一个**非隐藏**消息，隐藏 reasoning 不得拆开本应 glued 的活动行。
  思考过程由 `• Thinking... xN` 呼吸灯表示；`assistant/chunk` 只带 reasoning 时不 `stop_thinking`，
  直到真正的 answer text 到达才结算绿色。因此 `stop_thinking` 必须向后查找 Running 的 Thinking
  行（隐藏的 reasoning 块会堆在它后面），不能只查 `msgs.last()`。
- **文件活动折叠**：`FileGroup` 用统一 `FileItem + FileAction` 保留 `read/view/edit/replace/insert`
  标签，连续的 `str_replace_editor` view/str_replace/insert 与 read/edit 进入同一折叠活动行；编辑器
  的绝对路径按 `session_cwd` 转成工作区相对路径。create 不进入 FileGroup，单独显示为
  `<指示灯> create <相对路径>`，完成后也不追加输出行数/耗时。所有活动行保持单显示行，超宽时在
  `styled_msg_lines` 按已解析的页面内容宽度（含 `page_max_width`）截断并追加 `…`，不得按终端宽度
  预截断后在较窄页面中折行。
- **Surface 语义**：`HostEvent` 解析事件顶层 `time`、`surfaceOp`、`sourceEventSeqs`；replace
  必须先移除 shadowed surface owner，再在原 surface 位置插入替代节点。未知但带 `surfaceOp` 的事件
  也必须进入快照/历史兼容路径。历史前插时保存 shadowed seq，后到的旧页不得复活压缩内容；被分页
  拆开的 tool/command/Code Mode/workflow terminal half 先暂存，旧页 start 到达时直接重建最终状态；retry
  schedule 与较新 retry-started 跨页时须在恢复 saved rows 后回填 delay/failure/maxRetries，不得只为去重而
  丢详情；只按真正新增渲染行数移动 viewport。workflow 的 completed/failed/cancelled 必须保留为 typed
  outcome 并映射到 Success/Failure/Cancelled。compaction 的 log-only summary 不单独画卡，唯一 summary
  card 由 replacement 创建并拥有，确保后续 replace 能精确删除。
- **渲染缓存**（`cache.rs::TranscriptRenderCache` + `ui.rs`）：只有结构性事件使缓存
  失效并全量重建；流式 chunk 只标记 `tail_dirty`，渲染时**尾部拼接**并仅重算 tail
  display-row suffix/prefix，不得清空整份 layout；spinner/settle 只 patch 活动 range，settle
  到期必须再提交一次精确目标色 patch 后才停钟。复制行号由 `ui.rs::copy_layout_rows` 从同一个
  `styled_msg_lines` 布局过程产生，主循环用 `CopyRowsCache` 按 width/generation 复用 provenance，
  禁止每个 copy 按键和随后绘帧各自全量 `flatten`。折行扫描按 Unicode grapheme cluster 计算显示宽度，
  combining mark/emoji ZWJ 即使跨 style span 也不得拆开。
- **性能红线**（都有回归测试）：终端输入通过 `EventStream` 直接唤醒主循环，禁止恢复固定
  ticker 轮询；交互/内容/动画 deadline 分离，bridge backlog 每轮受条数+时间预算约束。终端由
  `terminal_runtime.rs::TerminalOwner` 单点初始化/恢复，帧用 64KiB `BufWriter` + DEC 2026
  synchronized output 原子提交（`DSHE_DISABLE_SYNC_OUTPUT=1` 仅作兼容诊断）。每事件不得全量
  重渲染；重绘 P95 ≤30ms 且只在 dirty/deadline 到期时；动画只 patch 活动消息 range，流式只
  splice tail；display-row layout 按 width/generation 缓存，每帧只物化/克隆可见窗口。禁止破坏
  `valid/tail_dirty/dirty_messages`、history display-row anchor 与 copy provenance 共用布局语义。
- **主循环锁纪律**：Rust 2021 的 `if let`/`match` scrutinee 临时值会活到整个表达式
  结束；不得把 `state_r.lock()` 直接写进 scrutinee 后又在分支中重锁或 `.await`，否则
  会自死锁、表现为 TUI 完全无法输入。先在独立作用域算出普通值/动作再匹配，或像
  `prepare_next_queued_prompt` 一样在单个 guard 内完成原子状态变更；`main.rs` 已 deny
  `clippy::significant_drop_in_scrutinee`，并有队列派发/复制模式释放锁的回归测试。
- **输入交互与字符边界**：`InputState.cursor` 是**字符索引**，`String::insert/remove`
  和切片要字节索引——用 `char_to_byte()`（`input.rs`），CJK 有回归测试；光标 x 坐标用
  `unicode_width`。普通输入固定 `Enter` 发送、`Shift+Enter` 换行；`↑/↓` 先按字符列在
  输入行间移动，到首/末行边界才切换上一/下一条历史提示词；`PageUp`/`PageDown` 按当前可见
  transcript 高度翻页，鼠标滚轮每格移动 3 行（Input Page 打开时也始终操作 transcript）。
  `Ctrl+H` 是 Input Page 之前处理的全局帮助键，带 Control/Alt/Super 的 `hjkl` 不得进入焦点图。
  `Config.enter_sends` 仅为旧配置反序列化
  兼容，不得再改变键位语义。终端硬件光标在 TUI 内必须始终隐藏，屏幕只画软件反色光标；
  `ui.rs::render_with_cursor` 只返回 IME anchor，主循环在帧完成后移动隐藏光标。禁止重新调用
  `Frame::set_cursor_position`，它会让 ratatui 在差量绘制期间显示并拖动光标，导致状态灯/输入栏闪烁。
- **覆盖层与 Input Page 渲染**：真正画在 transcript 之上的命令提示/会话选择器必须先
  `frame.render_widget(Clear, rect)` 再画背景，否则底下文字会透出（有测试
  `suggest_popup_is_opaque_over_transcript`）。`/settings` `/login` `/model` `/theme` 不是
  overlay：它们统一由 `InputPageSession` 替代输入区，无边框、不得 `Clear`，公共 shell 固定
  上下各 1 行、左右各 2 列内边距。
- **copy 语义**：复制永远取原始 markdown（`units` 表）；表格/代码/mermaid 是
  原子块（`RenderLine.atomic`）。渲染单元 id 在重渲染时复用（`unit_start`），
  别重新分配。
- **Markdown 标题与局部背景**：标题直接使用固定语义 `semantics.markdown.heading1..6`；当前
  ferra 的一级为 Coral `#ffa07a` 粗体（无背景），二级为 Sage `#b1b695` 粗体，三级为
  Blush `#fecdb2` 非粗体。inline code 的 `bg` 只能作用于 chip span；`render_transcript` 只允许
  `Line.style.bg` 触发整行补色，禁止从任意 span 的背景推断整行背景，否则会污染源码分隔空格与
  行尾空白。修改这些样式须同步内置主题 TOML、`render.rs` 与 TestBackend 回归测试。
- **表格单元格**：必须经 `cell_spans()`（`render.rs`）做行内渲染 + 显示列宽
  截断/补齐，不能塞裸字符串。
- **历史分页**：`min_seq`/`history_loading`/`history_exhausted`；前插走
  `prepend_events`（设置 `prepend_line_anchor`，渲染器按新增行数平移
  `scroll.offset` 保持视口）。顶部历史提示行是**纯显示**，不进缓存；
  `Thinking...` 行是 `Msg::Thinking` 卡片（进缓存），但快照回放/历史前插时
  不生成（`state.replaying`），文件组合并/结算扫描会跳过它。
- **Tracy/计时**（`profile.rs`）：埋点用 `e::tracy_zone!("字面量")`（宏，
  无 client 时安全空转）；阶段打点用 `PhaseTimers`。zone 名必须是字符串字面量。
- **底部布局与双行状态栏**：页面底部固定行序为 输入栏或 Input Page / gap / 状态第一行 /
  **会话标题行**（`ui.rs::render` 的 chunks 数组；accessory budget 公式里的 `+3` 与之一致）。
  两行都不设置背景色：第一行左侧依次为工作指示灯、`AppState.current_mode`、当前模型、
  `CH<缓存命中率%>`，其中模型和 CH 暂无值时整项省略（不画占位横线），右侧固定 `^h Help`；第二行左侧是 `AppState.session_title`（空时显示
  `新会话`），右侧是 `AppState.session_cwd` 绝对路径，标题过长以 `…` 截断以保住路径。
  mode 初值取 `welcome.mode`（最近 selection，缺省为创建 header），再由 `agent-preset/selected` 回放更新，
  按 event seq 保留最新值（历史前插不得回退）；CH 从
  assistant usage 的 input/cache read/cache write 累计计算，历史前插可增加旧总量但不得替换最新
  request 的 usage 锚点；这些页面状态更新**不得**触碰 `TranscriptRenderCache`。改底部行数时必须
  同步 ui 层测试里硬编码的行号。
- **命令范式**（`runtime_command.rs` + `input.rs`）：命令分为内置优化命令与 DSH
  接入命令。所有内置项只在 `BUILTIN_COMMANDS` 声明一次（名称/说明/input hint/补全策略/
  action 同项），禁止在 `input.rs` 再维护平行名称表；`match_command_catalog` 合并桥接下发
  的 `CommandInfo`，同名时内置优先。接入命令来自每个 agent 的有效 `ctx.commands.list`
  视图，至少支持名称模糊补全并显示 DSH 的 free-form input hint；DSH 当前无 typed argument
  completion schema，只有内置项可做 `/new ` 这类参数补全。收到新 `commands` 帧要立即刷新
  已打开的提示框，切会话先清旧 agent-scoped 目录。通用执行不得预先 `start_thinking`，结果
  由 `command-result` 直接投影为 System/Error。
- **启动即新会话**：新进程不带 `resumeSessionId` 发 hello，桥接就地建会话（
  `hello.cwd` 工作区 + `hello.mode` 默认模式，失效回退 standard）；只有 CLI 会话
  id 与「记住上次会话」（默认关）走续接。`/resume` 打开选择器、`/resume <id>`
  直接 attach（都是纯客户端命令）。
- **Input Page 控制器**（`input_page.rs` + `settings.rs` + `login.rs`）：主循环只持有一个
  `Option<InputPageSession>`，闭集 variant 为 Settings/Login/Model/Theme；页面按键只返回
  `PageOutcome`/`PageEffect`，caller 在释放页面借用和状态锁后再 save 或 `.await` 发送。
  浏览态方向键与 `hjkl` 共用稳定焦点图、Enter 执行，文本编辑态 `hjkl` 必须作为普通字符。
  动态 login/model roster 以 provider/model/proxy id 对焦点做 reconcile，空列表不得制造假焦点。
- **/login 页面**：一层三选一菜单（API key / Account / Proxy）→ 子页面（Menu /
  Providers / ApiKey / Account / ProxyList / ProxyForm / ProxyDelete）。状态来自桥接 `login`
  帧；API key 永不回传、编辑态画 ●，不可写 provider 不得获得操作焦点；已有代理 Enter
  必须先进入删除确认页，只有显式选择删除才发送 `login-proxy-delete`。
- 新增交互键位后同步更新：`ui.rs` 的 `help_overlay`、README 速查表、input 测试。

### bridge（Node.js）

- **模块布局**：`index.js` 只留 WebSocket 生命周期与消息分发；`host.js` 显式封装
  DSH service locator，`connection.js` 统一 detach/过期连接判定，`history.js` 管 surface
  缓存与分页，`session.js` 管创建/冷恢复/workspace/preset 组合，`command.js` 投影宿主
  命令目录/直接结果，`protocol.js` 读取共享
  wire contract；`trim.js`、`compose.js`、`login.js`、`skill.js`、`model.js` 留各自纯逻辑。
  这些模块均须有 `bridge/test/` 的 `node:test`，新代码不得再堆回 `index.js`。
- **DSH 命令接入**：attach 后用 `ctx.commands.list(agent)` 下发 handler-free
  `commands{commands[{name,description,input?:{hint}}]}`；监听 `commands/change` 后为每个连接
  重算有效目录（agent-scoped shadow 不能做全局增量 patch）。`command{line}` 走
  `commands.execute(agent,line,signal)`；`undefined` 是未注册/语法无效，settled result 走
  `command-result{commandId,kind,text?}`，不得变成模型消息。执行跨 await，回结果前必须做
  current-conn 校验，防止 attach 后串会话。
- **跨 await 的 conn 纪律**：消息处理器里凡是 `await` 之后要动 `conn`（detach/
  重绑）的，必须在 await 前捕获局部 `current = conn`，await 后校验
  `conn === current && conns.has(current)` 再操作——连续 attach/`/new` 会并发
  交换闭包里的 `conn`，操作过期连接会串会话（`attach` 分支有对照实现）。
- 一切副作用挂 `ctx.effect()`；连接对象在 `conns` 集合，detach 必须清理监听、
  撤销待审批（`done('cancelled')`）。
- **快照/历史数据源**：活跃会话取 `agent.session.events`（内存，零磁盘读）；
  只有非常驻会话才回退 `persistence.readFrom(id, 0)`（全量读盘，慢，结果缓存到
  `conn.log`）。跨该异步读取后必须先检查 `conn.abort.signal.aborted`，旧连接不得被写入或发送快照。
  surface 列表按会话缓存并增量追加（`surfaceState`），除契约 roster 外还保留任何显式带
  `surfaceOp` 的未知事件；未知事件进入 wire 前必须裁成有界的 type/seq/time/surface 元数据 envelope，
  不得携带任意 data。所有下行帧经 `frame.js::encodeBoundedFrame` 执行 `MAX_FRAME_BYTES`：snapshot/history
  只留最新可容纳后缀并置 truncated/hasMore，单体超限帧改发 `frame-too-large` 错误。
- **负载裁剪**：`trimToolResultEvent`（模块级纯函数，导出为
  `_trimToolResultEvent` 供测试）——read 结果整段剥掉、其余工具只留末尾 2000
  字符（exit marker 在末尾）。实时事件转发也要过它，别恢复全量转发。
- 改消息 roster、surface 类型或容量时只改 `bridge/protocol-contract.json`，随后运行
  `node tools/generate-protocol-doc.mjs`；Rust 常量由 `client/build.rs` 自动生成。载荷
  结构仍同步改 `client/src/protocol.rs`（serde camelCase）与桥接 handler，并加双方契约测试。
  会话事件必须先解析为 `HostEventKind`，不得让 `serde_json::Value` 进入 reducer。历史 roster
  只收录重建已支持显示/输入 accessory 所需事件；approval/request/header/title-llm 等审计或重建
  记录默认不进 transcript。工具结果、Code Mode 子调用、compaction summary 与 `meta` 都必须有界裁剪，
  裁剪后带 `data.dshTuiTrimmed: true`，客户端不得把尾部行数冒充完整输出行数。
- **`/new` 的工作区继承**：新会话必须同时做两件事，缺一不可——`agents.create`
  的 `meta.cwd` 指向目标目录，然后经 `ctx.get('workspaceRegistry')` 的
  `resolveByPath`（无则 `create`）找到该 cwd 的工作区并 `attachSession(agent.id)`。
  只有 cwd 头、不 attach，会话不会进入工作区的 `sessionIds` 台账（host 自己的
  `session.create` 也是两步都做）。**cwd 优先级**：客户端 `hello.cwd`（TUI 的
  启动目录，桥接侧用 `isExistingDirectory` 校验）> 当前会话 `header.cwd` >
  `process.cwd()`——TUI 在哪个目录启动，`/new` 就落在哪个目录的工作区。
- **`/new <mode>` 与模式提示**：`/new` 是桥接自有命令（DSH 命令注册表没有它）。
  裸 `/new` 继承当前会话的 preset（`agentPresets.composedPreset(current.ctx)`，
  回退 `header.agentPreset`，再回退 roster 默认）；`/new <mode>` 直接按 preset
  id 解析（`agentPresets.resolve`，未知名会带 available 列表报错）。新会话必须
  在 `agents.create` 的 `setup` 里 `agentPresets.mount(agentCtx, preset.id)`——
  只写 `meta.agentPreset` 头不 mount，会话拿不到 preset 的工具/提示词（与 host
  `session.create` 的 compose 一致）。attach 后桥接下发 `presets{presets[]}`
  roster 帧（id/name/description/order/broken），客户端用它渲染 `/new ` 后的
  模式提示弹窗（broken 的 preset 不下发）。
- 连接对象跨会话复用（`/new`、picker attach）时必须保留 `conn.clientCwd`，
  否则重连后工作区继承退化回 header.cwd。
- **hello 即建会话**：hello 不带 `resumeSessionId` 时桥接调用同一个
  `createNewSession(ws, null, mode, { clientCwd, fallbackStandard: true })`
  就地建会话（`conn` 为 null，无镜像/无 detach）；`fallbackStandard` 把
  `resolve` 失败链降级为 `standard` → roster 默认，供 /settings「默认模式」
  失效回退。带 `resumeSessionId`（或 picker/`/resume` attach）而 id 不在
  活跃注册表时，先 `resumePersistedSession`：`sessionPersistence.list/inspect`
  + `agents.resume`，preset 取会话记录（`sessionPresetOf`：最近一条
  `agent-preset/selected` > `header.agentPreset`）并在 resume `setup` 里
  `agentPresets.mount`（历史不得换组合重放；preset 没了就不恢复）——都拿不到
  就新建会话，**任何"会话不活跃"都不许 close 连接**（重启竞态不是用户错误）。
  仅创建失败发 `hello-failed` 错误帧 + `ws.close(4001)`。
- **会话标题**：`welcome.title` 取 `agent.session.events` 里最近一条
  `session/title`（`latestTitle` 纯函数）；冷恢复会话日志不在内存，attach 后经
  `sessionQuery.readTitleSnapshots` 补发 `title{title}` 帧（发送前校验连接仍挂在
  同一会话，防跨会话串标题）；此后的标题更新不必专门推送——`session/event`
  全量转发已把 `session/title` 事件带给客户端。`session/title` **不进**
  `SNAPSHOT_SURFACE`：历史前插会经 `apply_event` 回放，旧标题会覆盖新标题。
  工作区路径走 `welcome.cwd`（会话头部 `header.cwd`，attach 时随 welcome 下发），
  客户端存进 `session_cwd` 渲染在标题行右侧。
- **model selection 必须装**：桥接创建/恢复的每个会话都要在 `setup` 里先
  `installModelSelection(agentCtx, { current, assembled: undefined })`（内联自
  `@deepseek-ai/dsh-agent` 的同名函数，两个 waterfall：`system-prompt/assemble`
  注入 `variables.{provider,model}`、`agent/request` 路由选中模型）——不装则
  persona 的 `{{model}}` 变量无值，每条消息都报
  `prompt variable "{{model}}" has no value (section "deployment:persona")`。
  `current` 取 `/new` 镜像的当前会话 provider/model（`mirror`），否则
  `ctx.get('agentDefaultModel').currentSelection()`；web/headless 入口都装，
  桥接不能漏。只 mount preset 不够——这是两个正交的 setup 步骤。
- **/login 字段落点**（桥接 `login.js`）：上行 `login-get` / `login-set-api-key`
  / `login-codex-start` / `login-codex-cancel` / `login-proxy-create` /
  `login-proxy-delete`；下行 `login{providers[],proxies[],codex?,error?}` +
  `login-codex{status,userCode?,verificationUri?,accountId?,error?}`。
  - API key：`ctx.llm.listProviders()` 列提供商，`providerCredentialRef` 从
    settings 读 `apiKeyEnv`（缺省回退 `<ID>_API_KEY`），走 `ctx.credentials` 的
    `describe/set/unset(ref)`（**值永不回传**，只发 `…末四位` hint，env 来源只读）。
  - Account（codex）：设备码流程（`auth.openai.com` usercode→轮询 token→换
    OAuth token），凭证存 `%DSH_HOME%\dsh-tui-codex.json`；**端到端生效还需宿主
    `dsh-llm-pi-ai` 接持久化 OAuth 凭证**（当前 `InMemoryCredentialStore`）。
  - Proxy：存 `%DSH_HOME%\dsh-tui-proxies.json`（api key 不回传）。
  写失败经同一 `login` 帧的 `error` 回给面板，不走 transcript 错误流。
- **/model（桥接）**：上行 `model-get` / `model-set{provider,model}`；下行
  `model{providers[{id,name,models[{id,name,description?}]}],current?}`。
  `sendModel` 用 `ctx.llm.listProviders()` + `ctx.llm.listModels(id)`（adapter
  无目录时该 provider 返回空列表不整体失败）。会话创建/恢复时把
  `modelSelections.set(agent.id, selection)` 存下那个 `{current,assembled}` 对；
  `model-set` 改 `selection.current`（下一次 `system-prompt/assemble` 生效）并
  顺手更新 `agent.options`，再回 `model` 帧刷新客户端状态栏。
- **/skill（桥接）**：`/skill:<名称>` 或 `/skill <名称>` 由桥接拦截（`skill.js`
  的 `parseSkillCommand`），经 `ctx.get('skills').get(name, {cwd, signal, scope})`
  查技能——**DSH 的 skill-filesystem 已按 `<workspace>/.agents/skills/` >
  `~/.agents/skills/` 优先级发现**，桥接只负责把 `renderSkillContent(skill)`
  （`<skill_content>` 块）以 `createUserMessage` + `source:{kind:"skill-invocation"}`
  `followup` 进会话（镜像 dsh-tool-skill 的用户显式调用注入）；未知名回
  `error{code:"skill-unknown"}`。跨 await 后要校验 `conns.has(current)`。
- **配置/主题/启动器（客户端）**：配置默认值只维护在
  `client/assets/default_config.toml`，由 `config.rs` 用 `include_str!` 嵌入并解析；
  `%APPDATA%\dshe\config.toml` 经 `PartialConfig` 覆盖嵌入默认值，须继续兼容旧文件缺少新字段，
  `Config::default()` 不得恢复 Rust 字段字面量。`Config.theme` 存主题名，`resolved_theme` 为
  `#[serde(skip)]` 的解析结果缓存，渲染期零磁盘读。主题是两层
  TOML：开放 `[colors]` 允许任意色名，固定 `[semantics.*]`（surface/markdown/input/
  working_status/log/activity/card/overlay）把语义样式链接到色名；每个样式仅 `fg` 必填，`bg`/
  `bold`/`italic`/`underline` 可选，未知引用、缺少固定字段或非法 hex 整个文件拒绝。内置
  `deepseek-e`/`ferra` 源文件在 `client/assets/themes/`，由 `include_str!` 嵌入并走与用户文件
  相同的解析器，同时无覆盖地复制到 `%APPDATA%\dshe\themes\`；合法同名用户文件优先，非法
  旧文件不得遮蔽嵌入回退。`launcher.rs`：`probe(url)` TCP 探测 → 无 dsh 则 spawn
  `dsh --profile dshe`（`dsh` 或 `npx @deepseek-ai/dsh`）→ `%DSH_HOME%\dsh-tui.lock`
  计数「最后一个 tui 关闭时关 dsh」；Windows 的 child handle 指向 `cmd /C` shim，正常关闭和启动超时
  清理都必须 `taskkill /T` 整棵进程树，禁止只 `Child::kill` 留下孤儿 Node；子进程回收必须有界，终止失败时
  保留 `instances: 0` 的锁供下次 attach 重试（服务已消失则视为 stale 后重建）。`release` 只在确实关闭托管服务时返回
  `true`，主程序退出 alternate screen 后输出 `dsh 服务器已关闭。`。启动器必须使用专属 `dshe` profile，不能复用
  DSH 自带/用户已有的 `tui` profile（其中的终端 UI 会抢占 stdio，且不提供桥接依赖的
  `webServer`）。`/reload` 重读 config + 重扫主题。

## 维护纪律

- **任何任务完成后，必须同步更新 `README.md`、本文件（AGENTS.md）与 `docs/`**
  （design.md 等）：功能、交互键位、协议字段、配置默认值、命令清单有变时，
  三处文档要与代码一致，不得滞后。键位变更还要同步 `ui.rs` 的 `help_overlay`。
- 客户端新增/修改了可见行为时，README「功能」与「快捷键速查」两节按需增补。

## 测试纪律

- 每次改动跑 `cargo test`；渲染/间距类改动必须有 ui 层回归测试（TestBackend
  断言缓存行数/颜色/内容），不能只靠模型层测试。
- 已知偶发：全量并行测试偶有一次 flake（tool 卡片断言），单跑或复跑即过，勿
  据此大改。
- 桥接侧有 `node:test`（`bridge/test/`，`cd bridge && npm test`，用
  `--test-isolation=none` 避免沙箱 spawn EPERM）：除 trim/compose/login/skill/model 外，
  host/connection/history/session/protocol 边界也必须覆盖；文件层测试走 temp home（不要
  碰真实 `%DSH_HOME%`）。
  DSH 升级后跑 `node tools/smoke-bridge.mjs` 对部署副本做契约冒烟
  （`installModelSelection` 是内联副本，钉在 DSH 版本上）。

## 已知事项

- **重启 DSH 才能加载新桥接**：改动 `bridge/src` 后需重新挂载（
  `.\tools\mount-bridge.ps1 -Profile <web|tui>`，等价 robocopy）+ 用户重启 dsh。
  旧桥接的启动全量读盘约 14s；新桥接活跃会话 <100ms。
- 设计文档里 M 里程碑编号已落后于实现（功能已超出 M6），以代码与 README 为准。
- `DSH_TUI_TIMING=1` 下各启动阶段耗时打印到 stderr，定位启动回归用。
