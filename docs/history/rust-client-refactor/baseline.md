# Rust client refactor baseline

> Status: Historical
> Authority: Non-normative. This dated baseline is frozen evidence and does not define current commands, performance, or implementation.

This file records the pre-migration baseline for the architecture and Reading View refactor. Measurements were captured on 2026-08-19 at commit `68e3511` on the local Windows reference workstation.

## Environment

- Rust: `rustc 1.96.0 (ac68faa20 2026-05-25)`
- Cargo: `cargo 1.96.0 (30a34c682 2026-05-25)`
- Node.js: `v25.8.0`
- npm: `11.11.0`
- Package: `e 0.0.3` at `client/`
- Binary: `dshe`
- Root workspace/default member: `client`

## Scoped regression commands

All commands below passed before files moved.

| Area | Command | Result |
| --- | --- | --- |
| Runtime/controller | `cargo test --lib runtime` | 30 passed; 335 filtered out |
| Projection | `cargo test --lib projection` | 15 passed; 350 filtered out |
| Transcript layout | `cargo test --lib transcript_layout` | 4 passed; 361 filtered out |
| UI and TestBackend | `cargo test --lib ui` | 71 passed; 294 filtered out |
| Setup | `cargo test --lib setup` | 21 passed; 344 filtered out |
| Launcher | `cargo test --lib launcher` | 19 passed; 346 filtered out |
| Wire conformance | `cargo test --test wire_contract` | 3 passed |
| Architecture | `cargo test --test architecture` | 5 passed |

The architecture baseline covers the production SCC scan, leaf-boundary rules, single transcript path, one strict config schema/default source, and scanner self-tests.

## Build and static checks

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | Passed |
| `cargo clippy --workspace --all-targets` | Passed with the existing warning baseline (44 library warnings and additional test/example warnings) |
| `cargo build --workspace` | Passed |
| `node tools/sync-protocol-contract.mjs --check` | Passed |

The protocol check is run from the repository root because the synchronization tool is under root `tools/`, not `bridge/tools/`.

## Performance baseline

### Continuous frames

Command: `cargo run --release --example timing_frames`

| Terminal | P50 | P95 | Changed cells P95 | Emitted bytes P95 | Rebuilds | Patches | Cached lines |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 80x40 | 1.372 ms | 2.315 ms | 2509 | 4578 | 0 | 120 | 2604 |
| 160x50 | 1.951 ms | 3.185 ms | 4003 | 5749 | 0 | 120 | 2404 |
| 240x70 | 2.675 ms | 4.470 ms | 10653 | 15773 | 0 | 120 | 2404 |

The fixture contains 1002 messages and renders 120 streaming, scrolling, and animated frames per size.

### Snapshot fold and first frame

A live `tools/cache/snapshot-sample.json` could not be captured because no bridge was online (`dump-snapshot.mjs` timed out). The deterministic canonical event fixture was therefore recorded as the repeatable fallback:

```powershell
cargo run --release --example timing_snapshot -- client/testdata/dsh-events.json
```

- Frame: 2381 bytes, 15 events
- Read and parse: 0.08 ms
- Model fold: 14.60 ms (4 display nodes, 7 render units)
- First frame: 0.56 ms (9 cached lines)
- Total: 15.43 ms

This small fixture is a correctness and relative-regression baseline, not a replacement for a later live large-snapshot measurement.

## Main-column visual characterization

The existing `ui` suite supplies the pre-refactor TestBackend baseline at representative sizes. Important fixtures include:

- `status_bar_has_spacing_and_working_bullet` at 80x40;
- `title_row_renders_below_status_bar` at 80x24;
- `running_redraw_keeps_hardware_cursor_hidden_and_only_moves_ime_anchor` at 80x40;
- `page_max_width_caps_and_wraps` with a 40-column page cap;
- `user_block_has_no_bg_gaps` at 80x12;
- `input_page_replaces_editor_without_touching_status_title_or_draft` at 60x18;
- `all_input_pages_are_bounded_on_a_short_terminal` at 14x5;
- `reasoning_blocks_are_hidden_but_cards_keep_copy_provenance` for hidden-content provenance.

The 71-test UI filter also covers card padding and backgrounds, Markdown/code colors, status and title content, input wrapping, accessories, history anchors, display-row scrolling, copy provenance, tail splice, targeted animation patches, and hidden cursor behavior.

## Install, setup, and launcher baseline

- Source install command: `cargo install --path client --locked`.
- Root `cargo run` resolves the default `client` member and launches the `dshe` binary.
- Current generated embedded bridge digest: `592edf743831963ce7a6e7eb4d195087c5e7793261f6dfd1fb70352fa65faca2`.
- Setup uses that digest and the generated wire protocol in `.dshe-setup.json`; setup tests verify normalized unique embedding, idempotent extraction, stale/damaged record classification, and successful-record consistency.
- Launcher tests verify dedicated `dshe` profile arguments, stale-lock recovery, process-tree termination on Windows, bounded startup timeout, external-service ownership, retry locks, and last-instance release.
- The package move must preserve the binary name, root run behavior, embedded bridge content identity, setup record semantics, and launcher lifecycle behavior.
