// hello-test.mjs — send a bare hello (fresh-process path) to the live bridge
// and print every frame until close, so the real creation error is visible.
// Uses the Node >=22 built-in global WebSocket (like probe-online.mjs).
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

const home = process.env.DSH_HOME ?? join(process.env.USERPROFILE ?? '.', '.dsh')
const token = readFileSync(join(home, 'dsh-tui.token'), 'utf8').trim()
const url = process.argv[2] ?? 'ws://127.0.0.1:3080/dsh-tui'
const mode = process.argv[3] // optional mode override

const ws = new WebSocket(url)
const cwd = process.env.TEST_CWD ?? process.cwd()
ws.addEventListener('open', () => {
  console.log('OPEN — sending hello {cwd, mode} without resumeSessionId')
  ws.send(JSON.stringify({ type: 'hello', token, cwd, mode: mode ?? 'standard' }))
})
ws.addEventListener('message', (event) => {
  const msg = JSON.parse(event.data.toString())
  if (msg.type === 'event') return // skip transcript events
  console.log('FRAME', JSON.stringify(msg).slice(0, 400))
})
ws.addEventListener('close', (event) => {
  console.log('CLOSE', event.code, event.reason)
  process.exit(0)
})
ws.addEventListener('error', (e) => {
  console.log('ERROR', e.message)
  process.exit(1)
})
setTimeout(() => {
  console.log('TIMEOUT — no close after 15s')
  process.exit(2)
}, 15000)
