# Configuration and storage contracts

## Configuration

- `e-tui` owns config and theme schemas; adapters own platform paths and filesystem I/O.
- Embedded default TOML is the sole default source. User config overlays only known keys, then deserializes strictly; malformed known values fall back safely and unknown deprecated keys do not become runtime fields.
- Runtime-only adapter overrides MUST NOT overwrite unrelated shared settings.
- `error_auto_retry` is a default-on Behavior setting for Pi fallback recovery. It MUST persist in frontend config and apply live, including after reload. Disabling it cancels pending fallback attempts without aborting an already running model call or changing Pi's native retry configuration; DSH ignores this preference.
- Per-model default efforts belong only to e's shared `config.toml`, keyed by exact provider/model identity. Setting a preference MUST use adapter-owned frontend config persistence, report write failures, and MUST NOT send a backend command or modify Pi configuration.
- Rendering MUST perform no config, theme, session, Preview, or path-completion filesystem I/O.
- The compaction override is a user-wide route shared by both adapters, persisted separately in `compaction-model.json` beside frontend config. The `e-tui::config::CompactionModel` schema owns its version and fields; JSON `null` or a missing file means no override. Writes MUST atomically replace the file without overwriting unrelated settings, and failures MUST NOT report successful configuration.
- DSH manual/automatic compaction and Pi manual compaction read the latest persisted route; Pi automatic compaction remains native. An active compaction keeps its captured selection. Invalid configuration or an unavailable route MUST NOT silently select a different model.

## Key mappings and themes

- The embedded default key mapping is the sole binding default source. A user mapping replaces values by action; invalid startup input falls back to defaults, while invalid reload retains the last valid mapping.
- Runtime handlers consume semantic actions, not reconstructed legacy key events. Effective help and hints MUST use the resolved mapping. Help and history inherit movement, half-page, and page navigation from `full_screen`; help close remains owned by `help`, while history owns its view toggle.
- Built-in and user themes use the same parser. A valid user theme wins by name; an invalid file MUST NOT shadow the embedded fallback.
- Theme references and fixed semantic roles MUST validate as a complete unit. Presentation caches MUST invalidate when resolved styling changes.

## External effects

- Clipboard, path completion, theme discovery/installation, config writes, Preview file resolution, browser opening, and native authentication remain adapter-owned and execute outside frontend locks.
- Pi credentials remain in Pi's native store and environment resolution. Rust MUST NOT read or write `auth.json`, persist tokens in shared frontend config/history, or retry a credential mutation automatically after an uncertain or partial outcome.
- Async results MUST carry enough generation, draft, target, and workspace identity to reject stale completion.

## Execution history

- Each adapter stores append-only JSONL under its frontend-specific config cache and registers workspace identity through bounded locking and atomic replacement.
- Workspace identity uses versioned lexical normalization of the backend-confirmed absolute cwd; it MUST NOT silently promote to a Git root, resolve symlinks, or blanket-fold case.
- Shared records MUST exclude tool output, file contents, patches, model text, and unknown raw arguments. They MAY persist exact provider/model identity, normalized message kind, turn identity, disjoint input/output/cache token counts, and provider-reported price as integer USD nanodollars. Unsupported versions or conflicting identities fail explicitly; malformed originals are preserved.
- A recorded history turn begins with an observed normalized user message and ends with normalized agent stop. Reasoning, assistant, tool call/result, and model-change records remain events inside that turn. Final assistant usage is recorded once; streaming chunks and tool events MUST NOT duplicate it.
- Native price is optional. Pi records the per-response price exposed by native usage; DSH leaves price absent while its host contract provides none. Missing price MUST NOT be stored or aggregated as zero.
- History reads use a finite watermark. Chronological records and the bounded deterministic ranking used by the history page MUST share that watermark; absence of valid duration data MUST NOT produce invented measurements.
