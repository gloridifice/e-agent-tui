/**
 * dump-snapshot.mjs — capture a real surface-event sample from the live
 * session for offline smoke tests. Writes tools/cache/snapshot-sample.json.
 */
import { readFileSync, writeFileSync, mkdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const home = process.env.DSH_HOME ?? join(process.env.HOME ?? process.env.USERPROFILE ?? '.', '.dsh')
const token = readFileSync(join(home, 'dsh-tui.token'), 'utf8').trim()
const url = process.argv[2] ?? 'ws://127.0.0.1:3080/dsh-tui'

const SURFACE = new Set([
  'user/message', 'assistant/message', 'tool/call', 'tool/result',
  'turn/start', 'turn/end', 'todo/write',
])
const CAP = Number(process.env.DUMP_CAP ?? 2000)

const ws = new WebSocket(url)
let snapshot = null

ws.addEventListener('open', () => {
  ws.send(JSON.stringify({ type: 'hello', token }))
})

ws.addEventListener('message', (event) => {
  const msg = JSON.parse(event.data)
  if (msg.type === 'snapshot') {
    snapshot = msg.events.filter((e) => SURFACE.has(e.type)).slice(-CAP)
    const out = join(here, 'cache', 'snapshot-sample.json')
    mkdirSync(dirname(out), { recursive: true })
    writeFileSync(out, JSON.stringify(snapshot))
    console.log(`captured ${snapshot.length} surface events -> ${out}`)
    ws.close()
    process.exit(0)
  }
  if (msg.type === 'error') {
    console.log(`bridge error: ${msg.code} ${msg.message}`)
    process.exit(1)
  }
})

setTimeout(() => { console.log('timeout waiting for snapshot'); process.exit(1) }, 60000)
