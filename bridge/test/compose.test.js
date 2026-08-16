// Composition-helper contracts (node --test test/compose.test.js).
// installModelSelection gets a minimal fake Cordis context: `on` records
// the waterfall listener so the test can drive it directly.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync, mkdirSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  installModelSelection,
  isExistingDirectory,
  latestTitle,
  sessionPresetOf,
} from '../src/compose.js'

test('latestTitle returns the newest session/title', () => {
  const events = [
    { type: 'user/message' },
    { type: 'session/title', data: { title: '第一个' } },
    { type: 'session/title', data: { title: '第二个' } },
  ]
  assert.equal(latestTitle(events), '第二个')
  assert.equal(latestTitle([]), undefined)
  assert.equal(latestTitle(undefined), undefined)
})

test('sessionPresetOf prefers agent-preset/selected over the header', () => {
  const events = [{ type: 'agent-preset/selected', data: { agentPreset: 'minimal' } }]
  assert.equal(sessionPresetOf({ agentPreset: 'standard' }, events), 'minimal')
  assert.equal(sessionPresetOf({ agentPreset: 'cordis' }, []), 'cordis')
  assert.equal(sessionPresetOf({}, []), undefined)
})

test('isExistingDirectory accepts real dirs only', () => {
  const dir = mkdtempSync(join(tmpdir(), 'dsh-tui-compose-'))
  assert.equal(isExistingDirectory(dir), true)
  assert.equal(isExistingDirectory(join(dir, 'missing')), false)
  assert.equal(isExistingDirectory(''), false)
  assert.equal(isExistingDirectory(undefined), false)
})

/** Drive the recorded waterfall listener like the host would:
 *  args = the listener's positional payloads + a next() continuation. */
async function drive(agentCtx, event, ...args) {
  const listener = agentCtx.listeners.get(event)
  assert.ok(listener, `listener registered for ${event}`)
  return listener(...args)
}

test('installModelSelection injects provider/model into assembly', async () => {
  const agentCtx = { listeners: new Map(), on: (event, fn) => {
    agentCtx.listeners.set(event, fn)
    return () => {}
  } }
  const selection = { current: { provider: 'p', model: 'm' }, assembled: undefined }
  installModelSelection(agentCtx, selection)
  const assembled = await drive(agentCtx, 'system-prompt/assemble', {}, {}, async () => ({
    variables: { cwd: '/tmp' },
  }))
  assert.deepEqual(assembled.variables, { cwd: '/tmp', provider: 'p', model: 'm' })
  assert.deepEqual(selection.assembled, selection.current)
})

test('installModelSelection routes requests to the assembled model', async () => {
  const agentCtx = { listeners: new Map(), on: (event, fn) => {
    agentCtx.listeners.set(event, fn)
    return () => {}
  } }
  const selection = {
    current: { provider: 'p', model: 'm', reasoningEffort: 'low' },
    assembled: undefined,
  }
  installModelSelection(agentCtx, selection)
  await drive(agentCtx, 'system-prompt/assemble', {}, {}, async () => ({ variables: {} }))
  const request = await drive(agentCtx, 'agent/request', {}, async () => ({
    reasoningEffort: 'high',
    stream: true,
  }))
  assert.deepEqual(request, { stream: true, provider: 'p', model: 'm', reasoningEffort: 'low' })
})

test('installModelSelection is a pass-through without a selection', async () => {
  const agentCtx = { listeners: new Map(), on: (event, fn) => {
    agentCtx.listeners.set(event, fn)
    return () => {}
  } }
  const selection = { current: undefined, assembled: undefined }
  installModelSelection(agentCtx, selection)
  const assembled = await drive(agentCtx, 'system-prompt/assemble', {}, {}, async () => ({ variables: {} }))
  assert.deepEqual(assembled, { variables: {} })
  const request = await drive(agentCtx, 'agent/request', {}, async () => ({ stream: true }))
  assert.deepEqual(request, { stream: true })
})
