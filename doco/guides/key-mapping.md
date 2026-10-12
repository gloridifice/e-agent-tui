# Key mappings

> Status: Current
> Authority: User configuration workflow. Defaults and registered actions live in [`default_key_mapping.toml`](../../crates/e-tui/assets/default_key_mapping.toml); `/help` displays effective bindings. Mapping and persistence rules belong to [configuration and storage](../specs/configuration-and-storage.md#key-mappings-and-themes).

## Customize bindings

Both `dshe` and `pie` use the embedded defaults. Create `key_mapping.toml` beside the shared `config.toml` to override either frontend. Use `/econfig` or Settings to find the actual path; platform locations are listed in the [root README](../../README.md#config).

```toml
[global]
choose_model = "osmain-m"

[message]
history_search = "osmain-shift-r"
toggle_multiline = "nop"

[read_mode]
exit = ["esc", "q"]

[full_screen]
move_down_half = "ctrl-d"
```

Copy action names from the default file rather than inventing tables or actions. `osmain` means Command on macOS and Ctrl on Windows/Linux. Use a string for one chord, an array for alternatives, or `"nop"` / `[]` to disable an action. Omitted actions keep their defaults. For a hyphen key, use `minus`, for example `ctrl-minus`; supported chord syntax is defined by [the parser](../../crates/e-tui/src/key_mapping.rs).

Save the file, run `/reload`, then reopen `/help` to check the resolved bindings. A diagnostic identifies invalid fields or conflicts; correct the file and reload again. Remove the user file and reload to restore defaults. Ordinary Settings changes do not rewrite this file.

## Choose the right context

Use `[global]` for page-opening shortcuts, `[message]` and its registered child tables for composer actions, `[read_mode]` / `[read_mode.item]` for Reading, and `[full_screen]` for shared browsing movement. History owns its view switch; Help owns its close binding. Editing, completion, search, questions, and approvals have their own registered contexts.

A shortcut may be unavailable while another context owns input. Check the [input ownership contract](../specs/interaction-and-sessions.md#input-ownership) rather than assigning the same key across overlapping contexts. Disabling a child action does not make it fall through to the composer or a parent action.

## Model letter marks

In `/model`, focus a model and press Shift plus an available ASCII letter to toggle its mark. Press the plain letter to select that route, including from another provider's list. If a letter does nothing, check the effective page and global bindings; default navigation letters are reserved. A conflicting reload hides a saved mark until the conflict is removed.

Marks are saved in shared `config.toml`, not `key_mapping.toml`. For example:

```toml
model_marks = [{ letter = "a", provider = "openai", model = "model-id" }]
```

See [storage rules](../specs/configuration-and-storage.md#configuration) and [model selection](../specs/interaction-and-sessions.md#commands-and-model-selection) for identity, reassignment, and conflict behavior.

## Quick link copy

Use the effective `global.copy_link` binding (Ctrl+Y by default), then the lowercase letter or digit displayed after `~`. Esc or an invalid ordinary key cancels without editing the draft. Remap or disable the entry action in `[global]` and reload normally.

Only eligible targets in the latest completed assistant answer receive tags. Put paths containing spaces in quotes, inline code, or Markdown link destinations. URI targets need no filesystem probe; local paths are checked by the adapters, with platform and workspace containment restrictions. See [discovery and presentation](../specs/presentation.md#screen-and-copy) and [filesystem validation](../specs/runtime-and-adapters.md#runtime-effects-and-locking) for eligibility rules. Copy returns the target without its display tag.

## Terminal limitations

Applications cannot receive shortcuts intercepted by the terminal or OS. macOS Command+H and Command+, often belong to the terminal application; configure forwarding or choose another binding. Legacy terminal encodings may not distinguish Shift+Enter or Ctrl+Enter. Kitty CSI-u / modifyOtherKeys can preserve modifiers when delivered.

Bracketed paste, mouse-wheel movement, selection dragging, and pane resizing are not keyboard bindings. Disabling application paste does not disable independently delivered bracketed text paste. Reading suppresses both paste paths. Follow the [terminal compatibility gate](testing.md#terminal-compatibility-gate) before claiming support for a physical terminal.
