import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'

const contract = JSON.parse(readFileSync(new URL('../protocol-contract.json', import.meta.url), 'utf8'))
import { createCompactionModels } from '../src/compaction.js'

function fixture() {
  const listeners = new Map()
  const agent = { session: { id: 's' } }
  const other = { session: { id: 'other' } }
  const models = createCompactionModels({
    ctx: { on: (name, handler) => listeners.set(name, handler) },
    host: { agents: () => ({ get: id => id === 's' ? agent : other }) },
    sessionModel: { catalogModels: async () => ({ groups: [
      { id: 'p', models: [{ id: 'small', name: 'Small' }] },
      { id: 'q', models: [{ id: 'small' }] },
    ] }) },
  })
  const emit = (type, id = 'c') => {
    const event = Object.freeze({ type, data: Object.freeze({ compactionId: id }) })
    listeners.get('session/event')(agent.session, event)
    return event
  }
  const route = options => listeners.get('llm/stream')(options, () => ({ ...options }))
  return { models, agent, emit, route }
}

test('compaction override routes manual and automatic summary calls without changing conversation routing', async () => {
  const { models, agent, emit, route } = fixture()
  await models.configure(agent, 'set-model p/small', () => true)
  for (const id of ['manual', 'automatic']) {
    const start = emit('compaction/start', id)
    assert.equal(models.project(start).data.modelName, 'Small')
    const options = { purpose: 'compaction', sessionId: 's', provider: 'original', model: 'large' }
    assert.equal(route(options).provider, 'p')
    assert.equal(options.model, 'small', 'host summary provenance observes the actual route')
    const end = emit('compaction/end', id)
    assert.equal(models.project(end).data.modelName, 'Small')
    for (const event of [start, end]) {
      const projected = models.project(event)
      const added = Object.keys(projected.data).filter(key => !(key in event.data))
      assert.deepEqual(added, Object.keys(contract.hostEventDataExtensions[event.type].optional))
      for (const field of added) assert.equal(typeof projected.data[field], contract.hostEventDataExtensions[event.type].optional[field])
    }
    assert.equal(start.data.modelName, undefined, 'durable events stay untouched')
  }
  const normal = Object.freeze({ purpose: 'agent', sessionId: 's', provider: 'original', model: 'large' })
  assert.equal(route(normal).model, 'large')
  assert.equal(route({ purpose: 'compaction', sessionId: 'other', model: 'other-model' }).model, 'other-model')
})

test('compaction captures each run selection; unset only affects future runs and legacy history is not relabeled', async () => {
  const { models, agent, emit, route } = fixture()
  await models.configure(agent, 'set-model p/small', () => true)
  const start = emit('compaction/start')
  await models.configure(agent, 'unset-model', () => true)
  assert.equal(route({ purpose: 'compaction', sessionId: 's', model: 'large' }).model, 'small')
  assert.equal(models.project(emit('compaction/end')).data.modelName, 'Small')
  assert.equal(models.project(start).data.modelName, 'Small')
  emit('compaction/start', 'next')
  assert.equal(route({ purpose: 'compaction', sessionId: 's', model: 'large' }).model, 'large')
  assert.equal(models.project(emit('compaction/end', 'next')).data.modelName, 'large')
  const historical = { type: 'compaction/end', data: { compactionId: 'old' } }
  assert.equal(models.project(historical), historical)
})

test('invalid, ambiguous and stale selections do not replace the override', async () => {
  const { models, agent, emit, route } = fixture()
  await models.configure(agent, 'set-model p/small', () => true)
  for (const args of ['set-model small', 'set-model missing', 'unset-model extra']) {
    await assert.rejects(models.configure(agent, args, () => true))
  }
  await models.configure(agent, 'set-model q/small', () => false)
  emit('compaction/start')
  assert.equal(route({ purpose: 'compaction', sessionId: 's', model: 'large' }).provider, 'p')
})
