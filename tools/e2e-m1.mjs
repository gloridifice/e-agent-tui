/**
 * e2e-m1.mjs — automated M1 protocol verification.
 * hello/welcome/snapshot/event-stream/ping-pong. Deliberately avoids `input`
 * and `interrupt` (side effects on the live agent); those are exercised
 * interactively in M2.
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'

const url = process.argv[2] ?? 'ws://127.0.0.1:3080/dsh-tui'
const home = process.env.DSH_HOME
  ?? join(process.env.HOME ?? process.env.USERPROFILE ?? '.', '.dsh')
const token = readFileSync(join(home, 'dsh-tui.token'), 'utf8').trim()

const report = { welcome: null, snapshotEvents: -1, eventCount: 0, eventTypes: [], pong: false, error: null }

const ws = new WebSocket(url)
const checks = {}

ws.addEventListener('open', () => {
  console.log('1. connected, sending hello')
  ws.send(JSON.stringify({ type: 'hello', token }))
  // negative auth check after positive one
})

ws.addEventListener('message', (event) => {
  const msg = JSON.parse(event.data)
  switch (msg.type) {
    case 'welcome':
      report.welcome = msg
      console.log(`2. WELCOME session=${msg.sessionId} status=${msg.status} provider=${msg.provider} model=${msg.model}`)
      ws.send(JSON.stringify({ type: 'ping' }))
      break
    case 'snapshot':
      report.snapshotEvents = msg.events.length
      console.log(`3. SNAPSHOT ${msg.events.length} historical events`)
      break
    case 'event':
      report.eventCount++
      if (!report.eventTypes.includes(msg.event.type)) report.eventTypes.push(msg.event.type)
      if (report.eventCount <= 8) console.log(`   live event [${msg.event.seq}] ${msg.event.type}`)
      break
    case 'status':
      console.log(`   agent status -> ${msg.status}`)
      break
    case 'pong':
      report.pong = true
      console.log('4. PONG received')
      break
    case 'error':
      report.error = msg
      console.log(`   ERROR ${msg.code}: ${msg.message}`)
      break
    default:
      console.log('   other:', JSON.stringify(msg).slice(0, 160))
  }
})

ws.addEventListener('error', (e) => {
  console.log('WS ERROR:', e.message ?? e)
  process.exit(1)
})

// Observe the live stream for a few seconds, then judge.
setTimeout(async () => {
  console.log('')
  console.log('--- M1 report ---')
  console.log('welcome:        ', report.welcome ? `OK (${report.welcome.sessionId})` : 'MISSING')
  console.log('snapshot:       ', report.snapshotEvents >= 0 ? `OK (${report.snapshotEvents} events)` : 'MISSING')
  console.log('live stream:    ', report.eventCount > 0 ? `OK (${report.eventCount} events: ${report.eventTypes.join(', ')})` : 'no live events during window')
  console.log('ping/pong:      ', report.pong ? 'OK' : 'MISSING')
  console.log('errors:         ', report.error ? `${report.error.code}: ${report.error.message}` : 'none')
  const ok = report.welcome && report.snapshotEvents >= 0 && report.pong && !report.error
  console.log(ok ? 'M1 PASS' : 'M1 FAIL')
  ws.close()
  process.exit(ok ? 0 : 1)
}, 6000)
