# status-selection-feedback Specification

## Purpose
Give immediate, transient visual feedback when the confirmed model or effective reasoning effort changes in either frontend.

## Requirements

### Requirement: Confirmed status selections flash independently
On a confirmed provider/model route change, the status model name SHALL immediately use the active theme's Mist foreground. On a changed visible effective effort, the entire effort entry SHALL immediately use Ember for `max`, Honey for `xhigh`, Blush for `high`, and Mist otherwise, using the effective effort id rather than its display name. Each foreground SHALL smoothly interpolate back to its normal status foreground over 0.6 seconds, preserving text, backgrounds, and modifiers. Model and effort flashes SHALL be independent.

#### Scenario: Model changes while effort stays the same
- **WHEN** a confirmed route changes but its effective effort id is unchanged
- **THEN** only the model name starts a flash, retaining temporary-model italics when applicable

#### Scenario: Effort changes
- **WHEN** a confirmed effort changes to max, xhigh, high, or another value
- **THEN** only the effort entry starts a flash from Ember, Honey, Blush, or Mist respectively unless the route also changed

#### Scenario: Default and display label resolution
- **WHEN** the new effective effort comes from model default metadata and has a custom display label
- **THEN** its flash color follows the effective id while the label remains unchanged

#### Scenario: Rapid repeated changes
- **WHEN** a different value is confirmed before its previous flash finishes
- **THEN** that entry restarts at its new flash foreground without restarting the other entry

### Requirement: Selection feedback respects session and presentation lifecycle
First catalog hydration after attachment SHALL establish a baseline without flashing. Repeated unchanged catalogs and unsuccessful selections SHALL NOT start or restart a flash. Session replacement SHALL discard prior feedback. Hidden effort entries SHALL have no active flash. Confirmed changes during a deferred new-conversation draft SHALL retain the same feedback behavior. Feedback SHALL use bounded event-driven animation deadlines, SHALL leave transcript and Preview caches untouched, and SHALL stop scheduling when complete, including a final normal-foreground frame.

#### Scenario: Attach or refresh
- **WHEN** a session is attached and its first catalog arrives, or an unchanged catalog is refreshed
- **THEN** no selection flash is started

#### Scenario: Effort becomes unavailable
- **WHEN** the current route no longer exposes reasoning metadata
- **THEN** its effort entry and effort flash disappear

#### Scenario: Deferred draft selection
- **WHEN** a model or effort change is confirmed while a deferred new-conversation draft is open
- **THEN** the changed status entry flashes without materializing the session

#### Scenario: Idle completion
- **WHEN** the last flash finishes without any other animated work
- **THEN** a final normal-foreground frame is requested and animation-only wakeups cease without invalidating transcript or Preview caches
