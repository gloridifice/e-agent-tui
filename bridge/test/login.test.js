// /login field-layer contracts (node --test test/login.test.js).
// File helpers run against temp homes; the credentials seam is faked.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { existsSync, mkdtempSync, readFileSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { join } from 'node:path'
import {
  ACCOUNT_UUID_PATTERN,
  PROXY_ENV_NAME,
  readAccount,
  readEnvLine,
  sendLogin,
  setLoginField,
  writeAccount,
  writeEnvLine,
} from '../src/login.js'

const UUID = 'a1b2c3d4-0000-0000-0000-000000000000'

function tempHome() {
  return mkdtempSync(join(tmpdir(), 'dsh-tui-login-'))
}

test('writeEnvLine adds/replaces/removes only its own line', () => {
  const home = tempHome()
  writeFileSync(join(home, '.env'), 'A=1\nB=2\n', 'utf8')
  writeEnvLine(PROXY_ENV_NAME, 'http://p:1', home)
  let text = readFileSync(join(home, '.env'), 'utf8')
  assert.equal(text, 'A=1\nB=2\nHTTPS_PROXY=http://p:1\n')
  assert.equal(readEnvLine(PROXY_ENV_NAME, home), 'http://p:1')

  writeEnvLine(PROXY_ENV_NAME, 'http://p:2', home)
  text = readFileSync(join(home, '.env'), 'utf8')
  assert.equal(text, 'A=1\nB=2\nHTTPS_PROXY=http://p:2\n')

  writeEnvLine(PROXY_ENV_NAME, undefined, home)
  text = readFileSync(join(home, '.env'), 'utf8')
  assert.equal(text, 'A=1\nB=2\n')
  assert.equal(readEnvLine(PROXY_ENV_NAME, home), undefined)
})

test('writeEnvLine preserves CRLF and deletes an emptied file', () => {
  const home = tempHome()
  writeFileSync(join(home, '.env'), 'A=1\r\n', 'utf8')
  writeEnvLine(PROXY_ENV_NAME, 'x', home)
  assert.equal(readFileSync(join(home, '.env'), 'utf8'), 'A=1\r\nHTTPS_PROXY=x\r\n')
  // Removing the only line of a proxy-only file deletes the file itself.
  const lone = tempHome()
  writeEnvLine(PROXY_ENV_NAME, 'x', lone)
  writeEnvLine(PROXY_ENV_NAME, undefined, lone)
  assert.equal(existsSync(join(lone, '.env')), false)
})

test('account roundtrip: UUID validated, blank deletes', () => {
  const home = tempHome()
  writeAccount(UUID, home)
  assert.equal(readAccount(home), UUID)
  writeAccount('', home)
  assert.equal(existsSync(join(home, '.anonymous-user-id')), false)
  assert.equal(readAccount(home), undefined)
  assert.throws(() => writeAccount('not-a-uuid', home), /UUID/)
  assert.ok(ACCOUNT_UUID_PATTERN.test(UUID))
})

test('setLoginField routes apiKey through the credentials seam', async () => {
  const calls = []
  const ctx = {
    get: (name) => name === 'credentials' ? {
      set: async (ref, value) => { calls.push(['set', ref, value]) },
      unset: async (ref) => { calls.push(['unset', ref]) },
    } : undefined,
  }
  const home = tempHome()
  await setLoginField(ctx, 'apiKey', 'sk-test', home)
  await setLoginField(ctx, 'apiKey', '', home)
  assert.deepEqual(calls, [['set', 'DEEPSEEK_API_KEY', 'sk-test'], ['unset', 'DEEPSEEK_API_KEY']])
  // A host without the credentials seam refuses the key write.
  const bareCtx = { get: () => undefined }
  await assert.rejects(
    () => setLoginField(bareCtx, 'apiKey', 'x', home),
    /credentials service unavailable/,
  )
  await setLoginField(ctx, 'account', UUID, home)
  await setLoginField(ctx, 'proxy', 'http://p:1', home)
  assert.equal(readAccount(home), UUID)
  assert.equal(readEnvLine(PROXY_ENV_NAME, home), 'http://p:1')
})

test('sendLogin emits a view, never the secret', async () => {
  const frames = []
  const send = (ws, frame) => frames.push(frame)
  const ctx = {
    get: () => ({
      describe: async () => ({ configured: true, writable: true, source: 'file' }),
      resolve: async () => ({ value: 'sk-super-secret-1234' }),
    }),
  }
  const home = tempHome()
  writeAccount(UUID, home)
  writeEnvLine(PROXY_ENV_NAME, 'http://p:1', home)
  await sendLogin(ctx, send, {}, undefined, home)
  assert.equal(frames.length, 1)
  const frame = frames[0]
  assert.equal(frame.type, 'login')
  assert.equal(frame.apiKeyConfigured, true)
  assert.equal(frame.apiKeyHint, '…1234')
  assert.equal(frame.account, UUID)
  assert.equal(frame.proxy, 'http://p:1')
  assert.ok(!JSON.stringify(frame).includes('sk-super-secret'), 'secret never on the wire')
})

test('sendLogin carries the rejected-write error', async () => {
  const frames = []
  const ctx = { get: () => ({ describe: async () => { throw new Error('bad doc') } }) }
  await sendLogin(ctx, (ws, frame) => frames.push(frame), {}, '写失败', tempHome())
  assert.equal(frames[0].error, '写失败')
})
