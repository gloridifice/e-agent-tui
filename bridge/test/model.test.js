// /model frame-shaping contracts (node --test test/model.test.js).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { shapeModelFrame } from '../src/model.js'

test('shapeModelFrame projects groups, models, reasoning, and the current selection', () => {
  const frame = shapeModelFrame(
    [
      {
        id: 'deepseek',
        name: 'DeepSeek',
        models: [
          {
            id: 'deepseek-v4-pro',
            name: 'DeepSeek V4 Pro',
            description: 'flagship',
            contextWindow: 276000,
            reasoning: {
              efforts: [{ id: 'low', name: 'Low' }, { id: 'high', name: 'High' }],
              defaultEffort: 'low',
            },
          },
          { id: 'deepseek-v4', name: 'DeepSeek V4' },
        ],
      },
      { id: 'anthropic', models: [{ id: 'claude-sonnet', name: 'Claude Sonnet' }] },
    ],
    { provider: 'deepseek', model: 'deepseek-v4-pro', reasoningEffort: 'high' },
  )
  assert.equal(frame.providers.length, 2)
  assert.equal(frame.providers[0].id, 'deepseek')
  assert.equal(frame.providers[0].name, 'DeepSeek')
  assert.equal(frame.providers[0].models.length, 2)
  assert.equal(frame.providers[0].models[0].description, 'flagship')
  assert.equal(frame.providers[0].models[0].contextWindow, 276000)
  assert.deepEqual(frame.providers[0].models[0].reasoning, {
    efforts: [{ id: 'low', name: 'Low' }, { id: 'high', name: 'High' }],
    defaultEffort: 'low',
  })
  assert.equal(frame.providers[0].models[1].reasoning, undefined)
  // A provider with no name falls back to its id; no models → empty list.
  assert.equal(frame.providers[1].name, 'anthropic')
  assert.deepEqual(frame.current, {
    provider: 'deepseek',
    model: 'deepseek-v4-pro',
    reasoningEffort: 'high',
  })
})

test('shapeModelFrame omits current when unset and tolerates empty input', () => {
  const frame = shapeModelFrame(undefined, undefined)
  assert.deepEqual(frame.providers, [])
  assert.equal('current' in frame, false)
})

test('shapeModelFrame drops empty reasoning efforts and missing defaults', () => {
  const frame = shapeModelFrame(
    [{ id: 'openai', name: 'OpenAI', models: [{ id: 'gpt', name: 'GPT', reasoning: { efforts: [] } }] }],
    { provider: 'openai', model: 'gpt' },
  )
  assert.deepEqual(frame.providers[0].models[0].reasoning, { efforts: [] })
  assert.equal('defaultEffort' in frame.providers[0].models[0].reasoning, false)
  assert.deepEqual(frame.current, { provider: 'openai', model: 'gpt' })
})
