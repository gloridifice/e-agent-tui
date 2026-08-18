/**
 * test-new-swap.mjs — deployed bridge session/model routing smoke.
 *
 * Runs the DEPLOYED bridge through a real local WebSocket transport, while a
 * deterministic mock supplies the DSH service boundary. It verifies that the
 * public model-selection adapter is installed from both /new and cold resume,
 * and that /model changes the *next* assembled request without breaking the
 * same-socket session swap.
 *
 * Usage: node tools/test-new-swap.mjs
 * Optional: DSH_TUI_SMOKE_PROFILE=dshe node tools/test-new-swap.mjs
 */
import { createServer } from 'node:http'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'

const home = process.env.DSH_HOME ?? join(process.env.USERPROFILE ?? '.', '.dsh')
const profile = process.env.DSH_TUI_SMOKE_PROFILE ?? 'web'
const bridgeEntry = join(home, 'profiles', profile, 'packages', 'dsh-tui-bridge', 'src', 'index.js')
const { apply } = await import(pathToFileURL(bridgeEntry).href)
const token = readFileSync(join(home, 'dsh-tui.token'), 'utf8').trim()

function modelContext() {
  const listeners = new Map()
  return {
    listeners,
    on(event, listener) {
      listeners.set(event, listener)
      return () => listeners.delete(event)
    },
  }
}

const cwd = 'C:\\test'
const agentA = {
  id: 'session-test-a',
  status: 'idle',
  options: { provider: 'mock', model: 'mock-1' },
  session: { events: [], header: { cwd } },
  followup() {},
  cancel() {},
}
const agentB = {
  id: 'session-test-b',
  status: 'idle',
  options: { provider: 'mock', model: 'mock-1' },
  session: { events: [], header: { cwd } },
  followup() {},
  cancel() {},
}
const agentC = {
  id: 'session-test-resumed',
  status: 'idle',
  options: {},
  session: { events: [], header: { cwd } },
  followup() {},
  cancel() {},
}
const createdContext = modelContext()
const resumedContext = modelContext()
const agents = {
  roots: () => [agentA],
  list: () => [agentA],
  // The resumed id deliberately is not live; attach must exercise persistence.
  get: (id) => (id === agentA.id ? agentA : id === agentB.id ? agentB : undefined),
  create: async (options) => {
    await options.setup(createdContext)
    return { agent: agentB }
  },
  resume: async (options) => {
    await options.setup(resumedContext)
    return { agent: agentC }
  },
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
    if (name === 'sessionPersistence') {
      return {
        list: async () => [{ id: agentC.id }],
        inspect: async () => ({ meta: { agentPreset: 'standard' }, events: [] }),
      }
    }
    if (name === 'agentDefaultModel') {
      return { currentSelection: () => ({ provider: 'resume', model: 'resume-1' }) }
    }
    return undefined
  },
  on() { return () => {} },
  effect(fn) { fn(); return () => {} },
}
apply(ctx, {})
if (!route) throw new Error('bridge did not register the upgrade route')

// ---- real local transport: http upgrade -> deployed bridge handler ----
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
ws.send(JSON.stringify({ type: 'hello', token, resumeSessionId: agentA.id }))

const send = (obj) => ws.send(JSON.stringify(obj))
const waitFor = (pred, timeoutMs = 5000) => new Promise((resolve, reject) => {
  const t0 = Date.now()
  const timer = setInterval(() => {
    const hit = received.find(pred)
    if (hit) { clearInterval(timer); resolve(hit) }
    else if (Date.now() - t0 > timeoutMs) {
      clearInterval(timer)
      reject(new Error(`timeout; log: ${received.join(' | ')}`))
    }
  }, 20)
})
ws.addEventListener('message', (event) => {
  const msg = JSON.parse(event.data)
  received.push(msg.type === 'welcome' ? `WELCOME ${msg.sessionId}` : msg.type === 'error' ? `ERROR ${msg.code}` : msg.type)
})

// Initial live attach.
await waitFor((m) => m === 'WELCOME session-test-a')
received.length = 0
send({ type: 'ping' })
await waitFor((m) => m === 'pong')

// /new must retain the socket and execute the official adapter in setup.
received.length = 0
send({ type: 'command', line: '/new' })
await waitFor((m) => m === 'WELCOME session-test-b')
await waitFor((m) => m === 'snapshot')
if (closed) throw new Error('socket closed during /new')
if (!createdContext.listeners.has('system-prompt/assemble') || !createdContext.listeners.has('agent/request')) {
  throw new Error('/new did not install model-selection listeners')
}
console.log('PASS /new: same socket re-attached and installed model-selection adapter')
// `welcome`/`snapshot` are emitted synchronously by attach; let the dispatcher's
// promise continuation publish its new connection before sending a second
// client command (the real client receives those frames on a later event turn).
await new Promise((resolve) => setTimeout(resolve, 0))
received.length = 0
send({ type: 'ping' })
await waitFor((m) => m === 'pong')

// /model changes the current selection; DSH snapshots it at the following
// system-prompt assembly, so route the request through the deployed helper.
received.length = 0
send({ type: 'model-set', provider: 'after', model: 'new' })
await waitFor((m) => m === 'model')
const newAssembly = await createdContext.listeners.get('system-prompt/assemble')({}, {}, async () => ({ variables: {} }))
const newRequest = await createdContext.listeners.get('agent/request')({}, async () => ({
  stream: true,
  reasoningEffort: 'inherited',
}))
if (newAssembly.variables.provider !== 'after' || newAssembly.variables.model !== 'new'
  || newRequest.provider !== 'after' || newRequest.model !== 'new'
  || Object.hasOwn(newRequest, 'reasoningEffort')) {
  throw new Error('/model did not route the next assembled request through the selected model')
}
console.log('PASS /model: next assembly/request uses the new provider/model')

// A non-live id must cold-resume through persistence and install the same
// adapter before it is attached to the existing socket.
received.length = 0
send({ type: 'attach', sessionId: agentC.id })
await waitFor((m) => m === `WELCOME ${agentC.id}`)
await waitFor((m) => m === 'snapshot')
if (closed) throw new Error('socket closed during cold resume')
if (!resumedContext.listeners.has('system-prompt/assemble') || !resumedContext.listeners.has('agent/request')) {
  throw new Error('cold resume did not install model-selection listeners')
}
const resumedAssembly = await resumedContext.listeners.get('system-prompt/assemble')({}, {}, async () => ({ variables: {} }))
const resumedRequest = await resumedContext.listeners.get('agent/request')({}, async () => ({ stream: true }))
if (resumedAssembly.variables.provider !== 'resume' || resumedRequest.model !== 'resume-1') {
  throw new Error('cold resume did not route through the default model selection')
}
console.log('PASS resume: persisted attach installs and routes default model selection')

ws.close()
server.close()
console.log('ALL PASS')
