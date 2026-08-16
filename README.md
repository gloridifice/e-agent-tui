# dsh-tui

DeepSeek Harness（DSH）的终端 UI 客户端：ferra 主题的 ratatui 界面 + DSH 宿主侧
WebSocket 桥接插件。与 Web GUI 共享同一会话日志，可同时使用、随时切换。

设计文档见 [docs/design.md](docs/design.md)（D1–D30 决策、协议、里程碑）；
性能方案见 [docs/tracy.md](docs/tracy.md)。

> **维护纪律**：任何任务完成后，必须同步更新本 README、`AGENTS.md` 与 `docs/`
> （功能、键位、协议、默认值有变时，三处文档不得滞后于代码）。

## 结构

```
Cargo.toml  Cargo workspace（默认成员 = client；根目录 cargo run 即启动 TUI）
bridge/     dsh-tui-bridge — DSH host 插件：WS 升级路由 + 会话事件转发 + 输入/中断/审批
client/     dsh-tui        — Rust ratatui 客户端（单 exe，无运行时依赖）
tools/      node 脚本      — 联调探针（probe-*/ws-test/dump-snapshot 等）
docs/       design.md / tracy.md
```

## 功能

- **消息流**：快照回放 + 实时流式输出；`Think...` 思考行；spinner 动画
- **Markdown 渲染**（glow/glamour dark 风格、ferra 配色）：标题分色、行内
  代码、链接 URL、任务清单、引用嵌套、盒装表格（单元格支持粗体/代码）、
  mermaid（grok-mermaid WASM，失败降级源码围栏）、代码块折叠 + Enter 展开
- **工具卡片**：命令摘要 + 行数/耗时；read/edit 自动合并一行
  （`read foo.rs x2; edit model.rs x2, bar.rs`——重复文件折叠为 xN），
  工具与读写行之间无空行；**相邻**的思考阶段（中间无任何可见活动）折叠为
  `Thinking... xN`
- **交互**：`/` 命令提示（前缀/子串/子序列模糊匹配、↑↓ 选择、Tab 填充）；
  `/new ` 后跟空格会提示可选**模式**（标准/创造/极简/PTC 等，来自宿主
  agent-presets roster，↑↓ 选择、Enter 发送）；`/settings` 即改即存（含
  **默认模式**选项——新进程建会话所用的 preset，失效自动回退标准模式）；
  `/login` 把输入栏变成**登录设置页**（API key / 账号 / proxy，即改即存）；
  `/resume` 打开会话选择器、`/resume <会话ID>` 直接切换；Ctrl+R 历史搜索；
  Shift+Enter 换行、Alt+Enter 多行（多行模式下 ↑↓ 在行间移动）；
  超宽内容在输入栏内自动折行；粘贴超过 64 字符折叠为原子粘贴块（光标不可
  进入，退格整块删除）；
  **预发队列**：AI 运行中发送的提示词进入队列（输入栏上方逐行显示，Night 底
  Bark 字，超宽 `…` 截断），AI 回到空闲后自动逐条发出；Esc 中断清空队列；
  Esc 中断、Ctrl+C 清空/空闲退出、`/exit` `/q` `/quit` 退出（不断对话）
- **复制模式**（Ctrl+B）：vim 键位，复制**原始 markdown 源码**，表格/代码/
  mermaid 整块原子复制
- **会话**：**每个新 TUI 进程默认新建一个会话**（落在启动目录的工作区，
  模式取 /settings 的「默认模式」，失效自动回退标准模式；`dsh tui <会话ID>`
  或开启「记住上次会话」则改为续接——宿主重启后旧会话不在活跃表时自动从
  持久化恢复，恢复不了才开新会话，不会因会话不活跃而启动失败）；Ctrl+N
  选择器、`/resume` / `/resume <会话ID>` 切换（冷会话同样自动恢复）——多个
  TUI 进程可同时各显示一个会话；状态栏下方一行显示当前会话标题（随
  `session/title` 实时更新）；**增量历史回看**——启动只加载最近消息，滚到
  顶部按 PageUp 逐页前插更早历史
- **性能**：增量渲染缓存 + 尾部拼接 + 30ms 节流重绘 + 仅克隆可见窗口；
  桥接侧活跃会话零磁盘读、tool/result 负载裁剪、surface 列表按会话缓存
- **Profiling**：Tracy 接入（feature `tracy`，`DSH_TUI_TRACY=1`）+ 无 GUI 的
  阶段打点（`DSH_TUI_TIMING=1`），见 docs/tracy.md

## 挂载桥接到 DSH

桥接是 host-composition 插件，装入 web profile（`%DSH_HOME%/profiles/web`）：

```powershell
# 1. 拷贝（不要用 junction —— 曾导致 node 模块解析失败）
robocopy bridge\src "$env:DSH_HOME\profiles\web\packages\dsh-tui-bridge\src" /MIR

# 2. profile 的 pnpm workspace 含 packages/*；package.json 依赖
#    "dsh-tui-bridge": "workspace:*"；cordis.patch.yml 注册 id: tui-bridge

# 3. 安装 + 验证 + 重启
dsh plugin --profile web install
dsh --profile web --dump-config | Select-String tui-bridge
# 重启 dsh web 后生效
```

修改桥接代码后：`robocopy ... /MIR` 同步到 profile，**重启 `dsh web` 生效**。

## 客户端构建与测试

根目录就是 Cargo workspace（默认成员 `client`），所有命令直接在根目录执行：

```powershell
cargo run                                # 编译并启动 dsh-tui（debug）
cargo run --release                      # 编译并启动（release）
cargo build --release                    # 产物 target\release\dsh-tui.exe
cargo test                               # 全量单测
cargo build --release --features tracy   # Tracy profiling 版本
```

运行：`target\release\dsh-tui.exe`（默认 `ws://127.0.0.1:3080/dsh-tui`，token 读
`%DSH_HOME%\dsh-tui.token`；可选参数：`<url> <sessionId>`）。在 `client/` 里执行
cargo 命令同样可用（成员目录，共享根 `target/`）。

## 快捷键速查

| 按键 | 行为 |
|---|---|
| Enter | 发送；`enter_sends=false` 风格下改插换行 |
| Shift+Enter | 输入栏内换行（无文字时同样扩一行） |
| Ctrl+Enter | 发送（任何风格/多行模式下） |
| Alt+Enter | 多行模式开关 |
| Esc | 中断当前对话 |
| Ctrl+C | 清空输入栏；空闲且输入栏为空时退出 |
| `/exit` `/q` `/quit` | 退出客户端（不中断对话） |
| Ctrl+B / Ctrl+N / Ctrl+H | 复制模式 / 会话选择 / 帮助 |
| `/new` | 新建会话并切换：在**本目录**（TUI 启动目录）的工作区创建，继承当前会话的 preset；保留旧会话 |
| `/new <模式>` | 按模式新建：`/new ` 后空格即提示可用模式（标准/创造/极简/PTC 等，↑↓ 选择、Tab 填充、Enter 发送） |
| `/resume` / `/resume <会话ID>` | 打开会话选择器 / 直接切换到指定会话（冷会话自动恢复） |
| `/settings` | 设置面板（主题色、spinner、**默认模式**、行数折叠等即改即存） |
| `/login` | 登录设置页：**API key**（写入宿主 credentials，即改即生效，值永不显示）、**账号**（harness 匿名用户 id，留空自动生成）、**proxy**（写入 `%DSH_HOME%\.env`，重启 dsh web 生效） |
| ←→ / Enter / Esc（提问中） | 切换选项 / 选中并下一题（最后一题确定提交）/ 取消提问 |

## 联调工具（tools/）

```powershell
node probe-online.mjs       # 桥接在线探测
node hello-test.mjs         # 发 hello 并打印全部帧（复现/验证启动路径）
node probe-startup.mjs      # attach 延迟 + 快照帧大小（启动瓶颈测量）
node dump-snapshot.mjs      # 抓真实快照样本（供 smoke/时序 example 离线用）
node ws-test.mjs            # 协议往返测试
```
