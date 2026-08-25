import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import { createSessionService } from '../src/session.js'

function agentContext() {
  return { on: () => () => {} }
}

function modelSelection(calls, current) {
  return {
    defaultSelection: () => current,
    install: async (_agentCtx, selection) => calls.push(['install-model-selection', selection.current]),
  }
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
    modelSelection: modelSelection(calls, { provider: 'p', model: 'm' }),
    attach: (_ws, agent, clientCwd) => ({ agent, clientCwd }),
    detach: (_conn, options) => calls.push(['detach', options.keepSocket]),
  })

  const next = await service.createNewSession({}, old)
  assert.equal(next.agent, createdAgent)
  assert.deepEqual(calls, [
    ['create', cwd, 'standard'],
    ['install-model-selection', { provider: 'p', model: 'm' }],
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
    modelSelection: modelSelection(calls),
    attach: () => {},
    detach: () => {},
  })
  assert.equal(await service.resumePersistedSession('s1'), resumed)
  assert.deepEqual(calls, [
    ['install-model-selection', undefined],
    ['mount', 'standard'],
  ])
  assert.ok(modelSelections.has('s1'))
})

test('createNewSession inherits the full reasoning-effort triple from the old session', async () => {
  const cwd = mkdtempSync(join(tmpdir(), 'dsh-tui-session-'))
  const modelSelections = new Map()
  const old = {
    agent: { id: 'old-1', options: { provider: 'openai', model: 'gpt' }, session: { header: { cwd } } },
    clientCwd: cwd,
  }
  modelSelections.set('old-1', { current: { provider: 'openai', model: 'gpt', reasoningEffort: 'high' }, assembled: undefined })
  const createdAgent = { id: 'new-1', options: {}, session: { header: { cwd } } }
  const host = {
    agents: () => ({
      create: async (options) => {
        await options.setup(agentContext())
        return { agent: createdAgent }
      },
    }),
    presets: () => ({ composedPreset: () => 'standard', resolve: async (id) => ({ id }), mount: async () => {} }),
    workspaces: () => ({ resolveByPath: async () => ({ attachSession: async () => {} }) }),
    persistence: () => undefined,
  }
  const ctx = { get: (name) => name === 'agentDefaultModel'
    ? { currentSelection: () => ({ provider: 'p', model: 'm' }) }
    : undefined }
  const service = createSessionService({
    host,
    ctx,
    modelSelections,
    modelSelection: { defaultSelection: () => ({ provider: 'p', model: 'm' }), install: async () => {} },
    attach: (_ws, agent, clientCwd) => ({ agent, clientCwd }),
    detach: () => {},
  })
  await service.createNewSession({}, old)
  assert.deepEqual(modelSelections.get('new-1').current, {
    provider: 'openai',
    model: 'gpt',
    reasoningEffort: 'high',
  })
})

test('createNewSession hydrates a host-created session effort from session.models', async () => {
  const cwd = mkdtempSync(join(tmpdir(), 'dsh-tui-session-'))
  const modelSelections = new Map()
  // Host-created live session: no bridge mirror, only `agent.options`.
  const old = {
    agent: { id: 'old-1', options: { provider: 'openai', model: 'gpt' }, session: { header: { cwd } } },
    clientCwd: cwd,
  }
  const createdAgent = { id: 'new-1', options: {}, session: { header: { cwd } } }
  const host = {
    agents: () => ({
      create: async (options) => {
        await options.setup(agentContext())
        return { agent: createdAgent }
      },
    }),
    presets: () => ({ composedPreset: () => 'standard', resolve: async (id) => ({ id }), mount: async () => {} }),
    workspaces: () => ({ resolveByPath: async () => ({ attachSession: async () => {} }) }),
    persistence: () => undefined,
  }
  const ctx = { get: (name) => name === 'agentDefaultModel'
    ? { currentSelection: () => ({ provider: 'p', model: 'm' }) }
    : undefined }
  const service = createSessionService({
    host,
    ctx,
    modelSelections,
    modelSelection: { defaultSelection: () => ({ provider: 'p', model: 'm' }), install: async () => {} },
    sessionModel: { models: async () => ({ current: { provider: 'openai', model: 'gpt-pro', reasoningEffort: 'high' } }) },
    attach: (_ws, agent, clientCwd) => ({ agent, clientCwd }),
    detach: () => {},
  })
  await service.createNewSession({}, old)
  assert.deepEqual(modelSelections.get('new-1').current, {
    provider: 'openai',
    model: 'gpt-pro',
    reasoningEffort: 'high',
  })
})

test('cold resume hydrates the session selection from session.models', async () => {
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
    presets: () => ({ resolve: async (id) => ({ id }), mount: async () => {} }),
    workspaces: () => undefined,
  }
  const service = createSessionService({
    host,
    ctx: { get: () => undefined },
    modelSelections,
    modelSelection: { defaultSelection: () => ({ provider: 'p', model: 'm' }), install: async () => {} },
    sessionModel: { models: async () => ({ current: { provider: 'openai', model: 'gpt', reasoningEffort: 'low' }, groups: [] }) },
    attach: () => {},
    detach: () => {},
  })
  assert.equal(await service.resumePersistedSession('s1'), resumed)
  assert.deepEqual(modelSelections.get('s1').current, {
    provider: 'openai',
    model: 'gpt',
    reasoningEffort: 'low',
  })
})
