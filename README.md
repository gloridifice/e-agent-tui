# e — DeepSeek Harness 终端客户端（`dshe`）

DeepSeek Harness（DSH）的终端 UI 客户端，项目名 **e**（需要明确功能时称 **e tui**），
可执行文件 **`dshe`**。默认主题 **deepseek-e**（另有 **ferra**）。与 Web GUI 共享同一
会话日志，可同时使用、随时切换。

设计文档见 [docs/design.md](docs/design.md)（D1–D30 决策、协议、里程碑）；
性能方案见 [docs/tracy.md](docs/tracy.md)。

> **维护纪律**：任何任务完成后，必须同步更新本 README、`AGENTS.md` 与 `docs/`
> （功能、键位、协议、默认值有变时，三处文档不得滞后于代码）。

## 启动方式

`dshe` 是唯一的入口，会根据 DSH 是否已在运行自动选择模式：

- **`dshe`（DSH 未运行）**：自动启动 `dsh --profile tui`（本机已全局安装则用 `dsh`，
  否则用 `npx @deepseek-ai/dsh`），等桥接就绪后运行 e tui；**关闭最后一个 e tui
  会随之关闭它启动的 dsh 服务**（多进程通过 `%DSH_HOME%\dsh-tui.lock` 计数）。
- **`dsh --profile tui`**：dsh 原生启动方式（效果同上）；随后 `dshe` 桥接上去。
- **`dshe`（DSH 已在后台运行）**：桥接到该 dsh 服务；**关闭 e tui 不影响现有服务**。

`tui` profile 的桥接挂载见下文「挂载桥接到 DSH」。

## 结构

```
Cargo.toml  Cargo workspace（默认成员 = client；根目录 cargo run 即启动 TUI）
bridge/     dsh-tui-bridge — DSH host 插件：WS 升级路由 + 会话事件转发 + 输入/命令/
            中断/审批 + /login /model /skill 桥接
client/     e             — Rust ratatui 客户端（产物 dshe.exe，无运行时依赖）
tools/      node 脚本     — 联调探针 + mount-bridge.ps1（web/tui profile 挂载）
docs/       design.md / tracy.md
```

## 功能

- **消息流**：快照回放 + 实时流式输出；`Think...` 思考行；spinner 动画；事件时序
  与 markdown 渲染（标题/表格/代码/mermaid 等）
- **Markdown 渲染**：标题分色、行内代码、链接 URL、任务清单、引用嵌套、盒装表格、
  mermaid（失败降级源码围栏）、代码块折叠 + Enter 展开
- **工具卡片**：命令摘要 + 行数/耗时；read/edit 自动合并一行；相邻思考阶段折叠
- **交互**：`/` 命令提示；`/new [模式]`；`/settings` 即改即存；`/login` 登录页
  （API key / Account / Proxy）；`/resume` 会话切换；Ctrl+R 历史搜索；粘贴原子块；
  预发队列；Esc 中断
- **模型与主题**：`/model` 选择 provider × model（写入当前会话）；`/theme` 选择主题
  （`%APPDATA%\dshe\themes\` 下所有合法 toml，默认 deepseek-e，另内置 ferra）；
  `/reload` 重载配置/主题/技能列表
- **Skill**：`/skill:<名称>`（或 `/skill <名称>`）从 `~/.agents/skills/` 与
  `<workspace>/.agents/skills/` 加载并注入技能（**同名时工作区优先**——由 DSH 的
  skill-filesystem 提供）
- **复制模式**（Ctrl+B）：vim 键位，复制**原始 markdown 源码**，表格/代码/mermaid
  整块原子复制
- **会话**：每个新 TUI 进程默认新建会话；Ctrl+N 选择器、`/resume` 切换；增量历史回看
- **性能**：增量渲染缓存 + 尾部拼接 + 30ms 节流重绘 + 仅克隆可见窗口
- **Profiling**：Tracy（feature `tracy`）+ 阶段打点（`DSH_TUI_TIMING=1`）

## 配置与主题

- 配置目录 `$CONFIG` = `%APPDATA%\dshe`：主配置 `config.toml`，主题目录 `themes/`。
- 主题：扫描 `themes/` 下所有 `.toml` 的合法主题（`name` + 11 个 hex 色）；首次运行
  自动写入两个默认主题 `deepseek-e.toml` 与 `ferra.toml`；默认主题 **deepseek-e**。
- `/theme` 选择主题并写回 `config.toml`；`/settings` 里也可改主题；`/reload` 重新
  扫描主题目录与配置。

## 挂载桥接到 DSH

桥接是 host-composition 插件。`tools/mount-bridge.ps1` 可挂到 `web` 或 `tui`
profile（`-Profile` 参数，默认 `web`；profile 不存在时自动创建骨架）：

```powershell
# 挂到 web profile（与 Web GUI 共用）
.\tools\mount-bridge.ps1 -Profile web

# 挂到 tui profile（`dshe` 与 `dsh --profile tui` 用）
.\tools\mount-bridge.ps1 -Profile tui

# 安装 + 验证 + 重启
dsh plugin --profile tui install
dsh --profile tui --dump-config | Select-String tui-bridge
# 重启 dsh 后生效（改动 bridge/ 后需重新挂载 + 重启）
```

## 客户端构建与测试

根目录就是 Cargo workspace（默认成员 `client`，crate 名 `e`，产物 `dshe.exe`）：

```powershell
cargo run                                # 编译并启动（debug）
cargo build --release                    # 产物 target\release\dshe.exe
cargo test                               # 全量单测（约 160 个）
cargo build --release --features tracy   # Tracy profiling 版
```

运行：`target\release\dshe.exe`（默认 `ws://127.0.0.1:3080/dsh-tui`，token 读
`%DSH_HOME%\dsh-tui.token`；可选参数：`<url> <sessionId>`；未运行 dsh 时自动
spawn `dsh --profile tui`）。

## 快捷键速查

| 按键 | 行为 |
|---|---|
| Enter | 发送；`enter_sends=false` 风格下改插换行 |
| Shift+Enter | 输入栏内换行 |
| Ctrl+Enter | 发送（任何风格/多行模式下） |
| Alt+Enter | 多行模式开关 |
| Esc | 中断当前对话 |
| Ctrl+C | 清空输入栏；空闲且输入栏为空时退出 |
| `/exit` `/q` `/quit` | 退出客户端（不中断对话） |
| Ctrl+B / Ctrl+N / Ctrl+H | 复制模式 / 会话选择 / 帮助 |
| `/new [模式]` | 新建会话并切换 |
| `/resume [会话ID]` | 打开会话选择器 / 直接切换 |
| `/settings` | 设置面板（主题、spinner、默认模式、行数折叠等即改即存） |
| `/login` | 登录页三选一：API key / Account / Proxy |
| `/model` | 选择 provider × model |
| `/theme` | 选择配色主题 |
| `/reload` | 重载配置、主题列表、技能 |
| `/skill:<名称>` | 注入技能（`~/.agents/skills/` 或 `<workspace>/.agents/skills/`） |
| ←→ / Enter / Esc（提问中） | 切换选项 / 选中并下一题 / 取消 |

## 联调工具（tools/）

```powershell
node probe-online.mjs       # 桥接在线探测
node hello-test.mjs         # 发 hello 打印全部帧
node dump-snapshot.mjs      # 抓快照样本
node smoke-bridge.mjs       # 对部署副本跑纯函数契约冒烟
.\mount-bridge.ps1          # 挂载桥接到 profile（-Profile web|tui）
```

桥接侧单测（node:test；trim/compose/login/skill/model 五个模块）：
`cd bridge && npm test`。
