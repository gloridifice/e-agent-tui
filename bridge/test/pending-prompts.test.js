import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createPendingPrompts } from '../src/pending-prompts.js'

function harness() {
  const frames = []
  const listeners = new Map()
  let current = true
  const agent = { id: 's1', inbox: { nextStep: [], nextTurn: [{ content: [{ type: 'text', text: 'external follow-up' }] }] } }
  const conn = { agent, ws: {} }
  const emit = (kind) => listeners.get(`agent/inbox/${kind}`)?.({ agent })
  agent.inbox.splice = (target, start, count, inserted) => {
    assert.equal(target, 'next-step')
    agent.inbox.nextStep.splice(start, count, ...inserted)
    emit('discarded')
  }
  const pending = createPendingPrompts({ send: (_, frame) => frames.push(frame), isCurrent: () => current })
  const off = pending.watch({ on: (name, listener) => {
    listeners.set(name, listener)
    return () => listeners.delete(name)
  } }, conn)
  return { pending, conn, agent, frames, emit, off, detach: () => { current = false } }
}

const message = (text) => ({ content: [{ type: 'text', text }] })

test('admission acknowledgment replaces optimistic candidate with latest authoritative queue', async () => {
  const h = harness()
  await h.pending.submit(h.conn, () => {
    h.agent.inbox.nextStep.push(message('same'), message('same'))
    h.emit('inserted')
    assert.equal(h.frames.length, 1, 'intermediate snapshots stay buffered')
  })
  assert.deepEqual(h.frames.at(-1), { type: 'asap-queue', sessionId: 's1', prompts: ['same', 'same'], operation: 'submit' })
  h.agent.inbox.nextStep.shift()
  h.emit('claimed')
  assert.deepEqual(h.frames.at(-1).prompts, ['same'])
  h.off()
  h.agent.inbox.nextStep.shift()
  const count = h.frames.length
  h.emit('claimed')
  assert.equal(h.frames.length, count)
})

test('clear waits for image admission and clears next-step without interrupting or touching next-turn', async () => {
  const h = harness()
  let release
  const admission = new Promise((resolve) => { release = resolve })
  const submit = h.pending.submit(h.conn, async () => {
    await admission
    h.agent.inbox.nextStep.push({ content: [{ type: 'image', ref: 'asset' }] })
    h.emit('inserted')
  })
  const clear = h.pending.clear(h.conn)
  await Promise.resolve()
  assert.equal(h.frames.length, 1)
  release()
  await submit
  await clear
  assert.deepEqual(h.frames.slice(1).map((frame) => [frame.operation, frame.prompts]), [['submit', ['[Image]']], ['clear', []]])
  assert.equal(h.agent.inbox.nextTurn.length, 1)
})

test('admission failure and clear failure report errors with current pending snapshot', async () => {
  const h = harness()
  await h.pending.submit(h.conn, () => { throw new Error('rejected') })
  assert.equal(h.frames.at(-1).operation, 'submit')
  assert.equal(h.frames.at(-1).error, 'rejected')
  h.agent.inbox.nextStep.push(message('retained'))
  h.agent.inbox.splice = () => { throw new Error('cannot clear') }
  await h.pending.clear(h.conn)
  assert.deepEqual(h.frames.at(-1), { type: 'asap-queue', sessionId: 's1', prompts: ['retained'], operation: 'clear', error: 'cannot clear' })
})

test('consumption during admission and detached callbacks cannot revive candidates', async () => {
  const h = harness()
  await h.pending.submit(h.conn, () => {
    h.agent.inbox.nextStep.push(message('consumed'))
    h.emit('inserted')
    h.agent.inbox.nextStep.shift()
    h.emit('claimed')
  })
  assert.deepEqual(h.frames.at(-1).prompts, [])
  const count = h.frames.length
  await h.pending.submit(h.conn, () => h.detach())
  await h.pending.clear(h.conn)
  h.emit('inserted')
  assert.equal(h.frames.length, count)
})
