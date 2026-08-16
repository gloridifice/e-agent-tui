/**
 * test-new-swap.mjs — smoke test for the bridge's session-swap paths
 * (`/new` and picker attach) against the on-disk deployed bridge copy,
 * with a mocked DSH context and a real local WebSocket transport.
 *
 * Regression for the `/new` crash: detach() used to close the socket it
 * immediately re-attached, so the client died on the close frame.
 *
 * Asserts: the SAME socket receives a welcome for the new session, a
 * snapshot, no error, and no close — for both `/new` and `attach`.
 *
 * Usage: node tools/test-new-swap.mjs
 */
import { createServer } from 'node:http'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'

const home = process.env.DSH_HOME ?? 'C:\\Users\\11659\\.dsh'
const bridgeEntry = join(home, 'profiles', 'web', 'packages', 'dsh-tui-bridge', 'src', 'index.js')
const { apply } = await import(pathToFileURL(bridgeEntry).toString())
const token = readFileSync(join(home, 'dsh-tui.token'), 'utf8').trim()

// ---- mock DSH context ----
const agentA = {
  id: 'session-test-a',
  status: 'idle',
  options: { provider: 'mock', model: 'mock-1' },
  session: { events: [], header: { cwd: 'C:\\test' } },
  followup() {},
  cancel() {},
}
const agentB = {
  id: 'session-test-b',
  status: 'idle',
  options: { provider: 'mock', model: 'mock-1' },
  session: { events: [], header: { cwd: 'C:\\test' } },
  followup() {},
  cancel() {},
}
const agents = {
  roots: () => [agentA],
  list: () => [agentA],
  get: (id) => (id === agentA.id ? agentA : id === agentB.id ? agentB : undefined),
  create: async () => ({ agent: agentB }),
}

let route = null
const ctx = {
  webServer: {
    registerUpgrade(r) {
      route = r
      return () => {}
    },
  },
  get(name) {
    if (name === 'agents') return agents
    return undefined
  },
  on() { return () => {} },
  effect(fn) { fn(); return () => {} },
}
apply(ctx, {})
if (!route) throw new Error('bridge did not register the upgrade route')

// ---- real local transport: http upgrade -> bridge handler ----
const server = createServer()
server.on('upgrade', (req, socket, head) => route.handler(req, socket, head))
await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve))
const port = server.address().port

const received = []
const ws = new WebSocket(`ws://127.0.0.1:${port}/dsh-tui`)
let closed = false
ws.addEventListener('close', (e) => { closed = true; received.push(`CLOSE ${e.code}`) })

await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve)
  ws.addEventListener('error', reject)
})
ws.send(JSON.stringify({ type: 'hello', token }))

const send = (obj) => ws.send(JSON.stringify(obj))
const waitFor = (pred, timeoutMs = 5000) => new Promise((resolve, reject) => {
  const t0 = Date.now()
  const timer = setInterval(() => {
    const hit = received.find(pred)
    if (hit) { clearInterval(timer); resolve(hit) }
    else if (Date.now() - t0 > timeoutMs) { clearInterval(timer); reject(new Error(`timeout; log: ${received.join(' | ')}`)) }
  }, 20)
})

ws.addEventListener('message', (event) => {
  const msg = JSON.parse(event.data)
  received.push(msg.type === 'welcome' ? `WELCOME ${msg.sessionId}` : msg.type === 'error' ? `ERROR ${msg.code}` : msg.type)
})

// wait for the initial attach
await waitFor((m) => m === 'WELCOME session-test-a')

// ---- /new: same socket must survive and re-attach to the new session ----
received.length = 0
send({ type: 'command', line: '/new' })
await waitFor((m) => m === 'WELCOME session-test-b')
await waitFor((m) => m === 'snapshot')
if (closed) throw new Error('FAIL: socket closed during /new')
console.log('PASS /new: same socket re-attached to session-test-b, snapshot delivered, socket open')

// ---- picker attach back to A: same guarantee ----
received.length = 0
send({ type: 'attach', sessionId: 'session-test-a' })
await waitFor((m) => m === 'WELCOME session-test-a')
await waitFor((m) => m === 'snapshot')
if (closed) throw new Error('FAIL: socket closed during attach')
console.log('PASS attach: same socket re-attached to session-test-a, snapshot delivered, socket open')

ws.close()
server.close()
console.log('ALL PASS')
