# Key mappings

> Status: Current
> Authority: User configuration workflow. The complete action inventory and defaults live in [`default_key_mapping.toml`](../default_key_mapping.toml); `/help` displays effective bindings.

Both `dshe` and `pie` embed the project default file. They do not read a workspace-local default at runtime. Create `key_mapping.toml` beside that executable's `config.toml` to override bindings. On Windows the directories are `%APPDATA%\dshe\` and `%APPDATA%\pie\`, respectively; the Settings page displays the actual config path.

```toml
[global]
choose_model = "osmain-m"

[message]
history_search = "osmain-shift-r"
toggle_multiline = "nop"

[read_mode]
exit = ["esc", "q"]
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

Input Page browsing, choice editing, text editing, resume filtering, and question answering have separate contexts. `page.question.edit` retains arrow-based question switching during free-text answers without interpreting hjklq as navigation. Page-opening and Reading-entry shortcuts do not replace an active page, Reading View, or pending approval. Help remains available. Approvals react only to explicit allow/deny bindings; unrelated keys are ignored.

The old Reading Ctrl+Y and history-search Ctrl+R defaults are retired. History search remains available but unbound; the example above assigns it a replacement. `clear_or_quit` intentionally defaults to literal Ctrl+C on all platforms. Empty-input exit still requires idle state. Cancel removes all `⌁` ASAP candidates first, otherwise the newest `○` after-turn candidate; only an empty queue permits interruption. While backend cancellation is awaiting acknowledgment, repeated Cancel does not interrupt or remove after-turn candidates. Pi uses its native whole-queue clear, which also removes extension-origin steering/follow-up messages; this frontend's after-turn messages remain local and survive it.

## Terminal limitations

Applications cannot receive shortcuts intercepted by the terminal or OS. In particular, macOS Command+H and Command+, often belong to the terminal application. Configure terminal forwarding or choose another binding. Legacy terminal encodings may not distinguish Shift+Enter or Ctrl+Enter; modern Kitty CSI-u / modifyOtherKeys support preserves modifiers when delivered.

Bracketed paste, mouse wheel, selection dragging, and pane resizing are not keyboard bindings. Disabling `paste` prevents an application shortcut clipboard read, including the Windows physical Ctrl+V fallback, but does not disable text delivered independently as bracketed paste. Reading suppresses both paste paths. See the [terminal binding gate](subsystem/client/terminal-binding-gate.md) for automated checks and the manual validation checklist.
