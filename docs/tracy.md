# dshe Tracy 与帧性能测量

Tracy（[wolfpld/tracy](https://github.com/wolfpld/tracy)）用于定位启动、事件循环、缓存布局和终端帧瓶颈；无 GUI 时可用启动计时、聚合帧指标与 release benchmark。

## 1. 依赖与特性开关

- `tracy-client = 0.18` 为 optional dependency，feature 为 `tracy`，默认构建不包含客户端。
- `enable + manual-lifetime + ondemand`：只有显式启动并连接 profiler 时记录。
- profiling 构建：`cargo build --release --features tracy`。
- zone 必须通过 `e::tracy_zone!("字面量")` 创建；没有运行中的 Client 时安全空转。

## 2. 运行开关

| 变量 | 作用 |
|---|---|
| `DSH_TUI_TRACY=1` | 启动 Tracy 客户端，等待 profiler 连接 |
| `DSH_TUI_TIMING=1` | 启动阶段 `PhaseTimers` 输出距上一阶段的耗时 |
| `DSHE_FRAME_TIMING=1` | 启用有界帧样本并每 120 帧输出一条聚合统计 |
| `DSHE_DISABLE_SYNC_OUTPUT=1` | 仅用于兼容诊断：关闭 DEC 2026 synchronized output |

普通运行不保存帧样本、不输出性能日志。帧诊断最多保留最近 240 个样本，按 nearest-rank `ceil(N×p)` 统一计算并输出 count、P50、P95、P99、max，避免逐帧 stderr 破坏 TUI。

## 3. 指标封装（`client/src/profile.rs`）

- `PhaseTimers`：启动阶段计时。
- `IoCounters` + `CountingWriter`：统计一帧实际交给 writer 的 ANSI 字节。
- `CountingBackend`：统计 Ratatui diff 的 changed cells，并分别计时 backend draw/flush。
- `FrameMetrics`：聚合 scheduler delay、state/update、render、draw、flush、完整 transaction、cache rebuild/patch 与可见行物化数。
- 非 Tracy 构建的宏展开为 `NoopSpan`，不会调用或链接 profiler runtime。

## 4. Tracy zone 清单

| 阶段 | Zone | 位置 |
|---|---|---|
| 读 token | `read_token` | `main.rs` |
| 配置加载 | `config load` | `main.rs::run` |
| WebSocket 连接 | `ws connect` | `main.rs::run` |
| 快照 reducer | `snapshot apply` | `handle_msg` |
| 主循环 scheduler turn | `main loop` | 事件驱动循环 |
| 有界入站批处理 | `inbound batch` | bridge recv 分支 |
| transcript 全量重建 | `transcript rebuild` | `ui.rs` |
| display-row prefix/layout | `display layout` | `cache.rs` |
| 同步终端帧 | `frame transaction` | `terminal_runtime.rs` |
| 首帧 | `first frame` | 首次 frame transaction |

## 5. 无 GUI 测量

```powershell
# bridge / 网络启动段
node tools/probe-startup.mjs

# 真实快照：JSON parse → reducer → 首帧
node tools/dump-snapshot.mjs
cargo run --release --example timing_snapshot -- tools/cache/snapshot-sample.json

# 1002 消息；持续 tail、动画和滚动；真实 Crossterm ANSI 写入内存
cargo run --release --example timing_frames

# 真机聚合 scheduler 与完整 frame transaction
$env:DSHE_FRAME_TIMING='1'; dshe
```

`timing_frames` 固定运行 120 帧，覆盖 80×40、160×50、240×70，报告 frame P50/P95、changed cells、ANSI bytes、cache rebuild 和 range patch；tail chunk 只更新 layout suffix，基准不得靠全量 prefix 重算取得结果。普通 CI 测试只断言工作量/缓存语义，不用易波动的 wall-clock 阈值；30ms P95 在 release/reference 环境判断。

## 6. 实测结果

### 6.1 启动基线（2026-08-16）

旧 bridge 对活跃会话也全量 `readFrom(id, 0)` 时：

```text
welcome:  +2.9 ms
snapshot: +13991.0 ms
frame:    11767060 bytes, 3178 events
TOTAL attach: 14019.6 ms
```

客户端 2000 事件样本：

```text
read+parse:   26.14 ms
model fold:   57.57 ms（573 messages, 322 units）
first frame:   1.21 ms（1720 cached lines）
total:         88.65 ms
```

该启动瓶颈在 bridge 读盘路径；活跃会话改取 `agent.session.events` 并分页后，客户端不是启动主瓶颈。

### 6.2 滚动帧优化（2026-08-17，本机 release）

优化前：50ms 输入 ticker；活动期间每帧全量 transcript rebuild；无 display-row prefix。

```text
80x40:  p50=3.325ms p95=4.556ms cells_p95=2290  bytes_p95=4009  rebuilds=120
160x50: p50=4.216ms p95=4.941ms cells_p95=5773  bytes_p95=10213 rebuilds=120
240x70: p50=5.256ms p95=6.440ms cells_p95=10562 bytes_p95=16037 rebuilds=120
```

优化后：EventStream/deadline scheduler；动画 message-range patch；线性 display-row layout；可见窗口物化。

```text
80x40:  p50=0.410ms p95=0.655ms cells_p95=2150 bytes_p95=3866  rebuilds=0 patches=120
160x50: p50=0.817ms p95=1.328ms cells_p95=3807 bytes_p95=5303  rebuilds=0 patches=120
240x70: p50=1.497ms p95=2.441ms cells_p95=9909 bytes_p95=14580 rebuilds=0 patches=120
```

三个尺寸均远低于 30ms P95 红线，暂不引入需要手工同步 Ratatui previous/current buffer 的 hardware scroll-region。Windows Terminal 1.24.11911.0 真机冒烟确认普通同步模式与 `DSHE_DISABLE_SYNC_OUTPUT=1` 回退模式均能即时输入并重绘，命令建议浮层和新会话底栏稳定；连续滚动/流式/动画由同一 release fixture 与 TestBackend 布局回归覆盖。终端 tearing 由 BufWriter + DEC 2026 Begin/End 原子提交处理；真机 scheduler delay 可继续用 `DSHE_FRAME_TIMING=1` 观察。

## 7. Tracy GUI

1. `cargo build --release --features tracy`
2. 启动 Tracy profiler GUI（通常监听 `127.0.0.1:8086`）。
3. `$env:DSH_TUI_TRACY='1'; .\target\release\dshe.exe`
4. 查看 `main loop`、`inbound batch`、`transcript rebuild`、`display layout`、`frame transaction` 与首帧 zone。
5. 不设置环境变量时 profiling binary 也保持 ondemand 空转。

## 8. 维护纪律

- 新 zone 名必须为字符串字面量，只包真实工作区。
- 新增帧路径时同步 `FrameSample`/benchmark 字段，普通运行不得引入无界历史或逐帧日志。
- 性能修改必须同时断言缓存工作量和 UI 结果；不能只用本机时间掩盖语义回归。
- 新依赖直接更新 `client/Cargo.toml` 与 workspace `Cargo.lock`；历史 `client/vendor/` 不再使用。
