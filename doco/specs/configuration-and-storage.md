# Configuration and storage contracts

## Configuration

- `e-tui` owns config and theme schemas; adapters own platform paths and filesystem I/O.
- Embedded default TOML is the sole default source. User config overlays only known keys, then deserializes strictly; malformed known values fall back safely and unknown deprecated keys do not become runtime fields.
- Runtime-only adapter overrides MUST NOT overwrite unrelated shared settings.
- Rendering MUST perform no config, theme, session, Preview, or path-completion filesystem I/O.

## Key mappings and themes

- The embedded default key mapping is the sole binding default source. A user mapping replaces values by action; invalid startup input falls back to defaults, while invalid reload retains the last valid mapping.
- Runtime handlers consume semantic actions, not reconstructed legacy key events. Effective help and hints MUST use the resolved mapping. Help inherits movement, half-page, and page navigation from `full_screen`; its close binding remains owned by `help`.
- Built-in and user themes use the same parser. A valid user theme wins by name; an invalid file MUST NOT shadow the embedded fallback.
- Theme references and fixed semantic roles MUST validate as a complete unit. Presentation caches MUST invalidate when resolved styling changes.

## External effects

- Clipboard, path completion, theme discovery/installation, config writes, Preview file resolution, browser opening, and native authentication remain adapter-owned and execute outside frontend locks.
- Pi credentials remain in Pi's native store and environment resolution. Rust MUST NOT read or write `auth.json`, persist tokens in shared frontend config/history, or retry a credential mutation automatically after an uncertain or partial outcome.
- Async results MUST carry enough generation, draft, target, and workspace identity to reject stale completion.

## Execution history

- Each adapter stores append-only JSONL under its frontend-specific config cache and registers workspace identity through bounded locking and atomic replacement.
- Workspace identity uses versioned lexical normalization of the backend-confirmed absolute cwd; it MUST NOT silently promote to a Git root, resolve symlinks, or blanket-fold case.
- Shared records MUST exclude tool output, file contents, patches, model text, and unknown raw arguments. Unsupported versions or conflicting identities fail explicitly; malformed originals are preserved.
- History reads use a finite watermark. Shared ranking remains bounded and deterministic; absence of valid duration data MUST NOT produce invented measurements.
