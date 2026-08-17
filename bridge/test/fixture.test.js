import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

const events = JSON.parse(readFileSync(new URL('./fixtures/session-events.json', import.meta.url), 'utf8'))

test('bounded DSH history fixture covers display and audit families', () => {
  assert.ok(events.length <= 32, 'fixture stays bounded')
  const types = new Set(events.map((event) => event.type))
  for (const type of [
    'command/run', 'command/done',
    'compaction/start', 'compaction/summary', 'compaction/end',
    'llm/retry', 'llm/retry-started',
    'tool/code-dispatch-start', 'tool/code-dispatch',
    'tool-workflow/run-start', 'tool-workflow/agent-start',
    'tool-workflow/agent-end', 'tool-workflow/run-end',
    'todo/write', 'approval/asked', 'request/header',
  ]) assert.ok(types.has(type), `fixture contains ${type}`)
  const replacement = events.find((event) => event.surfaceOp?.op === 'replace')
  assert.deepEqual(replacement.sourceEventSeqs, [20, 30, 40])
  assert.equal(replacement.time, 1030)
})
