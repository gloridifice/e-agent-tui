import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createSessionLister, titleFromObservation } from '../src/session-list.js'

test('title observations unwrap the current settled DSH shape', () => {
  assert.deepEqual(
    titleFromObservation({
      status: 'fulfilled',
      value: { session: { id: 's1' }, title: { title: '真实标题', seq: 9 } },
    }),
    { sessionId: 's1', title: '真实标题' },
  )
  assert.deepEqual(
    titleFromObservation({ status: 'rejected', reason: new Error('bad') }, 's2'),
    { sessionId: 's2', title: undefined },
  )
})

test('session list sends headers first then enriches persisted titles', async () => {
  let resolveTitles
  const requested = []
  const host = {
    persistence: () => ({
      list: async () => [
        { id: 'old', createdAt: 1 },
        { id: 'live', createdAt: 3 },
        { id: 'cold', createdAt: 2 },
      ],
    }),
    agents: () => ({
      list: () => [{
        id: 'live',
        session: { events: [
          { type: 'turn/start', seq: 0, data: { turn: 0 } },
          { type: 'session/title', seq: 1, data: { title: '在线标题' } },
        ] },
      }],
    }),
    sessionQuery: () => ({
      readTitleSnapshots: (ids) => {
        requested.push(...ids)
        return new Promise((resolve) => { resolveTitles = resolve })
      },
    }),
  }
  const partials = []
  const pending = createSessionLister(host, 2)((sessions) => partials.push(sessions))
  await new Promise((resolve) => setImmediate(resolve))
  assert.deepEqual(requested, ['cold'], 'cap and live-title folding happen before disk title reads')
  assert.deepEqual(partials[0].map((session) => [session.id, session.title]), [
    ['live', '在线标题'],
    ['cold', ''],
  ])

  resolveTitles([{
    status: 'fulfilled',
    value: { session: { id: 'cold' }, title: { title: '冷会话标题' } },
  }])
  const final = await pending
  assert.deepEqual(final.map((session) => [session.id, session.title]), [
    ['live', '在线标题'],
    ['cold', '冷会话标题'],
  ])
})

test('session list excludes live and cold setup-only sessions before applying the cap', async () => {
  const headers = [
    { id: 'live-blank', createdAt: 5 },
    { id: 'cold-blank', createdAt: 4 },
    { id: 'real-new', createdAt: 3 },
    { id: 'real-old', createdAt: 2 },
  ]
  const reads = []
  const host = {
    persistence: () => ({
      list: async () => headers,
      readFrom: async (id) => {
        reads.push(id)
        return { events: id === 'cold-blank'
          ? [
              { type: 'permission/preset', seq: 0 },
              { type: 'sandbox/mode', seq: 1 },
            ]
          : [{ type: 'turn/start', seq: 0 }] }
      },
    }),
    agents: () => ({
      list: () => [{
        id: 'live-blank',
        session: {
          header: headers[0],
          events: [{ type: 'approval/policy', seq: 0 }],
        },
      }],
    }),
    sessionQuery: () => undefined,
  }
  const sessions = await createSessionLister(host, 2)()
  assert.deepEqual(sessions.map((session) => session.id), ['real-new', 'real-old'])
  assert.ok(reads.includes('cold-blank'))
})

test('blank classification uses the persisted projection cache before log reads', async () => {
  let logReads = 0
  const host = {
    persistence: () => ({
      list: async () => [
        { id: 'blank', createdAt: 2 },
        { id: 'real', createdAt: 1 },
      ],
      readFrom: async () => { logReads += 1; return { events: [] } },
    }),
    agents: () => ({ list: () => [] }),
    sessionProjectionCache: () => ({
      cachedSnapshot: (header) => ({
        values: { sessionListMetadata: { blank: header.id === 'blank' } },
      }),
      coldSnapshot: async (id) => ({
        values: { sessionListMetadata: { blank: id === 'blank' } },
      }),
    }),
    sessionQuery: () => undefined,
  }
  const sessions = await createSessionLister(host)()
  assert.deepEqual(sessions.map((session) => session.id), ['real'])
  assert.equal(logReads, 0)
})
