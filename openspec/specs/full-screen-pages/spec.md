# full-screen-pages Specification

## Purpose
Support reusable full-height message-pane browsing pages that temporarily replace conversation presentation while preserving split Preview, conversation data, editing state, and viewport for return.
## Requirements
### Requirement: Exclusive full-screen page ownership
The frontend SHALL provide an explicit full-screen page context distinct from configuration Input Pages and Reading View. An active full-screen page SHALL replace the ordinary transcript, composer/Input Page area, and queue/status/title areas with its own bounded presentation occupying the full height of the message pane. Split Preview and its separator SHALL remain visible at their ordinary responsive geometry, and Preview SHALL continue its normal presentation updates. When Preview cannot fit beside the message pane, the page SHALL use the main-only area even if Preview-only mode was previously selected, without changing the saved Preview mode. It SHALL own navigation and suppress ordinary prompt editing/submission, hidden-view paste, separator resizing, and unrelated page-opening shortcuts. Existing configuration pages SHALL retain their current input-area-only behavior. Full-screen pages SHALL not be serialized into the conversation or create transcript messages merely by opening, scrolling, or closing.

#### Scenario: Open history from the normal screen
- **WHEN** a history-show action is admitted with no blocking interaction
- **THEN** history occupies only the message pane, preserving the split Preview and separator without appending history content to the transcript

#### Scenario: Protected interaction already owns input
- **WHEN** a full-screen page action is requested while an approval, question, or protected page editor owns input
- **THEN** the existing interaction retains ownership and its contents are not replaced

#### Scenario: History opened with narrow Preview-only mode retained
- **WHEN** history is active and the terminal cannot fit split Preview
- **THEN** history uses the main-only area and closing it restores the retained Preview presentation mode

### Requirement: Preserve and restore the conversation view
Entering, navigating and leaving a full-screen page SHALL preserve the authoritative transcript store and original content, composer text/atomic blocks/cursor/completion state, prior conversation scroll anchor/follow preference, and inactive Input Page/Reading/Preview state. Page scrolling SHALL use independent state. Returning without backend or geometry changes SHALL restore the same conversation content and viewport. Backend reduction and recording SHALL continue while hidden, without replacing the original store with a page-owned copy or replaying a stale snapshot on exit. New background content SHALL not force the restored anchor to the tail; legitimate backend mutations SHALL remain authoritative. Resize SHALL recompute layout while retaining logical anchors where they still exist. Hidden presentation caches SHALL be reused when valid and invalidated only by their normal data/geometry dependencies, not by page navigation itself.

#### Scenario: Return to an unfinished draft
- **WHEN** a full-screen page is opened and scrolled with a multiline draft and existing paste/image blocks retained
- **THEN** exiting restores the same draft, atomic ranges, cursor, and prior conversation anchor without page keys becoming draft text

#### Scenario: Agent finishes behind history
- **WHEN** backend output arrives while history owns the screen
- **THEN** output remains in the canonical conversation and trace, and exiting does not discard it or force a jump away from the saved viewport

#### Scenario: Resize and return
- **WHEN** the terminal resizes during a full-screen page
- **THEN** the page stays within the new bounds and return rewraps the conversation around its surviving logical anchor rather than restoring invalid pixel coordinates

### Requirement: Shared scoped scrolling and exit
Browse-only full-screen pages SHALL use registered semantic key mappings with defaults `j`/Down for one row down, `k`/Up for one row up, `d`/`u` for half a visible body page down/up, `f`/`b` and PageDown/PageUp for a full visible body page down/up, and `q`/Esc for exit. Half-page movement SHALL be at least one row where a body row is available, and all movement SHALL clamp to valid bounds. Defaults SHALL be shared by applicable full-screen pages without altering existing configuration-page or Reading bindings. Remapped/disabled actions SHALL not fall back to hardcoded keys. Page-owned editors introduced later SHALL treat printable browse keys as text while editing. Hints SHALL reflect effective bindings.

#### Scenario: Scroll by fractions of the viewport
- **WHEN** a page has a ten-row visible body and receives `d`, `u`, `f`, or `b`
- **THEN** its own viewport moves five rows down/up or ten rows down/up respectively, clamped to the available content

#### Scenario: Exit is not quit
- **WHEN** the active page receives its effective exit binding
- **THEN** only the page closes; the application stays running and the key is not forwarded to the composer or backend

#### Scenario: Disable a default binding
- **WHEN** the page's half-page-down action is disabled and the user presses `d`
- **THEN** no half-page movement or hidden composer edit occurs

### Requirement: Foreground transitions and asynchronous isolation
A full-screen page SHALL be session-scoped, reject stale query results, and cancel pending presentation on closure or session replacement. An incoming blocking approval/question SHALL close the full-screen page before acquiring input so agent work does not deadlock behind a hidden prompt. Captured mouse selection and separator drags SHALL be cancelled on incompatible page transitions. Visible-page selection SHALL use the existing committed-screen copy path; scrolling on the page SHALL not scroll or fetch older history for the hidden transcript.

#### Scenario: Approval arrives while browsing
- **WHEN** the backend requests a blocking approval while history is open
- **THEN** history closes and the approval becomes actionable without losing the retained composer or pending history records

#### Scenario: History load finishes after exit
- **WHEN** an asynchronous history page load completes after `q` closed that page
- **THEN** it does not reopen the page, replace conversation content, or change its scroll position

### Requirement: Bounded page rendering and loading
Full-screen pages SHALL expose distinct loading, empty, error and ready states and render only the visible body plus bounded chrome. Large history files SHALL be queried in bounded portions rather than synchronously parsed in a render pass. Pure page scroll SHALL not rebuild hidden transcript/Preview layouts. A static page SHALL not introduce a fixed redraw ticker; live-page updates SHALL follow actual data changes and existing independent deadlines. Hidden reveal/spinner presentation SHALL not cause useless repeated painting of the page.

#### Scenario: Long trace is opened
- **WHEN** history contains more records than fit in the terminal
- **THEN** the page remains responsive while fetching and rendering bounded portions, with loading distinguishable from empty data

#### Scenario: Static page is idle
- **WHEN** no data, input, or page animation changes
- **THEN** the terminal remains idle without periodic page redraws or hidden transcript layout rebuilds

