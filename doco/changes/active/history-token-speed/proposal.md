<!-- doco:lifecycle v=1 created-at=2026-11-02T02:08:03Z completed-at=- archived-at=- -->
# History token speed

## Purpose
Show how quickly each model response was produced in execution history, with a weighted summary at the end of the enclosing agent run. Total token counts and tool-inclusive elapsed times cannot measure output throughput.

## Scope and acceptance
- Add completed-response output-token speed and a whole-turn weighted summary to the chronological history view, preserving existing usage, price, ranking, navigation and five-second time scale.
- Speed is output tokens divided by measured model-response seconds, including first-token latency but excluding tool execution and user/idle waits. Input/cache tokens are not output. Aggregate only corresponding successful, measured samples; label incomplete coverage and unsuccessful responses.
- Pi uses optional native `durationMs` and response outcome/identity. Native internal turns must not split the enclosing recorded agent run; capture settles at `agent_settled`, without changing existing transcript lifecycle behavior.
- Missing/zero duration or missing usage is unknown, never an estimate or infinity. Legacy traces remain readable without fabricated speed. No native-history backfill, live meter, new keys, or pure decoding/TTFT measurement.
- DSH uses the same presentation but leaves speed unknown until its wire contract exposes reliable response-only timing. Its current step span includes tool execution and retry waits and must not be reused as model duration.
- Preserve each response inside a shared five-second bucket instead of averaging coincident responses. Narrow layouts must retain speed before optional details.
- Extend the output-free execution record with optional response metadata; synchronize current storage/presentation contracts. Do not modify README or tests.

## Result
Pending — implementation authorized; lifecycle completion and archive are not requested.
