import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { createHistoryStore } from '../src/history.js'
import { SNAPSHOT_SURFACE } from '../src/protocol.js'

test('history store caches live surfaces, appends by seq, and pages', () => {
  const events = [
    { seq: 1, type: 'user/message' },
    { seq: 2, type: 'assistant/chunk' },
    { seq: 3, type: 'assistant/message' },
  ]
  const store = createHistoryStore({
    host: { persistence: () => undefined },
    surfaceTypes: new Set(['user/message', 'assistant/message']),
    trimEvent: (event) => event,
    buildToolNames: () => new Map(),
    snapshotCap: 2,
  })
  const conn = { agent: { id: 's1', session: { events } }, toolNames: new Map() }
  assert.deepEqual(store.surfaceFor(conn).map((event) => event.seq), [1, 3])
  events.push({ seq: 4, type: 'user/message' })
  assert.deepEqual(store.surfaceFor(conn).map((event) => event.seq), [1, 3, 4])
  assert.deepEqual(store.historyEvents(conn, 4, 1), {
    events: [{ seq: 3, type: 'assistant/message' }],
    hasMore: true,
  })
})

test('expanded history roster preserves ordering and surface metadata but omits audit', () => {
  const fixture = JSON.parse(readFileSync(new URL('./fixtures/session-events.json', import.meta.url), 'utf8'))
  const events = [
    { seq: 0, type: 'approval/asked', data: {} },
    ...fixture,
    { seq: 21, type: 'user/message', surfaceOp: { op: 'replace', start: 4, end: 4 }, sourceEventSeqs: [4], data: {} },
    { seq: 22, type: 'future/surface', surfaceOp: 'append', data: {} },
    { seq: 23, type: 'future/audit', data: {} },
  ]
  const store = createHistoryStore({
    host: { persistence: () => undefined },
    surfaceTypes: SNAPSHOT_SURFACE,
    trimEvent: (event) => event,
    buildToolNames: () => new Map(),
    snapshotCap: 100,
  })
  const conn = { agent: { id: 'expanded', session: { events } }, toolNames: new Map() }
  const projected = store.surfaceFor(conn)
  assert.ok(projected.some((event) => event.type === 'command/run'))
  assert.ok(projected.some((event) => event.type === 'llm/retry'))
  assert.ok(projected.some((event) => event.type === 'tool-workflow/run-end'))
  assert.ok(!projected.some((event) => event.type === 'approval/asked'))
  const replacement = projected.find((event) => event.seq === 21)
  assert.deepEqual(replacement.surfaceOp, { op: 'replace', start: 4, end: 4 })
  assert.deepEqual(replacement.sourceEventSeqs, [4])
  assert.ok(projected.some((event) => event.type === 'future/surface'))
  assert.ok(!projected.some((event) => event.type === 'future/audit'))
  assert.deepEqual(projected.map((event) => event.seq), [...projected.map((event) => event.seq)].sort((a, b) => a - b))
})

test('cold snapshot reads persistence once and emits a bounded frame', async () => {
  const frames = []
  const persistence = {
    readFrom: async () => ({ events: [
      { seq: 1, type: 'user/message' },
      { seq: 2, type: 'assistant/message' },
    ] }),
  }
  const store = createHistoryStore({
    host: { persistence: () => persistence },
    surfaceTypes: new Set(['user/message', 'assistant/message']),
    trimEvent: (event) => event,
    buildToolNames: () => new Map(),
    snapshotCap: 1,
  })
  const conn = { ws: {}, agent: { id: 'cold', session: undefined }, toolNames: new Map(), log: null, surface: null }
  store.sendSnapshot(conn, (_ws, frame) => frames.push(frame))
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(frames[0].events.length, 1)
  assert.equal(frames[0].truncated, true)
})

test('cold snapshot is dropped when its connection detaches during persistence read', async () => {
  let resolveRead
  const persistence = {
    readFrom: () => new Promise((resolve) => { resolveRead = resolve }),
  }
  const store = createHistoryStore({
    host: { persistence: () => persistence },
    surfaceTypes: new Set(['user/message']),
    trimEvent: (event) => event,
    buildToolNames: () => new Map(),
    snapshotCap: 1,
  })
  const abort = new AbortController()
  const frames = []
  const conn = { ws: {}, agent: { id: 'old', session: undefined }, abort, toolNames: new Map(), log: null, surface: null }
  store.sendSnapshot(conn, (_ws, frame) => frames.push(frame))
  abort.abort()
  resolveRead({ events: [{ seq: 1, type: 'user/message' }] })
  await new Promise((resolve) => setImmediate(resolve))
  assert.deepEqual(frames, [])
  assert.equal(conn.log, null, 'detached connection is not mutated by the stale read')
})
