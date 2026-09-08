# Key mappings

> Status: Current
> Authority: User configuration workflow. The complete action inventory and defaults live in [`crates/e-tui/assets/default_key_mapping.toml`](../crates/e-tui/assets/default_key_mapping.toml); `/help` displays effective bindings.

Both `dshe` and `pie` embed the project default file. They do not read a workspace-local default at runtime. Create `key_mapping.toml` beside the shared `config.toml` to override bindings in both frontends. See the [configuration directory](../README.md#config) for platform paths; the Settings page displays the actual config path.

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

[history]
toggle_view = "tab"
```

## Values and reload

- Omitted actions inherit defaults. A configured string or array replaces the entire binding, not just one alternative.
- `"nop"` or `[]` leaves that action unbound. `nop` cannot be mixed with other keys. It does not disable a physical key in other contexts or prevent ordinary text entry.
- `osmain` resolves to Command (`Super`) on macOS and Ctrl on Windows/Linux. Explicit `ctrl`, `alt`, `shift`, and `super` modifiers are also supported. Modifiers match exactly; `ctrl-shift-r` is distinct from `ctrl-r`.
- Keys are single characters (including `,`), `enter`, `esc`, `tab`, `space`, `minus`, `backspace`, `delete`, `insert`, `left`, `right`, `up`, `down`, `home`, `end`, `pageup`, `pagedown`, and `f1` through `f12`. Use `minus` for the hyphen key. Multi-step sequences and command macros are not supported.
- Unknown scopes/actions, malformed values, and conflicts between simultaneously active actions reject the whole user mapping, with a diagnostic naming the field. An invalid startup mapping falls back to defaults; an invalid `/reload` retains the previous valid mapping. Removing the user file restores defaults on reload.
- `/reload` refreshes the mapping and subsequent help/hints. Saving ordinary settings never rewrites `key_mapping.toml`. Existing `/help` transcript messages remain historical snapshots; invoke `/help` again after reload for an updated list.

## Context ownership

The dotted tables describe registered contexts, not arbitrary command namespaces. Ordinary input combines `message`, `message.edit`, and either `message.idle` or `message.working`. Search and completion replace their overlapping navigation/accept/cancel bindings, while retaining relevant message controls. Disabled child actions do not fall back to a parent binding.

`read_mode.item` inherits exit/copy and fast movement and replaces Block navigation. `read_mode.move_up_fast` (PageUp) and `move_down_fast` (PageDown) repeat ordinary up/down cursor movement 15 times, following any Item-to-Block transition and stopping at document boundaries. The viewport follows the selection; this is not a 15-row viewport scroll. Global page scrolling is inactive while Reading owns input, so disabling or remapping fast movement does not fall back to global paging. By default Esc/q exits Reading from either level; Backspace returns only from Items to Blocks. Copy always uses the complete owning Block source, not visible clipped text.

Full-screen browsing combines `full_screen` navigation with its page-specific scope. Execution history uses `history` for `toggle_view` and inherits row, half-page, full-page, and exit actions from `full_screen`; disabling a history-specific action does not fall through to composer or transcript behavior. The fixed footer is generated from the effective bindings after overrides. Mouse-wheel movement and visible-screen text selection remain available independently of keyboard mappings.

Input Page browsing, choice editing, text editing, resume filtering, and question answering have separate contexts. `page.question.edit` retains arrow-based question switching during free-text answers without interpreting hjklq as navigation. Page-opening and Reading-entry shortcuts do not replace an active page, Reading View, or pending approval. Help remains available. Approvals react only to explicit allow/deny bindings; unrelated keys are ignored.

The old Reading Ctrl+Y and history-search Ctrl+R defaults are retired. History search remains available but unbound; the example above assigns it a replacement. `clear_or_quit` intentionally defaults to literal Ctrl+C on all platforms. Empty-input exit still requires idle state. Cancel removes all `⌁` ASAP candidates first, otherwise the newest `○` after-turn candidate; only an empty queue permits interruption. While backend cancellation is awaiting acknowledgment, repeated Cancel does not interrupt or remove after-turn candidates. Pi uses its native whole-queue clear, which also removes extension-origin steering/follow-up messages; this frontend's after-turn messages remain local and survive it.

## Model letter marks

Inside `/model`, Shift plus an ASCII letter toggles that letter on the focused model. Assigning a used letter moves it to the new model; assigning another letter replaces the model's old mark. Press the plain letter to select its exact provider/model route and close the menu, including from another provider's list. Missing catalog entries do nothing.

Both the plain and shifted chord must be free of effective page and global mappings. Default `h/j/k/l/q` are therefore reserved; Ctrl/Alt/Super shortcuts do not reserve the plain letter. Reloading a conflicting mapping temporarily hides and disables the mark without deleting it. Other menus and text editors are unchanged.

Marks appear as Bark-equivalent ` [a]` suffixes and persist in shared `config.toml`, not `key_mapping.toml`. Older configs default to no marks. The saved array uses lowercase letters and unique letters/routes, for example:

```toml
model_marks = [{ letter = "a", provider = "openai", model = "model-id" }]
```

## Terminal limitations

Applications cannot receive shortcuts intercepted by the terminal or OS. In particular, macOS Command+H and Command+, often belong to the terminal application. Configure terminal forwarding or choose another binding. Legacy terminal encodings may not distinguish Shift+Enter or Ctrl+Enter; modern Kitty CSI-u / modifyOtherKeys support preserves modifiers when delivered.

Bracketed paste, mouse wheel, selection dragging, and pane resizing are not keyboard bindings. Disabling `paste` prevents an application shortcut clipboard read, including the Windows physical Ctrl+V fallback, but does not disable text delivered independently as bracketed paste. Reading suppresses both paste paths. See the [terminal binding gate](subsystem/client/terminal-binding-gate.md) for automated checks and the manual validation checklist.
