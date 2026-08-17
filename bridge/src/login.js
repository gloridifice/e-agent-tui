// /login layer: model-provider API keys (host credentials seam) and custom
// proxy routes. Pure helpers take an explicit `home` so tests run against
// temp dirs (test/login.test.js); production callers rely on the dshHome()
// default.
import { randomUUID } from 'node:crypto'
import { readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { dshHome } from './compose.js'

// --- provider API keys ---------------------------------------------------

/**
 * The credential ref (env-var name) one provider route resolves its API key
 * through. Read from the provider's registered settings section (`apiKeyEnv`,
 * either top-level or under `providers.<id>`), falling back to a conventional
 * `<ID>_API_KEY` name.
 */
export function providerCredentialRef(ctx, providerId) {
  const llm = ctx?.get?.('llm')
  const settings = ctx?.get?.('settings')
  if (llm && settings) {
    for (const entry of llm.listConfigurableProviders?.() ?? []) {
      if (entry.provider !== providerId) continue
      const value = settings.get?.(entry.settingsNs)
      if (value && typeof value === 'object') {
        if (typeof value.apiKeyEnv === 'string' && value.apiKeyEnv !== '') return value.apiKeyEnv
        const nested = value.providers?.[providerId]
        if (nested && typeof nested.apiKeyEnv === 'string' && nested.apiKeyEnv !== '') return nested.apiKeyEnv
      }
    }
  }
  return `${providerId.toUpperCase().replace(/[^A-Z0-9]/g, '_')}_API_KEY`
}

/** Provider routes with their display names, in registration order. */
export function listProviders(ctx) {
  const llm = ctx?.get?.('llm')
  return (llm?.listProviders?.() ?? []).map((p) => ({
    id: p.id,
    name: p.name ?? p.id,
    ref: providerCredentialRef(ctx, p.id),
  }))
}

/** The configured/source/hint VIEW of one credential ref (value never read). */
export async function describeApiKey(ctx, ref) {
  const credentials = ctx.get('credentials')
  if (!credentials) return { configured: false, writable: false, source: undefined, hint: undefined }
  const view = await credentials.describe(ref)
  const configured = view?.configured === true
  const writable = view?.writable === true
  let hint
  if (configured) {
    const hit = await credentials.resolve(ref)
    const value = hit?.value
    if (typeof value === 'string' && value.length > 4) hint = `…${value.slice(-4)}`
  }
  return { configured, writable, source: view?.source, hint }
}

/** Store (or clear, when blank) one provider's API key through the seam. */
export async function setProviderApiKey(ctx, provider, value) {
  const ref = providerCredentialRef(ctx, provider)
  const credentials = ctx.get('credentials')
  if (!credentials) throw new Error('credentials service unavailable')
  if (value === '') await credentials.unset(ref)
  else await credentials.set(ref, value)
}

// --- proxy routes --------------------------------------------------------

export function proxyFile(home = dshHome()) { return join(home, 'dsh-tui-proxies.json') }

/** Full proxy entries (with apiKey) — internal mutation only. */
function readRawProxies(home = dshHome()) {
  try {
    const parsed = JSON.parse(readFileSync(proxyFile(home), 'utf8'))
    return Array.isArray(parsed) ? parsed.filter((p) => p && typeof p.id === 'string') : []
  } catch { return [] }
}

/** Saved proxy routes for the wire view: the apiKey is stripped. */
export function listProxies(home = dshHome()) {
  return readRawProxies(home).map(({ apiKey: _apiKey, ...rest }) => rest)
}

/** Append one proxy route; returns the saved entry (apiKey stripped). */
export function createProxy({ baseUrl, apiKey, protocol, model }, home = dshHome()) {
  const id = `proxy-${randomUUID().slice(0, 8)}`
  let name = model?.trim()
  if (!name) {
    try { name = new URL(baseUrl).host } catch { name = baseUrl.trim() }
  }
  const entry = {
    id,
    name,
    baseUrl: baseUrl.trim(),
    protocol: protocol || 'openai-completions',
    model: model.trim(),
    ...(apiKey !== '' ? { apiKey } : {}),
  }
  const proxies = readRawProxies(home)
  proxies.push(entry)
  writeFileSync(proxyFile(home), `${JSON.stringify(proxies, null, 2)}\n`, 'utf8')
  const { apiKey: _apiKey, ...view } = entry
  return view
}

/** Remove one proxy route by id. */
export function deleteProxy(id, home = dshHome()) {
  const proxies = readRawProxies(home).filter((p) => p.id !== id)
  writeFileSync(proxyFile(home), `${JSON.stringify(proxies, null, 2)}\n`, 'utf8')
}

/** Push the login page state; `error` is the message of a rejected write. */
export async function sendLogin(ctx, send, ws, error, home = dshHome()) {
  const providers = []
  for (const p of listProviders(ctx)) {
    try {
      const key = await describeApiKey(ctx, p.ref)
      providers.push({
        id: p.id,
        name: p.name,
        apiKeyConfigured: key.configured,
        apiKeyWritable: key.writable,
        ...(key.source !== undefined ? { apiKeySource: key.source } : {}),
        ...(key.hint !== undefined ? { apiKeyHint: key.hint } : {}),
      })
    } catch (e) {
      error = error ?? String(e?.message ?? e)
      providers.push({ id: p.id, name: p.name, apiKeyConfigured: false, apiKeyWritable: false })
    }
  }
  const proxies = listProxies(home)
  send(ws, {
    type: 'login',
    providers,
    proxies,
    ...(error !== undefined ? { error } : {}),
  })
}
