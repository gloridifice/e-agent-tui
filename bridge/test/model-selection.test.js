// Adapter contracts. The installer fixture mirrors the public DSH declaration
// only inside the test: production code delegates to @deepseek-ai/dsh-agent.
import { readFileSync } from 'node:fs'
import { test } from 'node:test'
import assert from 'node:assert/strict'
import {
  createModelSelectionAdapter,
  MODEL_SELECTION_UPSTREAM,
} from '../src/model-selection.js'

function agentContext() {
  const listeners = new Map()
  const disposed = []
  return {
    listeners,
    disposed,
    on(event, listener) {
      listeners.set(event, listener)
      return () => disposed.push(event)
    },
  }
}

async function drive(ctx, event, ...args) {
  const listener = ctx.listeners.get(event)
  assert.ok(listener, `listener registered for ${event}`)
  return listener(...args)
}

// Test-only stand-in for the behavior promised by dsh-agent's public
// installModelSelection declaration. Its purpose is to exercise the adapter
// port and session-owned mutable selection, not to provide a fallback copy.
function publishedInstallerFixture(agentCtx, selection) {
  const disposeAssembly = agentCtx.on('system-prompt/assemble', async (_assembly, _context, next) => {
    const selected = selection.current
    const assembled = await next()
    selection.assembled = selected
    if (selected === undefined) return assembled
    return {
      ...assembled,
      variables: { ...assembled.variables, provider: selected.provider, model: selected.model },
    }
  })
  const disposeRequest = agentCtx.on('agent/request', async (_payload, next) => {
    const resolved = await next()
    const selected = selection.assembled
    if (selected === undefined) return resolved
    const { reasoningEffort: _inheritedEffort, ...withoutInheritedEffort } = resolved
    return {
      ...withoutInheritedEffort,
      provider: selected.provider,
      model: selected.model,
      ...(selected.reasoningEffort === undefined ? {} : { reasoningEffort: selected.reasoningEffort }),
    }
  })
  return () => { disposeAssembly(); disposeRequest() }
}

function adapter() {
  let loads = 0
  const value = createModelSelectionAdapter({
    loadInstaller: async () => {
      loads += 1
      return publishedInstallerFixture
    },
  })
  return { value, loads: () => loads }
}

test('adapter records the public, tested DSH export rather than a bridge copy', () => {
  assert.deepEqual(MODEL_SELECTION_UPSTREAM, {
    package: '@deepseek-ai/dsh-agent',
    export: 'installModelSelection',
    testedHost: '0.1.0-rc.6',
    testedPackage: '0.1.0-rc.6',
    signature: '(agentCtx, selection) => disposer',
  })
  const manifest = JSON.parse(readFileSync(new URL('../package.json', import.meta.url), 'utf8'))
  assert.equal(manifest.peerDependencies[MODEL_SELECTION_UPSTREAM.package], MODEL_SELECTION_UPSTREAM.testedPackage)
  assert.equal(manifest.dshCompatibility.testedHost, MODEL_SELECTION_UPSTREAM.testedHost)
  assert.equal(manifest.dshCompatibility.modelSelection.package, MODEL_SELECTION_UPSTREAM.package)
  assert.equal(manifest.dshCompatibility.modelSelection.export, MODEL_SELECTION_UPSTREAM.export)
  assert.equal(manifest.dshCompatibility.modelSelection.testedPackage, MODEL_SELECTION_UPSTREAM.testedPackage)
  assert.equal(manifest.dshCompatibility.verification, 'node tools/verify-dsh-upgrade.mjs')
})

test('adapter gets the default selection from the host service without expanding the session port', () => {
  const { value } = adapter()
  const selection = { provider: 'deepseek', model: 'v4' }
  assert.equal(value.defaultSelection({ get: (name) => name === 'agentDefaultModel'
    ? { currentSelection: () => selection }
    : undefined }), selection)
  assert.equal(value.defaultSelection({ get: () => undefined }), undefined)
})

test('adapter installs assembly variables and preserves the assembled selection across a switch', async () => {
  const { value, loads } = adapter()
  const ctx = agentContext()
  const selection = {
    current: { provider: 'provider-a', model: 'model-a', reasoningEffort: 'low' },
    assembled: undefined,
  }
  const dispose = await value.install(ctx, selection)
  assert.equal(loads(), 1)

  const firstAssembly = await drive(ctx, 'system-prompt/assemble', {}, {}, async () => ({
    variables: { cwd: '/work' },
  }))
  assert.deepEqual(firstAssembly.variables, {
    cwd: '/work', provider: 'provider-a', model: 'model-a',
  })
  selection.current = { provider: 'provider-b', model: 'model-b' }
  const firstRequest = await drive(ctx, 'agent/request', {}, async () => ({
    stream: true,
    reasoningEffort: 'inherited',
  }))
  assert.deepEqual(firstRequest, {
    stream: true, provider: 'provider-a', model: 'model-a', reasoningEffort: 'low',
  })

  const secondAssembly = await drive(ctx, 'system-prompt/assemble', {}, {}, async () => ({ variables: {} }))
  assert.deepEqual(secondAssembly.variables, { provider: 'provider-b', model: 'model-b' })
  const secondRequest = await drive(ctx, 'agent/request', {}, async () => ({
    stream: true,
    reasoningEffort: 'inherited',
  }))
  assert.deepEqual(secondRequest, { stream: true, provider: 'provider-b', model: 'model-b' })
  assert.equal(Object.hasOwn(secondRequest, 'reasoningEffort'), false)

  dispose()
  assert.deepEqual(ctx.disposed, ['system-prompt/assemble', 'agent/request'])
})

test('adapter passes through absent selections and caches the upstream loader', async () => {
  const { value, loads } = adapter()
  const ctx = agentContext()
  const selection = { current: undefined, assembled: undefined }
  await value.install(ctx, selection)
  await value.install(agentContext(), { current: undefined, assembled: undefined })
  assert.equal(loads(), 1, 'one adapter resolves the official export once')

  const assembled = { variables: { cwd: '/work' } }
  assert.equal(
    await drive(ctx, 'system-prompt/assemble', {}, {}, async () => assembled),
    assembled,
  )
  const request = { stream: true, reasoningEffort: 'host-default' }
  assert.equal(await drive(ctx, 'agent/request', {}, async () => request), request)
})
