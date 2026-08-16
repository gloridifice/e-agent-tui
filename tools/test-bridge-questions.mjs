/**
 * test-bridge-questions.mjs — smoke test for the bridge's user-question
 * relay: mux frames (question/requested, question/resolved) forwarded to the
 * attached TUI, and answer/cancel messages routed back through
 * apiProxy.respond with the right client-response envelopes.
 *
 * Uses the on-disk deployed bridge copy, a mocked DSH context (agents +
 * apiProxy) and a real local WebSocket transport.
 *
 * Usage: node tools/test-bridge-questions.mjs
 */
import { createServer } from 'node:http'
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { pathToFileURL } from 'node:url'

const home = process.env.DSH_HOME ?? 'C:\\Users\\11659\\.dsh'
// BRIDGE_ENTRY overrides the entry for testing an un-deployed copy (e.g.
// tools/.bridge-stage/index.js before the robocopy sync).
const bridgeEntry = process.env.BRIDGE_ENTRY
  ?? join(home, 'profiles', 'web', 'packages', 'dsh-tui-bridge', 'src', 'index.js')
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
const agents = {
  roots: () => [agentA],
  list: () => [agentA],
  get: (id) => (id === agentA.id ? agentA : undefined),
  create: async () => ({ agent: agentA }),
}

// Mock apiproxy mux: a frame queue the test pushes into; the bridge's
// for-await loop pulls from it.
const frameQueue = []
let wake = null
async function* muxStream() {
  while (true) {
    if (frameQueue.length === 0) {
      await new Promise((resolve) => { wake = resolve })
    }
    yield frameQueue.shift()
  }
}
const pushFrame = (frame) => {
  frameQueue.push(frame)
  if (wake) { const w = wake; wake = null; w() }
}

const responses = []
const apiProxy = {
  events: { mux: () => muxStream() },
  respond: async (message) => {
    responses.push(message)
    return { accepted: true }
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
    if (name === 'apiProxy') return apiProxy
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
await new Promise((resolve, reject) => {
  ws.addEventListener('open', resolve)
  ws.addEventListener('error', reject)
})
ws.send(JSON.stringify({ type: 'hello', token }))

const waitFor = (pred, timeoutMs = 5000) => new Promise((resolve, reject) => {
  const t0 = Date.now()
  const timer = setInterval(() => {
    const hit = received.find(pred)
    if (hit) { clearInterval(timer); resolve(hit) }
    else if (Date.now() - t0 > timeoutMs) { clearInterval(timer); reject(new Error(`timeout; log: ${JSON.stringify(received)}`)) }
  }, 20)
})

ws.addEventListener('message', (event) => {
  const msg = JSON.parse(event.data)
  received.push(msg)
})

// wait for the initial attach
await waitFor((m) => m.type === 'welcome')

// ---- question/requested relay ----
pushFrame({
  rpcId: 'r1',
  payload: {
    type: 'question/requested',
    sessionId: 'session-test-a',
    questions: [
      { id: 'q1', question: '选哪个?', options: [{ label: 'A' }, { label: 'B' }] },
      { id: 'q2', question: '文本?', header: 'Fill' },
    ],
  },
})
const question = await waitFor((m) => m.type === 'question')
if (question.rpcId !== 'r1' || question.questions.length !== 2) {
  throw new Error(`FAIL: bad question frame: ${JSON.stringify(question)}`)
}
console.log('PASS question/requested relayed with questions intact')

// ---- answer-questions -> apiProxy.respond ----
ws.send(JSON.stringify({
  type: 'answer-questions',
  rpcId: 'r1',
  answers: [{ id: 'q1', selected: ['A'] }, { id: 'q2', selected: [], custom: '手写' }],
}))
await waitFor(() => responses.length >= 1)
const resp = responses[0]
if (resp.type !== 'client-response' || resp.rpcId !== 'r1' || resp.result?.ok !== true) {
  throw new Error(`FAIL: bad respond envelope: ${JSON.stringify(resp)}`)
}
if (resp.result.value.sessionId !== 'session-test-a') {
  throw new Error(`FAIL: respond routed to wrong session: ${JSON.stringify(resp.result.value)}`)
}
if (resp.result.value.answer.answers.length !== 2 || resp.result.value.answer.answers[1].custom !== '手写') {
  throw new Error(`FAIL: answers corrupted: ${JSON.stringify(resp.result.value.answer)}`)
}
console.log('PASS answer-questions routed through apiProxy.respond')

// ---- cancel-questions ----
ws.send(JSON.stringify({ type: 'cancel-questions', rpcId: 'r1' }))
await waitFor(() => responses.length >= 2)
const cancel = responses[1]
if (cancel.result?.ok !== false || cancel.result?.error?.code !== 'cancelled') {
  throw new Error(`FAIL: bad cancel envelope: ${JSON.stringify(cancel)}`)
}
console.log('PASS cancel-questions sends a cancelled client-response')

// ---- question/resolved relay ----
pushFrame({
  rpcId: 'r2',
  payload: { type: 'question/resolved', sessionId: 'session-test-a', questionRpcId: 'r1', outcome: 'answered' },
})
const resolved = await waitFor((m) => m.type === 'question-resolved')
if (resolved.questionRpcId !== 'r1' || resolved.outcome !== 'answered') {
  throw new Error(`FAIL: bad resolved frame: ${JSON.stringify(resolved)}`)
}
console.log('PASS question/resolved relayed')

ws.close()
server.close()
console.log('ALL PASS')
