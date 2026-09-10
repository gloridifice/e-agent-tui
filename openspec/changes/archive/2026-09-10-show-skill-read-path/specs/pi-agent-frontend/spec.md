## MODIFIED Requirements

### Requirement: Agent-initiated skill read presentation
When a Pi read tool targets a file whose basename is exactly `SKILL.md`, pie SHALL display `• read [skill] <name> at <path>`, using the parent directory name. The leading indicator SHALL follow ordinary read activity state, and the row SHALL retain ordinary activity indentation, compact spacing, and single-row truncation. `[skill]` SHALL use Rose and the name SHALL use Mist; the remaining text SHALL use ordinary read styling. The displayed path SHALL follow ordinary read workspace-relative formatting, retaining outside-workspace paths. Both forward and backward path separators SHALL be supported, including paths outside the workspace. A bare `SKILL.md` SHALL use the session cwd basename, or `SKILL.md` if unavailable. Recognition SHALL use event data only and SHALL NOT change tool execution or explicit skill invocation behavior; explicit user skill cards SHALL remain `[Skill] <name>`. The activity SHALL retain call/result correlation, failure visibility, original path Preview, and read execution-history classification, without file-group folding or trailing line-count/duration text. Live and resumed calls SHALL use the same presentation.

#### Scenario: Global skill loaded automatically
- **WHEN** the agent reads `C:\Users\user\.agents\skills\review\SKILL.md`
- **THEN** pie displays `read [skill] review at` followed by the outside-workspace path, with the ordinary state indicator, and retains the original read path in Preview

#### Scenario: Bare filename
- **WHEN** the agent reads `SKILL.md` with session cwd `/skills/review`
- **THEN** pie displays `read [skill] review at SKILL.md` with the ordinary state indicator

#### Scenario: Failed read and history replay
- **WHEN** a recognized skill read fails or is replayed from session history
- **THEN** its skill identity and path are preserved and a failed read remains visibly failed through the ordinary activity indicator

#### Scenario: Ordinary file operations
- **WHEN** a read targets `README.md` or an edit targets `SKILL.md`
- **THEN** the ordinary file activity presentation remains unchanged

#### Scenario: Explicit user invocation
- **WHEN** the user explicitly invokes a skill
- **THEN** its `[Skill] <name>` card remains unchanged, without a read prefix or path suffix
