// /login field layer (D33) — the three login fields and where they live:
// API key through the host credentials seam (hot reload), account and proxy
// in harness-home files read at the next host launch. The file helpers take
// an explicit `home` so tests run against temp dirs (test/login.test.js);
// production callers rely on the dshHome() default.
import { randomUUID } from 'node:crypto'
import { readFileSync, unlinkSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { dshHome } from './compose.js'

// API key: the deepseek provider's credential ref, stored through the host's
// credentials service (same document the web Models page writes) — the value
// never crosses the wire, only its configured/source/hint view.
export const DEEPSEEK_API_KEY_REF = 'DEEPSEEK_API_KEY'
// 账号: the harness's anonymous user id (sent as x-deepseek-harness-user-id),
// a bare UUID line in <harness home>/.anonymous-user-id. Blank = delete →
// the next launch mints a fresh id.
export const ACCOUNT_UUID_PATTERN = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i
// proxy: the HTTPS_PROXY line of <harness home>/.env — the harness loads
// that file into its launch environment at boot, so edits apply after the
// next dsh web restart.
export const PROXY_ENV_NAME = 'HTTPS_PROXY'

export function accountFile(home = dshHome()) { return join(home, '.anonymous-user-id') }
export function envFile(home = dshHome()) { return join(home, '.env') }

/** The persisted harness account id, or undefined (auto-generated). */
export function readAccount(home = dshHome()) {
  try {
    const text = readFileSync(accountFile(home), 'utf8').trim()
    return ACCOUNT_UUID_PATTERN.test(text) ? text : undefined
  } catch { return undefined }
}

/** Value of one KEY= line in the harness-home .env, or undefined. */
export function readEnvLine(name, home = dshHome()) {
  try {
    const text = readFileSync(envFile(home), 'utf8')
    for (const line of text.split(/\r?\n/)) {
      const eq = line.indexOf('=')
      if (eq !== -1 && line.slice(0, eq).trim() === name) return line.slice(eq + 1)
    }
    return undefined
  } catch { return undefined }
}

/**
 * Replace (or remove, when `value` is undefined/empty) one KEY line in the
 * harness-home .env, leaving every other line byte-identical. The file's own
 * line-ending style is preserved.
 */
export function writeEnvLine(name, value, home = dshHome()) {
  const file = envFile(home)
  let text = ''
  try { text = readFileSync(file, 'utf8') } catch {}
  const eol = text.includes('\r\n') ? '\r\n' : '\n'
  const lines = text.split(/\r?\n/)
  if (lines.length > 0 && lines[lines.length - 1] === '') lines.pop()
  const kept = lines.filter((line) => {
    const eq = line.indexOf('=')
    return eq === -1 || line.slice(0, eq).trim() !== name
  })
  if (typeof value === 'string' && value !== '') kept.push(`${name}=${value}`)
  if (kept.length === 0) {
    try { unlinkSync(file) } catch {}
    return
  }
  writeFileSync(file, `${kept.join(eol)}${eol}`, 'utf8')
}

/** Persist the harness account id; an empty value deletes it (fresh id next launch). */
export function writeAccount(value, home = dshHome()) {
  if (value === '') {
    try { unlinkSync(accountFile(home)) } catch {}
    return
  }
  if (!ACCOUNT_UUID_PATTERN.test(value)) {
    throw new Error(`账号必须是 UUID 格式（如 ${randomUUID()}），留空则自动生成`)
  }
  writeFileSync(accountFile(home), `${value.trim()}\n`, 'utf8')
}

/**
 * Read/write one login field through the host's own seams: the API key goes
 * through the credentials service (hot reload), the account and proxy are
 * harness-home files read at the next host launch.
 */
export async function setLoginField(ctx, field, value, home = dshHome()) {
  if (field === 'apiKey') {
    const credentials = ctx.get('credentials')
    if (!credentials) throw new Error('credentials service unavailable')
    if (value === '') await credentials.unset(DEEPSEEK_API_KEY_REF)
    else await credentials.set(DEEPSEEK_API_KEY_REF, value)
  } else if (field === 'account') {
    writeAccount(value, home)
  } else if (field === 'proxy') {
    writeEnvLine(PROXY_ENV_NAME, value === '' ? undefined : value, home)
  } else {
    throw new Error(`unknown login field "${field}"`)
  }
}

/**
 * Push the login page state; `error` is the message of a rejected write.
 * The API key VALUE never leaves the host — only its view (configured /
 * writable / source / last-4 hint).
 */
export async function sendLogin(ctx, send, ws, error, home = dshHome()) {
  let apiKey = { configured: false, writable: false, source: undefined, hint: undefined }
  try {
    const credentials = ctx.get('credentials')
    if (credentials) {
      const view = await credentials.describe(DEEPSEEK_API_KEY_REF)
      apiKey.configured = view?.configured === true
      apiKey.writable = view?.writable === true
      apiKey.source = view?.source
      if (apiKey.configured) {
        const hit = await credentials.resolve(DEEPSEEK_API_KEY_REF)
        const value = hit?.value
        if (typeof value === 'string' && value.length > 4) apiKey.hint = `…${value.slice(-4)}`
      }
    }
  } catch (loginError) {
    error = error ?? String(loginError?.message ?? loginError)
  }
  // Read each file exactly once — the frame must be self-consistent.
  const account = readAccount(home)
  const proxy = readEnvLine(PROXY_ENV_NAME, home)
  send(ws, {
    type: 'login',
    apiKeyConfigured: apiKey.configured,
    apiKeyWritable: apiKey.writable,
    ...(apiKey.source !== undefined ? { apiKeySource: apiKey.source } : {}),
    ...(apiKey.hint !== undefined ? { apiKeyHint: apiKey.hint } : {}),
    ...(account !== undefined ? { account } : {}),
    ...(proxy !== undefined ? { proxy } : {}),
    ...(error !== undefined ? { error } : {}),
  })
}
