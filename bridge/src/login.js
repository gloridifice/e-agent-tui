// /login layer: model-provider API keys (host credentials seam), OpenAI
// Codex (ChatGPT subscription) device-code login, and custom proxy routes.
// Pure helpers take an explicit `home` so tests run against temp dirs
// (test/login.test.js); production callers rely on the dshHome() default.
import { randomUUID } from 'node:crypto'
import { readFileSync, unlinkSync, writeFileSync } from 'node:fs'
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

// --- OpenAI Codex (ChatGPT) device-code login ----------------------------

export function codexFile(home = dshHome()) { return join(home, 'dsh-tui-codex.json') }

export function readCodex(home = dshHome()) {
  try {
    const parsed = JSON.parse(readFileSync(codexFile(home), 'utf8'))
    if (parsed && typeof parsed.accountId === 'string' && typeof parsed.access === 'string') {
      return { loggedIn: true, accountId: parsed.accountId }
    }
    return { loggedIn: false }
  } catch { return { loggedIn: false } }
}

function writeCodex(home, cred) {
  writeFileSync(codexFile(home), `${JSON.stringify(cred, null, 2)}\n`, 'utf8')
}

// pi-ai's openai-codex OAuth constants (see @earendil-works/pi-ai
// dist/auth/oauth/openai-codex.js).
const CODEX_CLIENT_ID = 'app_EMoamEEZ73f0CkXaXp7hrann'
const CODEX_USER_CODE_URL = 'https://auth.openai.com/api/accounts/deviceauth/usercode'
const CODEX_TOKEN_URL = 'https://auth.openai.com/api/accounts/deviceauth/token'
const CODEX_OAUTH_TOKEN_URL = 'https://auth.openai.com/oauth/token'
export const CODEX_VERIFICATION_URI = 'https://auth.openai.com/codex/device'
const CODEX_REDIRECT_URI = 'https://auth.openai.com/deviceauth/callback'
const CODEX_TIMEOUT_SECONDS = 15 * 60

function decodeJwtAccountId(token) {
  try {
    const payload = token.split('.')[1] ?? ''
    const json = JSON.parse(Buffer.from(payload, 'base64url').toString('utf8'))
    const id = json?.['https://api.openai.com/auth']?.chatgpt_account_id
    return typeof id === 'string' && id !== '' ? id : null
  } catch { return null }
}

async function startCodexDeviceAuth(signal) {
  const res = await fetch(CODEX_USER_CODE_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ client_id: CODEX_CLIENT_ID }),
    signal,
  })
  if (!res.ok) throw new Error(`Codex device code request failed (${res.status})`)
  const json = await res.json()
  const intervalSeconds = typeof json?.interval === 'string' ? Number(json.interval.trim()) : json?.interval
  if (!json?.device_auth_id || !json.user_code || !Number.isFinite(intervalSeconds) || intervalSeconds < 0) {
    throw new Error(`Invalid Codex device code response: ${JSON.stringify(json)}`)
  }
  return { deviceAuthId: json.device_auth_id, userCode: json.user_code, intervalSeconds }
}

async function pollCodexDeviceAuth(device, signal) {
  const deadline = Date.now() + CODEX_TIMEOUT_SECONDS * 1000
  let intervalMs = Math.max(1000, Math.floor(device.intervalSeconds * 1000))
  while (Date.now() < deadline) {
    if (signal?.aborted) throw new Error('Login cancelled')
    const res = await fetch(CODEX_TOKEN_URL, {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ device_auth_id: device.deviceAuthId, user_code: device.userCode }),
      signal,
    })
    if (res.ok) {
      const json = await res.json()
      if (!json?.authorization_code || !json.code_verifier) throw new Error(`Invalid Codex device auth token response: ${JSON.stringify(json)}`)
      return { authorizationCode: json.authorization_code, codeVerifier: json.code_verifier }
    }
    if (res.status !== 403 && res.status !== 404) {
      const body = await res.text().catch(() => '')
      let code
      try { code = JSON.parse(body)?.error?.code } catch {}
      if (code === 'slow_down') intervalMs += 5000
      else if (code !== 'deviceauth_authorization_pending') throw new Error(`Codex device auth failed (${res.status}): ${body}`)
    }
    const remaining = deadline - Date.now()
    if (remaining <= 0) break
    await new Promise((resolve, reject) => {
      const t = setTimeout(resolve, Math.min(intervalMs, remaining))
      signal?.addEventListener('abort', () => { clearTimeout(t); reject(new Error('Login cancelled')) }, { once: true })
    })
  }
  throw new Error('Device flow timed out')
}

async function exchangeCodexCode(authorizationCode, codeVerifier, signal) {
  const res = await fetch(CODEX_OAUTH_TOKEN_URL, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      grant_type: 'authorization_code',
      client_id: CODEX_CLIENT_ID,
      code: authorizationCode,
      code_verifier: codeVerifier,
      redirect_uri: CODEX_REDIRECT_URI,
    }),
    signal,
  })
  if (!res.ok) throw new Error(`Codex token exchange failed (${res.status})`)
  const json = await res.json()
  if (!json?.access_token || !json.refresh_token || typeof json.expires_in !== 'number') {
    throw new Error(`Codex token response missing fields: ${JSON.stringify(json)}`)
  }
  const accountId = decodeJwtAccountId(json.access_token)
  if (!accountId) throw new Error('Failed to extract accountId from token')
  return {
    type: 'oauth',
    access: json.access_token,
    refresh: json.refresh_token,
    expires: Date.now() + json.expires_in * 1000,
    accountId,
  }
}

/** Run the whole device flow: start → notify → poll → exchange → store. */
export async function runCodexLogin(send, ws, signal, home = dshHome()) {
  const device = await startCodexDeviceAuth(signal)
  send(ws, {
    type: 'login-codex',
    status: 'pending',
    userCode: device.userCode,
    verificationUri: CODEX_VERIFICATION_URI,
  })
  const code = await pollCodexDeviceAuth(device, signal)
  const cred = await exchangeCodexCode(code.authorizationCode, code.codeVerifier, signal)
  writeCodex(home, cred)
  send(ws, { type: 'login-codex', status: 'done', accountId: cred.accountId })
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
  const codex = readCodex(home)
  send(ws, {
    type: 'login',
    providers,
    proxies,
    ...(codex?.loggedIn ? { codex } : {}),
    ...(error !== undefined ? { error } : {}),
  })
}
