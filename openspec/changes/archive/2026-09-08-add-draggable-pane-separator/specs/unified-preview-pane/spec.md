## MODIFIED Requirements

### Requirement: Responsive pane layout
At sufficient width, the Screen SHALL calculate message width from the validated committed message-pane percentage and assign the remaining columns to Preview. The message share MUST NOT be less than 25%, and Preview SHALL render beside it only when the resulting Preview rectangle is at least 19 columns wide, providing a separator column, a one-column gap, 16 usable content columns, and a one-column right margin. Otherwise normal presentation SHALL remain main-only with a separator grip in the right margin, and the existing explicit toggle SHALL show Preview full-screen rather than rendering an unusably narrow pane. A responsive collapse caused by terminal width MUST preserve the committed percentage. Main page content SHALL use one-column ordinary horizontal margins; Main-only mode SHALL additionally reserve the collapsed grip geometry.

#### Scenario: Wide terminal renders both panes
- **WHEN** the committed percentage leaves at least 19 columns for the split Preview rectangle
- **THEN** the message and full-height Preview panes render in non-overlapping asserted rectangles calculated from that percentage

#### Scenario: Terminal is too narrow
- **WHEN** usable width is below the tested combined minimum
- **THEN** the Screen renders the main pane without a truncated sidebar and permits Preview to be shown as a full-screen view

#### Scenario: Committed percentage reaches the message minimum
- **WHEN** the committed or pending message share is 25%
- **THEN** message width is calculated as 25% of usable width and no interaction can reduce it further

#### Scenario: Preview would be too narrow
- **WHEN** percentage calculation leaves fewer than 19 columns for the split Preview rectangle
- **THEN** the Screen renders main-only with the right-margin separator grip and permits Preview to be shown as a full-screen view

#### Scenario: Responsive collapse later regains width
- **WHEN** terminal width first forces the split Preview rectangle below 19 columns and later grows enough to satisfy the threshold at the unchanged committed percentage
- **THEN** the split Preview reappears automatically without a persisted config change

#### Scenario: Effective transcript width changes
- **WHEN** entering, leaving, or committing a resized two-pane layout changes message-pane content width
- **THEN** transcript layout and copy provenance invalidate together before visible rows are materialized
