# DSH TUI 设计文档（草案 v0.5）

> 状态：**设计已定稿**（D1–D24 全部确认），进入实施阶段前不再改动；实施验证项见 §8。
> v0.4 变更：消息格式规范（用户消息原样、shell 卡 spinner+行数、read 合并折叠）；输入栏无边框背景块 + 粘贴占位。
> v0.5 变更（v0.1.0 里程碑）：项目改名 **e**（可执行文件 **`dshe`**）；配置迁到
> `%APPDATA%\dshe\config.toml`、主题目录 `%APPDATA%\dshe\themes\`（默认
> **deepseek-e**，另内置 ferra）；新增 `/theme` `/model` `/reload` `/skill:<名称>`；
> 启动器 `dshe` 自动 spawn `dsh --profile tui`（或 npx）／桥接已运行的 dsh。

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
| D17 | 启动行为 | `dshe` 记住上次会话直达；首次无历史弹选择器；`--list` 强制选择器 |
| D18 | mermaid 超宽 | v1 截断 + 折叠提示（复制仍拿完整源码）；v2 全屏图形模式 hjkl 四向滚动 |
| D19 | 复制提示 | 复制成功后输入区临时提示 `已复制 N 行`，约 2 秒后消失 |
| D20 | 用户消息展示 | 逐字原样展示，不做 markdown 渲染；前缀 `❯` Coral |
| D21 | 命令执行工具卡 | 圆点 spinner（默认半月旋转 `◐◓◑◒` ~120ms/帧，Honey）；只显示命令 + 实时输出行数；exit 0→`✓`Sage，非 0→`✗`Ember；命令超行截断，输出展开查看 |
| D22 | read 合并与折叠 | 同一 turn 内相邻 read 合并为紧凑状态列表；全部结束折叠为 Bark 灰 `<a>, <b>, <c>`（只文件名，超宽截断 `+N`）；Enter 展开还原、Esc 收回 |
| D23 | 输入栏形态 | 无边框背景块：Ash 底、上边距 1 行 + 文本区 + 下边距 1 行；前缀 `❯` Coral；多行模式最多显示 3 行，超出滚动、光标行可见 |
| D24 | 粘贴超长占位 | 粘贴 >1000 字符显示 Rose 色 `[N text pasted]`；发送原样完整内容；Alt+Enter 多行模式可展开编辑 |
| D25 | spinner 可配置 | 默认 A 半月旋转 `◐◓◑◒`（~120ms/帧）；帧序做成可配置枚举（`config.toml` 可换 B/C/D/E）；字体缺字形自动降级 ASCII `\|/-\` |
| D26 | 设置面板入口 | `/settings` 命令进入全屏覆盖层（无快捷键）；左分类栏 + 右项目列表；`↑↓` 选择 `←→` 切枚举 `Enter` 编辑数值/颜色 `Space` 切布尔 `Esc` 退出 |
| D27 | 配置存储 | `%APPDATA%\dshe\config.toml`（toml+serde）；优先级 默认值 < 文件 < 运行时；**即改即存、即时生效**，修改过的值 Honey 短暂高亮 |
| D28 | TUI 内可改项 | 见 §4.7 清单：外观/行为/显示三类全部可改，高级类只读 |
| D29 | 不提供 TUI 修改 | 连接参数（启动 flag）、字体字号（终端侧）、剪贴板后端（平台）、键位重绑定（v2）、语法高亮主题（二期） |
| D30 | 发送键风格 | 设置项：`Enter 即发 + Alt+Enter 多行` ／ `Ctrl+Enter 发送 + Enter 换行`，默认前者 |

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

## 3. 视觉设计

### 3.1 整体布局

```
┌──────────────────────────────────────────────────┐
│ e · default · deepseek-v4-pro · ●running   │ ← 状态栏（1 行，固定顶部）
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

- 运行中不展示输出正文，行数随输出实时增长；命令文本超一行截断（尾部 `…`）。
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

- Markdown：标题、粗体、斜体、行内代码、围栏代码块（语言标签 + syntect 高亮，主题随 ferra）、
  有序/无序列表、引用、分隔线、**表格**（框线渲染，列宽自适应，超宽截断标注）、**mermaid**。
- 代码块：左侧竖线边框 + 顶部语言标签；v1 超宽折行，横向滚动 v2。
- mermaid：grok-mermaid WASM 渲染为 Unicode 框图，渲染失败时降级显示源码围栏块（可复制）。
- 长内容折叠：普通长文本/工具结果 > N 行（默认 20）折叠，显示头尾 + `… [Enter] 展开`。
  （原子块表格/mermaid/代码块不折叠，见 O12 是否例外）
- 流式渲染：token 级追加；未上翻自动跟随底部，上翻暂停跟随并显示 `↓ 新消息` 指示。

### 3.4 ferra 色板（来源：casperstorm/ferra README）

| 名称 | Hex | 用途（客户端） |
|------|-----|----------------|
| Night | `#2b292d` | 客户端背景色 / 代码块与行内代码底色 |
| Ash | `#383539` | 卡片/输入栏/用户消息块底色 |
| Umber | `#4d424b` | 选区底色（原子块选中） |
| Bark | `#6f5d63` | 次要文本 / 工具卡 |
| Mist | `#d1d1e0` | 正文前景 |
| Sage | `#b1b695` | assistant 色条 / 成功 |
| Blush | `#fecdb2` | 链接 |
| Coral | `#ffa07a` | 用户 `❯` / 用户高亮 |
| Rose | `#f6b6c9` | 强调 / 行内代码 |
| Ember | `#e06b75` | 错误 / 失败 |
| Honey | `#f5d76e` | 运行中 / 审批卡 / 警告 |

- 终端侧：官方 Windows Terminal 配色方案（[ferra ports/windows terminal](https://github.com/casperstorm/ferra/tree/main/ports/windows%20terminal)）。
- 真彩色（24-bit）为主；降级环境回落到 ferra 最近似的 256 色索引。
- 支持 `NO_COLOR` / `--no-color` 纯文本模式。

### 3.5 状态呈现

- 状态栏：最前是状态符号 `•`（与工具卡一致：**只要处于 running 状态即黄色呼吸**，
  无论是否有可见的 thinking/命令/读写活动，空闲才灰色），随后一个空格接 `e ·
  模型`；右侧快捷键提示。不再显示 idle/running 文字指示。
- 审批/提问进行中：状态栏该段 Honey 色 `⏳等待审批`。
- 复制模式：输入区切换为指示条 `-- COPY --`（§3.1），显示选中行数/字节数与可用键。

### 3.6 会话选择器（启动 / Ctrl+N）

- 全屏覆盖层：模糊搜索 + 会话列表（标题、时间、消息数），↑↓ 选择，Enter 进入；无匹配 Enter = 新建。

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
  超宽内容在输入栏内**自动折行**（折行后窗口同样跟随光标）。
- 前缀 `❯` Coral，与用户消息前缀一致；输入文本 Mist。
- **粘贴占位**（bracketed paste）：粘贴内容 > **64 字符** → 输入栏显示 Rose 色
  `[N text pasted]` **粘贴块**，块内文本不展开；粘贴块是原子的——光标不可进入其内部
  （←/→ 整块跳过），退格/Delete 删除整个占位内容；直接 Enter 发送时内容原样完整发送。
- **预发队列**：AI 运行中按 Enter 发送的提示词不立即发出，而是进入客户端队列，
  在输入栏上方逐行显示（Night 底 Bark 字，左缩进 2 空格 `* ` 前缀，每项一行，
  超宽 `…` 截断；行数受面板高度限制，超出显示 `… 还有 N 条`）。AI 回到空闲后
  **自动逐条发出**（每条发出即开始新 turn）；`Esc` 中断会清空尚未发出的队列；
  切换会话时队列随之清空。空闲状态发送的提示词仍即时发出。
- 其余规则不变：`Alt+Enter` 切多行；发送键按设置项（D30）：默认单行 `Enter` 即发 / 多行 `Ctrl+Enter`，
  切换风格后单行 `Ctrl+Enter` 发送、`Enter` 换行；单行模式 `↑↓` 翻历史 + `Ctrl+R` 搜索；
  **多行模式 `↑↓` 在行间移动光标**；补全 `Tab`。

### 4.2 快捷键表（v1 提案）

| 键 | 功能 | 备注 |
|----|------|------|
| Enter | 发送（单行模式） | |
| Alt+Enter | 切换多行模式 | |
| Ctrl+Enter | 发送（多行模式） | |
| ↑ / ↓ | 输入历史 | 空输入行时 |
| Ctrl+R | 历史反向搜索 | |
| Tab | 命令补全 | |
| Ctrl+C | running→中断 turn；idle→退出 | |
| Ctrl+L | 重绘 | |
| PgUp / PgDn / 滚轮 | 滚动消息流 | 上翻暂停自动跟随 |
| Esc | 关闭浮层/卡片；取消输入 | |
| Enter（折叠卡） | 展开/收起工具结果 | 焦点导航 v2 |
| Ctrl+N | 会话选择器 | |
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
普通文本支持 V 行选与 Ctrl+V 块选；跨块选择以块为单位扩展（见 O13 细节）。

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
- `Ctrl+N` / `/resume` 打开会话选择器；`/resume <session-id>` 直接 attach 切换
  （选择器/`/resume` 对冷会话同样先 resume，找不到才报错、不断连）。
- 状态栏下方固定一行显示当前会话：左侧标题、右侧工作区路径。标题由
  `welcome.title`（会话日志最近一条 `session/title`，由桥接在 attach 时读取）
  初始填充；冷恢复会话日志不在内存，桥接经 `sessionQuery.readTitleSnapshots`
  补发 `title{title}` 帧；此后 `session/title` 事件经普通 event 帧实时更新
  （客户端只更新该行，不重建 transcript 缓存）。路径由 `welcome.cwd`（会话头部
  `header.cwd`，桥接在 attach 时读取）填充；标题过长以 `…` 截断以保住右侧路径，
  标题/路径均无时该行为空。
- `/new`：新建会话并切换（保留旧会话）。新会话落在 **TUI 启动目录**的工作区——
  客户端在 `hello` 里带上 `cwd`，桥接用它（校验为真实目录后）作为 `agents.create`
  的 `meta.cwd`，再把新会话 `attachSession` 进该 cwd 的 workspace 台账（与 host
  `session.create` 的两步一致）；客户端没发 cwd（旧客户端）时回退当前会话头部的
  cwd / `process.cwd()`。裸 `/new` 继承当前会话的 agent preset（桥接在
  `agents.create` 的 `setup` 里 `agentPresets.mount`，只写 header 不 mount 会拿
  不到 preset 的工具与提示词）。
- `/new <模式>`：按 agent preset id 新建会话（standard/code/minimal/cordis 及
  用户自建 preset）。桥接在每次 attach 后下发 `presets` roster 帧（id/name/
  description/order/broken）；客户端在输入 `/new `（含尾部空格）时弹出模式提示
  （同命令提示的 ↑↓/Tab/Enter/Esc 语义，按 id/显示名前缀-子串-子序列模糊匹配，
  broken 的 preset 不下发）。未知模式报错并列出可用 id。
- **model selection**：桥接创建/恢复的每个会话都在 `setup` 里先装
  `installModelSelection`（内联自 dsh-agent：`system-prompt/assemble` 注入
  `variables.{provider,model}`、`agent/request` 路由）——persona 的 `{{model}}`
  变量依赖它，缺失时每条消息报 `prompt variable "{{model}}" has no value`。
  `/new` 镜像当前会话的 provider/model，其余取 `agentDefaultModel.currentSelection()`。
  与 preset mount 是两个正交步骤，都要做（web/headless 入口同样如此）。

### 4.7 设置面板（D26–D30）

- **入口**：输入 `/settings` 命令（仅命令，无快捷键）。
- **形态**：不开浮窗——无边框无标题，**替代输入栏并占页面高度 2/3**，随页面
  最大宽度居中；消息流保留上方 1/3。面板背景默认 **Ash**，被选中的元素背景
  **Night**。
  ```
                外观  行为  显示  高级                ← 页签：居中、不可选中
    主题                 ● ferra   ○ custom
    ferra 预设或自定义色板（自定义色板在 TOML 中手改）
    纯色模式              ○ 开   ● 关
    降级为纯色输出（NO_COLOR 语义）
  ←/→ 分类  ↑/↓ 选择  Enter 编辑  Esc 退出 · 即改即存
  ```
- **键位**：页签行居中且**不可选中**，`←/→`（h/l）直接切换分类页；`↑/↓`（j/k）
  在条目间移动（两端钳制）；`Enter` 进入编辑；`Esc` 退出面板。分类内条目位置按页
  记忆，列表超出可视高度时滚动并自动把选中项带回视野。
- **两列**：左列为较小的名称列（30%）：名称 fg、说明 **Bark** 前景（过长换行）；
  右列为值。
- **选中高亮**：只选中**名称**（Night 底），描述不高亮；编辑时焦点移到值上，
  名称取消高亮。
- **值呈现**：选项未被选中 = `○ 文字`（默认 fg）；被选中 = `● 文字`（绿色）。
  布尔值即 开/关 两选项；输入类（数值）直接显示文本，编辑时绿色 + 光标块（Night
  底）；选择类编辑时 `←/→` 移动光标，光标所在选项 Night 底。
- **编辑语义**：`Enter` 确认修改、`Esc` 取消退回；编辑期间按键不外泄。退出面板
  后消息流/输入栏状态原样恢复。
- 保存：**即改即存**写入 `%APPDATA%\dshe\config.toml` 并即时生效。

**可配置项清单（TUI 内可改）**

| 分类 | 项目 | 类型 | 默认 |
|------|------|------|------|
| 外观 | spinner 样式（A/B/C/D/E） | 枚举 | A 半月旋转 |
| 外观 | spinner 帧率 | 数值 ms | 120 |
| 外观 | 主题色（ferra 预设 / 自定义十六进制：背景、前景、用户、assistant、成功、失败、运行中、警告、次要文本、输入栏底） | 枚举+颜色 | ferra 预设 |
| 外观 | 纯色模式（NO_COLOR） | 布尔 | 关 |
| 行为 | 记住上次会话 | 布尔 | **关**（新进程默认新建会话） |
| 行为 | 默认模式（新进程建会话使用的 preset，来自桥接 `presets` roster；配置值已失效时仍可显示/选择，桥接回退 standard） | 枚举 | standard |
| 行为 | 发送键风格（Enter 即发 / Ctrl+Enter 发送） | 枚举 | Enter 即发 |
| 行为 | 粘贴占位阈值 | 数值字符 | 1000 |
| 行为 | 长内容折叠阈值 | 数值行 | 20 |
| 行为 | 原子块折叠阈值 | 数值行 | 40 |
| 行为 | 复制提示停留时长 | 数值秒 | 2 |
| 行为 | 输入历史条数 | 数值 | 1000 |
| 显示 | 状态栏字段（模型名 / turn:step / 子agent数） | 布尔×3 | 全开 |
| 显示 | 工具耗时显示 | 布尔 | 开 |
| 显示 | read 自动合并 | 布尔 | 开 |
| 显示 | 消息时间戳 | 布尔 | 关 |
| 显示 | mermaid 渲染（关 = 源码围栏） | 布尔 | 开 |
| 高级 | 桥接地址 / 端口 / token 路径 | 只读展示 | — |

**不提供 TUI 内修改（D29）**：连接参数（改则断连，仅启动 flag / 环境变量）、
字体字号（终端侧）、剪贴板后端（平台决定）、键位重绑定（v2）、语法高亮主题（等 syntect 二期）。

### 4.8 登录设置（/login，D33）

- **入口**：输入 `/login`（仅命令，无快捷键）；输入栏变为登录页，形态与
  /settings 一致（无边框 Ash 底、占页面高度 2/3）。登录页是一层三选一菜单：
  **API key / Account / Proxy**，Enter 进入对应子页面，Esc 逐层返回。
- **API key**：子页面列出模型提供商（`ctx.llm.listProviders()`）；Enter 进入该
  提供商的 key 填写。落点走 `ctx.credentials` 的该提供商 `apiKeyEnv` 引用（
  `providerCredentialRef` 从 settings 读取，缺省回退 `<ID>_API_KEY`），
  `credentials.set/unset` 写入后经 `credentials/updated` 即时生效。**密钥值永不
  回传**——下行只带 `configured/writable/source/hint(…末四位)`；编辑框输入画 ●，
  环境变量来源只读。
- **Account**：OpenAI Codex（ChatGPT 订阅）网页登录，走**设备码流程**（无需本地
  回调端口）：桥接 POST `auth.openai.com/api/accounts/deviceauth/usercode` 拿
  `user_code`，面板显示 `https://auth.openai.com/codex/device` + 用户代码，轮询
  `deviceauth/token` 自动检测登录完成，换 token 得 `{access,refresh,accountId}`，
  存 `%DSH_HOME%\dsh-tui-codex.json`。⚠️ 端到端生效还需宿主
  `dsh-llm-pi-ai` 接入持久化 OAuth 凭证（当前用 `InMemoryCredentialStore`，无
  登录流程）。
- **Proxy**：列出已保存代理 + `+ New`；新建表单填 base url / api key / 协议模式
  （`openai-completions` / `openai-responses` / `anthropic-messages` 三选一，非必选）
  / 模型名称（非必填）。落点存 `%DSH_HOME%\dsh-tui-proxies.json`（api key 不回传）。
- **错误呈现**：写失败由桥接经同一 `login` 帧的 `error` 字段回传，显示在面板页脚
  （红色 ✗），不走 transcript 错误流。

## 5. 桥接与协议（v1 草案）

### 5.1 端点与安全

- 端点：`ws://127.0.0.1:<dsport>/dsh-tui`；v1 仅 loopback；鉴权见 O7。

### 5.2 消息协议（JSON，serde 两侧严格对齐）

**上行（TUI → DSH）**：`hello` / `input` / `command` / `interrupt` / `approval-answer` /
`login-get` / `login-set-api-key{provider,value}` / `login-codex-start` /
`login-codex-cancel` / `login-proxy-create{baseUrl,apiKey,protocol,model}` /
`login-proxy-delete{id}` / `ping`。`hello` 另带可选 `cwd`（TUI 启动目录，新会话的
工作区）与 `mode`（新会话的默认 preset id，仅在不带 `resumeSessionId` 时发送）。

**下行（DSH → TUI）**：`welcome` / `snapshot` / `event` / `status` / `presets` / `title` /
`login` / `login-codex` / `approval` / `resolved` / `error` / `pong`。`welcome` 另带可选
`title`（attach 时日志最近一条 `session/title`；冷恢复会话日志不在内存，桥接经
`readTitleSnapshots` 补发 `title{title}` 帧；此后标题更新走普通 `event` 帧的
`session/title` 事件）与可选 `cwd`（会话头部 `header.cwd`，标题行右侧显示的工作区路径）。

`login` 载荷 `{ providers: [{id,name,apiKeyConfigured,apiKeyWritable,apiKeySource?,
apiKeyHint?}], proxies: [{id,name,baseUrl,protocol,model}], codex?: {loggedIn,
accountId?}, error? }`（§4.8）：API key 只有视图没有值。`login-codex` 载荷
`{ status: pending|done|error, userCode?, verificationUri?, accountId?, error? }`
（设备码登录进度）。

`presets` 载荷 `{ presets: [{ id, name?, description?, order?, broken? }] }`：agent-presets
roster 快照，每次 attach（hello/`/new`/picker）后紧随 `welcome` 下发；客户端用它渲染
`/new ` 模式提示弹窗。

`snapshot` 载荷 `{ events, truncated? }`：只含 surface 事件（user/message、assistant/message、
tool/call、tool/result、turn/step 边界、todo/write），assistant/chunk 不下发（assistant/message
携带最终文本）；最多最近 4000 条，超出时 `truncated: true`（实施中因 36MB 单帧触发
tungstenite `max_frame_size` 而加入的裁剪）。

事件层是 DSH 会话日志的直通投影；markdown/表格/mermaid 解析与 source mapping 全部在客户端
本地完成——**协议薄、渲染厚**。

## 6. Rust 技术栈

| 层 | 选型 | 备注 |
|----|------|------|
| TUI | ratatui + crossterm | 双缓冲 diff、resize、鼠标 |
| 异步/WS | tokio + tokio-tungstenite | 连桥接端点 |
| 协议 | serde + serde_json | 与 §5 严格对齐 |
| Markdown | pulldown-cmark | 保留块区间做 source map |
| Mermaid | **wasmtime + grok-mermaid WASM** | 备选：wasmi；失败降级源码围栏 |
| 代码高亮 | syntect（ferra 自定义主题） | 见 O14 首期确认 |
| 剪贴板 | arboard（系统剪贴板） | Windows 直写剪贴板 |
| 配置 | toml + serde（`%APPDATA%\dshe\config.toml`） | 即改即存（§4.7） |
| 宽字符 | unicode-width | 中文/emoji 宽度 |
| 分发 | 单 exe（仓库根为 Cargo workspace，根目录 `cargo run` 即启动） | 用户无需 Node |

## 7. 平台与边界

- Windows 为主目标：crossterm 原生支持 Windows Terminal / ConPTY；宽度用 unicode-width。
- 不做：终端内图片内联（无 iTerm2/kitty 协议），附件以引用行展示。
- 不做（v1）：横向滚动、文件路径补全、多列布局、键位重绑定、块内局部选择（表格/mermaid/代码块）。
- 主题自定义（含色板 UI）已纳入设置面板（§4.7），不再是 v1 排除项。

## 8. 决策收尾

所有开放问题（O1–O16）已逐项讨论并记录为 D 系列决策。剩余为**实施验证项**，非设计问题：

- O17 grok-mermaid WASM 接口确认（输入/输出格式、体积、许可证、wasmtime 集成方式）——M5 开始前验证。
- 桥接 token 文件的具体位置与权限（DSH 数据目录内，随实施确定）。
- ferra 色板到 256 色降级映射表——实施时用算法（最近色距）生成。

## 9. 里程碑草案（设计定稿后细化）

1. M1 桥接插件 + 协议联调（TS 侧 + 最小 Rust 客户端回显）
2. M2 消息流渲染 + 输入 + 流式 + 中断
3. M3 markdown + 表格渲染（含行映射 source map 骨架）
4. M4 复制模式（vim 键位 + 原子块整块复制）
5. M5 mermaid（WASM）+ 审批卡 + 会话选择器 + 帮助浮层
6. M6 补全/历史/多行 + 设置面板（config.toml + /settings 覆盖层）+ ferra 主题打磨 + 打包分发
