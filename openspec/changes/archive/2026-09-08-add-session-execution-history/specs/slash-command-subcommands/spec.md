## Purpose

Provide described, discoverable slash-command subcommands while sharing completion behavior with existing dynamic command arguments and preserving provider command compatibility.

## ADDED Requirements

### Requirement: Declared subcommands and default dispatch
Built-in commands with fixed subcommands SHALL declare each subcommand's name, localized description, and local action in the central command catalog used by dispatch and completion. An optional default SHALL handle the argument-free form. `/history` SHALL declare `show`, `path`, `copy`, and `copy-10`, with `show` as its default. Unknown fixed subcommands or unsupported trailing arguments SHALL produce usage feedback without falling through to provider execution. Whitespace handling SHALL agree between dispatch and completion.

#### Scenario: Default and explicit action agree
- **WHEN** `/history` or `/history show` is submitted
- **THEN** both resolve to the same local action

#### Scenario: Reject an invalid history invocation
- **WHEN** `/history missing` or `/history copy extra` is submitted
- **THEN** the client lists valid usage and sends no provider command or prompt

### Requirement: Described subcommand completion
After `/<command><whitespace>`, the existing suggestion surface SHALL offer that command's fixed subcommands with their descriptions. Filtering SHALL reuse the established ranking behavior and retain deterministic ordering. Accepting a candidate SHALL fill the appropriate command argument without executing it; explicit submission SHALL remain required. Candidate fill text SHALL remain separate from localized descriptions. Existing suppression for atomic paste/image blocks and protected editing contexts SHALL remain effective.

#### Scenario: Browse history actions
- **WHEN** the user types `/history `
- **THEN** `show`, `path`, `copy`, and `copy-10` are offered with descriptions of their distinct effects

#### Scenario: Complete a filtered action
- **WHEN** the user types `/history cop` and accepts `copy-10`
- **THEN** the input becomes `/history copy-10` without copying anything until submitted

#### Scenario: Localization changes
- **WHEN** the active language changes while subcommand suggestions are open
- **THEN** descriptions reflect the active language while subcommand identities and fill text remain unchanged

### Requirement: Dynamic arguments remain compatible
Fixed subcommands and dynamic command arguments SHALL share the candidate fill/description presentation and ranking infrastructure without treating model routes or effort values as a static subcommand registry. `/model` and `/effort` without arguments SHALL retain their current Input Pages. Their argument completion, canonical model-route selection, effort validation, asynchronous catalog refresh and ambiguous-ID handling SHALL remain unchanged. Built-ins SHALL continue winning name collisions; unrelated integrated commands SHALL retain provider forwarding semantics.

#### Scenario: Complete a model route
- **WHEN** the model catalog changes while `/model ` completion is open
- **THEN** candidates refresh using the existing provider/model identities and accepting one does not execute a history action or change argument syntax

#### Scenario: Select an effort
- **WHEN** the user submits an existing valid `/effort <id>` invocation
- **THEN** the current model's supported effort is selected through the existing provider request
