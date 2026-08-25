// session-model adapter contracts (node --test test/session-model.test.js).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createSessionModelAdapter } from '../src/session-model.js'

function apiProxy(handlers) {
  return {
    sessions: {
      models: async (request) => handlers.models(request),
      selectModel: async (request) => handlers.selectModel(request),
    },
    llm: {
      models: async (request) => handlers.llmModels(request),
    },
  }
}

test('models unwraps the RpcResult and passes the sessionId in the payload', async () => {
  const calls = []
  const adapter = createSessionModelAdapter(apiProxy({
    models: async (request) => {
      calls.push(request)
      return {
        rpcId: request.rpcId,
        result: { ok: true, value: { current: { provider: 'p', model: 'm', reasoningEffort: 'high' }, groups: [] } },
      }
    },
  }))
  const value = await adapter.models('s1')
  assert.deepEqual(value.current, { provider: 'p', model: 'm', reasoningEffort: 'high' })
  assert.equal(calls[0].payload.sessionId, 's1')
  assert.equal(typeof calls[0].rpcId, 'string')
})

test('selectModel submits the full triple and omits an absent effort', async () => {
  const payloads = []
  const adapter = createSessionModelAdapter(apiProxy({
    selectModel: async (request) => {
      payloads.push(request.payload)
      return { rpcId: request.rpcId, result: { ok: true, value: { selected: { ...request.payload } } } }
    },
  }))
  await adapter.selectModel({ sessionId: 's1', provider: 'p', model: 'm', reasoningEffort: 'high' })
  await adapter.selectModel({ sessionId: 's1', provider: 'p', model: 'm' })
  assert.deepEqual(payloads[0], { sessionId: 's1', provider: 'p', model: 'm', reasoningEffort: 'high' })
  assert.deepEqual(payloads[1], { sessionId: 's1', provider: 'p', model: 'm' })
})

test('models and selectModel throw a code-prefixed error on rejection', async () => {
  const adapter = createSessionModelAdapter(apiProxy({
    models: async () => ({ rpcId: 'r', result: { ok: false, error: { code: 'agent-busy', message: 'busy' } } }),
    selectModel: async () => ({ rpcId: 'r', result: { ok: false, error: { code: 'model-unavailable', message: 'no' } } }),
  }))
  await assert.rejects(() => adapter.models('s1'), /agent-busy: busy/)
  await assert.rejects(() => adapter.selectModel({ sessionId: 's1', provider: 'p', model: 'm' }), /model-unavailable: no/)
})

test('a missing apiProxy.sessions fails loudly instead of hanging', async () => {
  const adapter = createSessionModelAdapter(() => undefined)
  await assert.rejects(() => adapter.models('s1'), /session model API .* unavailable/)
})

test('catalogModels reads the host-scoped llm.models groups without a session', async () => {
  const payloads = []
  const adapter = createSessionModelAdapter(apiProxy({
    models: async () => { throw new Error('unused') },
    selectModel: async () => { throw new Error('unused') },
    llmModels: async (request) => {
      payloads.push(request.payload)
      return { rpcId: request.rpcId, result: { ok: true, value: { groups: [{ id: 'p', models: [] }], failures: [] } } }
    },
  }))
  const value = await adapter.catalogModels()
  assert.equal(value.groups[0].id, 'p')
  assert.deepEqual(payloads, [{}])
})
