import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createHostPort, openApiProxyMux } from '../src/host.js'

test('host port declares capabilities and validates core services', () => {
  const services = new Map([['agents', {}], ['sessionPersistence', {}], ['llm', {}]])
  const host = createHostPort({ get: (name) => services.get(name) })
  host.assertCore()
  assert.equal(host.capabilities().agents, true)
  assert.equal(host.capabilities().llm, true)
  assert.equal(host.capabilities().commands, false)
  assert.equal(host.llm(), services.get('llm'))
})

test('host port reports missing required services at one boundary', () => {
  const host = createHostPort({ get: () => undefined })
  assert.throws(() => host.assertCore(), /agents, sessionPersistence/)
})

test('api proxy mux receives a complete RpcRequest envelope', () => {
  const signal = new AbortController().signal
  const expected = Symbol('stream')
  let received
  const apiProxy = {
    events: {
      mux(request, actualSignal) {
        received = { request, signal: actualSignal }
        return expected
      },
    },
  }

  assert.equal(openApiProxyMux(apiProxy, signal, 'rpc-test'), expected)
  assert.deepEqual(received.request, { rpcId: 'rpc-test', payload: {} })
  assert.equal(received.signal, signal)
})
