# Performance measurement methodology

> Status: Current
> Authority: Repeatable profiling and benchmark method. Concrete measurements belong in dated history records.

Use these workflows to locate startup, reducer, layout, scheduler, and terminal-frame regressions. Normal builds and runs must not retain unbounded samples or emit per-frame diagnostics.

## Build and runtime modes

Build with Tracy support only when a GUI trace is needed:

```powershell
cargo build --release --features tracy
$env:DSH_TUI_TRACY='1'; .\target\release\dshe.exe
```

The profiling binary remains effectively inactive unless profiling is enabled at runtime. Startup-stage timing and aggregate frame timing can be enabled independently without a Tracy GUI:

```powershell
$env:DSH_TUI_TIMING='1'; dshe
$env:DSHE_FRAME_TIMING='1'; dshe
```

`DSHE_DISABLE_SYNC_OUTPUT=1` is a compatibility diagnostic for comparing the normal synchronized terminal transaction with unsynchronized output; it is not a performance mode.

## Repeatable workloads

Run release builds for timing comparisons:

```powershell
# Bridge attach and startup path
node tools/probe-startup.mjs

# Capture a real snapshot, then measure parse -> reduction -> first frame
node tools/dump-snapshot.mjs
cargo run --release --example timing_snapshot -- tools/cache/snapshot-sample.json

# Continuous streaming, scrolling, animation, Preview, and syntax work
cargo run --release --example timing_frames
cargo run --release --example timing_syntax
```

Use the same terminal dimensions, build profile, input fixture, machine power state, and profiler attachment state when comparing runs. Record the commit, environment, command, fixture provenance, and relevant feature/environment switches with any published result.

## Interpretation

Separate the stages before optimizing:

- bridge attach and snapshot production;
- JSON parsing and event reduction;
- semantic materialization and layout/cache work;
- visible-row rendering;
- terminal draw and flush;
- scheduler delay.

A faster wall-clock result is not acceptable if it comes from stale output, omitted work, full-cache recomputation hidden by a smaller fixture, or weakened rendering/copy semantics. Pair timing observations with workload counters and UI/cache regression tests. Ordinary CI should assert deterministic workload and cache behavior rather than volatile local timing.

## Tracy use

Start the Tracy GUI before launching the profiling build, then inspect startup, main-loop, inbound-batch, transcript-layout, and terminal-transaction regions. Zone names must be string literals and should wrap meaningful work rather than individual trivial operations.

## Recording results

Concrete benchmark numbers are immutable evidence snapshots. Add a dated file under [`docs/history/`](../../history/README.md) when a measurement is worth preserving; do not update architecture or this methodology merely because implementation performance changed. Update this document only when the workload, metric definition, comparison discipline, or profiling procedure changes.
