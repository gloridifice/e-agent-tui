## ADDED Requirements

### Requirement: Admitted cancellation precedes local dispatch

Both runners SHALL process an already-admitted terminal event before claiming a pending prompt for dispatch.

#### Scenario: Escape and dispatch are ready together
- **WHEN** the ordinary composer has a pending candidate and the runner has received its configured cancel key
- **THEN** all ASAP candidates are cancelled before dispatch eligibility is evaluated, or the newest after-turn candidate when no ASAP candidate remains
- **AND** that key does not interrupt the agent

### Requirement: New submissions preserve pending priority

New submissions SHALL NOT bypass existing pending candidates; dispatch SHALL prefer ASAP candidates and preserve FIFO order within each delivery class.

#### Scenario: New prompt arrives as agent becomes idle
- **WHEN** an older ASAP candidate exists and a new ASAP prompt is submitted while the agent is idle
- **THEN** the older candidate is dispatched before the new prompt

#### Scenario: ASAP overtakes a waiting after-turn prompt
- **WHEN** an after-turn candidate exists and an ASAP prompt is submitted
- **THEN** the ASAP prompt is selected first without altering total submission order

### Requirement: Pending records survive backend admission

A candidate SHALL remain pending until authoritative consumption or successful cancellation. Each visible candidate SHALL occupy one display row, with ASAP marked `⌁` above after-turn candidates marked `○`.

#### Scenario: Steering waits in the backend
- **WHEN** the backend accepts a steering prompt but has not consumed it
- **THEN** its candidate indicator remains visible and the prompt remains eligible for cancellation

### Requirement: Batch ASAP cancellation precedes after-turn cancellation

Cancellation SHALL remove all unconsumed ASAP candidates without interrupting active work. After-turn candidates SHALL remain local and SHALL be cancelled newest-first only when no ASAP candidates or cancellation barrier remain. Pi clearing SHALL include its entire backend steering and follow-up queues, including extension-origin messages.

#### Scenario: Alternating delivery classes
- **WHEN** unconsumed candidates were submitted as ASAP a, after-turn b, ASAP c, after-turn d
- **THEN** their display order is a, c, b, d
- **AND** the first Escape cancels a and c together, retaining b and d
- **AND** subsequent Escape presses after acknowledgment cancel d then b

#### Scenario: Cancellation during admission
- **WHEN** Escape arrives before a pending steering admission is acknowledged
- **THEN** the frontend waits for admission acknowledgment before clearing the backend
- **AND** repeated Escape does not interrupt or remove after-turn candidates
- **AND** new submissions after that Escape remain local and survive the clear

#### Scenario: Clear fails
- **WHEN** the backend rejects the clear operation
- **THEN** an error is displayed, backend candidates remain visible, and the frontend does not interrupt work

#### Scenario: Consumption races cancellation
- **WHEN** a candidate is consumed before its cancellation commits
- **THEN** the provider reports that outcome and the same cancel operation does not become an interrupt
