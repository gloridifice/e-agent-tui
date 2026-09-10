# dshe Tracy and frame performance measurement

> Status: Historical
> Authority: Non-normative. This document is a 2026-08 measurement snapshot and does not define the current implementation or benchmark method.

Tracy ([wolfpld/tracy](https://github.com/wolfpld/tracy)) is used to locate startup, event-loop, cache-layout, and terminal-frame bottlenecks; without a GUI you can use startup timing, aggregate frame metrics, and release benchmarks.

## 1. Dependencies and feature switches

- `tracy-client = 0.18` is an optional dependency behind the `tracy` feature; the default build does not include the client.
- `enable + manual-lifetime + ondemand`: recording only happens when the profiler is explicitly started and connected.
- Profiling build: `cargo build --release --features tracy`.
- Zones must be created via `e::tracy_zone!("literal")`; it safely no-ops when no Client is running.

## 2. Runtime switches

| Variable | Purpose |
|---|---|
| `DSH_TUI_TRACY=1` | start the Tracy client and wait for the profiler to connect |
| `DSH_TUI_TIMING=1` | startup-stage `PhaseTimers` print elapsed time since the previous stage |
| `DSHE_FRAME_TIMING=1` | enable bounded frame samples and print one aggregate statistic every 120 frames |
| `DSHE_DISABLE_SYNC_OUTPUT=1` | compatibility diagnostic only: disable DEC 2026 synchronized output |

Normal runs save no frame samples and print no performance logs. Frame diagnostics keep at most the last 240 samples, compute count, P50, P95, P99, and max uniformly via nearest-rank `ceil(N×p)`, to avoid per-frame stderr corrupting the TUI.

## 3. Metric wrappers (`crates/e-dsh/src/profile.rs`)

- `PhaseTimers`: startup-stage timing.
- `IoCounters` + `CountingWriter`: count the ANSI bytes actually handed to the writer in one frame.
- `CountingBackend`: count Ratatui diff changed cells, and time backend draw/flush separately.
- `FrameMetrics`: aggregate scheduler delay, state/update, render, draw, flush, full transaction, cache rebuild/patch, and visible-row materialization count.
- In non-Tracy builds the macros expand to `NoopSpan` and neither call nor link the profiler runtime.

## 4. Tracy zone list

| Stage | Zone | Location |
|---|---|---|
| read token | `read_token` | `main.rs` |
| config load | `config load` | `main.rs::run` |
| WebSocket connect | `ws connect` | `main.rs::run` |
| snapshot reducer | `snapshot apply` | `handle_msg` |
| main-loop scheduler turn | `main loop` | event-driven loop |
| bounded inbound batch | `inbound batch` | bridge recv branch |
| transcript full rebuild | `transcript rebuild` | `ui.rs` |
| display-row prefix/layout | `display layout` | `cache.rs` |
| synchronized terminal frame | `frame transaction` | `terminal_runtime.rs` |
| first frame | `first frame` | first frame transaction |

## 5. GUI-less measurement

```powershell
# bridge / network startup segment
node tools/probe-startup.mjs

# real snapshot: JSON parse -> reducer -> first frame
node tools/dump-snapshot.mjs
cargo run --release --example timing_snapshot -- tools/cache/snapshot-sample.json

# 1002 messages; continuous tail, animation, and scroll; real Crossterm ANSI written to memory
cargo run --release --example timing_frames

# on-device aggregate scheduler and full frame transaction
$env:DSHE_FRAME_TIMING='1'; dshe
```

`timing_frames` runs a fixed 120 frames covering 80×40, 160×50, 240×70, and reports frame P50/P95, changed cells, ANSI bytes, cache rebuilds, and range patches; tail chunks only update the layout suffix, and the benchmark must not achieve its results via full prefix recomputation. Ordinary CI tests only assert workload/cache semantics, not volatile wall-clock thresholds; the 30ms P95 is judged in release/reference environments.

## 6. Measured results

### 6.1 Startup baseline (2026-08-16)

With the old bridge doing a full `readFrom(id, 0)` even for active sessions:

```text
welcome:  +2.9 ms
snapshot: +13991.0 ms
frame:    11767060 bytes, 3178 events
TOTAL attach: 14019.6 ms
```

Client 2000-event sample:

```text
read+parse:   26.14 ms
model fold:   57.57 ms (573 messages, 322 units)
first frame:   1.21 ms (1720 cached lines)
total:         88.65 ms
```

The startup bottleneck was the bridge's disk-read path; after active sessions read `agent.session.events` with paging, the client is no longer the primary startup bottleneck.

### 6.2 Scroll frame optimization (2026-08-17, local release)

Before: 50ms input ticker; full transcript rebuild every frame while active; no display-row prefix.

```text
80x40:  p50=3.325ms p95=4.556ms cells_p95=2290  bytes_p95=4009  rebuilds=120
160x50: p50=4.216ms p95=4.941ms cells_p95=5773  bytes_p95=10213 rebuilds=120
240x70: p50=5.256ms p95=6.440ms cells_p95=10562 bytes_p95=16037 rebuilds=120
```

After: EventStream/deadline scheduler; animation message-range patch; linear display-row layout; visible-window materialization.

```text
80x40:  p50=0.410ms p95=0.655ms cells_p95=2150 bytes_p95=3866  rebuilds=0 patches=120
160x50: p50=0.817ms p95=1.328ms cells_p95=3807 bytes_p95=5303  rebuilds=0 patches=120
240x70: p50=1.497ms p95=2.441ms cells_p95=9909 bytes_p95=14580 rebuilds=0 patches=120
```

All three sizes are far below the 30ms P95 red line; for now we do not introduce a hardware scroll-region that would require manually syncing Ratatui's previous/current buffers. Windows Terminal 1.24.11911.0 on-device smoke confirmed both the normal synchronized mode and the `DSHE_DISABLE_SYNC_OUTPUT=1` fallback mode accept input and redraw immediately, and that the command suggestion popup and new-session status bar are stable; continuous scroll/stream/animation are covered by the same release fixture and TestBackend layout regression. Terminal tearing is handled by the BufWriter + DEC 2026 Begin/End atomic commit; on-device scheduler delay can continue to be observed with `DSHE_FRAME_TIMING=1`.

## 7. Tracy GUI

1. `cargo build --release --features tracy`
2. Start the Tracy profiler GUI (usually listening on `127.0.0.1:8086`).
3. `$env:DSH_TUI_TRACY='1'; .\target\release\dshe.exe`
4. Inspect the `main loop`, `inbound batch`, `transcript rebuild`, `display layout`, `frame transaction`, and first-frame zones.
5. Without the environment variable, the profiling binary stays in ondemand no-op.

## 8. Maintenance discipline

- New zone names must be string literals and only wrap real work regions.
- When adding a frame path, sync the `FrameSample`/benchmark fields; normal runs must not introduce unbounded history or per-frame logging.
- Performance changes must also assert cache workload and UI results; do not use local wall-clock to mask semantic regressions.
- New dependencies go directly into `crates/e-dsh/Cargo.toml` and the workspace `Cargo.lock`; the historical `crates/e-dsh/vendor/` is no longer used.
