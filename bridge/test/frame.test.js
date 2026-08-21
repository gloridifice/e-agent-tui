import { test } from 'node:test'
import assert from 'node:assert/strict'
import { Buffer } from 'node:buffer'
import { encodeBoundedFrame, shapeWelcomeFrame } from '../src/frame.js'

test('welcome reports the actual session preset from header or latest selection', () => {
  const agent = {
    id: 's1',
    status: 'idle',
    options: { provider: 'deepseek', model: 'chat' },
    session: {
      header: { cwd: '/workspace', agentPreset: 'minimal' },
      events: [{ type: 'session/title', data: { title: 'Work' } }],
    },
  }
  assert.deepEqual(shapeWelcomeFrame(agent, 4, 1024), {
    type: 'welcome',
    protocolVersion: 4,
    maxFrameBytes: 1024,
    sessionId: 's1',
    status: 'idle',
    provider: 'deepseek',
    model: 'chat',
    mode: 'minimal',
    title: 'Work',
    cwd: '/workspace',
  })

  agent.session.events.push({
    type: 'agent-preset/selected',
    data: { agentPreset: 'cordis' },
  })
  assert.equal(shapeWelcomeFrame(agent, 4, 1024).mode, 'cordis')
})

test('welcome reports the installed model selection when agent options are unset', () => {
  const agent = {
    id: 's2',
    status: 'idle',
    options: undefined,
    session: {
      header: { cwd: '/work', agentPreset: 'standard' },
      events: [],
    },
  }
  const frame = shapeWelcomeFrame(agent, 4, 1024, { provider: 'deepseek', model: 'chat' })
  assert.equal(frame.provider, 'deepseek')
  assert.equal(frame.model, 'chat')
  assert.equal(frame.mode, 'standard')
  assert.equal(frame.cwd, '/work')
})

test('welcome prefers the installed model selection over stale agent options', () => {
  const frame = shapeWelcomeFrame(
    {
      id: 's3',
      status: 'idle',
      options: { provider: 'stale', model: 'old' },
      session: { header: {}, events: [] },
    },
    4,
    1024,
    { provider: 'deepseek', model: 'chat' },
  )
  assert.equal(frame.provider, 'deepseek')
  assert.equal(frame.model, 'chat')
})

test('ordinary frames are encoded unchanged below the byte budget', () => {
  const message = { type: 'status', status: 'running' }
  assert.deepEqual(JSON.parse(encodeBoundedFrame(message, 1024)), message)
})

test('snapshot keeps the newest suffix and marks truncation', () => {
  const message = {
    type: 'snapshot',
    truncated: false,
    events: Array.from({ length: 8 }, (_, index) => ({ seq: index + 1, type: 'user/message', data: { text: '界'.repeat(80) } })),
  }
  const wire = encodeBoundedFrame(message, 700)
  const frame = JSON.parse(wire)
  assert.ok(Buffer.byteLength(wire, 'utf8') <= 700)
  assert.equal(frame.truncated, true)
  assert.ok(frame.events.length > 0 && frame.events.length < message.events.length)
  assert.equal(frame.events.at(-1).seq, 8)
})

test('singular oversized frame becomes a bounded compatibility error', () => {
  const wire = encodeBoundedFrame({ type: 'event', event: { data: 'x'.repeat(10_000) } }, 256)
  const frame = JSON.parse(wire)
  assert.ok(Buffer.byteLength(wire, 'utf8') <= 256)
  assert.equal(frame.code, 'frame-too-large')
})
