# Testing and validation

> Status: Current
> Authority: Repository validation policy and compatibility gates.

Choose checks according to the risk and scope of the change. Run existing checks where relevant; adding, modifying, or expanding tests requires explicit user authorization. Tests should protect behavior or architecture that can realistically regress, not merely increase coverage.

## Rust

Prefer the narrowest relevant test command, for example:

```powershell
cargo test --lib <module>
cargo test <test_name>
```

Do not run the full `cargo test --lib` or `cargo test` suite unless the user requests it or the change genuinely requires workspace-wide validation. Small changes may require no tests.

For small and medium Rust tasks, do not run workspace formatting or Clippy merely as an end-of-task ritual. For large or cross-cutting Rust changes, run:

```powershell
cargo fmt --all
cargo clippy --all-targets
```

Before committing Rust changes, `cargo fmt --all --check` must pass.

Tests must not depend on external configuration or theme files. Do not add tests for theme styling.

## Bridge

Bridge tests use `node:test` and live under `bridge/test/`:

```powershell
cd bridge
npm test
```

The package script uses `--experimental-test-isolation=none` to avoid `EPERM` when tests spawn sandboxed processes while remaining compatible with Node.js 22. File-layer tests must use a temporary home directory and must never touch the real `%DSH_HOME%`.

## Release assets and package archives

The bridge embedded in the published `e-dsh` crate is a generated mirror of the authoritative `bridge/` package. After any production bridge change, regenerate it from the repository root:

```powershell
node tools/sync-release-assets.mjs
```

Bridge tests run the corresponding `--check` mode. Before a release, also verify the actual package archives rather than relying only on workspace builds:

```powershell
node tools/verify-crate-packages.mjs
```

The verifier packages all three crates and checks the extracted adapters against the packaged `e-tui`, so it also works before a new synchronized `e-tui` version exists on crates.io.

## Protocol changes

The machine-readable authority is `bridge/protocol-contract.json`. After changing the protocol, run `node tools/sync-protocol-contract.mjs` to regenerate the [wire reference](../specs/wire-protocol.md), fixtures, and package metadata, then verify from the repository root:

```powershell
node tools/sync-protocol-contract.mjs --check
```

Protocol changes require contract-driven coverage on both the Rust and Node.js sides.

## DSH compatibility gate

After a DSH upgrade or a compatibility-sensitive bridge change, first deploy the bridge to the profile under test by following [DSH integration](dsh-integration.md), run `dsh plugin --profile <p> install`, and restart DSH. Then run the gate against that deployed copy:

```powershell
$env:DSH_TUI_SMOKE_PROFILE = 'e'
cd bridge
npm run verify-dsh-upgrade
```

Set `DSH_TUI_SMOKE_PROFILE` to another deployed profile, such as `web`, when appropriate. The gate verifies public exports, helper compatibility, `/new`, cold resume, and `/model` routing; it must not be replaced by testing only the source checkout.

## Terminal compatibility gate

Binding defaults live in [`default_key_mapping.toml`](../../crates/e-tui/assets/default_key_mapping.toml), not in this checklist. Use the [key mapping guide](key-mapping.md) to configure and reload overrides. Automated routing success does not establish physical shortcut delivery by every terminal.

### Automated gate

Choose the relevant existing checks from this gate:

```powershell
cargo test -p e-tui --lib key_mapping
cargo test -p e-tui --lib model_marks
cargo test -p e-tui --lib quick_links
cargo test -p e-tui --lib runtime::input
cargo test -p e-tui --lib input
cargo test -p e-tui --lib runtime::controller
cargo test -p e-dsh --test architecture
```

The gate checks platform-aware chords, removed defaults, disabled actions, protected pages and approvals, working queue order, atomic reload, Reading exit/Item return, and complete-source copy. Raw VT checks cover Windows BS/DEL/ETB handling, modified Enter, Kitty CSI-u, modifyOtherKeys, partial-Escape deadlines, bracketed paste, and Ctrl+V fallback delivery. These establish parser/router behavior, not physical terminal compatibility.

### Manual matrix

Before claiming compatibility for a terminal family, test Windows Terminal and ConHost, macOS terminal applications, and xterm/VTE/kitty-compatible Linux terminals independently. Record the terminal version, keyboard protocol, configured host shortcuts, and observed events using the adapter's [`input_probe`](../../crates/e-dsh/examples/input_probe.rs) example.

1. Exercise help, Reading entry, model, effort, settings, resume, and Preview defaults. On macOS verify whether Command combinations reach the application.
2. Verify plain/Shift/osmain Enter are distinguishable and perform their configured actions without sending on pasted line breaks.
3. Rebind a shortcut to F1, reload, and verify the old binding stops performing that action and all subsequent hints use F1.
4. Disable application paste; verify a delivered Ctrl+V key/fallback no longer reads the clipboard, while independent bracketed text paste remains text.
5. Enter Reading with a multiline/image draft. For ordinary conversation Blocks and Items, verify PageDown/PageUp repeats 15 ordinary cursor steps, including Item-to-Block transitions and document boundaries, rather than scrolling 15 viewport rows. Focus local terminal output separately and verify fast movement scrolls that output. Disable or remap fast actions and verify no global paging fallback. Copy complete source, return from Items with Backspace, and exit with Esc/q without changing the draft.
6. Open a text editor, question, or approval and verify page-opening shortcuts do not replace it; ordinary hjklq text and explicit approval responses retain their ownership.
7. Type `@` in a draft, navigate candidates, and complete nested directories and a file. Verify acceptance does not send, Esc dismisses without changing the draft, paths with spaces are quoted, and Enter sends only after completion closes.
8. In `/model`, use Shift+A to assign and remove a mark. Mark again, reopen, browse another provider, and press `a` to select and close. Verify default `h/j/k/l/q` are reserved, conflicting reloads suppress but retain saved marks, and the Bark suffix remains visible when names are truncated.
9. Open `/history` and verify it starts with the session-wide Top 50 ranking in the message pane while split Preview remains visible. Tab switches to the chronological timeline without another query or draft edit. Check independent offsets, movement, exit, and remapped or disabled `full_screen` bindings.
10. After an answer containing URLs and workspace paths settles, check Umber `~1`…`~z` tags. Press the link-copy binding then a tag, paste elsewhere, and verify the exact target without its suffix. Check Esc cancellation, preserved draft, input ownership, tag retirement on a new turn, and remapped or disabled `global.copy_link` without the old binding falling through.

Record physical runs against the tested revision. Automated success alone does not mark this matrix complete; older observations do not establish delivery for new defaults.
