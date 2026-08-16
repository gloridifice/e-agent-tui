// probe-online.mjs — is the dsh-tui bridge accepting upgrades right now?
const url = process.argv[2] ?? 'ws://127.0.0.1:3080/dsh-tui'
const ws = new WebSocket(url)
let done = false
const finish = (code, msg) => {
  if (done) return
  done = true
  console.log(msg)
  try { ws.close() } catch {}
  process.exit(code)
}
ws.addEventListener('open', () => finish(0, 'UPGRADE OK — bridge is online'))
ws.addEventListener('error', (e) => finish(1, `bridge NOT loaded (${e.message ?? 'handshake rejected'})`))
setTimeout(() => finish(1, 'timeout — bridge NOT loaded'), 8000)
