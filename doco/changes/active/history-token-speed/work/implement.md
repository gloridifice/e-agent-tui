# Implementation design

## Baseline and goals
The checkout was clean. [Capture](../../../../../crates/e-tui/src/execution_capture.rs) records one usage event after each final assistant message. Execution history has tokens/model/cost but no response timing. The history renderer sums usage and groups events into five-second rows. Pi discards native duration and response identity and projects every internal turn end as a transcript end. Installed Pi 1.1.0 supplies optional monotonic duration; older/deferred messages may omit it. Installed DSH's step ends after tools and can contain retries, so it is not a response timer.

## Overall approach
Pi normalizes native response metrics and enclosing run boundaries into capture-only timeline facts. Shared capture consumes them; projection ignores them. Existing transcript lifecycle, queue behavior, adapter-owned storage and runtime locks are unchanged. Shared pure history policy joins response/usage records and calculates weighted rates; rendering remains free of I/O and provider names.

## APIs and data model
Add response metrics for the immediately following final assistant response: optional duration, outcome and exact response model identity. Add run-start/run-finished facts at Pi's native agent start and settled completion. Capture keeps the current history turn across internal turn-end events while explicit run ownership is active.

Add an output-free `ResponseFinished` execution event referencing the recorded assistant-message sequence and optional usage sequence, with optional measured duration/source and outcome. This additive event preserves existing usage events and requires no rewrite of legacy traces. Old records remain readable; older readers can diagnose/skip the unfamiliar metadata event without losing ordinary usage records. Usage is stored once. Pending metadata is bounded to one final response, cleared on settlement, and never sourced from snapshot replay. Native response identity overrides the selected route for that response only.

## Algorithms and rules
Join responses and usage by run identity plus sequence; never pair across runs. A successful response with usage and positive measured duration qualifies for aggregate speed. Sum its output tokens and duration, then divide; count all assistant responses for coverage. Unknown/failed samples produce a partial coverage label when any eligible samples exist. Missing/zero time or missing usage produces no rate. Failed/cancelled responses may show their measured rate with explicit status, but do not enter the successful average.

Only explicit run settlement closes a Pi recorded turn while run ownership is active. The last internal outcome determines settlement; missing/duplicate terminal events do not invent closed turns. DSH retains existing turn boundaries and unknown response timing.

Retain the five-second chronological axis. Coincident responses have individual rate annotations, not a bucket average. Put rates first in the available detail width, followed by existing stop token/cost totals or single-response output/duration. Use existing bounded clipping for overflow; add no interaction bindings.

## Fixed decisions and discretion
No DSH bridge/protocol change or step-derived estimate, native-history backfill, live animation, test edits, or new keys. Local formatting and helper names are discretionary. Legacy response speed remains unknown. Keep the change active; completion/archive are not authorized.

## Verification and documentation impact
Run existing scoped capture/history, Pi adapter/store and DSH history checks, workspace check and proportional formatting/Clippy. Do not add or modify tests. Current storage and presentation specs change; architectural ownership and README do not. Record actual results and limitations in tasks.
