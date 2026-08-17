import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createSessionService } from '../src/session.js'

function agentContext() {
  return { on: () => () => {} }
}

test('session service composes preset/model, claims workspace, then reattaches', async () => {
  const cwd = mkdtempSync(join(tmpdir(), 'dsh-tui-session-'))
  const calls = []
  const modelSelections = new Map()
  const presets = {
    composedPreset: () => 'standard',
    resolve: async (id) => ({ id: id ?? 'standard' }),
    mount: async (_ctx, id) => calls.push(['mount', id]),
  }
  const createdAgent = { id: 'new-1', options: {}, session: { header: { cwd } } }
  const agents = {
    create: async (options) => {
      calls.push(['create', options.meta.cwd, options.meta.agentPreset])
      await options.setup(agentContext())
      return { agent: createdAgent }
    },
  }
  const workspace = { attachSession: async (id) => calls.push(['workspace', id]) }
  const host = {
    agents: () => agents,
    presets: () => presets,
    workspaces: () => ({ resolveByPath: async () => workspace }),
    persistence: () => undefined,
  }
  const ctx = { get: (name) => name === 'agentDefaultModel'
    ? { currentSelection: () => ({ provider: 'p', model: 'm' }) }
    : undefined }
  const old = { agent: { options: {}, session: { header: { cwd } } }, clientCwd: cwd }
  const service = createSessionService({
    host,
    ctx,
    modelSelections,
    attach: (_ws, agent, clientCwd) => ({ agent, clientCwd }),
    detach: (_conn, options) => calls.push(['detach', options.keepSocket]),
  })

  const next = await service.createNewSession({}, old)
  assert.equal(next.agent, createdAgent)
  assert.deepEqual(calls, [
    ['create', cwd, 'standard'],
    ['mount', 'standard'],
    ['workspace', 'new-1'],
    ['detach', true],
  ])
  assert.deepEqual(modelSelections.get('new-1').current, { provider: 'p', model: 'm' })
})

test('cold resume restores the recorded preset behind the host port', async () => {
  const calls = []
  const modelSelections = new Map()
  const resumed = { id: 's1' }
  const host = {
    agents: () => ({ resume: async (options) => {
      await options.setup(agentContext())
      return { agent: resumed }
    } }),
    persistence: () => ({
      list: async () => [{ id: 's1' }],
      inspect: async () => ({ meta: { agentPreset: 'standard' }, events: [] }),
    }),
    presets: () => ({
      resolve: async (id) => ({ id }),
      mount: async (_ctx, id) => calls.push(['mount', id]),
    }),
    workspaces: () => undefined,
  }
  const ctx = { get: () => undefined }
  const service = createSessionService({
    host,
    ctx,
    modelSelections,
    attach: () => {},
    detach: () => {},
  })
  assert.equal(await service.resumePersistedSession('s1'), resumed)
  assert.deepEqual(calls, [['mount', 'standard']])
  assert.ok(modelSelections.has('s1'))
})
