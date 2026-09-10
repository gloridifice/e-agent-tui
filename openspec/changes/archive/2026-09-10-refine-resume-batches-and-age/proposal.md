## Why

Resume should show its first sessions sooner than a viewport-sized metadata scan allows. Compact relative modification ages make recent conversations easier to distinguish than calendar timestamps.

## What Changes

- Cap each asynchronous content batch at three candidates, exposing each completed batch before requesting more while preserving viewport-driven prefetch, search, ordering, and stale-result isolation.
- Replace calendar labels with integer relative ages using at most two adjacent units from days through seconds, omitting zero remainders. Refresh visible ages without rescanning session files.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `pi-agent-frontend`: small incremental native Resume batches and modification metadata.
- `input-page`: relative modification ages in Resume rows.

## Impact

Pi session indexing, shared Resume demand/rendering, and presentation deadlines. Modification timestamps replace preformatted labels at the in-process adapter boundary; DSH continues to omit unavailable modification metadata and retains its list transport.
