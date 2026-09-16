import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createClientDispatcher } from '../src/dispatcher.js'

function harness(options = {}) {
  const frames = []
  const modelSelections = options.modelSelections ?? new Map()
  const closes = []
  const followups = []
  const steerings = []
  const cancellations = []
  const conn = {
    agent: {
      id: 'a1',
      followup: (message) => followups.push(message),
      steer: (message) => steerings.push(message),
      cancel: (reason) => cancellations.push(reason),
      options: {},
    },
    abort: new AbortController(),
    pending: new Map(),
    commandAborts: new Set(),
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
    injectSkill: options.injectSkill ?? (() => {}),
    historyEvents: () => ({ events: [], hasMore: false }),
    listSessions: options.listSessions ?? (async () => []),
    apiProxy: options.apiProxy,
    questionSessions: options.questionSessions ?? new Map(),
    sendModel: options.sendModel ?? (async () => {}),
    modelSelections,
    sessionModel: options.sessionModel ?? {
      models: async () => ({ current: undefined, groups: [] }),
      selectModel: async ({ provider, model, reasoningEffort }) => ({
        selected: { provider, model, ...(reasoningEffort === undefined ? {} : { reasoningEffort }) },
      }),
    },
    sessionPrompt: options.sessionPrompt,
    compactionModels: options.compactionModels,
    reloadResources: options.reloadResources,
    createUserMessage: (message) => message,
  })
  return { dispatcher, frames, closes, followups, steerings, cancellations, conn, conns, modelSelections }
}

test('first new-input skill is invoked on the materialized session, not sent literally', async () => {
  for (const prompt of ['', '检查  code\n  next line  ']) {
    const skills = []
    const h = harness({
      injectSkill: async (_ws, conn, name, prompt) => skills.push([conn.agent.id, name, prompt]),
      sessionService: {
        createNewSession: async (_ws, current) => {
          if (!current) return h.conn
          const next = { ...current, agent: { ...current.agent, id: 'new' } }
          h.conns.delete(current)
          h.conns.add(next)
          return next
        },
      },
    })
    const send = (message) => h.dispatcher.handle(Buffer.from(JSON.stringify(message)))
    send({ type: 'hello', token: 'secret' })
    await new Promise((resolve) => setImmediate(resolve))
    send({ type: 'new-input', mode: 'standard', content: [{ type: 'text', text: `/skill:review ${prompt}` }] })
    await new Promise((resolve) => setImmediate(resolve))
    assert.deepEqual(skills, [['new', 'review', prompt]])
    assert.equal(h.followups.length, 0)
  }
})

test('compaction configuration is acknowledged before a dependent compact command', async () => {
  const calls = []
  let finish
  const h = harness({
    compactionModels: { configure: async (_agent, args, isCurrent) => {
      calls.push(args)
      await new Promise(resolve => { finish = resolve })
      assert.equal(isCurrent(), true)
      return 'configured'
    } },
    commands: { execute: async () => { calls.push('compact'); return undefined } },
  })
  const send = message => h.dispatcher.handle(Buffer.from(JSON.stringify(message)))
  send({ type: 'hello', token: 'secret' })
  await new Promise(resolve => setImmediate(resolve))
  send({ type: 'command', line: '/compact set-model p/small' })
  send({ type: 'command', line: '/compact' })
  await new Promise(resolve => setImmediate(resolve))
  assert.deepEqual(calls, ['set-model p/small'])
  finish()
  await new Promise(resolve => setImmediate(resolve))
  assert.deepEqual(calls, ['set-model p/small', 'compact'])
  assert(h.frames.some(frame => frame.type === 'command-result' && frame.text === 'configured'))
})

test('reload waits for backend refresh and suppresses stale results', async () => {
  for (const stale of [false, true]) {
    let finish
    const h = harness({ reloadResources: () => new Promise(resolve => { finish = resolve }) })
    const send = message => h.dispatcher.handle(Buffer.from(JSON.stringify(message)))
    send({ type: 'hello', token: 'secret' })
    await new Promise(resolve => setImmediate(resolve))
    send({ type: 'command', line: '/reload' })
    await new Promise(resolve => setImmediate(resolve))
    assert.equal(h.frames.length, 0)
    if (stale) h.conns.delete(h.conn)
    finish([{ type: 'skills', skills: [{ name: 'new-skill', description: 'new' }] }])
    await new Promise(resolve => setImmediate(resolve))
    assert.equal(h.frames.length, stale ? 0 : 2)
    if (!stale) assert.equal(h.frames[1].commandId, 'reload')
  }
})

test('attached skill command forwards its trailing prompt without generic execution', async () => {
  for (const prefix of ['/skill:review', '/skill review']) {
    const skills = []
    const h = harness({
      injectSkill: async (_ws, conn, name, prompt) => skills.push([conn.agent.id, name, prompt]),
      commands: { execute: () => assert.fail('skill must not use generic commands') },
    })
    const send = (message) => h.dispatcher.handle(Buffer.from(JSON.stringify(message)))
    send({ type: 'hello', token: 'secret' })
    await new Promise((resolve) => setImmediate(resolve))
    send({ type: 'command', line: `${prefix}\t检查  code\n  next line  ` })
    await new Promise((resolve) => setImmediate(resolve))
    assert.deepEqual(skills, [['a1', 'review', '检查  code\n  next line  ']])
  }
})

test('model and effort changes settle in order before the first prompt or skill', async () => {
  const selections = new Map([['a1', { current: { provider: 'p', model: 'B', reasoningEffort: 'low' } }]])
  const gates = []
  const skills = []
  const h = harness({
    modelSelections: selections,
    sessionModel: {
      selectModel: ({ sessionId: _id, ...selected }) => new Promise((resolve) => {
        gates.push(() => resolve({ selected }))
      }),
    },
    injectSkill: async () => skills.push({ ...selections.get('a1').current }),
  })
  const send = (message) => h.dispatcher.handle(Buffer.from(JSON.stringify(message)))
  const tick = () => new Promise((resolve) => setImmediate(resolve))
  send({ type: 'hello', token: 'secret' })
  await tick()
  send({ type: 'model-set', provider: 'p', model: 'A' })
  send({ type: 'model-set', provider: 'p', model: 'A', reasoningEffort: 'high' })
  send({ type: 'input', text: 'hello' })
  send({ type: 'command', line: '/skill:review' })
  await tick()
  assert.equal(gates.length, 1)
  assert.equal(h.followups.length, 0)
  assert.equal(skills.length, 0)
  gates.shift()()
  await tick()
  assert.deepEqual(selections.get('a1').current, { provider: 'p', model: 'A' })
  assert.equal(h.followups.length, 0)
  gates.shift()()
  await tick()
  assert.equal(h.followups.length, 1)
  assert.deepEqual(skills, [{ provider: 'p', model: 'A', reasoningEffort: 'high' }])
})

test('dispatcher authenticates, attaches, and routes typed input', async () => {
  const h = harness()
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 2,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'input', content: [{ type: 'text', text: 'hello' }],
  })))
  assert.equal(h.dispatcher.connection(), h.conn)
  assert.equal(h.followups[0].content[0].text, 'hello')
})

test('dispatcher routes steer input to the active turn', async () => {
  const prompts = []
  const h = harness({
    sessionPrompt: {
      prompt: async (sessionId, content, mode) => prompts.push({ sessionId, content, mode }),
    },
  })
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 9,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'input', mode: 'steer', content: [{ type: 'text', text: 'now' }],
  })))
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'input', mode: 'steer', content: [{ type: 'image', mediaType: 'image/png', data: 'AA==' }],
  })))
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(h.steerings[0].content[0].text, 'now')
  assert.deepEqual(prompts, [{
    sessionId: 'a1',
    content: [{ type: 'image', mediaType: 'image/png', data: 'AA==' }],
    mode: 'steer',
  }])
  assert.equal(h.followups.length, 0)
})

test('dispatcher keeps legacy text input compatible while accepting protocol v7 content', async () => {
  const h = harness()
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 6,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({ type: 'input', text: 'legacy input' })))
  assert.deepEqual(h.followups.at(-1).content, [{ type: 'text', text: 'legacy input' }])

  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'new-input', mode: 'standard', text: 'legacy first prompt',
  })))
  await new Promise((resolve) => setImmediate(resolve))
  assert.deepEqual(h.followups.at(-1).content, [{ type: 'text', text: 'legacy first prompt' }])
})

test('dispatcher routes mixed and image-only input through Host prompt admission', async () => {
  const prompts = []
  const h = harness({
    sessionPrompt: {
      prompt: async (sessionId, content) => prompts.push({ sessionId, content }),
    },
  })
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 7,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'input',
    content: [
      { type: 'text', text: 'inspect ' },
      { type: 'image', mediaType: 'image/png', data: 'AA==', name: 'clip.png' },
    ],
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'input',
    content: [{ type: 'image', mediaType: 'image/png', data: 'AA==' }],
  })))
  await new Promise((resolve) => setImmediate(resolve))
  assert.deepEqual(prompts, [
    {
      sessionId: 'a1',
      content: [
        { type: 'text', text: 'inspect ' },
        { type: 'image', mediaType: 'image/png', data: 'AA==', name: 'clip.png' },
      ],
    },
    {
      sessionId: 'a1',
      content: [{ type: 'image', mediaType: 'image/png', data: 'AA==' }],
    },
  ])
  assert.equal(h.followups.length, 0, 'encoded images never enter direct durable messages')
})

test('dispatcher reports image admission failure without direct fallback', async () => {
  const h = harness({
    sessionPrompt: { prompt: async () => { throw new Error('image-invalid: bad bytes') } },
  })
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 7,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'input', content: [{ type: 'image', mediaType: 'image/png', data: 'AA==' }],
  })))
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(h.followups.length, 0)
  assert.equal(h.frames.at(-1).code, 'image-input-failed')
  assert.match(h.frames.at(-1).message, /image-invalid/)
})

test('dispatcher resolves apiProxy lazily when answering a question', async () => {
  const responses = []
  let accessorCalls = 0
  const questionSessions = new Map([['rpc-1', 'a1']])
  const h = harness({
    questionSessions,
    apiProxy: () => {
      accessorCalls += 1
      return {
        respond: async (message, signal) => {
          responses.push({ message, signal })
          return { accepted: true }
        },
      }
    },
  })
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 5,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'answer-questions',
    rpcId: 'rpc-1',
    answers: [{ id: 'choice', selected: ['A'] }],
  })))
  await new Promise((resolve) => setImmediate(resolve))

  assert.equal(accessorCalls, 1)
  assert.equal(responses.length, 1)
  assert.equal(responses[0].signal, h.conn.abort.signal)
  assert.deepEqual(responses[0].message, {
    type: 'client-response',
    rpcId: 'rpc-1',
    result: {
      ok: true,
      value: {
        sessionId: 'a1',
        answer: { answers: [{ id: 'choice', selected: ['A'] }] },
      },
    },
  })
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
    type: 'new-input', mode: 'code', content: [{ type: 'text', text: 'first prompt' }],
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
    type: 'new-input', mode: 'standard', content: [{ type: 'text', text: 'must not reach old' }],
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
      execute: async (agent, line, images, signal) => {
        calls.push({ agent, line, images, signal })
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
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'command',
    line: '/feedback good',
    images: [{ mediaType: 'image/png', data: 'AA==', name: 'clip.png' }],
  })))
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(calls[0].line, '/feedback good')
  assert.deepEqual(calls[0].images, [
    { mediaType: 'image/png', data: 'AA==', name: 'clip.png' },
  ])
  assert.ok(calls[0].signal instanceof AbortSignal)
  assert.equal(calls[0].signal.aborted, false)
  assert.deepEqual(h.frames.at(-1), {
    type: 'command-result', commandId: 'cmd-1', kind: 'success', text: 'feedback recorded',
  })
})

test('dispatcher rejects images on bridge-owned commands instead of dropping them', async () => {
  const h = harness()
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 7,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'command',
    line: '/skill:review',
    images: [{ mediaType: 'image/png', data: 'AA==' }],
  })))
  assert.deepEqual(h.frames.at(-1), {
    type: 'error', code: 'command-failed', message: '/skill does not accept images',
  })
})

test('dispatcher interrupt aborts an active integrated command', async () => {
  let commandSignal
  const h = harness({
    commands: {
      execute: (_agent, _line, _images, signal) => {
        commandSignal = signal
        return new Promise((_resolve, reject) => {
          signal.addEventListener('abort', () => reject(signal.reason), { once: true })
        })
      },
    },
  })
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 5,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({ type: 'command', line: '/slow' })))
  await new Promise((resolve) => setImmediate(resolve))
  assert.equal(commandSignal.aborted, false)

  h.dispatcher.handle(Buffer.from(JSON.stringify({ type: 'interrupt' })))
  await new Promise((resolve) => setImmediate(resolve))

  assert.equal(commandSignal.aborted, true)
  assert.deepEqual(h.cancellations, [{ kind: 'user' }])
  assert.equal(h.frames.at(-1).code, 'command-cancelled')
  assert.equal(h.conn.commandAborts.size, 0)
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

test('dispatcher model-set forwards reasoningEffort and rejects without mutating', async () => {
  const selection = { current: { provider: 'openai', model: 'gpt' }, assembled: undefined }
  const seen = []
  const h = harness({
    modelSelections: new Map([['a1', selection]]),
    sessionModel: {
      models: async () => ({ current: selection.current, groups: [] }),
      selectModel: async (sel) => { seen.push(sel); const { sessionId: _sid, ...rest } = sel; return { selected: rest } },
    },
  })
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 5,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'model-set', provider: 'openai', model: 'gpt', reasoningEffort: 'high',
  })))
  await new Promise((resolve) => setImmediate(resolve))
  assert.deepEqual(seen.at(-1), { sessionId: 'a1', provider: 'openai', model: 'gpt', reasoningEffort: 'high' })
  assert.deepEqual(selection.current, { provider: 'openai', model: 'gpt', reasoningEffort: 'high' })
})

test('dispatcher model-set leaves selection untouched when selectModel rejects', async () => {
  const selection = { current: { provider: 'openai', model: 'gpt' }, assembled: undefined }
  const h = harness({
    modelSelections: new Map([['a1', selection]]),
    sessionModel: {
      models: async () => ({ current: selection.current, groups: [] }),
      selectModel: async () => { throw new Error('model-unavailable: unsupported effort') },
    },
  })
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'hello', token: 'secret', protocolVersion: 5,
  })))
  await new Promise((resolve) => setImmediate(resolve))
  h.dispatcher.handle(Buffer.from(JSON.stringify({
    type: 'model-set', provider: 'openai', model: 'gpt', reasoningEffort: 'bogus',
  })))
  await new Promise((resolve) => setImmediate(resolve))
  assert.deepEqual(selection.current, { provider: 'openai', model: 'gpt' })
  assert.equal(h.frames.at(-1).code, 'model-failed')
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
