import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createClientDispatcher } from '../src/dispatcher.js'

function harness(options = {}) {
  const frames = []
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
    sessionService: {
      createNewSession: async () => conn,
      resumePersistedSession: async () => undefined,
    },
    injectSkill: () => {},
    historyEvents: () => ({ events: [], hasMore: false }),
    listSessions: async () => [],
    apiProxy: undefined,
    questionSessions: new Map(),
    sendModel: async () => {},
    modelSelections: new Map(),
    createUserMessage: (message) => message,
  })
  return { dispatcher, frames, closes, followups, conn, conns }
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
