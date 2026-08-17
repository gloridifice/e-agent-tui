// /model frame-shaping contracts (node --test test/model.test.js).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { shapeModelFrame } from '../src/model.js'

test('shapeModelFrame projects providers, models, and the current selection', () => {
  const frame = shapeModelFrame(
    [{ id: 'deepseek', name: 'DeepSeek' }, { id: 'anthropic' }],
    {
      deepseek: [
        { id: 'deepseek-v4-pro', name: 'DeepSeek V4 Pro', description: 'flagship' },
        { id: 'deepseek-v4', name: 'DeepSeek V4' },
      ],
      anthropic: [{ id: 'claude-sonnet', name: 'Claude Sonnet' }],
    },
    { provider: 'deepseek', model: 'deepseek-v4' },
  )
  assert.equal(frame.providers.length, 2)
  assert.equal(frame.providers[0].id, 'deepseek')
  assert.equal(frame.providers[0].name, 'DeepSeek')
  assert.equal(frame.providers[0].models.length, 2)
  assert.equal(frame.providers[0].models[0].description, 'flagship')
  assert.equal(frame.providers[0].models[1].description, undefined)
  // A provider with no name falls back to its id; no models → empty list.
  assert.equal(frame.providers[1].name, 'anthropic')
  assert.deepEqual(frame.current, { provider: 'deepseek', model: 'deepseek-v4' })
})

test('shapeModelFrame omits current when unset and tolerates empty input', () => {
  const frame = shapeModelFrame(undefined, undefined, undefined)
  assert.deepEqual(frame.providers, [])
  assert.equal('current' in frame, false)
})
