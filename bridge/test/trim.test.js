// Payload-trimming contracts (node --test test/trim.test.js).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  COMPACTION_SUMMARY_MAX_CHARS,
  TOOL_META_MAX_CHARS,
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
  assert.equal(out.data.dshTuiTrimmed, true)
  assert.equal(out.data.dshTuiOutputTrimmed, true)
})

test('non-read results keep only the tail', () => {
  const names = new Map([['c1', 'bash']])
  const out = trimToolResultEvent(resultEvent('c1', `head${'x'.repeat(5000)}tail`), names)
  const text = out.data.message.content[0].content[0].text
  assert.equal(text.length, TOOL_RESULT_TAIL_CHARS)
  assert.ok(text.endsWith('tail'))
  assert.equal(out.data.dshTuiTrimmed, true)
  assert.equal(out.data.dshTuiOutputTrimmed, true)
})

test('nested code output, rich meta, and compaction summaries are bounded', () => {
  const code = trimToolResultEvent({
    type: 'tool/code-dispatch', data: { content: [{ type: 'text', text: 'x'.repeat(5000) }] },
  }, new Map())
  assert.equal(code.data.content[0].text.length, TOOL_RESULT_TAIL_CHARS)
  assert.equal(code.data.dshTuiTrimmed, true)

  const withMeta = resultEvent('c1', 'short')
  withMeta.data.meta = { diff: 'x'.repeat(TOOL_META_MAX_CHARS + 100) }
  const meta = trimToolResultEvent(withMeta, new Map())
  assert.equal(meta.data.meta.dshTuiTrimmed, true)
  assert.equal(meta.data.dshTuiOutputTrimmed, undefined, 'meta-only trim does not qualify output lines')
  assert.ok(meta.data.meta.preview.length <= TOOL_META_MAX_CHARS)

  const compact = trimToolResultEvent({
    type: 'compaction/summary', data: { summary: [{ type: 'text', text: 'x'.repeat(COMPACTION_SUMMARY_MAX_CHARS + 10) }] },
  }, new Map())
  assert.equal(compact.data.summary[0].text.length, COMPACTION_SUMMARY_MAX_CHARS)
})

test('unknown surface events keep only bounded identity and surface metadata', () => {
  const event = {
    seq: 9,
    time: 10,
    type: `future/${'x'.repeat(500)}`,
    surfaceOp: 'append',
    sourceEventSeqs: Array.from({ length: 500 }, (_, index) => index),
    data: { huge: 'z'.repeat(100_000) },
  }
  const out = trimToolResultEvent(event, new Map(), new Set(['user/message']))
  assert.ok(out.type.length <= 160)
  assert.equal(out.surfaceOp, 'append')
  assert.equal(out.sourceEventSeqs.length, 256)
  assert.deepEqual(out.data, { dshTuiUnknownSurface: true })
  assert.ok(JSON.stringify(out).length < 4_000)
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
