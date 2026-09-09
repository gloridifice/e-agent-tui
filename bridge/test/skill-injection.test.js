import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createSkillInjector } from '../src/skill-injection.js'

const skill = { name: 'review', provider: 'test', content: 'Review carefully.' }

function harness(get = async () => skill) {
  const messages = []
  const errors = []
  const conn = {
    agent: { followup: (message) => messages.push(message) },
    abort: new AbortController(),
  }
  const conns = new Set([conn])
  const inject = createSkillInjector({
    host: { skills: () => ({ get }) },
    conns,
    send: (_ws, frame) => errors.push(frame),
    createUserMessage: (message) => message,
  })
  return { messages, errors, conn, conns, inject }
}

test('skill instructions and the complete user prompt are separate ordered messages', async () => {
  const h = harness()
  const prompt = '检查  code\n  next line  '
  await h.inject({}, h.conn, 'review', prompt)
  assert.equal(h.messages.length, 2)
  assert.equal(h.messages[0].source.kind, 'skill-invocation')
  assert.ok(h.messages[0].content[0].text.includes(skill.content))
  assert.deepEqual(h.messages[1], { content: [{ type: 'text', text: prompt }], source: { kind: 'user' } })
  assert.deepEqual(h.errors, [])
})

test('a bare skill does not enqueue an empty user message', async () => {
  const h = harness()
  await h.inject({}, h.conn, 'review', ' \n ')
  assert.equal(h.messages.length, 1)
})

test('failed skill lookup never sends the trailing prompt', async () => {
  for (const get of [async () => undefined, async () => { throw new Error('lookup failed') }]) {
    const h = harness(get)
    await h.inject({}, h.conn, 'review', 'must not send')
    assert.deepEqual(h.messages, [])
    assert.equal(h.errors.length, 1)
  }
})

test('detaching during skill lookup discards both messages and stale errors', async () => {
  for (const fails of [false, true]) {
    let finish
    const h = harness(() => new Promise((resolve, reject) => {
      finish = () => fails ? reject(new Error('late failure')) : resolve(skill)
    }))
    const pending = h.inject({}, h.conn, 'review', 'must not send')
    h.conns.delete(h.conn)
    finish()
    await pending
    assert.deepEqual(h.messages, [])
    assert.deepEqual(h.errors, [])
  }
})
