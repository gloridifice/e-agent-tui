## ADDED Requirements

### Requirement: Latest completed Markdown targets
The frontend SHALL discover links dynamically from the most recent completed assistant Markdown message in the current conversation turn, including ordinary prose, inline/fenced code, and Markdown link destinations. URI schemes and Windows/POSIX absolute paths SHALL be recognized without requiring local existence. Workspace-relative candidates SHALL support both slash forms, extensionless path components, dotfiles, and recognizable extensionless filenames such as README and Makefile. A new user turn or session/draft replacement SHALL retire old tags. Streaming messages SHALL not receive tags before settlement. Duplicate target strings SHALL share one tag.

#### Scenario: Mixed targets
- **WHEN** a completed answer contains `https://example.com/docs`, `C:\Users\test\README.md`, `/tmp/session.jsonl`, and an existing `src/main`
- **THEN** each is eligible regardless of whether Markdown link syntax was used

#### Scenario: Only the latest output is active
- **WHEN** a newer assistant Markdown message settles or a new user turn begins
- **THEN** the previous message's tags are removed and cannot be copied through the shortcut

### Requirement: Confidence-ranked bounded workspace validation
Candidate extraction SHALL be pure and SHALL not perform filesystem I/O. Explicit relative syntax, file extensions, dotfiles, and directory suffixes SHALL be high confidence; separator-bearing extensionless paths, code-delimited names, and recognizable extensionless filenames SHALL be medium confidence. Ordinary prose words SHALL be low confidence and SHALL not trigger filesystem validation. Only high/medium relative candidates SHALL be validated against the current workspace by adapters outside state guards. Existing contained candidates SHALL be accepted; syntactically strong high-confidence missing paths MAY remain eligible. Lexical parent escape and symlink escape SHALL be rejected, including missing children of escaping symlinks. Validation SHALL have a finite candidate bound and stale completions SHALL not modify a newer message, workspace, session, or draft.

#### Scenario: Extensionless workspace file
- **WHEN** an answer mentions existing `src/main`, `README`, or code-delimited `Makefile`
- **THEN** filesystem confirmation accepts it rather than requiring an extension or trailing slash

#### Scenario: Escaping candidate
- **WHEN** a relative candidate resolves outside the workspace lexically or through a symlink
- **THEN** it is not tagged even if it exists or otherwise has strong path syntax

#### Scenario: Ordinary prose
- **WHEN** an answer contains ordinary prose words without path features
- **THEN** those low-confidence words do not cause filesystem probes

### Requirement: Bounded presentation-only link tags
The first 36 distinct accepted targets SHALL receive tags in first-output order from `1234567890abcdefghijklmnopqrstuvwxyz`. Their displayed occurrences SHALL have an immediate `~<tag>` suffix in the theme's Umber-equivalent tone; overflow SHALL remain untagged. Tags SHALL participate in layout without changing semantic Markdown, submitted content, source copy payloads, or persisted history. Marked layout SHALL refresh on target changes and resize without filesystem work during rendering.

#### Scenario: Capacity
- **WHEN** a completed answer contains more than 36 accepted distinct targets
- **THEN** the first is tagged `~1`, the tenth `~0`, the thirty-sixth `~z`, and later targets are untagged

#### Scenario: Inline code or wrapped link
- **WHEN** an eligible target is styled as inline code or wraps at the pane width
- **THEN** its tag follows the target in the rendered flow while complete-source copy retains only the original Markdown

### Requirement: One-key link copy selection
The configurable global `copy_link` action SHALL default to literal Ctrl+Y on all platforms and enter tag selection only from ordinary conversation input with available tags. The next unmodified lowercase ASCII letter or digit SHALL copy its target through the existing clipboard effect without inserting the key into the draft. Esc or an invalid ordinary key SHALL cancel selection without sending, editing, or copying. Other context-changing input SHALL cancel the pending selection. Pages, approvals, Reading and History SHALL retain input ownership. Help SHALL show the effective entry binding and describe the following tag key. Remapping or disabling the action SHALL remove its old binding without hardcoded fallback.

#### Scenario: Copy target
- **WHEN** the user presses Ctrl+Y then `1`
- **THEN** the first target, without its tag or Markdown syntax, is copied exactly once and existing clipboard feedback is shown

#### Scenario: Disabled action
- **WHEN** `global.copy_link` is disabled
- **THEN** Ctrl+Y does not enter link selection and the following character retains ordinary input behavior

#### Scenario: Protected context
- **WHEN** the entry binding is pressed in a page, approval, Reading or History
- **THEN** it cannot take over that context or copy an old target
