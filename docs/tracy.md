# dsh-tui Tracy 接入方案与启动性能测量

Tracy（[wolfpld/tracy](https://github.com/wolfpld/tracy)）实时 profiler 接入方案，
用于定位 dsh-tui 的启动性能瓶颈。当前已实现 + 实测。

## 1. 依赖与特性开关

- 依赖：`tracy-client = { version = "0.18", optional = true, default-features = false,
  features = ["enable", "manual-lifetime", "ondemand"] }`
  - `enable`：编译 Tracy C++ 客户端（经 `tracy-client-sys`，MSVC 构建）
  - `manual-lifetime`：`Client::start()` 返回可持有的句柄，显式控制生命周期
  - `ondemand`：按需模式——**没有 profiler 监听时客户端静默等待，零崩溃、零开销**
- Cargo feature：`tracy = ["dep:tracy-client"]`，**默认关闭**，普通构建不含任何 Tracy 代码
- 构建 profiling 二进制：`cargo build --release --features tracy`

## 2. 运行模式（双环境变量）

| 变量 | 作用 |
|---|---|
| `DSH_TUI_TRACY=1` | 启动 Tracy 客户端（`Client::start()`），等待 profiler 连接 |
| `DSH_TUI_TIMING=1` | 无需 profiler：各阶段耗时打印到 stderr（PhaseTimers） |

注意 `tracy_client::span!` 在没有运行中的 Client 时会 panic，所以接入层不直接用它，
而是封装成受保护的宏（见下）。

## 3. 封装层（`client/src/profile.rs`）

```rust
// 宏：zone 名必须是字面量（底层 span_location! 用 concat! 烘焙静态名）；
// Client 未运行（未设 DSH_TUI_TRACY=1 或 profiler 未连接）时返回 None，安全空转。
let _z = dsh_tui::tracy_zone!("ws connect");
```

- `start_tracy()`：feature + 环境变量双重门控；句柄存活整个进程
- `PhaseTimers`：`mark(name)` 打印距上次 mark 的毫秒数（DSH_TUI_TIMING=1 时）
- 非 tracy 构建：宏展开为 `NoopSpan`（非 Copy 的 drop 占位），显式 `drop(_z)` 语义一致

## 4. 启动路径埋点清单

| 阶段 | Tracy zone | 位置 |
|---|---|---|
| 读 token 文件 | `read_token` | `main()` |
| 终端初始化（raw mode/备用屏） | —（PhaseTimers 计数） | `main()` |
| 配置加载（Config + StateFile） | `config load` | `run()` |
| WebSocket 连接 | `ws connect` | `run()` |
| hello 发送 | —（PhaseTimers 计数） | `run()` |
| 快照帧到达（网络+桥接侧） | —（PhaseTimers 计数） | `run()` 收包循环 |
| 快照回放折叠（apply） | `snapshot apply` | `handle_msg` |
| 首帧渲染（缓存构建 + draw） | `first frame` | 节流渲染块 |

扩展：帧率分析可在节流渲染块加 `tracy_client::frame_mark()`；事件循环热点可给
`handle_msg` 各 arm 加 zone。

## 5. 无需 GUI 的替代测量

Tracy GUI 需要人工操作，日常可先用三件套做无头测量：

1. `tools/probe-startup.mjs` — WS 侧：hello→welcome→snapshot 延迟 + 帧大小
   （桥接/网络那一半）
2. `tools/dump-snapshot.mjs` — 抓取真实快照样本存 `tools/cache/snapshot-sample.json`
3. `cargo run --release --example timing_snapshot -- <json>` — 客户端侧：
   JSON 解析 → 模型折叠 → 首帧渲染 三阶段耗时（headless，TestBackend）
4. 真机运行 `DSH_TUI_TIMING=1 dsh-tui.exe` — 端到端逐阶段打点（stderr）

## 6. 实测结果（2026-08-16，本机）

**桥接侧（probe-startup.mjs，运行中的旧桥接）：**

```
welcome:  +2.9 ms
snapshot: +13991.0 ms  ← 瓶颈
frame:    11767060 bytes, 3178 events
TOTAL attach: 14019.6 ms
```

**客户端侧（timing_snapshot，2000 事件样本）：**

```
read+parse:   26.14 ms
model fold:   57.57 ms（573 messages, 322 units）
first frame:   1.21 ms（1720 cached lines）
total:         88.65 ms
```

**结论：启动瓶颈 99% 在桥接侧，不在客户端。** 当前运行中的 DSH 仍加载旧桥接：
每次连接 `readFrom(id, 0)` 全量读盘（33 万事件日志，~14 秒）再裁尾 4000 条。
修复已在 `bridge/src/index.js`：活跃会话直接取内存 `agent.session.events` 尾部
600 条（快照 <100ms、帧 ~1MB），并支持 history 分页回看——**重启 `dsh web`
加载新桥接后生效**。客户端自身 89ms 已达标，无需优化。

## 7. Tracy GUI 操作步骤

1. 构建：`cargo build --release --features tracy`
2. 先启动 Tracy profiler GUI（默认监听 `127.0.0.1:8086`；release 见
   wolfpld/tracy Releases）
3. 运行：`$env:DSH_TUI_TRACY=1; .\target\release\dsh-tui.exe`
4. 连上后在时间线上看 `ws connect` / `snapshot apply` / `first frame` 各 zone 的
   精确耗时与线程分布；退出时 profiler 端保存 trace 即可离线分析
5. 不设 `DSH_TUI_TRACY` 时二进制行为与普通版完全一致（ondemand 空转）

## 8. 维护说明

- `tools/vendor-crates.mjs` 与 `client/vendor/` 已弃用（cargo 网络修复后走
  crates.io 官方源）；`client/.cargo/config.toml` 已删除。如需重新离线构建可再用该脚本
- 新增依赖需同步更新 `Cargo.lock`（提交入库）
- 埋点新增原则：zone 名用字面量、只包真实工作区、显式 `drop(_z)` 提前结束 zone
