// Payload-trimming contracts (node --test test/trim.test.js).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  TOOL_RESULT_TAIL_CHARS,
  buildToolNames,
  trimToolResultEvent,
} from '../src/trim.js'

function resultEvent(toolCallId, text) {
  return {
    type: 'tool/result',
    data: {
      message: {
        content: [{
          type: 'tool-result',
          toolCallId,
          content: [{ type: 'text', text }],
        }],
      },
    },
  }
}

test('read results are stripped entirely', () => {
  const names = new Map([['c1', 'read']])
  const out = trimToolResultEvent(resultEvent('c1', 'x'.repeat(5000)), names)
  assert.equal(out.data.message.content[0].content[0].text, '')
})

test('non-read results keep only the tail', () => {
  const names = new Map([['c1', 'bash']])
  const out = trimToolResultEvent(resultEvent('c1', `head${'x'.repeat(5000)}tail`), names)
  const text = out.data.message.content[0].content[0].text
  assert.equal(text.length, TOOL_RESULT_TAIL_CHARS)
  assert.ok(text.endsWith('tail'))
})

test('unchanged events return the same object (no clone)', () => {
  const e = resultEvent('c1', 'short')
  assert.equal(trimToolResultEvent(e, new Map()), e)
})

test('non-tool-result events pass through untouched', () => {
  const e = { type: 'user/message' }
  assert.equal(trimToolResultEvent(e, new Map()), e)
})

test('buildToolNames maps callId to name', () => {
  const names = buildToolNames([
    { type: 'tool/call', data: { callId: 'c1', name: 'bash' } },
    { type: 'user/message' },
  ])
  assert.equal(names.get('c1'), 'bash')
  assert.equal(names.size, 1)
})
