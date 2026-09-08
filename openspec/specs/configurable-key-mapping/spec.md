# configurable-key-mapping Specification

## Purpose
TBD - created by archiving change add-configurable-key-mapping. Update Purpose after archive.
## Requirements
### Requirement: Declarative scoped key mappings
The frontend SHALL embed root `default_key_mapping.toml` as its sole default binding source. Each executable SHALL overlay `<config_path>/key_mapping.toml` by registered scope and action. A string or array SHALL replace the entire default binding; `nop` or an empty array SHALL disable that action without legacy fallback. Unknown scopes/actions, malformed bindings and effective-context conflicts SHALL reject the complete overlay with a diagnostic field path.

#### Scenario: Partial override
- **WHEN** a user changes only global.choose_model to osmain-m
- **THEN** other actions retain defaults and osmain-l no longer opens the model page

#### Scenario: Disable inherited movement
- **WHEN** read_mode.item.move_up is nop
- **THEN** Item mode does not fall back to Block movement for that action

#### Scenario: Conflicting actions
- **WHEN** two simultaneously active actions bind the same chord
- **THEN** loading fails and identifies both action paths

### Requirement: Platform-aware exact chords
The parser SHALL support single characters, named navigation/editing keys, F1 through F12, minus, and ctrl/alt/shift/super/osmain modifiers. osmain SHALL resolve to Super on macOS and Control on Windows/Linux. Matching SHALL normalize character case with Shift, ignore key release events and require exact modifiers. Multi-step sequences SHALL not be supported.

#### Scenario: Platform selection
- **WHEN** osmain-shift-r is resolved for macOS or Windows/Linux
- **THEN** it matches Super-Shift-R or Control-Shift-R respectively, not unshifted R

### Requirement: Semantic scoped dispatch
All existing keyboard commands SHALL resolve through mappings into semantic actions once. Active search, suggestion, Reading, approval and page contexts SHALL preserve their ownership. Browse hjklq SHALL be text in editors, resume filtering and free-text answers. Global model/effort/settings/resume shortcuts SHALL not replace a protected page or approval. Unbound modified characters SHALL not enter text. Approval SHALL accept only explicit allow or deny bindings, ignoring other keys.

#### Scenario: Working submission
- **WHEN** the default Enter or osmain-Enter binding is pressed during active work
- **THEN** the complete prompt queues as soon as possible or after the turn respectively

#### Scenario: Protected editor
- **WHEN** a page editor owns input and a global page-opening key is pressed
- **THEN** the page and its unsubmitted text remain unchanged

#### Scenario: Approval ignores unrelated keys
- **WHEN** an approval is open and a key matches neither allow nor deny
- **THEN** no approval response is sent

### Requirement: Atomic loading and truthful hints
Adapters SHALL own filesystem reads, startup SHALL fall back to defaults with a visible diagnostic on error, and reload SHALL preserve the previous mapping on error. Missing user files SHALL use defaults. Persisting ordinary settings SHALL not write runtime mappings. Help overlays, local help Markdown, status and page hints SHALL display effective bindings and disabled actions without stale default labels.

#### Scenario: Reload failure
- **WHEN** an invalid user mapping is reloaded after a valid custom mapping
- **THEN** the old map remains active and the error is displayed

#### Scenario: Updated hints
- **WHEN** global.print_help is changed to f1
- **THEN** status and help identify F1 rather than Ctrl-H

### Requirement: Terminal transport remains independent
Bracketed paste and mouse gestures SHALL remain separate from key remapping. Reading SHALL suppress clipboard and text paste. Windows physical Ctrl-V fallback SHALL pass through the configured key resolver and SHALL not bypass a disabled or remapped paste action.

#### Scenario: Disabled physical paste shortcut
- **WHEN** message.paste is nop and Windows emits the Ctrl-V fallback key
- **THEN** no clipboard read is requested

