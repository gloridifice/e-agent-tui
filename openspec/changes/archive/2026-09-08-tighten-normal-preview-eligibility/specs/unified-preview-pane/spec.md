## ADDED Requirements

### Requirement: Normal-mode Preview eligibility and reconciliation
Normal-mode automatic Preview following SHALL ignore plain transcript blocks, including frontend system and error messages, and SHALL ignore user cards and user attachments. It SHALL continue to consider specialized context, reasoning, tool, activity, and unknown-surface content eligible. Reading View SHALL retain explicit Preview access to its selected Block or Item regardless of normal-mode automatic eligibility.

#### Scenario: Plain or user content is appended
- **WHEN** a plain block, user card, or user attachment is appended after an eligible normal-mode Preview target
- **THEN** the current target, revision, scroll, and reveal state remain unchanged

#### Scenario: Only ignored content exists
- **WHEN** the transcript contains only plain blocks, user cards, user attachments, or assistant Markdown
- **THEN** normal-mode Preview renders its empty state

#### Scenario: Ignored content is selected in Reading View
- **WHEN** Reading View explicitly selects a plain or user-owned Block
- **THEN** Preview renders that selected Block through the existing complete-source behavior

#### Scenario: Direct command activity settles
- **WHEN** a direct command-result updates the activity that owns the current normal-mode Preview target
- **THEN** Preview refreshes the same target identity to the activity's settled content without waiting for another timeline event
