## ADDED Requirements

### Requirement: Agent-initiated skill read presentation
When a Pi read tool targets a file whose basename is exactly `SKILL.md`, pie SHALL display a compact `[Skill] <name>` activity instead of an ordinary file-read row, using the parent directory name. Both forward and backward path separators SHALL be supported, including paths outside the workspace. A bare `SKILL.md` SHALL use the session cwd basename, or `SKILL.md` if unavailable. Recognition SHALL use event data only and SHALL NOT change tool execution or explicit skill invocation behavior. The activity SHALL retain call/result correlation, failure visibility, original path Preview, and read execution-history classification, without file-group folding or trailing line-count/duration text. Live and resumed calls SHALL use the same presentation.

#### Scenario: Global skill loaded automatically
- **WHEN** the agent reads `C:\Users\user\.agents\skills\review\SKILL.md`
- **THEN** pie displays `[Skill] review` and retains the original read path in Preview

#### Scenario: Bare filename
- **WHEN** the agent reads `SKILL.md` with session cwd `/skills/review`
- **THEN** pie displays `[Skill] review`

#### Scenario: Failed read and history replay
- **WHEN** a recognized skill read fails or is replayed from session history
- **THEN** its skill identity is preserved and a failed read remains visibly failed

#### Scenario: Ordinary file operations
- **WHEN** a read targets `README.md` or an edit targets `SKILL.md`
- **THEN** the ordinary file activity presentation remains unchanged
