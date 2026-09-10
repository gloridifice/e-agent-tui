## MODIFIED Requirements

### Requirement: Shared ruled replacement layout
Every Input Page SHALL render in the bottom page area used in place of the input bar, without a floating window, `Clear`, or background fill. A shared full-width ruled shell SHALL place a prompt-style command header between horizontal rules; page-internal dividers SHALL use the Umber semantic tone.

#### Scenario: Render a page at normal terminal size
- **WHEN** any Input Page is rendered
- **THEN** the terminal background remains visible, full-width rules frame the prompt-style header and page, and no floating or centered overlay is drawn

#### Scenario: Render page interaction states
- **WHEN** an actionable item is focused or a value is selected
- **THEN** focused text uses the Sage semantic tone, selected text uses the Coral semantic tone, and neither state adds a background fill, except Resume title/date columns retain their fixed tones with focus conveyed by a separate marker

#### Scenario: Render on a small terminal
- **WHEN** an Input Page is rendered in a terminal too small for all body rows
- **THEN** layout calculations remain bounded, visible content is clipped or scrolled, and rendering does not panic or overlap the status and title rows

## ADDED Requirements

### Requirement: Resume title and modification date rows
Resume rows SHALL omit session paths and display a left-aligned title in the theme's Mist-equivalent tone and an available last-modified date right-aligned in its Bark-equivalent tone. A focus marker SHALL identify the selected row without overriding either text tone. Title truncation SHALL reserve room for the date and separate the columns when space permits. A backend that does not provide last-modified metadata SHALL leave the date absent rather than mislabel creation time as modification time. Paths SHALL remain usable for identity filtering and attachment despite being omitted from row presentation.

#### Scenario: Focused dated row
- **WHEN** a Resume row with modification metadata is focused
- **THEN** its title remains Mist-equivalent, its date remains Bark-equivalent at the right edge, and a separate marker identifies focus without showing the session path

#### Scenario: Narrow row
- **WHEN** a title and date cannot fit in the available row width
- **THEN** the title is truncated before the date and neither column wraps or overlaps neighboring rows

#### Scenario: Missing modification date
- **WHEN** a backend provides a session without last-modified metadata
- **THEN** the title remains visible with no fabricated modification date
