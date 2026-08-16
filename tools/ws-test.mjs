/**
 * ws-test.mjs — quick protocol probe for the dsh-tui bridge.
 * Connects, authenticates, prints message summaries, forwards stdin lines as
 * user input and Ctrl+C as interrupt. Node >= 22 (built-in WebSocket client).
 *
 * Usage: node ws-test.mjs [url] [sessionId]
 */
import { readFileSync } from 'node:fs'
import { join } from 'node:path'
import readline from 'node:readline'

const url = process.argv[2] ?? 'ws://127.0.0.1:3080/dsh-tui'
const sessionId = process.argv[3]

const home = process.env.DSH_HOME
  ?? join(process.env.HOME ?? process.env.USERPROFILE ?? '.', '.dsh')
const token = readFileSync(join(home, 'dsh-tui.token'), 'utf8').trim()

const ws = new WebSocket(url)
console.log(`connecting ${url} …`)

ws.addEventListener('open', () => {
  ws.send(JSON.stringify({ type: 'hello', token, resumeSessionId: sessionId }))
  console.log('sent hello')
})

ws.addEventListener('message', (event) => {
  let msg
  try { msg = JSON.parse(event.data) } catch { console.log('raw:', event.data); return }
  switch (msg.type) {
    case 'welcome':
      console.log(`WELCOME session=${msg.sessionId} status=${msg.status} provider=${msg.provider} model=${msg.model}`)
      break
    case 'snapshot':
      console.log(`SNAPSHOT ${msg.events.length} events`)
      for (const e of msg.events) console.log(`  [${e.seq}] ${e.type}`)
      break
    case 'event':
      console.log(`EVENT [${msg.event.seq}] ${msg.event.type}`)
      break
    case 'status':
      console.log(`STATUS ${msg.status}`)
      break
    case 'error':
      console.log(`ERROR ${msg.code}: ${msg.message}`)
      process.exitCode = 1
      break
    case 'pong':
      console.log('pong')
      break
    default:
      console.log('msg:', JSON.stringify(msg).slice(0, 200))
  }
})

ws.addEventListener('close', () => { console.log('closed'); process.exit(process.exitCode ?? 0) })
ws.addEventListener('error', (e) => { console.log('ws error:', e.message ?? e) })

const rl = readline.createInterface({ input: process.stdin })
rl.on('line', (line) => {
  if (line.trim() === '') return
  if (ws.readyState === 1) ws.send(JSON.stringify({ type: 'input', text: line }))
})

process.on('SIGINT', () => {
  if (ws.readyState === 1) ws.send(JSON.stringify({ type: 'interrupt' }))
  else process.exit(0)
})
