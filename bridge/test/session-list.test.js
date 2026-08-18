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
        session: { events: [{ type: 'session/title', data: { title: '在线标题' } }] },
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
