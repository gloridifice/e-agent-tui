import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createClientDispatcher } from '../src/dispatcher.js'

function harness(options = {}) {
  const frames = []
  const modelSelections = options.modelSelections ?? new Map()
  const closes = []
  const followups = []
  const conn = {
    agent: { id: 'a1', followup: (message) => followups.push(message), options: {} },
    abort: { signal: {} },
    pending: new Map(),
    clientCwd: undefined,
  }
  const ws = { close: (code) => closes.push(code) }
  const conns = new Set([conn])
  conns.isCurrent = (current, actual) => current === actual && conns.has(current)
  const dispatcher = createClientDispatcher({
    ws,
    token: 'secret',
    ctx: {},
    host: { agents: () => ({ get: () => undefined }), commands: () => options.commands },
    send: (_ws, frame) => frames.push(frame),
    conns,
    attach: () => conn,
    detach: (current) => conns.delete(current),
    sessionService: options.sessionService ?? {
      createNewSession: async () => conn,
      resumePersistedSession: async () => undefined,
    },
    injectSkill: () => {},
    historyEvents: () => ({ events: [], hasMore: false }),
    listSessions: options.listSessions ?? (async () => []),
    apiProxy: undefined,
    questionSessions: new Map(),
    sendModel: options.sendModel ?? (async () => {}),
    modelSelections,
    createUserMessage: (message) => message,
  })
  return { dispatcher, frames, closes, followups, conn, conns, modelSelections }
}

test('dispatcher authenticates, attaches, and routes typed input', async () => {
  const h = harness()
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 2,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({ type: 'input', text: 'hello' })))
  assert.equal(h.dispatcher.connection(), h.conn)
  assert.equal(h.followups[0].content[0].text, 'hello')
})

test('dispatcher atomically creates a new session before delivering new-input', async () => {
  const calls = []
  const nextFollowups = []
  const next = {
    agent: { id: 'a2', followup: (message) => nextFollowups.push(message), options: {} },
    abort: { signal: {} },
    pending: new Map(),
  }
  const h = harness({
    sessionService: {
      createNewSession: async (_ws, current, mode) => {
        if (!current) return h.conn
        calls.push([current.agent.id, mode])
        h.conns.delete(current)
        h.conns.add(next)
        return next
      },
      resumePersistedSession: async () => undefined,
    },
  })
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 5,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'new-input', mode: 'code', text: 'first prompt',
  })))
  await new Promise((resolve) => setImmediate(resolve))
  assert.deepEqual(calls, [['a1', 'code']])
  assert.equal(h.dispatcher.connection(), next)
  assert.equal(nextFollowups.length, 1)
  assert.equal(nextFollowups[0].content[0].text, 'first prompt')
  assert.equal(nextFollowups[0].source.kind, 'user')
  assert.equal(h.followups.length, 0, 'the retained old agent receives no prompt')
})

test('new-input creation failure keeps the old connection and never misroutes the prompt', async () => {
  const h = harness({
    sessionService: {
      createNewSession: async (_ws, current) => {
        if (!current) return h.conn
        throw new Error('cannot create')
      },
      resumePersistedSession: async () => undefined,
    },
  })
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 5,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'new-input', mode: 'standard', text: 'must not reach old',
  })))
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(h.dispatcher.connection(), h.conn)
  assert.equal(h.followups.length, 0)
  assert.equal(h.frames.at(-1).code, 'new-failed')
})

test('dispatcher bounds authentication/version failure and tolerates bad JSON', () => {
  const h = harness()
  h.dispatcher.handle(Buffer.from('not json'))
  h.dispatcher.handle(Buffer.from(JSON.stringify({ type: 'hello', token: 'wrong' })))
  assert.equal(h.frames[0].code, 'bad-token')
  assert.deepEqual(h.closes, [4003])
})

test('dispatcher routes keepalive without a session', () => {
  const h = harness()
  h.dispatcher.handle(Buffer.from(JSON.stringify({ type: 'ping' })))
  assert.deepEqual(h.frames, [{ type: 'pong' }])
})

test('dispatcher streams a title-pending session list before the enriched list', async () => {
  const sessions = [{ id: 's1', title: '', live: false, createdAt: 1 }]
  const h = harness({
    listSessions: async (onPartial) => {
      onPartial(sessions)
      return [{ ...sessions[0], title: 'Session title' }]
    },
  })
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 4,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({ type: 'list-sessions' })))
  await new Promise((resolve) => setImmediate(resolve))
  assert.deepEqual(h.frames.slice(-2), [
    { type: 'sessions', sessions, titlesPending: true },
    { type: 'sessions', sessions: [{ ...sessions[0], title: 'Session title' }] },
  ])
})

test('dispatcher executes an integrated command and relays its direct UI result', async () => {
  const calls = []
  const h = harness({
    commands: {
      execute: async (agent, line, signal) => {
        calls.push({ agent, line, signal })
        return {
          commandId: 'cmd-1',
          result: { kind: 'success', text: 'feedback recorded' },
        }
      },
    },
  })
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 3,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({ type: 'command', line: '/feedback good' })))
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(calls[0].line, '/feedback good')
  assert.deepEqual(h.frames.at(-1), {
    type: 'command-result', commandId: 'cmd-1', kind: 'success', text: 'feedback recorded',
  })
})

test('dispatcher drops a command result after the connection becomes stale', async () => {
  let settle
  const h = harness({
    commands: {
      execute: () => new Promise((resolve) => { settle = resolve }),
    },
  })
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 3,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({ type: 'command', line: '/slow' })))
  await new Promise((resolve) => setImmediate(resolve))
  h.conns.delete(h.conn)
  settle({ commandId: 'cmd-stale', result: { kind: 'success', text: 'wrong session' } })
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(h.frames.some((frame) => frame.commandId === 'cmd-stale'), false)
})

test('dispatcher model-set updates the next assembly selection and agent options', async () => {
  const selection = { current: { provider: 'before', model: 'old' }, assembled: undefined }
  const refreshed = []
  const h = harness({
    modelSelections: new Map([['a1', selection]]),
    sendModel: async (_ws, agent) => refreshed.push(agent),
  })
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 4,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'model-set', provider: 'after', model: 'new',
  })))
  await new Promise((resolve) => setImmediate(resolve))
  assert.deepEqual(selection.current, { provider: 'after', model: 'new' })
  assert.deepEqual(h.conn.agent.options, { provider: 'after', model: 'new' })
  assert.deepEqual(refreshed, [h.conn.agent])
})

test('dispatcher reports unknown integrated commands without creating model input', async () => {
  const h = harness({ commands: { execute: async () => undefined } })
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 3,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({ type: 'command', line: '/missing' })))
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(h.frames.at(-1).code, 'command-unknown')
  assert.equal(h.followups.length, 0)
})
