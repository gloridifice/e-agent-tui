// smoke-bridge.mjs — contract smoke over the DEPLOYED bridge copy.
// Run after mounting a bridge change or upgrading DSH:
//   node tools/verify-dsh-upgrade.mjs
// It verifies that the deployed profile still resolves the public
// @deepseek-ai/dsh-agent model-selection helper and that bounded bridge
// helpers retain their expected contracts.
import { existsSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'

const home = process.env.DSH_HOME ?? join(process.env.USERPROFILE ?? '.', '.dsh')
const profile = process.env.DSH_TUI_SMOKE_PROFILE ?? 'web'
const bridgeRoot = join(home, 'profiles', profile, 'packages', 'dsh-tui-bridge')
const entry = join(bridgeRoot, 'src', 'index.js')
const modelSelectionEntry = join(bridgeRoot, 'src', 'model-selection.js')
if (!existsSync(entry)) {
  console.error(`smoke-bridge: deployed bridge not found at ${entry} (robocopy bridge/src first)`)
  process.exit(1)
}
if (!existsSync(modelSelectionEntry)) {
  console.error(`smoke-bridge: deployed model-selection adapter not found at ${modelSelectionEntry} (mount the current bridge first)`)
  process.exit(1)
}

const m = await import(pathToFileURL(entry).href)
const modelSelection = await import(pathToFileURL(modelSelectionEntry).href)
const checks = []

async function check(name, fn) {
  try {
    await fn()
    checks.push([name, true, ''])
  } catch (error) {
    checks.push([name, false, String(error?.message ?? error)])
  }
}

await check('exports apply/inject/name', () => {
  if (typeof m.apply !== 'function' || typeof m.inject === 'undefined' || typeof m.name !== 'string') {
    throw new Error('plugin surface missing')
  }
})

await check('_trimToolResultEvent strips read results', () => {
  const e = {
    type: 'tool/result',
    data: { message: { content: [{ toolCallId: 'c1', content: [{ type: 'text', text: 'x'.repeat(5000) }] }] } },
  }
  const out = m._trimToolResultEvent(e, new Map([['c1', 'read']]))
  if (out.data.message.content[0].content[0].text !== '') throw new Error('read payload not stripped')
})

await check('_trimToolResultEvent keeps the 2000-char tail', () => {
  const e = {
    type: 'tool/result',
    data: { message: { content: [{ toolCallId: 'c1', content: [{ type: 'text', text: `h${'x'.repeat(5000)}t` }] }] } },
  }
  const out = m._trimToolResultEvent(e, new Map([['c1', 'bash']]))
  const text = out.data.message.content[0].content[0].text
  if (text.length !== 2000 || !text.endsWith('t')) throw new Error(`tail wrong: ${text.length}`)
})

await check('_latestTitle finds the newest title', () => {
  if (m._latestTitle([{ type: 'session/title', data: { title: 'T' } }]) !== 'T') throw new Error('title missed')
})

await check('_sessionPresetOf prefers selected events', () => {
  if (m._sessionPresetOf({ agentPreset: 'a' }, [{ type: 'agent-preset/selected', data: { agentPreset: 'b' } }]) !== 'b') {
    throw new Error('preset resolution wrong')
  }
})

await check('model-selection adapter resolves public DSH helper and preserves assembly routing', async () => {
  const expected = modelSelection.MODEL_SELECTION_UPSTREAM
  if (expected?.package !== '@deepseek-ai/dsh-agent' || expected.export !== 'installModelSelection') {
    throw new Error('adapter compatibility metadata is wrong')
  }
  const handlers = new Map()
  const ctx = {
    on(event, listener) {
      handlers.set(event, listener)
      return () => handlers.delete(event)
    },
  }
  const adapter = modelSelection.createModelSelectionAdapter()
  const selection = { current: { provider: 'p', model: 'm' }, assembled: undefined }
  const dispose = await adapter.install(ctx, selection)
  const assembled = await handlers.get('system-prompt/assemble')({}, {}, async () => ({ variables: { cwd: '/tmp' } }))
  if (assembled.variables.provider !== 'p' || assembled.variables.model !== 'm') {
    throw new Error('assembly variables missing')
  }
  const request = await handlers.get('agent/request')({}, async () => ({ stream: true, reasoningEffort: 'inherited' }))
  if (request.provider !== 'p' || request.model !== 'm' || Object.hasOwn(request, 'reasoningEffort')) {
    throw new Error('request routing or inherited effort handling drifted')
  }
  dispose()
  if (handlers.size !== 0) throw new Error('adapter disposer did not remove both listeners')
})

let failed = 0
for (const [name, ok, error] of checks) {
  if (ok) {
    console.log(`PASS  ${name}`)
  } else {
    failed += 1
    console.error(`FAIL  ${name}: ${error}`)
  }
}
if (failed > 0) {
  console.error(`smoke-bridge: ${failed} contract(s) drifted — the bridge may be out of sync with the installed DSH version`)
  process.exit(1)
}
console.log(`smoke-bridge: ${checks.length} contracts OK (${entry})`)
