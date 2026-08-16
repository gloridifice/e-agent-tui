// repro-new.mjs — reproduce the `/new` bridge path: hello, then command /new,
// and log what comes back (expect the socket to close with the bug).
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

const url = process.argv[2] ?? 'ws://127.0.0.1:3080/dsh-tui'
const home = process.env.DSH_HOME
  ?? join(process.env.HOME ?? process.env.USERPROFILE ?? '.', '.dsh')
const token = readFileSync(join(home, 'dsh-tui.token'), 'utf8').trim()

const ws = new WebSocket(url)
let welcomed = false

ws.addEventListener('open', () => {
  console.log('connected')
  ws.send(JSON.stringify({ type: 'hello', token }))
})

ws.addEventListener('message', (event) => {
  let msg
  try { msg = JSON.parse(event.data) } catch { console.log('raw:', event.data); return }
  switch (msg.type) {
    case 'welcome': {
      console.log(`WELCOME session=${msg.sessionId} status=${msg.status}`)
      if (!welcomed) {
        welcomed = true
        setTimeout(() => {
          console.log('sending command /new …')
          ws.send(JSON.stringify({ type: 'command', line: '/new' }))
        }, 500)
      }
      break
    }
    case 'snapshot':
      console.log(`SNAPSHOT ${msg.events.length} events truncated=${msg.truncated}`)
      break
    case 'event':
      console.log(`EVENT [${msg.event.seq}] ${msg.event.type}`)
      break
    case 'error':
      console.log(`ERROR ${msg.code}: ${msg.message}`)
      break
    default:
      console.log(`MSG ${msg.type}`)
  }
})

ws.addEventListener('close', (e) => {
  console.log(`SOCKET CLOSED code=${e.code} reason=${e.reason}`)
  process.exit(0)
})
ws.addEventListener('error', (e) => { console.log('ws error:', e.message ?? e) })
setTimeout(() => { console.log('timeout — socket still open'); process.exit(0) }, 15000)
