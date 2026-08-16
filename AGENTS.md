# AGENTS.md

面向编码 agent 的项目说明。人类读者请看 [README.md](README.md)；设计决策看
[docs/design.md](docs/design.md)（D1–D30、协议、里程碑）。

## 项目是什么

DeepSeek Harness（DSH）的终端客户端，两部分：

- `bridge/` — Node.js（ESM）DSH **host-composition 插件**。注册一条 WS 升级路由
  （`/dsh-tui`），把会话事件转发给 TUI，并接受输入/命令/中断/审批应答/会话切换/
  历史分页。注入依赖只有 `webServer`。
- `client/` — Rust（ratatui + crossterm）单 exe 客户端。无 TLS/网络依赖（除
  WebSocket 本体），纯本地渲染。

两个进程通过 JSON WebSocket 通信（协议见 `docs/design.md` §5 与
`client/src/protocol.rs`）；token 认证，token 在 `%DSH_HOME%\dsh-tui.token`。

## 常用命令（Windows / PowerShell）

```powershell
# 客户端（根目录是 Cargo workspace，默认成员 client，直接在根目录执行）
cargo run                                # 根目录编译并启动 dsh-tui
cargo build --release                    # 产物 target\release\dsh-tui.exe
cargo build --release --features tracy   # Tracy profiling 版（DSH_TUI_TRACY=1 激活）
cargo test                               # 全量单测（约 140 个）

# 桥接同步（改 bridge/ 后必做；重启 dsh web 后生效）
robocopy bridge\src "$env:DSH_HOME\profiles\web\packages\dsh-tui-bridge\src" /MIR

# 联调
node tools/probe-online.mjs       # 桥接是否在线
node tools/hello-test.mjs         # 发 hello 打印全部帧（验证启动路径）
node tools/probe-startup.mjs      # attach 延迟/快照大小
node tools/dump-snapshot.mjs      # 抓快照样本 → tools/cache/snapshot-sample.json
cargo run --release --example timing_snapshot -- tools/cache/snapshot-sample.json
cargo run --example smoke_snapshot -- tools/cache/snapshot-sample.json
```

cargo 走 crates.io 官方源（本机网络已修复）。`client/vendor/` 与
`tools/vendor-crates.mjs` 是历史遗留的离线兜底，已停用，勿再依赖；新依赖直接加
`client/Cargo.toml` 并提交根目录 `Cargo.lock`（workspace 锁文件）。

## 关键架构约定

### client（Rust）

- **消息模型**（`model.rs`）：桥接事件折叠为 `Msg`（User/Assistant/Streaming/
  Tool/FileGroup/System/Error）。`assistant/chunk` 只追加到 `Msg::Streaming`；
  `assistant/message` 才做完整 markdown 渲染并替换它。
- **渲染缓存**（`ui.rs` + `model.rs` 的 `render_cache`）：只有结构性事件使缓存
  失效并全量重建；流式 chunk 只标记 `tail_dirty`，渲染时**尾部拼接**；spinner
  帧推进也会触发重建。改间距/行数逻辑时必须**同步 `copy.rs::flatten`**（全局
  行号用于复制模式光标/滚动，两者必须一致，有对应测试）。
- **性能红线**（都有回归测试）：每事件不得全量重渲染；重绘 ≤30ms 一帧且只在
  dirty 时；每帧只克隆可见窗口。新功能别破坏 `cache_valid/tail_dirty` 语义。
- **字符边界**：`InputState.cursor` 是**字符索引**，`String::insert/remove` 和
  切片要字节索引——用 `char_to_byte()`（`input.rs`），CJK 有回归测试。光标 x
  坐标用 `unicode_width` 显示宽度。
- **覆盖层渲染**：任何画在 transcript 之上的面板（命令提示/会话选择/设置）必须
  先 `frame.render_widget(Clear, rect)` 再画背景，否则底下文字会透出（有测试
  `suggest_popup_is_opaque_over_transcript`）。
- **copy 语义**：复制永远取原始 markdown（`units` 表）；表格/代码/mermaid 是
  原子块（`RenderLine.atomic`）。渲染单元 id 在重渲染时复用（`unit_start`），
  别重新分配。
- **表格单元格**：必须经 `cell_spans()`（`render.rs`）做行内渲染 + 显示列宽
  截断/补齐，不能塞裸字符串。
- **历史分页**：`min_seq`/`history_loading`/`history_exhausted`；前插走
  `prepend_events`（设置 `prepend_line_anchor`，渲染器按新增行数平移
  `scroll.offset` 保持视口）。顶部历史提示行是**纯显示**，不进缓存；
  `Thinking...` 行是 `Msg::Thinking` 卡片（进缓存），但快照回放/历史前插时
  不生成（`state.replaying`），文件组合并/结算扫描会跳过它。
- **Tracy/计时**（`profile.rs`）：埋点用 `dsh_tui::tracy_zone!("字面量")`（宏，
  无 client 时安全空转）；阶段打点用 `PhaseTimers`。zone 名必须是字符串字面量。
- **底部布局与标题行**：页面底部固定行序为 输入栏/gap/状态栏/**会话标题行**
  （`ui.rs::render` 的 chunks 数组；`max_queue` 公式里的 `+3` 与之一致）。标题行
  是 `AppState.session_title`（`welcome.title` 初始化 + `session/title` 事件实时
  更新），画在 transcript 之外——更新它**不得**触碰 render_cache；改底部行数时
  必须同步 ui 层测试里硬编码的行号（status/queue/question/settings 四个测试）。
- **启动即新会话**：新进程不带 `resumeSessionId` 发 hello，桥接就地建会话（
  `hello.cwd` 工作区 + `hello.mode` 默认模式，失效回退 standard）；只有 CLI 会话
  id 与「记住上次会话」（默认关）走续接。`/resume` 打开选择器、`/resume <id>`
  直接 attach（都是纯客户端命令）。
- **/login 面板**（`login.rs` + `ui.rs::render_login`）：输入栏变登录设置页，
  三个字段的状态全部来自桥接 `login` 帧（客户端只发 `login-get` / `login-set`）。
  API key 永不回传、编辑态画 ●、环境变量来源只读；改 `render()` 参数（settings
  与 login 两个 Option<&mut>）时同步改所有调用点（ui 测试 + examples）。
- 新增交互键位后同步更新：`ui.rs` 的 `help_overlay`、README 速查表、input 测试。

### bridge（Node.js）

- 一切副作用挂 `ctx.effect()`；连接对象在 `conns` 集合，detach 必须清理监听、
  撤销待审批（`done('cancelled')`）。
- **快照/历史数据源**：活跃会话取 `agent.session.events`（内存，零磁盘读）；
  只有非常驻会话才回退 `persistence.readFrom(id, 0)`（全量读盘，慢，结果缓存到
  `conn.log`）。surface 列表按会话缓存并增量追加（`surfaceState`）。
- **负载裁剪**：`trimToolResultEvent`（模块级纯函数，导出为
  `_trimToolResultEvent` 供测试）——read 结果整段剥掉、其余工具只留末尾 2000
  字符（exit marker 在末尾）。实时事件转发也要过它，别恢复全量转发。
- 改协议字段时同步 `client/src/protocol.rs`（serde `rename_all_fields =
  "camelCase"`，客户端发 `beforeSeq`/`limit` 这类驼峰）。
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
- **model selection 必须装**：桥接创建/恢复的每个会话都要在 `setup` 里先
  `installModelSelection(agentCtx, { current, assembled: undefined })`（内联自
  `@deepseek-ai/dsh-agent` 的同名函数，两个 waterfall：`system-prompt/assemble`
  注入 `variables.{provider,model}`、`agent/request` 路由选中模型）——不装则
  persona 的 `{{model}}` 变量无值，每条消息都报
  `prompt variable "{{model}}" has no value (section "deployment:persona")`。
  `current` 取 `/new` 镜像的当前会话 provider/model（`mirror`），否则
  `ctx.get('agentDefaultModel').currentSelection()`；web/headless 入口都装，
  桥接不能漏。只 mount preset 不够——这是两个正交的 setup 步骤。
- **/login 字段落点**（桥接）：`login-get`/`login-set` 两个上行帧，
  `login{apiKeyConfigured,apiKeyWritable,apiKeySource?,apiKeyHint?,account?,
  proxy?,error?}` 下行帧。API key 走 `ctx.get('credentials')` 的
  `describe/set/unset(DEEPSEEK_API_KEY)`（**值永不回传**，只发 `…末四位` hint，
  环境变量来源 `writable=false`）；账号读写 `%DSH_HOME%\.anonymous-user-id`
  （裸 UUID 行，留空删除、下次启动重生成，即 `x-deepseek-harness-user-id`）；
  proxy 读写 `%DSH_HOME%\.env` 的 `HTTPS_PROXY` 行（`writeEnvLine` 只动这一行、
  保留换行风格，**重启 dsh web 生效**）。写失败经同一 `login` 帧的 `error` 回给
  面板，不走 transcript 错误流。

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
- 桥接侧无测试框架；纯函数用 `node -e` 从 profile 副本 import
  `_trimToolResultEvent` 做冒烟。

## 已知事项

- **重启 DSH 才能加载新桥接**：改动 `bridge/src` 后需 robocopy 同步 + 用户重启
  `dsh web`。旧桥接的启动全量读盘约 14s；新桥接活跃会话 <100ms。
- 设计文档里 M 里程碑编号已落后于实现（功能已超出 M6），以代码与 README 为准。
- `DSH_TUI_TIMING=1` 下各启动阶段耗时打印到 stderr，定位启动回归用。
