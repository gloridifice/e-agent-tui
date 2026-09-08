## MODIFIED Requirements

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
