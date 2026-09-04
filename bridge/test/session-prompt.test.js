import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  createSessionPromptAdapter,
  normalizeCommandImages,
  normalizePromptContent,
} from '../src/session-prompt.js'

test('prompt normalization preserves ordered text and encoded images', () => {
  const content = normalizePromptContent([
    { type: 'text', text: 'before' },
    { type: 'image', mediaType: 'image/png', data: 'AA==', name: 'clip.png' },
    { type: 'text', text: 'after' },
  ])
  assert.deepEqual(content, [
    { type: 'text', text: 'before' },
    { type: 'image', mediaType: 'image/png', data: 'AA==', name: 'clip.png' },
    { type: 'text', text: 'after' },
  ])
  assert.deepEqual(normalizeCommandImages([
    { mediaType: 'image/png', data: 'AA==', name: 'clip.png' },
  ]), [{ mediaType: 'image/png', data: 'AA==', name: 'clip.png' }])
  assert.throws(() => normalizePromptContent([]), /empty/)
  assert.throws(
    () => normalizePromptContent([{ type: 'image', mediaType: 'image/svg+xml', data: 'AA==' }]),
    /invalid part/,
  )
  assert.throws(
    () => normalizeCommandImages([{ type: 'text', text: 'not an image' }]),
    /invalid part/,
  )
})

test('session prompt adapter resolves apiProxy lazily and unwraps Host admission', async () => {
  const calls = []
  let service
  const adapter = createSessionPromptAdapter(() => service)
  await assert.rejects(() => adapter.prompt('s1', [{ type: 'text', text: 'x' }]), /unavailable/)
  service = {
    sessions: {
      prompt: async (request) => {
        calls.push(request)
        return { rpcId: request.rpcId, result: { ok: true, value: { accepted: true } } }
      },
    },
  }
  assert.deepEqual(
    await adapter.prompt('s1', [{ type: 'image', mediaType: 'image/png', data: 'AA==' }]),
    { accepted: true },
  )
  assert.equal(typeof calls[0].rpcId, 'string')
  assert.deepEqual(calls[0].payload, {
    sessionId: 's1',
    mode: 'queue',
    content: [{ type: 'image', mediaType: 'image/png', data: 'AA==' }],
  })
  await adapter.prompt('s1', [{ type: 'text', text: 'now' }], 'steer')
  assert.equal(calls[1].payload.mode, 'steer')
  await assert.rejects(
    () => adapter.prompt('s1', [{ type: 'text', text: 'bad' }], 'later'),
    /invalid prompt mode/,
  )
})

test('session prompt adapter surfaces Host rejection without a fallback', async () => {
  const adapter = createSessionPromptAdapter({
    sessions: {
      prompt: async () => ({
        rpcId: 'r1',
        result: { ok: false, error: { code: 'image-invalid', message: 'bad bytes' } },
      }),
    },
  })
  await assert.rejects(
    () => adapter.prompt('s1', [{ type: 'image', mediaType: 'image/png', data: 'AA==' }]),
    /image-invalid: bad bytes/,
  )
})
