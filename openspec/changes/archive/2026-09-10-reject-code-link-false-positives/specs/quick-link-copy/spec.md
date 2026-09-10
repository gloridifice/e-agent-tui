## MODIFIED Requirements

### Requirement: Latest completed Markdown targets
The frontend SHALL discover links dynamically from the most recent completed assistant Markdown message in the current conversation turn, including ordinary prose, inline/fenced code, and Markdown link destinations. URI schemes and Windows/POSIX absolute paths SHALL be recognized without requiring local existence, except that targets consisting entirely of slash or backslash separators SHALL NOT be recognized in any supported Markdown context. C++ scope-qualified identifiers using `::` SHALL NOT be recognized as URI targets. Workspace-relative candidates SHALL support both slash forms, extensionless path components, dotfiles, and recognizable extensionless filenames such as README and Makefile. A new user turn or session/draft replacement SHALL retire old tags. Streaming messages SHALL not receive tags before settlement. Duplicate target strings SHALL share one tag.

#### Scenario: Mixed targets
- **WHEN** a completed answer contains `https://example.com/docs`, `C:\Users\test\README.md`, `/tmp/session.jsonl`, and an existing `src/main`
- **THEN** each is eligible regardless of whether Markdown link syntax was used

#### Scenario: Only the latest output is active
- **WHEN** a newer assistant Markdown message settles or a new user turn begins
- **THEN** the previous message's tags are removed and cannot be copied through the shortcut

#### Scenario: Standalone slash
- **WHEN** an answer contains separator-only text such as `/`, `//`, or `\\` alone, as punctuation-separated prose, in quoted or code text, or as a Markdown link destination
- **THEN** that text is not a candidate and receives no copy tag
- **AND** non-bare absolute paths and URI targets remain eligible

#### Scenario: C++ scope syntax
- **WHEN** an answer contains `std::array<WorldEmitterArchetype, N>` or `drh1::work_graph_entrypoint_index(...)` in prose or code
- **THEN** the scope-qualified identifiers are not URI candidates and receive no copy tags
- **AND** actual URI targets such as `https://example.com`, `mailto:a@example.com`, and `custom:resource` remain eligible, including inside code
