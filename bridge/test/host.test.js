import { test } from 'node:test'
import assert from 'node:assert/strict'
import { createHostPort } from '../src/host.js'

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
