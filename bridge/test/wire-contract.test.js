import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { fileURLToPath } from 'node:url'

import { createClientDispatcher } from '../src/dispatcher.js'
import { encodeBoundedFrame } from '../src/frame.js'
import {
  MAX_FRAME_BYTES,
  MESSAGE_SHAPES,
  RECORD_SHAPES,
  WIRE_CONTRACT,
} from '../src/protocol.js'

const fixtures = JSON.parse(readFileSync(new URL('./fixtures/wire-contract-fixtures.json', import.meta.url), 'utf8'))

function assertValueType(value, type, path) {
  if (type.endsWith('[]')) {
    assert.ok(Array.isArray(value), `${path} must be ${type}`)
    value.forEach((item, index) => assertValueType(item, type.slice(0, -2), `${path}[${index}]`))
    return
  }
  if (type === 'string') assert.equal(typeof value, 'string', `${path} must be string`)
  else if (type === 'boolean') assert.equal(typeof value, 'boolean', `${path} must be boolean`)
  else if (type === 'integer') assert.ok(Number.isSafeInteger(value) && value >= 0, `${path} must be an unsigned integer`)
  else if (type === 'host-event') {
    assert.equal(typeof value, 'object', `${path} must be a host-event object`)
    assert.equal(typeof value?.type, 'string', `${path}.type must be string`)
  } else {
    assertShape(value, RECORD_SHAPES[type], path)
  }
}

function assertShape(value, shape, path) {
  assert.ok(value !== null && !Array.isArray(value) && typeof value === 'object', `${path} must be object`)
  for (const [field, type] of Object.entries(shape.required)) {
    assert.ok(Object.hasOwn(value, field), `${path}.${field} is required`)
    assertValueType(value[field], type, `${path}.${field}`)
  }
  for (const [field, type] of Object.entries(shape.optional)) {
    if (Object.hasOwn(value, field)) assertValueType(value[field], type, `${path}.${field}`)
  }
  const allowed = new Set(['type', ...Object.keys(shape.required), ...Object.keys(shape.optional)])
  for (const field of Object.keys(value)) assert.ok(allowed.has(field), `${path}.${field} is undeclared`)
}

function dispatcher() {
  const sent = []
  const ws = { close() {} }
  const conns = {
    has: () => false,
    isCurrent: () => false,
  }
  const instance = createClientDispatcher({
    ws,
    token: 'contract-token',
    ctx: {},
    host: { agents: () => undefined },
    send: (_ws, frame) => sent.push(frame),
    conns,
    attach: () => undefined,
    detach: () => {},
    sessionService: {
      createNewSession: async () => undefined,
      resumePersistedSession: async () => undefined,
    },
    injectSkill: () => {},
    historyEvents: () => ({ events: [], hasMore: false }),
    listSessions: async () => [],
    apiProxy: undefined,
    questionSessions: new Map(),
    sendModel: async () => {},
    modelSelections: new Map(),
    createUserMessage: (value) => value,
  })
  return { instance, sent }
}

test('generated protocol artifacts are synchronized with the canonical contract', () => {
  const tool = fileURLToPath(new URL('../../tools/sync-protocol-contract.mjs', import.meta.url))
  const result = spawnSync(process.execPath, [tool, '--check'], { encoding: 'utf8' })
  assert.equal(result.status, 0, `${result.stdout}${result.stderr}`)
})

test('generated samples cover every roster entry and conform to payload shapes', () => {
  assert.deepEqual(Object.keys(fixtures.client), WIRE_CONTRACT.clientMessages)
  assert.deepEqual(Object.keys(fixtures.server), WIRE_CONTRACT.serverMessages)
  assert.deepEqual(Object.keys(MESSAGE_SHAPES.client), WIRE_CONTRACT.clientMessages)
  assert.deepEqual(Object.keys(MESSAGE_SHAPES.server), WIRE_CONTRACT.serverMessages)

  for (const direction of ['client', 'server']) {
    for (const type of WIRE_CONTRACT[`${direction}Messages`]) {
      for (const form of ['minimal', 'full']) {
        const sample = fixtures[direction][type][form]
        assert.equal(sample.type, type)
        assertShape(sample, MESSAGE_SHAPES[direction][type], `${direction}.${type}.${form}`)
      }
    }
  }
})

test('dispatcher recognizes every canonical client sample and rejects unknown types', () => {
  const { instance } = dispatcher()
  for (const type of WIRE_CONTRACT.clientMessages) {
    const sample = fixtures.client[type].minimal
    assert.equal(instance.handle(Buffer.from(JSON.stringify(sample))), true, `dispatcher accepts ${type}`)
  }
  assert.equal(instance.handle(Buffer.from('{"type":"future-message"}')), false)
  assert.equal(instance.handle(Buffer.from('{')), false)
})

test('every canonical server sample uses bounded frame encoding', () => {
  for (const type of WIRE_CONTRACT.serverMessages) {
    for (const form of ['minimal', 'full']) {
      const sample = fixtures.server[type][form]
      const wire = encodeBoundedFrame(sample, MAX_FRAME_BYTES)
      assert.ok(Buffer.byteLength(wire, 'utf8') <= MAX_FRAME_BYTES)
      assert.deepEqual(JSON.parse(wire), sample, `${type}/${form} stays intact under the normal cap`)
    }
  }

  const oversized = {
    type: 'snapshot',
    events: Array.from({ length: 20 }, (_, seq) => ({ seq, type: 'user/message', data: { text: 'x'.repeat(200) } })),
  }
  const wire = encodeBoundedFrame(oversized, 600)
  assert.ok(Buffer.byteLength(wire, 'utf8') <= 600)
  const bounded = JSON.parse(wire)
  assert.equal(bounded.type, 'snapshot')
  assert.equal(bounded.truncated, true)
  assert.ok(bounded.events.length < oversized.events.length)
  assert.equal(bounded.events.at(-1).seq, oversized.events.at(-1).seq)
})
