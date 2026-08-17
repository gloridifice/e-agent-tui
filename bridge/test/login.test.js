// /login layer contracts (node --test test/login.test.js).
// File helpers run against temp homes; the credentials/llm/settings seams
// are faked.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { mkdtempSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  createProxy,
  deleteProxy,
  listProxies,
  providerCredentialRef,
  sendLogin,
  setProviderApiKey,
} from '../src/login.js'

function tempHome() {
  return mkdtempSync(join(tmpdir(), 'dsh-tui-login-'))
}

test('providerCredentialRef reads settings apiKeyEnv, else falls back', () => {
  const llm = {
    listConfigurableProviders: () => [
      { provider: 'deepseek', settingsNs: 'llm-deepseek' },
      { provider: 'mygateway', settingsNs: 'llm-pi-ai' },
      { provider: 'legacy', settingsNs: 'llm-legacy' },
    ],
  }
  const settings = {
    get: (ns) => ns === 'llm-deepseek'
      ? { apiKeyEnv: 'DEEPSEEK_API_KEY' }
      : ns === 'llm-pi-ai'
        ? { providers: { mygateway: { apiKeyEnv: 'MYGATEWAY_KEY' } } }
        : {},
  }
  const ctx = { get: (n) => (n === 'llm' ? llm : n === 'settings' ? settings : undefined) }
  assert.equal(providerCredentialRef(ctx, 'deepseek'), 'DEEPSEEK_API_KEY')
  assert.equal(providerCredentialRef(ctx, 'mygateway'), 'MYGATEWAY_KEY')
  assert.equal(providerCredentialRef(ctx, 'legacy'), 'LEGACY_API_KEY')
  // Without the seams the convention applies.
  assert.equal(providerCredentialRef({ get: () => undefined }, 'deepseek'), 'DEEPSEEK_API_KEY')
})

test('proxy create/list/delete roundtrip against a temp home', () => {
  const home = tempHome()
  const entry = createProxy({
    baseUrl: 'https://example.com/v1',
    apiKey: 'sk-key',
    protocol: 'openai-completions',
    model: 'gpt-4o',
  }, home)
  assert.equal(entry.protocol, 'openai-completions')
  assert.equal(entry.name, 'gpt-4o')
  assert.ok(!JSON.stringify(entry).includes('sk-key'), 'the api key is not echoed in the name view')
  const saved = listProxies(home)
  assert.equal(saved.length, 1)
  assert.equal(saved[0].baseUrl, 'https://example.com/v1')
  deleteProxy(entry.id, home)
  assert.equal(listProxies(home).length, 0)
  // A missing file yields [] rather than throwing.
  assert.deepEqual(listProxies(tempHome()), [])
})

test('setProviderApiKey routes the provider ref through the credentials seam', async () => {
  const calls = []
  const ctx = {
    get: (name) => name === 'credentials'
      ? { set: async (ref, v) => { calls.push(['set', ref, v]) }, unset: async (ref) => { calls.push(['unset', ref]) } }
      : name === 'llm'
        ? { listConfigurableProviders: () => [{ provider: 'deepseek', settingsNs: 'llm-deepseek' }] }
        : name === 'settings'
          ? { get: () => ({ apiKeyEnv: 'DEEPSEEK_API_KEY' }) }
          : undefined,
  }
  await setProviderApiKey(ctx, 'deepseek', 'sk-test')
  await setProviderApiKey(ctx, 'deepseek', '')
  assert.deepEqual(calls, [['set', 'DEEPSEEK_API_KEY', 'sk-test'], ['unset', 'DEEPSEEK_API_KEY']])
  await assert.rejects(
    () => setProviderApiKey({ get: () => undefined }, 'deepseek', 'x'),
    /credentials service unavailable/,
  )
})

test('sendLogin emits providers/proxies, never the secret', async () => {
  const frames = []
  const send = (ws, frame) => frames.push(frame)
  const ctx = {
    get: (name) => name === 'credentials'
      ? { describe: async () => ({ configured: true, writable: true, source: 'file' }), resolve: async () => ({ value: 'sk-super-secret-1234' }) }
      : name === 'llm'
        ? { listProviders: () => [{ id: 'deepseek', name: 'DeepSeek' }], listConfigurableProviders: () => [{ provider: 'deepseek', settingsNs: 'llm-deepseek' }] }
        : name === 'settings'
          ? { get: () => ({ apiKeyEnv: 'DEEPSEEK_API_KEY' }) }
          : undefined,
  }
  const home = tempHome()
  createProxy({ baseUrl: 'https://x/v1', apiKey: '', protocol: 'openai-completions', model: 'm' }, home)
  await sendLogin(ctx, send, {}, undefined, home)
  assert.equal(frames.length, 1)
  const frame = frames[0]
  assert.equal(frame.type, 'login')
  assert.equal(frame.providers[0].id, 'deepseek')
  assert.equal(frame.providers[0].apiKeyHint, '…1234')
  assert.equal(frame.proxies.length, 1)
  assert.equal(frame.codex, undefined)
  assert.ok(!JSON.stringify(frame).includes('sk-super-secret'), 'secret never on the wire')
})

test('sendLogin carries the rejected-write error', async () => {
  const frames = []
  const ctx = {
    get: (name) => name === 'credentials'
      ? { describe: async () => { throw new Error('bad doc') } }
      : name === 'llm'
        ? { listProviders: () => [{ id: 'deepseek', name: 'DeepSeek' }], listConfigurableProviders: () => [] }
        : undefined,
  }
  await sendLogin(ctx, (ws, frame) => frames.push(frame), {}, '写失败', tempHome())
  assert.equal(frames[0].error, '写失败')
  // The provider that failed its key read still appears, unconfigured.
  assert.equal(frames[0].providers[0].apiKeyConfigured, false)
})
