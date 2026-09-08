## ADDED Requirements

### Requirement: Pane separator preserves omitted-background transparency
The pane separator SHALL render its semantic foreground without substituting the themed base-surface color when the active separator bar or drag-guide role omits a background. An omitted separator background SHALL use terminal transparency, while an explicitly configured separator background SHALL remain authoritative. Drag placeholder boxes SHALL continue to use their independent placeholder background role.

#### Scenario: Separator bar omits its background
- **WHEN** the active theme defines a separator bar foreground but no bar background
- **THEN** the idle separator glyph uses the semantic foreground and a terminal-reset background

#### Scenario: Separator drag guide omits its background
- **WHEN** the active theme defines a separator line foreground but no line background and the user drags the separator
- **THEN** the full-height guide and grip retain terminal-reset backgrounds

#### Scenario: Separator background is explicit
- **WHEN** the active separator role defines a background
- **THEN** the corresponding separator glyphs use that configured background
