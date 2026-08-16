/**
 * probe-startup.mjs — measure the TUI attach/startup path over the bridge:
 * hello → welcome → snapshot latency, plus the snapshot frame size.
 * This covers the network + bridge half of TUI startup (the client half is
 * measured by client/examples/timing_snapshot.rs).
 *
 * Usage: node probe-startup.mjs [ws://127.0.0.1:3080/dsh-tui]
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { dirname } from 'node:path'

const here = dirname(fileURLToPath(import.meta.url))
const home = process.env.DSH_HOME ?? join(process.env.HOME ?? process.env.USERPROFILE ?? '.', '.dsh')
const token = readFileSync(join(home, 'dsh-tui.token'), 'utf8').trim()
const url = process.argv[2] ?? 'ws://127.0.0.1:3080/dsh-tui'

const t0 = performance.now()
let helloAt = null
let welcomeAt = null
let snapshotAt = null
let snapshotBytes = 0
let snapshotEvents = 0
let truncated = false

const ws = new WebSocket(url)

ws.addEventListener('open', () => {
  helloAt = performance.now()
  ws.send(JSON.stringify({ type: 'hello', token }))
})

ws.addEventListener('message', (event) => {
  const msg = JSON.parse(event.data)
  if (msg.type === 'welcome') {
    welcomeAt = performance.now()
    console.log(`  welcome:    +${(welcomeAt - helloAt).toFixed(1)} ms (session ${msg.sessionId}, status=${msg.status})`)
  } else if (msg.type === 'snapshot') {
    snapshotAt = performance.now()
    snapshotBytes = Buffer.byteLength(event.data)
    snapshotEvents = msg.events.length
    truncated = !!msg.truncated
    console.log(`  snapshot:   +${(snapshotAt - welcomeAt).toFixed(1)} ms after welcome`)
    console.log(`  frame:      ${snapshotBytes} bytes, ${snapshotEvents} events, truncated=${truncated}`)
    console.log(`  TOTAL attach: ${(snapshotAt - t0).toFixed(1)} ms`)
    ws.close()
    process.exit(0)
  } else if (msg.type === 'error') {
    console.log(`bridge error: ${msg.code} ${msg.message}`)
    process.exit(1)
  }
})

ws.addEventListener('error', (e) => {
  console.log(`ws error: ${e.message ?? 'handshake failed'}`)
  process.exit(1)
})

setTimeout(() => { console.log('timeout waiting for snapshot'); process.exit(1) }, 60000)
