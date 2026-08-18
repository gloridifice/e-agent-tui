# DSH TUI 设计文档（草案 v0.5）

> 状态：D1–D30 已实施；后续实现修订以本文件当前章节与机器可读协议契约为准。
> v0.4 变更：消息格式规范（用户消息原样、shell 卡 spinner+行数、read 合并折叠）；输入栏无边框背景块 + 粘贴占位。
> v0.5 变更（v0.1.0 里程碑）：项目改名 **e**（可执行文件 **`dshe`**）；配置迁到
> `%APPDATA%\dshe\config.toml`、主题目录 `%APPDATA%\dshe\themes\`（默认
> **deepseek-e**，另内置 ferra）；新增 `/theme` `/model` `/reload` `/skill:<名称>`；
> 启动器 `dshe` 自动 spawn `dsh --profile dshe`（或 npx）／桥接已运行的 dsh；使用
> 专属 `dshe` profile，避免与 DSH 自带或用户已有的 `tui` profile 冲突。由 `dshe` 启动的
> Windows 服务在启动超时清理及最后一个 TUI 关闭时均以 `taskkill /T` 终止 `cmd /C` shim 的完整进程树，
> 避免遗留孤儿 Node；回收等待有上限，关闭失败保留零实例锁供下次 attach 重试；所有实例锁均须重新探测
> bridge endpoint，即使正实例计数也不能证明服务存活，服务已消失时清除 stale 锁并重建。
> 确认关闭后离开 alternate screen 并输出 `dsh 服务器已关闭。`。桥接外部启动的 DSH 或仍有其他 TUI 时不输出。
> v0.6 架构收敛：生产客户端模块图由 `client/tests/architecture.rs` 自动检查为无 SCC；
> transcript 只存公共 Display 表面；wire shape/fixture/doc 由同一 JSON contract 同步；配置用单一严格 schema；
> DSH model selection 通过公开上游 adapter 安装并有部署副本升级验证。

## 0. 已定决策（✅）

| # | 决策 | 结论 |
|---|------|------|
| D1 | 运行形态 | 独立客户端进程（`dshe`，项目名 **e** / e tui），通过桥接连到运行中的 DSH，可与 Web GUI 并存 |
| D2 | 布局 | 单列对话流（Claude Code 风格） |
| D3 | 客户端语言 | Rust（ratatui + crossterm + tokio-tungstenite + serde） |
| D4 | 项目构成 | TS 桥接插件（DSH 侧）+ Rust 客户端 |
| D5 | 主题 | deepseek-e（默认）与 ferra 两个内置主题；`%APPDATA%\dshe\themes\` 下所有合法 toml 皆可选 |
| D6 | 工具调用卡 | 行内单行卡（默认折叠，可展开） |
| D7 | 用户消息前缀 | `❯` 符号 |
| D8 | Markdown 渲染 | 首期完整渲染：标题/粗斜体/行内码/代码块/列表/引用/**表格**/**mermaid** |
| D9 | Mermaid 渲染 | 使用 grok-mermaid（WASM，来源 xAI Grok CLI / Simon Willison 提取版） |
| D10 | 复制模式 | vim 风格：行选择 / 块选择，复制 AI 输出，渲染内容可映射回原始 markdown |
| D11 | 块复制语义 | 表格、mermaid、代码块按整体复制（复制原始 markdown 源码） |
| D12 | 复制模式键位 | `Ctrl+B` 进入；跨块选择自动升级为整块 |
| D13 | 超长原子块 | 超过阈值（默认 40 行）允许折叠；mermaid 图单独处理 |
| D14 | 语法高亮 | 二期再上 syntect；首期代码块纯色 + 语言标签 |
| D15 | 桥接鉴权 | 轻量 token：桥接插件生成随机 token 写入 DSH 数据目录，客户端自动读取 |
| D16 | 桥接插件形态 | 正式 TS 插件包（可用 `ws` 库），作为产品一部分长期维护 |
| D17 | 启动行为 | 每个新 `dshe` 进程默认新建会话；CLI 会话 id 或“记住上次会话”（默认关）才续接；`/resume`/Ctrl+N 打开续接 Input Page |
| D18 | mermaid 超宽 | v1 截断 + 折叠提示（复制仍拿完整源码）；v2 全屏图形模式 hjkl 四向滚动 |
| D19 | 复制提示 | 复制成功后输入区临时提示 `已复制 N 行`，约 2 秒后消失 |
| D20 | 用户消息展示 | 逐字原样展示，不做 markdown 渲染；前缀 `❯` Coral |
| D21 | 命令执行工具卡 | 圆点 spinner（默认半月旋转 `◐◓◑◒` ~120ms/帧，Honey）；只显示命令 + 实时输出行数；exit 0→`✓`Sage，非 0→`✗`Ember；命令超行截断，输出展开查看 |
| D22 | read 合并与折叠 | 同一 turn 内相邻 read 合并为紧凑状态列表；全部结束折叠为 Bark 灰 `<a>, <b>, <c>`（只文件名，超宽截断 `+N`）；Enter 展开还原、Esc 收回 |
| D23 | 输入栏形态 | 无边框背景块：Ash 底、上边距 1 行 + 文本区 + 下边距 1 行；前缀 `❯` Coral；`Enter` 固定发送、`Shift+Enter` 换行；`↑/↓` 行间移动并在首/末行边界切换提示词 |
| D24 | 粘贴超长占位 | 粘贴超过配置阈值显示 Rose 色 `[N text pasted]`；发送原样完整内容；普通文本用 `Shift+Enter` 插入换行 |
| D25 | spinner 可配置 | 默认 A 半月旋转 `◐◓◑◒`（~120ms/帧）；帧序做成可配置枚举（`config.toml` 可换 B/C/D/E）；字体缺字形自动降级 ASCII `\|/-\` |
| D26 | Input Page | `/settings` `/login` `/model` `/theme` `/resume` 统一替代输入区（非浮窗）；上下 1 行、左右 2 列内边距；单焦点用方向键/`hjkl` 移动、`Enter` 执行、`Esc` 返回 |
| D27 | 配置存储 | 默认值唯一来源为 `client/assets/default_config.toml`（`include_str!` 嵌入并解析）；`%APPDATA%\dshe\config.toml` 是允许缺字段的覆盖层；优先级 嵌入默认值 < 用户文件 < 运行时；**即改即存、即时生效** |
| D28 | TUI 内可改项 | 见 §4.7 清单：外观/行为/显示三类全部可改，高级类只读 |
| D29 | 不提供 TUI 修改 | 连接参数（启动 flag）、字体字号（终端侧）、剪贴板后端（平台）、键位重绑定（v2）、语法高亮主题（二期） |
| D30 | 发送键语义 | 固定 `Enter` 发送、`Shift+Enter` 换行；旧配置 `enter_sends` 仅保留反序列化兼容，不再改变交互 |

## 1. 目标与形态

- TUI 是 DSH Web GUI 的**第二个前端**：复用同一会话日志与事件流，渲染到终端。
- 单会话视图：一个终端窗口聚焦一个会话；多会话通过选择器切换。
- 设计原则：
  1. 消息流是主角，界面元素尽量不遮挡内容；
  2. 键盘优先，鼠标（滚轮/点击）作为增强；
  3. agent running 时用户仍可输入（消息排队），不用干等；
  4. **AI 输出是富内容**：markdown/表格/mermaid 完整渲染，且复制时拿到的是原始源码而非渲染残片。

## 2. 总体架构

```
┌─────────────────────┐        WebSocket         ┌──────────────────────┐
│  DSH 进程            │  ws://127.0.0.1:PORT/   │  dshe (e, Rust 进程) │
│  TS 桥接插件 bridge/ │ ◄────────────────────► │  ratatui 渲染        │
│  · session/event 流  │   JSON 协议（见 §5）     │  · 消息流 / 输入区    │
│  · agent/status      │                          │  · markdown+表格渲染  │
│  · commands.execute  │                          │  · mermaid (WASM)    │
│  · approval/提问     │                          │  · 复制模式(source map)│
└─────────────────────┘                          └──────────────────────┘
```

关键事实：`webServer.registerUpgrade(path, handler)` 的 handler 收到 Node 原生
`(req: IncomingMessage, socket: Duplex, head: Buffer)`——插件自行完成 WebSocket 握手与帧处理。
桥接层可用正式 TS 插件 + `ws` 库（推荐），理论上也可动态插件手写 RFC6455（见 O8）。

### 2.1 客户端渲染管线（核心架构，支撑 D10/D11）

```
markdown 源码
  │  pulldown-cmark 解析（保留每个块在源码中的区间/原始文本）
  ▼
块序列：Paragraph / Heading / CodeBlock / Table / Mermaid / List / Quote ...
  │  每个块 → 渲染为 RenderUnit，携带 source 元数据
  ▼
RenderUnit { kind, source: { blockType, raw: String }, cells: RenderedCells }
  │  raw = 该块在原始 markdown 中的完整源码文本
  ▼
屏幕缓冲区：每个渲染行登记其所属 RenderUnit（行 → 块映射表）
  │
  ▼
复制模式：光标所在行 → 查映射 → 命中 table/mermaid/code 块 ⇒ 整块选中
          普通文本 ⇒ 按行/字符选择，复制对应 raw 行/区间
```

- 复制的永远是**原始 markdown 源码**：表格复制出 `| a | b |` 竖线源码、标题复制出 `## 标题`、
  mermaid 复制出 ` ```mermaid ... ` 围栏源码。
- 行映射表随渲染增量维护；复制模式不重新解析，只查表。
- 表格/mermaid/代码块为**原子块**：光标落入块内任意位置即整块高亮，不提供块内局部选择。

### 2.2 依赖方向、运行时与投影边界

客户端生产模块遵守单向依赖，并由 `client/tests/architecture.rs` 的源码 edge scanner、Tarjan SCC
检查及禁止反向边断言持续守卫：

```text
main（Tokio composition root）
  ├─ runtime / runtime_ports ──> command_catalog / page_core
  ├─ input / runtime_command ──> command_catalog
  ├─ input_page ──> page_core + settings/login/model/theme/resume
  └─ ui + copy ──> transcript_layout ──> display/render/config

protocol（typed DTO + HostEvent family parser）
  └─ projection/{assistant,tool,lifecycle,retry,command,workflow,surface}
       └─ TranscriptStore（仅 DisplayItem 公共表面）
```

`RuntimeController` 接收已类型化的 bridge frame、终端事件和 deadline，并在持有短生命周期
状态锁时只产出内部 action 或完整 `RuntimeEffect`。`main.rs` 仅负责 `tokio::select!`、有界
inbound、deadline、terminal 生命周期与 effect executor；传输、终端事件、配置/状态文件、剪贴板、
时钟及 launcher 的进程/锁均通过窄 port 适配，因此脚本化替身可验证竞态而不在锁内 await/I/O。

生产 `AppState` 只以 `TranscriptStore` 保存 `ActivityRow`、`TranscriptBlock`、`ContentCard` 与
composite `DisplayItem`。`EventProjector` 和 family projection 是唯一的 HostEvent→显示入口；
`LegacyTestMsg` 仅保留在 `#[cfg(test)]` characterization fixture，不能重新进入 production renderer、
cache 或 copy path。`transcript_layout` 是 UI 与 copy 共用的 width/generation/provenance 布局内核，
因此 tail splice、activity range patch、history anchor 和原始 Markdown copy 使用同一行语义。

## 3. 视觉设计

### 3.1 整体布局

```
┌──────────────────────────────────────────────────┐
│ • standard deepseek-v4-pro CH80%     ^h Help │ ← 底部状态第一行（无背景）
├──────────────────────────────────────────────────┤
│ ❯ 把 foo 函数重构一下                             │ ← 用户消息：原样展示
│                                                  │
│ 🤖 我先读一下文件…（流式）                        │ ← 消息流（可滚动，占主体）
│    ◐ npm run build · 128 行                      │ ← 命令卡：spinner 转动 + 命令 + 实时行数
│    ◐ 读取中                                       │ ← 连续 read 合并为紧凑列表
│      src/foo.ts   ✓ 已读                          │
│      src/bar.ts   ◐ 读取中                        │
│      src/baz.ts   … 排队                          │
│    （全部结束 →） <src/foo.ts>, <src/bar.ts>, <…> │ ← 折叠成灰色单行，只留文件名
│    数据如下：                                     │
│    ┌──────────────────────────────────────────┐  │
│    │ 列A    列B    │  ← markdown 表格（框线渲染）│  │
│    └──────────────────────────────────────────┘  │
│                                                  │
│ ⚠ 审批 · 允许写入 src/foo.ts？                   │ ← 审批卡（固定输入区上方）
│    [Y] 允许   [n] 拒绝   [i] 查看详情            │
├──────────────────────────────────────────────────┤
│▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓│ ← 输入栏：无边框背景块
│▓ ❯ 输入消息…                       [Ctrl+H 帮助]▓│    上/下边距行 + 文本区（≤3 行滚动）
│▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓│    复制模式下切换为 -- COPY -- 指示条
└──────────────────────────────────────────────────┘
```

### 3.2 角色视觉语言（ferra 256 色）

| 角色 | 视觉 | 说明 |
|------|------|------|
| 用户消息 | Coral `#ffa07a` 前缀 `❯`，正文 Mist | **逐字原样展示，不渲染 markdown** |
| assistant 文本 | 正文 Mist `#d1d1e0`，左侧 1 格 Sage `#b1b695` 色条 | 与工具卡区分 |
| 命令执行卡 | spinner（默认半月旋转 `◐◓◑◒` ~120ms/帧，可配置）Honey；结束 `✓`Sage / `✗`Ember | 只显示命令 + 实时输出行数 |
| read 合并列表 | 缩进 2 格；`◐`Honey=读取中 `✓`Sage=已读 `✗`Ember=失败 `…`Bark=排队 | 结束后折叠为灰色单行 |
| read 折叠消息 | Bark `#6f5d63` 灰，`<a>, <b>, <c>` 只留文件名 | Enter 展开还原、Esc 收回 |
| 其他工具卡 | Bark 行内卡，缩进 2 格 | `●`Honey=运行中 `✓`Sage=成功 `✗`Ember=失败 |
| system / 提示 | Bark `#6f5d63` | 会话开始、compaction 提示 |
| 错误 | Ember `#e06b75`，`✗` 前缀 | agent/error、工具失败 |
| 审批卡 | Honey `#f5d76e` 边框块 | 详情可展开 |
| 链接/强调 | Blush `#fecdb2` / Rose `#f6b6c9` | markdown 行内 |
| 粘贴占位 | Rose `#f6b6c9` 的 `[N text pasted]` | 单行模式下的长粘贴折叠 |
| 输入栏 | Ash `#383539` 背景块，无边框；前缀 `❯` Coral | 上/下边距行 + 文本区 |
| 复制模式选区 | 选区反色（Night↔Mist 反转）；原子块整块选中用 Umber 底 | 高对比可见 |

### 3.3 消息渲染规范

#### 3.3.1 用户消息

- 逐字原样展示用户输入文本，**不解析 markdown**；前缀 `❯` Coral。
- 消息块为上/下边距行 + 文本区，水平内边距（gutter，默认 2 列）作用于**每一行**——
  超宽文本按页面宽度折行后，续行同样保持 gutter。
- 粘贴的超长内容照常发送；展示层按 §4.1 的占位规则处理。

#### 3.3.2 工具消息格式规范

**命令执行类（shell/bash/pwsh 等）** —— D21

```
◐ npm run build · 128 行        ← 运行中：spinner 转动（Honey）+ 命令 + 实时输出行数
✓ npm run build · 128 行        ← exit 0：Sage 绿
✗ npm run build · 64 行         ← exit 非 0：Ember 红 x
```

- 运行中不展示输出正文，行数随输出实时增长；命令文本超一行截断（尾部 `…`），截断列数按居中页面的实际内容宽度计算（含“页面最大宽度”设置），不得按外层终端宽度计算后再在窄页面内折行。
- 结束：`✓`/`✗` 颜色即退出码语义；**用户要求的"红 x"** 即非零退出码的 `✗`。
- 展开（Enter）：查看完整命令与输出/stderr（含折叠规则见下）。

**文件读取类（read 等）** —— D22

```
◐ 读取中                        ← 组头：spinner + 组状态
  src/foo.ts    ✓ 已读
  src/bar.ts    ◐ 读取中
  src/baz.ts    … 排队
──────────────────────（全部结束后折叠为）──────────────────────
<src/foo.ts>, <src/bar.ts>, <src/baz.ts>    ← Bark 灰，只显示文件名
```

- 合并规则：同一 turn 内**相邻**的 read 类调用（中间无 assistant 文本、无其他类型工具）归为一组。
- 组头在组内仍有未完成项时显示 spinner；单文件读取也走同一列表形态（1 项）。
- 折叠行超宽时截断并显示 `+N`（如 `<a>, <b>, <c> +2`）；Enter 展开还原列表、Esc 收回。
- 失败文件：`✗` Ember + 文件名；展开可看错误详情。

**其余工具（write/edit/search 等）**：维持 D6 行内单卡——`●` Honey 运行中 → `✓` Sage / `✗` Ember，
格式 `✓ 工具名 参数摘要 · 耗时`。

#### 3.3.3 Markdown 与富内容

- Markdown：标题、粗体、斜体、行内代码、围栏代码块（语言标签 + 纯色；syntect 仍属后续）、
  有序/无序列表、引用、分隔线、**表格**（框线渲染，列宽自适应，超宽截断标注）、**mermaid**。
  标题直接使用 `semantics.markdown.heading1..6`；ferra 下一级为 Coral `#ffa07a` 粗体且
  无背景，二级为 Sage `#b1b695` 粗体，三级为 Blush `#fecdb2` 非粗体。
- 行内代码：背景色严格限制在 inline-code chip 自身（含 chip 内边距）；源码中的后续分隔空格和
  行尾未使用单元保持普通行背景。整行背景补齐只认行级 `Line.style.bg`，不得从局部 span 推断。
- 代码块：左侧竖线边框 + 顶部语言标签；v1 超宽折行，横向滚动 v2。
- mermaid：grok-mermaid WASM 渲染为 Unicode 框图，渲染失败时降级显示源码围栏块（可复制）。
- 长内容折叠：普通长文本/工具结果 > N 行（默认 20）折叠，显示头尾 + `… [Enter] 展开`。
  （原子块表格/mermaid/代码块不折叠，见 O12 是否例外）
- 流式渲染：token 级追加；未上翻自动跟随底部，上翻暂停跟随并显示 `↓ 新消息` 指示。

### 3.4 两层主题系统与 ferra 色板

主题 TOML 分为两层：

1. `[colors]` 是开放色盘，键名完全由主题作者定义，值必须是 6 位十六进制颜色；运行时不依赖
   `night`、`ok` 等固定色名。
2. `[semantics.*]` 是固定语义 schema，包含 `surface`、`markdown`、`input`（含状态栏）、
   `working_status`、`log`、`activity`、`card`、`overlay`。每个固定角色是样式对象，仅 `fg`
   必填，`bg`、`bold`、`italic`、`underline` 可选，颜色值引用 `[colors]` 中的用户色名。

```toml
[colors]
night = "#2b292d"
blush = "#fecdb2"

[semantics.markdown]
heading3 = { fg = "blush" }
inline_code = { fg = "blush", bg = "night" }
```

缺少固定语义字段、出现未知语义字段、引用不存在的色名或使用非法颜色时，整个主题文件无效并
从主题目录中跳过。内置 `deepseek-e` 与 `ferra` 也不是 Rust 硬编码色板：源文件位于
`client/assets/themes/`，通过 `include_str!` 编译进程序并使用同一个解析器；首次加载时原样复制到
`%APPDATA%\dshe\themes\`，不覆盖用户已有文件。合法同名用户主题优先于嵌入版本，非法旧文件不会
遮蔽内置回退。解析后的固定语义样式缓存在 `Config.resolved_theme`，渲染期不读取磁盘。

Ferra 色板来源于 casperstorm/ferra README：

| 名称 | Hex | 用途（客户端） |
|------|-----|----------------|
| Night | `#2b292d` | 客户端背景色 / 代码块与行内代码底色 |
| Ash | `#383539` | 卡片/输入栏/用户消息块底色 |
| Umber | `#4d424b` | 选区底色（原子块选中） |
| Bark | `#6f5d63` | 次要文本 / 工具卡 |
| Mist | `#d1d1e0` | 正文前景 |
| Sage | `#b1b695` | assistant 色条 / 成功 / 二级标题（粗体） |
| Blush | `#fecdb2` | 链接 / 三级标题（非粗体） |
| Coral | `#ffa07a` | 用户 `❯` / 用户高亮 / 一级标题（粗体、无背景） |
| Rose | `#f6b6c9` | 强调 / 行内代码 |
| Ember | `#e06b75` | 错误 / 失败 |
| Honey | `#f5d76e` | 运行中 / 审批卡 / 警告 |

- 终端侧：官方 Windows Terminal 配色方案（[ferra ports/windows terminal](https://github.com/casperstorm/ferra/tree/main/ports/windows%20terminal)）。
- 真彩色（24-bit）为主；降级环境回落到 ferra 最近似的 256 色索引。
- 支持 `NO_COLOR` / `--no-color` 纯文本模式。

### 3.5 状态呈现

- 页面底部固定两行状态，均不设置背景色。第一行最前是状态符号 `•`（与工具卡一致：
  running 时黄色呼吸，空闲灰色），随后依次显示当前 agent preset 模式、当前模型和
  `CH<缓存命中率%>`；当前模型或 CH 暂无值时整项省略，不显示占位横线。CH 按 provider usage 的
  `cacheRead / (input + cacheRead + cacheWrite)` 累计计算。历史前插增加旧 usage 总量，但保留最新 request 的替换锚点；
  mode 初值由 `welcome.mode` 下发（最近 selection，缺省为创建 header），之后按
  `agent-preset/selected` 的 event seq 保留最新值，旧页不得回退。右侧固定 `^h Help`。
- 第二行左侧显示当前会话标题（无标题时为 `新会话`），右侧显示会话工作区绝对路径；
  标题过长时以 `…` 截断，优先保留路径。
- 复制模式：输入区切换为指示条 `-- COPY --`（§3.1），显示选中行数/字节数与可用键。

### 3.6 续接会话 Input Page（/resume / Ctrl+N）

- 与其它 Input Page 一样替代输入区而非覆盖 transcript：输入文字按标题或 id 筛选，↑↓ 选择，Enter 续接，Esc 返回。
- 页面先显示加载态；bridge 的 `session-list` 边界在持久化 header 列表完成后立即下发 `sessions{titlesPending:true}`，再只对最多 200 条候选折叠标题并下发最终 `sessions`。因此慢磁盘标题读取不会阻塞列表首屏。

### 3.7 帮助浮层（? / Ctrl+H）

- 半屏覆盖层：当前模式全部快捷键；q/Esc 关闭。进入复制模式后帮助浮层展示 copy-mode 键位。

## 4. 交互设计

### 4.1 输入模型（D23/D24）

**形态：无边框背景块**（Ash `#383539` 底，无框线）：

```
▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓   ← 上边距 1 行（纯背景色）
▓ ❯ 输入文本…                   ▓   ← 文本区：单行模式 1 行
▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓   ← 下边距 1 行（纯背景色）
```

- 文本区：单行模式 1 行；多行模式最多显示 **3 行**，内容超出时区内滚动、光标所在行保持可见；
  超宽内容在输入栏内**自动折行**（折行后窗口同样跟随光标）。屏幕光标由输入栏绘制反色块；终端硬件光标
  始终隐藏，仅在每帧完成后移动到同一位置作为 IME anchor，避免差量绘制时在运行状态灯与输入栏间闪动。
- 前缀 `❯` Coral，与用户消息前缀一致；输入文本 Mist。
- **粘贴占位**（bracketed paste）：粘贴内容 > **64 字符** → 输入栏显示 Rose 色
  `[N text pasted]` **粘贴块**，块内文本不展开；粘贴块是原子的——光标不可进入其内部
  （←/→ 整块跳过），退格/Delete 删除整个占位内容；直接 Enter 发送时内容原样完整发送。
- **预发队列**：AI 运行中按 Enter 发送的提示词不立即发出，而是进入客户端队列，
  在输入栏上方逐行显示（Night 底 Bark 字，左缩进 2 空格 `* ` 前缀，每项一行，
  超宽 `…` 截断；行数受面板高度限制，超出显示 `… 还有 N 条`）。AI 回到空闲后
  **自动逐条发出**（每条发出即开始新 turn）；`Esc` 中断会清空尚未发出的队列；
  切换会话时队列随之清空。空闲状态发送的提示词仍即时发出。客户端实现中，取出队首
  与开始 Thinking 必须在同一个短生命周期状态锁内完成，并在 WebSocket `.await` 前
  释放锁，保证自动派发时输入循环不被自死锁。
- 输入键固定为 `Enter` 发送、`Shift+Enter` 插入换行；`↑↓` 保持字符列在输入行间移动，
  只有光标已在最上行/最下行时才切换上一条/下一条历史提示词；`Ctrl+R` 搜索历史，
  `Tab` 补全。`PageUp`/`PageDown` 按当前可见 transcript 高度翻页，鼠标滚轮每格移动 3 行；
  二者始终滚动消息流，即使 Input Page 已打开。`Ctrl+H` 作为全局帮助键先于页面分发处理，
  修饰过的 `hjkl` 不参与页面焦点导航。

### 4.1.1 命令范式：内置命令与接入命令

命令统一使用 `/name [raw input]` 交互和同一个补全浮层，但按兼容深度分两类：

1. **内置命令**：dshe 做过交互优化的命令。`client/src/runtime_command.rs` 的
   `BUILTIN_COMMANDS` 是唯一注册表；一项同时声明 name、description、DSH 风格 input hint、
   参数补全策略和 action。注册一项就同时注册行为与补全，`input.rs` 不得维护第二份命令表。
   当前 `/settings`、`/login`、`/new`、`/resume`、`/model`、`/theme`、`/reload`、
   `/skill`、`/compact`、`/goal`、`/plan`、`/copy`、`/clear`、退出别名都属于内置项；
   其中 `/new ` 以 bridge 的 preset roster 做参数级补全；输入完整 `/skill` 即切换到当前会话
   user-invocable skill roster，继续按名称模糊过滤并补成规范 `/skill:<name>`。
2. **接入命令**：DSH 原生命令或其它 DSH 插件注册的命令。bridge 在 attach 后调用
   `ctx.commands.list(agent)` 自动获取该 agent 的有效目录（全局定义 + agent-scoped shadow），
   下发 handler-free `commands` 帧；`commands/change` 发生时为每条连接重新计算，而非要求
   dshe 发布版本。客户端与内置目录合并、同名时内置优先，统一按前缀→子串→子序列模糊
   补全。执行仍发 `command{line}`，bridge 调 `commands.execute`，将 direct UI outcome 通过
   `command-result` 显示为 System/Error；未知命令报错，绝不降级成模型 user message。

DSH 0.1.0-rc.6 的公开 [`CommandDescriptor`](https://deepseek-harness.github.io/deepseek-harness/en/reference/subsystems/commands)
只有 name、description 和可选 `input.hint`（free-form text），没有 typed argument
completion schema。因此接入命令都支持
**命令名补全**并展示参数 hint；参数候选补全只有被提升为内置优化项后才能提供。命令执行是
异步的，bridge 必须在 await 前捕获 current conn，返回结果前校验连接仍挂在同一会话。

### 4.2 快捷键表（v1 提案）

| 键 | 功能 | 备注 |
|----|------|------|
| Enter | 发送输入栏消息 | 单行/多行一致 |
| Shift+Enter | 输入换行 | |
| ↑ / ↓ | 输入行间移动；边界切换提示词 | 保持字符列 |
| Ctrl+R | 历史反向搜索 | |
| Tab | 命令补全 | |
| Ctrl+C | running→中断 turn；idle→退出 | |
| Ctrl+L | 重绘 | |
| PgUp / PgDn | 按当前可见 transcript 高度翻页 | 上翻暂停自动跟随 |
| 滚轮 | 每格滚动消息流 3 行 | Input Page 打开时仍只滚消息流 |
| Esc | Input Page 返回/关闭；取消输入或关闭浮层 | |
| 方向键 / hjkl（Input Page） | 移动唯一焦点 | 文本编辑态 hjkl 为文字 |
| Enter（折叠卡） | 展开/收起工具结果 | 焦点导航 v2 |
| Ctrl+N | 续接会话 Input Page | 输入筛选，↑↓ 选择 |
| /settings | 设置面板（§4.7） | 即改即存 |
| ? / Ctrl+H | 帮助浮层 | |
| **Ctrl+B** | **进入复制模式** | D12 |

### 4.3 复制模式（D10/D11，vim 最简子集）

进入后：输入区变为 `-- COPY --` 指示条；消息流进入可导航状态；光标起始于最近一条
assistant 消息顶部。**复制的永远是原始 markdown 源码**（经 §2.1 行映射表）。

| 键 | 功能 |
|----|------|
| h j k l | 移动光标（左/下/上/右） |
| w / b / 0 / $ | 词跳 / 行首 / 行尾 |
| g / G | 流首 / 流尾 |
| Ctrl+u / Ctrl+d | 半屏上 / 下 |
| **V** | 按行选择（从当前行开始） |
| **Ctrl+V** | 矩形块选择（普通文本区域内） |
| **y** | 复制选中内容到系统剪贴板，退出复制模式，输入区提示 `已复制 N 行` |
| Esc / q | 退出复制模式 |
| Enter | 光标在折叠块上时展开/收起（表格/mermaid/代码块不受折叠影响） |

**原子块语义（D11）**：光标移动到表格/mermaid/代码块的任意渲染行 ⇒ 整块自动选中
（Umber 底色高亮）；`y` 复制该块在原始消息中的完整 markdown 源码（含围栏/竖线语法）。
普通文本支持 V 行选与 Ctrl+V 块选；跨块选择以块为单位扩展（见 O13 细节）。复制模式
按键只在持有状态锁时计算 `CopyAction`，移动视口、展开单元或报告剪贴板错误等后续动作
必须在该锁释放后执行，避免同线程重入状态锁导致输入循环冻结。

### 4.4 审批与用户提问

- 审批卡固定输入区上方，不阻塞消息流（背景继续渲染）。
- 审批：`Y/n/i` 或 `←→` + Enter；被 Web GUI 抢先答复时卡片自动消失并提示。
- 用户提问（ask_user_question，单选/多选批）：问题面板固定在输入区上方
  （标题 + 问题 + 当前选项说明），**输入栏变为选择栏**——`←→` 在选项间切换，
  Enter 选中当前项并进入下一题；最后一题 Enter 确认整体提交；Esc 取消整批。
  无预设选项的问题退化为文本输入（直接打字，Enter 提交为 custom）。
  被 Web GUI 抢先答复/中止时（question/resolved）选择栏自动消失。
  桥接通过 apiproxy mux 帧（question/requested、question/resolved）转发，
  答复经 apiProxy.respond 的 client-response 回传——Web 与 TUI 均可回答，
  宿主以先到者为准。

### 4.5 中断语义

- `Ctrl+C` 一次 = 中断当前 turn；agent 回 idle 后输入立即可用；idle 再按 `Ctrl+C` = 退出。

### 4.6 会话管理

- 启动：**每个新 TUI 进程默认新建一个会话**（多进程各自一屏一会话）——客户端
  不带 `resumeSessionId` 发 `hello`，桥接就地创建：工作区取 `hello.cwd`（TUI 启动
  目录），模式取 `hello.mode`（/settings「默认模式」的 preset id，失效时桥接回退
  `standard`，再回退 roster 默认）；`dshe <session-id>` 或开启「记住上次会话」
  （默认关）则改为续接指定/上次会话。**续接对冷会话友好**：id 不在活跃注册表时
  （宿主重启后必然如此），桥接先经 `sessionPersistence` + `agents.resume` 按会话
  记录的 preset（`agent-preset/selected` 事件 > header）恢复该持久化会话，恢复
  不了（未持久化/preset 已删除）则新建会话——**绝不因"会话不活跃"断连**。仅创建
  失败（agents 缺失等）才以错误帧 + 4001 结束。
- `Ctrl+N` / `/resume` 打开续接会话 Input Page；`/resume <session-id>` 直接 attach 切换
  （页面/`/resume` 对冷会话同样先 resume，找不到才报错、不断连）。会话列表先下发 header 与在线日志可直接取得的标题，再异步补齐持久化标题；`sessionQuery.readTitleSnapshots` 的 settled result 必须从 `fulfilled.value.title.title` 解包，不能把结果误当成扁平 `{sessionId,title}`。
- 状态栏下方固定一行显示当前会话（无背景色）：左侧标题、右侧工作区路径。标题由
  `welcome.title`（会话日志最近一条 `session/title`，由桥接在 attach 时读取）
  初始填充；冷恢复会话日志不在内存，桥接经 `sessionQuery.readTitleSnapshots`
  补发 `title{title}` 帧；此后 `session/title` 事件经普通 event 帧实时更新
  （客户端只更新该行，不重建 transcript 缓存）。路径由 `welcome.cwd`（会话头部
  `header.cwd`，桥接在 attach 时读取）填充；标题过长以 `…` 截断以保住右侧路径，
  标题为空时显示 `新会话`，路径为空时右侧留空。
- `/new`：新建会话并切换（保留旧会话）。新会话落在 **TUI 启动目录**的工作区——
  客户端在 `hello` 里带上 `cwd`，桥接用它（校验为真实目录后）作为 `agents.create`
  的 `meta.cwd`，再把新会话 `attachSession` 进该 cwd 的 workspace 台账（与 host
  `session.create` 的两步一致）；客户端没发 cwd（旧客户端）时回退当前会话头部的
  cwd / `process.cwd()`。dshe 的裸 `/new` 会展开为 `/new <config.default_mode>`，因此设置页切换默认模式后立即影响下一次新建；显式 `/new <模式>` 仍是一次性覆盖。兼容旧客户端时，bridge 收到真正的裸 `/new` 才继承当前会话 agent preset。bridge 在 `agents.create` 的 `setup` 里 `agentPresets.mount`，只写 header 不 mount 会拿不到 preset 的工具与提示词。
- `/new <模式>`：按 agent preset id 新建会话（standard/code/minimal/cordis 及
  用户自建 preset）。桥接在每次 attach 后下发 `presets` roster 帧（id/name/
  description/order/broken）；客户端在输入 `/new `（含尾部空格）时弹出模式提示
  （同命令提示的 ↑↓/Tab/Enter/Esc 语义，按 id/显示名前缀-子串-子序列模糊匹配，
  broken 的 preset 不下发）。未知模式报错并列出可用 id。
- **model selection**：桥接创建/恢复的每个会话都在 `setup` 里先通过
  `bridge/src/model-selection.js` adapter 安装公开的
  `@deepseek-ai/dsh-agent@0.1.0-rc.6` package-root
  `installModelSelection(agentCtx, selection) -> disposer`。它在
  `system-prompt/assemble` 注入 `variables.{provider,model}`，并由该次 assembly
  snapshot 在 `agent/request` 路由，避免 persona 的 `{{model}}` 变量缺失。
  `/new` 镜像当前会话的 provider/model，其余取 `agentDefaultModel.currentSelection()`；
  adapter 安装与 preset mount 是两个正交步骤，且前者先执行。桥接不维护本地 waterfall 副本；
  `bridge/package.json` 精确声明已验证的 agent peer，DSH 升级后须运行
  `npm run verify-dsh-upgrade`，它检查 contract、host/agent version/export 以及部署副本的
  `/new`、cold resume、`/model` assembly/request 路由。

### 4.7 Input Page 与设置页面（D26–D30）

- **统一范围**：`/settings`、`/login`、`/model`、`/theme`、`/resume` 由一个
  `Option<InputPageSession>` 互斥管理。它们不是 overlay：不开浮窗、不画边框、不 `Clear`，
  而是**替代输入栏并占页面高度 2/3**，消息流保留在上方。
- **公共形态**：Ash 背景；所有内容外固定上下各 1 行、左右各 2 列空白内边距；公共
  header/body/footer 提供标题、正文、加载/错误和键位提示。只有当前可执行元素使用 Night
  焦点背景，当前已选值另以绿色 `●` 表示。
- **公共键位**：方向键与 `hjkl` 在稳定焦点图的可执行元素间移动，`Enter` 执行，`Esc`
  取消编辑/返回/关闭；只读、加载、信息和不可用元素不获得焦点。文本编辑态优先消费字符，
  因而 `hjkl` 会正常输入而不会导航。动态 provider/model/proxy/session roster 按稳定 id 保留焦点；`/resume` 的筛选框始终处于文本输入态，只用 ↑↓ 移动会话选择。
- **settings**：分类页签本身可聚焦，Enter 激活分类；Down 进入该分类的可编辑条目，
  Enter 打开数值或选择编辑。分类内条目位置按页记忆，超出可视高度时自动滚动。
  ```
                外观  行为  显示  高级
    主题                 ● ferra   ○ custom
    ferra 预设或自定义色板（自定义色板在 TOML 中手改）
    纯色模式              ○ 开   ● 关
    降级为纯色输出（NO_COLOR 语义）
  hjkl/方向键移动  Enter 执行  Esc 退出 · 即改即存
  ```
- **两列**：左列为较小的名称列（30%）：名称 fg、说明 **Bark** 前景（过长换行）；
  右列为值。
- **选中高亮**：只选中**名称**（Night 底），描述不高亮；编辑时焦点移到值上，
  名称取消高亮。
- **值呈现**：选项未被选中 = `○ 文字`（默认 fg）；被选中 = `● 文字`（绿色）。
  布尔值即 开/关 两选项；输入类（数值）直接显示文本，编辑时绿色 + 光标块（Night
  底）；选择类编辑时 `←/→` 移动光标，光标所在选项 Night 底。
- **编辑语义**：`Enter` 确认修改、`Esc` 取消退回；编辑期间按键不外泄。退出面板
  后消息流/输入栏状态原样恢复。
- 默认配置：`client/assets/default_config.toml` 通过 `include_str!` 编译进单 exe，启动时解析为
  `Config::default()`；默认值不得在 Rust 中维护平行字面量。持久化 `Config` 本身是唯一
  `#[serde(deny_unknown_fields)]` schema，运行时 resolved theme 用 `#[serde(skip)]` 缓存。
  加载时先把用户 TOML 的已知键递归覆盖嵌入 TOML，再严格反序列化一次；旧文件缺字段继承默认，
  已废弃未知键被过滤，已知键类型错误或 malformed TOML 则诊断后整体回退嵌入默认值。
- 保存：**即改即存**写入 `%APPDATA%\dshe\config.toml` 并即时生效。

**可配置项清单（TUI 内可改）**

| 分类 | 项目 | 类型 | 默认 |
|------|------|------|------|
| 外观 | spinner 样式（A/B/C/D/E） | 枚举 | A 半月旋转 |
| 外观 | spinner 帧率 | 数值 ms | 120 |
| 外观 | 主题（从 `%APPDATA%\dshe\themes\*.toml` 选择；色盘与语义映射在两层 TOML 中编辑） | 枚举 | deepseek-e |
| 外观 | 纯色模式（NO_COLOR） | 布尔 | 关 |
| 行为 | 记住上次会话 | 布尔 | **关**（新进程默认新建会话） |
| 行为 | 默认模式（裸 `/new` 与新进程建会话使用的 preset，来自桥接 `presets` roster；配置值已失效时仍可显示/选择，桥接回退 standard） | 枚举 | standard |
| 行为 | 粘贴占位阈值 | 数值字符 | 1000 |
| 行为 | 长内容折叠阈值 | 数值行 | 20 |
| 行为 | 原子块折叠阈值 | 数值行 | 40 |
| 行为 | 复制提示停留时长 | 数值秒 | 2 |
| 行为 | 输入历史条数 | 数值 | 1000 |
| 显示 | 工具耗时显示 | 布尔 | 开 |
| 显示 | read 自动合并 | 布尔 | 开 |
| 显示 | 消息时间戳 | 布尔 | 关 |
| 显示 | mermaid 渲染（关 = 源码围栏） | 布尔 | 开 |
| 高级 | 桥接地址 / 端口 / token 路径 | 只读展示 | — |

**不提供 TUI 内修改（D29）**：连接参数（改则断连，仅启动 flag / 环境变量）、
字体字号（终端侧）、剪贴板后端（平台决定）、键位重绑定（v2）、语法高亮主题（等 syntect 二期）。

### 4.8 登录设置（/login，D33）

- **入口**：输入 `/login`（仅命令，无快捷键）；输入栏变为登录页，形态与
  /settings 一致（无边框 Ash 底、占页面高度 2/3）。登录页是一层二选一菜单：
  **API key / Proxy**，Enter 进入对应子页面，Esc 逐层返回。
- **API key**：子页面列出模型提供商（`ctx.llm.listProviders()`）；Enter 进入该
  提供商的 key 填写。落点走 `ctx.credentials` 的该提供商 `apiKeyEnv` 引用（
  `providerCredentialRef` 从 settings 读取，缺省回退 `<ID>_API_KEY`），
  `credentials.set/unset` 写入后经 `credentials/updated` 即时生效。**密钥值永不
  回传**——下行只带 `configured/writable/source/hint(…末四位)`；编辑框输入画 ●，
  环境变量来源只读且不获得可执行焦点。
- **Proxy**：列出已保存代理 + `+ New`；已有代理 Enter 先进入“取消/删除”确认页，只有
  显式聚焦“删除”并 Enter 才发送 `login-proxy-delete`。新建表单填 base url / api key /
  协议模式（`openai-completions` / `openai-responses` / `anthropic-messages` 二选一）/
  模型名称。落点存 `%DSH_HOME%\dsh-tui-proxies.json`（api key 不回传）。
- **错误呈现**：写失败由桥接经同一 `login` 帧的 `error` 字段回传，显示在面板页脚
  （红色 ✗），不走 transcript 错误流。

### 4.9 模型与主题 Input Page

- `/model` 是单焦点双栏页：provider 在左、所选 provider 的 model 在右；左右键/h/l
  跨栏，上下键/j/k 在栏内移动，Enter 聚焦 provider 时激活该栏、聚焦 model 时发送
  `model-set`。绿色 `●` 只表示当前已应用模型，Night 背景表示当前焦点，二者不可混同。
  catalog 异步刷新时按 provider/model id 保留焦点；空 catalog 只显示说明，不创建假焦点。
- `/theme` 以主题名为可执行焦点，色块仅为装饰；Enter 应用并持久化主题。两页都使用
  §4.7 公共 shell，不再使用居中浮窗；终端过小时采用有界裁剪，不产生越界区域。

## 5. 桥接与协议（wire protocol v4）

### 5.1 端点与安全

- 端点：`ws://127.0.0.1:<dsport>/dsh-tui`；v1 仅 loopback；鉴权见 O7。

### 5.2 消息协议（JSON，单一契约生成）

消息名、surface 事件、容量、`shapeTypes`、payload `records` 与 client/server
`messageShapes` 的唯一机器可读来源是
[`bridge/protocol-contract.json`](../bridge/protocol-contract.json)。
`node tools/sync-protocol-contract.mjs` 校验该 JSON 并同步生成 [`docs/protocol.md`](protocol.md)、
Rust `build.rs` 常量/shape JSON、Rust/Node conformance fixtures 与
`bridge/package.json.dshCompatibility.wireProtocol`；`--check` 使任何未同步派生物失败。
`tools/generate-protocol-doc.mjs` 只是该同步器的兼容入口。Node 桥接运行时和 Rust 构建都读取这一个
contract，禁止两端手写 snapshot/history/frame 数值或独立消息 roster。

`hello` 带 `protocolVersion`，并可带 `resumeSessionId`、`cwd`（TUI 启动目录）和
`mode`（仅创建启动会话时的 preset id）。`welcome` 回传 `protocolVersion`、
`maxFrameBytes`、会话 id/状态及可选 `title`、`cwd`、provider/model；旧端可省略新增
能力字段。正常帧上限为 16 MiB；仅连接未更新的旧桥接时可显式设置
`DSHE_LEGACY_MAX_FRAME_MB` 放宽客户端上限。

每次 attach 后桥接还从 `ctx.commands.list(agent)` 下发
`commands{commands:[{name,description,input?:{hint}}]}`（不携带 handler）；客户端与内置
优化命令合并并由内置项覆盖同名。`commands/change` 会触发每连接的 agent-scoped 全量刷新。
通用命令仍经 `command{line}` 执行，直接 UI 结果以
`command-result{commandId,kind:success|error,text?}` 返回，followup 型命令继续通过普通会话
事件呈现；`execute` 返回 `undefined` 时桥接发 `command-unknown`，不创建模型消息。

`login` 载荷 `{ providers: [{id,name,apiKeyConfigured,apiKeyWritable,apiKeySource?,
apiKeyHint?}], proxies: [{id,name,baseUrl,protocol,model}], error? }`（§4.8）：API key 只有视图没有值。

`presets` 载荷 `{ presets: [{ id, name?, description?, order?, broken? }] }`：agent-presets
roster 快照，每次 attach（hello/`/new`/Resume）后紧随 `welcome` 下发；客户端用它渲染
`/new ` 模式提示弹窗。

`skills` 载荷 `{ skills: [{ name, description }] }`：bridge 按附着会话的 cwd/scope 调
`ctx.skills.list`，仅下发 `invocation.userInvocable` 的胜出项；每次 attach 及 `skills/change`
后全量刷新。客户端输入 `/skill`、`/skill:` 或兼容空格形式时按名称前缀→子串→子序列
过滤，并始终填入规范 `/skill:<name>`。

`snapshot` 载荷 `{ events, truncated? }`：含契约列出的重建事件，以及带 `surfaceOp` 的未知事件
（用于新宿主事件的兼容降级）；未知事件在 bridge 侧裁成只含 bounded type/seq/time/surface 元数据的
空 data envelope。assistant/chunk 不进入快照（assistant/message 带最终文本）。桥接最多发送最近
**600** 条，超出置 `truncated: true`，历史经 `history{beforeSeq,limit}` 分页，单页最多 2000。
所有下行 JSON 由 `frame.js::encodeBoundedFrame` 按 UTF-8 字节执行 16 MiB 上限：snapshot/history
保留最新可容纳后缀，单体超限改发 `frame-too-large`。冷日志读取完成后必须校验原连接仍有效，禁止
把旧会话快照发到已重绑的 socket。

事件 JSON 在 `protocol.rs::HostEvent` 边界转换为类型化 `HostEventKind`，同时读取事件顶层
`time`、`surfaceOp` 与 `sourceEventSeqs`；未知事件保留为 `Unknown`，但只有未知 append-surface
事件生成有界 fallback，未知/malformed replace 报兼容错误而不能伪装成 append。`projection.rs`
把事件穷尽分类为 display、surface mutation、page/session state、input accessory 或 ignore effect，
`AppState` reducer 不再遍历宿主原始 JSON。

事件显示统一为四种公共表面：

| 表面 | 用途 | 代表事件 |
|---|---|---|
| `ActivityRow` | Waiting/Running/Success/Failure/Cancelled 活动，可带 parent/depth | Thinking、tool、retry、command、Code Mode、workflow、compaction |
| `TranscriptBlock` | 无工作状态的 plain/Markdown/fallback 内容；compact 模式下 reasoning 块折叠进 `• Thinking...` 呼吸灯，不渲染、不进 copy provenance，并对活动行邻接透明；lines/full 模式直接渲染内容（lines 按折行后的显示行数截断），此时相邻 Thinking 指示行被接管隐藏 | assistant、turn notice/error |
| `ContentCard` | 统一内边距、背景与 copy source 的内容卡；context 注入卡按折行后的显示行数最多展示 5 行，超出时最后一行显示 `...`，但 copy source 保留完整原文 | 用户消息、context、附件占位、compaction summary |
| `InputAccessory` | 输入栏上方、统一高度预算/优先级/焦点 | queue、approval、question、todo、goal、plan |

文件活动由 `FileAction` 保留操作标签：连续的 `read`、`view`、`edit`、`replace`、`insert`
可折叠为同一活动行，其中后三者来自 `str_replace_editor` 的 view/str_replace/insert 命令；其绝对路径
优先按会话 `session_cwd` 显示为工作区相对路径。create 不参与折叠，单独显示
`<指示灯> create <相对路径>`，完成后不附加工具输出行数或耗时。

DSH surface replace 在显示前执行：shadowed surface node 及其拥有的工具活动从有效 transcript
删除，replacement 插回原 surface 位置；compaction 的 log-only summary 只更新 lifecycle，唯一可见
summary card 由紧随其后的 replacement 创建并拥有。历史从新到旧分页时长期保留 shadowed seq，后加载
旧页不会让被压缩消息复活；若 tool/command/Code Mode/workflow 的 terminal half 与 start 被分页
边界拆开，projector 暂存 terminal outcome，并在旧页 start 到达时直接重建最终状态；workflow
cancelled 保留为独立状态。retry schedule 晚于已加载 retry-started 回放时写入 deferred enrichment，
待 saved rows 恢复并完成 index shift 后回填完整 delay/failure/maxRetries 与 start time。`session/title`、provider/model、request
context 与策略状态只更新页面/会话状态；`request/header`、`session/end-seed`、approval audit、
title/search request 等默认忽略。

Markdown/source map 仍在客户端本地完成；渲染缓存封装为 `TranscriptRenderCache`，复制模式通过
`CopyRowsCache` 复用 UI 同一 width/generation display-row 布局产生的 provenance，不再在按键处理与随后
绘帧中各自全量推导 padding/折行/间距。滚轮、翻页、follow、历史前插 anchor 与 copy overlay 都使用
折行后的 display-row 坐标；row count 由线性 Unicode grapheme-width 扫描建立 prefix，combining mark 与
emoji ZWJ 跨 style span 也保持同一 grapheme，帧内只物化可见行。流式 text delta 只置 `tail_dirty`，
尾部拼接后只替换 tail row-count suffix 并增量续写 prefix；纯呼吸/settle 动画只 patch 活动消息的稳定
line range，settle 到期先清插值 source marker（保留完成时间哨兵）并提交精确目标色 patch，再停止 deadline。range 行数变化
则安全回退全量 rebuild；surface replace 等结构变更只做一次结构失效。

主循环用 Crossterm `EventStream` 将键盘/鼠标/paste/resize 直接接入 `tokio::select!`，不再依赖 50ms
输入轮询；交互帧 16ms、内容帧约 30ms、动画按 `spinner_frame_ms` deadline 合帧，空闲零周期唤醒。
bridge 入站 burst 每轮最多 64 条或约 2ms，避免饿死输入/到期帧。`TerminalOwner` 单点管理 raw mode、
alternate screen 与恢复，CrosstermBackend 使用 64KiB BufWriter；每帧以 DEC private mode 2026
Begin/End synchronized output 包住 diff、隐藏 IME anchor 与 flush，不支持该扩展的终端忽略序列继续
普通差量输出，`DSHE_DISABLE_SYNC_OUTPUT=1` 可显式诊断关闭。

## 6. Rust 技术栈

| 层 | 选型 | 备注 |
|----|------|------|
| TUI | ratatui + crossterm EventStream | 双缓冲 diff、事件驱动输入、缓冲/同步帧、resize、鼠标 |
| 异步/WS | tokio + tokio-tungstenite | 连桥接端点 |
| 协议 | serde + serde_json | 与 §5 严格对齐 |
| Markdown | pulldown-cmark | 保留块区间做 source map |
| Mermaid | **wasmi + grok-mermaid WASM** | 进程内解释执行；失败降级源码围栏 |
| 代码高亮 | 纯色 + 语言标签 | syntect 延后 |
| 剪贴板 | arboard（系统剪贴板） | Windows 直写剪贴板 |
| 配置 | toml + serde（嵌入 `client/assets/default_config.toml` + `%APPDATA%\dshe\config.toml` 覆盖层） | 缺字段继承默认值、即改即存（§4.7） |
| 宽字符 | unicode-width | 中文/emoji 宽度 |
| 分发 | 单 exe（仓库根为 Cargo workspace，根目录 `cargo run` 即启动） | 客户端运行时无 Node 依赖；首次安装/更新桥接需要 Node.js/DSH |

### 6.1 源码安装路径

README「Quick Start」是当前面向用户的安装入口：Windows / PowerShell 下先准备
Git、Node.js/npm 与 Rust/Cargo，全局安装 `@deepseek-ai/dsh`，把 `bridge/` 挂载到
专属 `dshe` profile 并执行 `dsh plugin --profile dshe install`，再用
`cargo install --path client --locked` 将 `dshe.exe` 安装到 Cargo bin 目录。未显式
设置 `DSH_HOME`（包括环境变量存在但值为空）时，安装命令与客户端统一使用
`%USERPROFILE%\.dsh`，挂载脚本会自动回退并打印实际目录；自定义目录也可显式传
`-DshHome`。挂载脚本须兼容 Windows PowerShell 5.1、可幂等执行，并以 UTF-8 无 BOM 写出供 Node 读取的
`package.json`。客户端安装后运行时仍为单 exe；Node.js 只用于 DSH 本身及首次安装/
更新桥接。改 bridge 或升级 DSH 后，重新 mount/restart 后从 `bridge/` 运行
`npm run verify-dsh-upgrade`；该命令会检查生成 contract、已声明的 DSH/agent 版本与公开 export，
并对部署副本运行 helper 和 session-routing smoke。

## 7. 平台与边界

- Windows 为主目标：crossterm 原生支持 Windows Terminal / ConPTY；宽度用 unicode-width。
- 不做：终端内图片内联（无 iTerm2/kitty 协议），附件以引用行展示。
- 不做（v1）：横向滚动、文件路径补全、多列布局、键位重绑定、块内局部选择（表格/mermaid/代码块）。
- 主题选择已纳入设置面板（§4.7）；开放色盘与固定语义映射目前通过主题 TOML 编辑。

## 8. 决策收尾

所有开放问题（O1–O16）已逐项讨论并记录为 D 系列决策。剩余为**实施验证项**，非设计问题：

- grok-mermaid WASM 已用 wasmi 集成，并有成功/失败降级测试。
- 桥接 token 固定为 `%DSH_HOME%\dsh-tui.token`，客户端启动时读取。
- ferra 色板到 256 色降级映射表——实施时用算法（最近色距）生成。
- 2026-08-18 Brooks Architecture Audit：94/100；生产依赖图无 SCC、单轨 transcript/strict Config/canonical contract 均有自动守卫。完整图与剩余 `AppState` 认知负荷建议见 [`architecture-audit.md`](architecture-audit.md)。

## 9. 里程碑草案（设计定稿后细化）

1. M1 桥接插件 + 协议联调（TS 侧 + 最小 Rust 客户端回显）
2. M2 消息流渲染 + 输入 + 流式 + 中断
3. M3 markdown + 表格渲染（含行映射 source map 骨架）
4. M4 复制模式（vim 键位 + 原子块整块复制）
5. M5 mermaid（WASM）+ 审批卡 + 会话选择器 + 帮助浮层
6. M6 补全/历史/多行 + 设置面板（config.toml + /settings 覆盖层）+ ferra 主题打磨 + 打包分发
