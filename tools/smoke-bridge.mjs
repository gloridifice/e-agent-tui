// smoke-bridge.mjs — contract smoke over the DEPLOYED bridge copy.
// The bridge inlines installModelSelection from @deepseek-ai/dsh-agent
// (pinned to the DSH version the bridge deploys against). After every DSH
// upgrade, run this against the live profile to confirm the pure helpers
// still export and behave: node tools/smoke-bridge.mjs
import { existsSync } from 'node:fs'
import { join } from 'node:path'

const home = process.env.DSH_HOME ?? join(process.env.USERPROFILE ?? '.', '.dsh')
const entry = join(home, 'profiles/web/packages/dsh-tui-bridge/src/index.js')
if (!existsSync(entry)) {
  console.error(`smoke-bridge: deployed bridge not found at ${entry} (robocopy bridge/src first)`)
  process.exit(1)
}

const m = await import(`file:///${entry.replace(/\\/g, '/')}`)
const checks = []

function check(name, fn) {
  try {
    fn()
    checks.push([name, true, ''])
  } catch (error) {
    checks.push([name, false, String(error?.message ?? error)])
  }
}

check('exports apply/inject/name', () => {
  if (typeof m.apply !== 'function' || typeof m.inject === 'undefined' || typeof m.name !== 'string') {
    throw new Error('plugin surface missing')
  }
})

check('_trimToolResultEvent strips read results', () => {
  const e = {
    type: 'tool/result',
    data: { message: { content: [{ toolCallId: 'c1', content: [{ type: 'text', text: 'x'.repeat(5000) }] }] } },
  }
  const out = m._trimToolResultEvent(e, new Map([['c1', 'read']]))
  if (out.data.message.content[0].content[0].text !== '') throw new Error('read payload not stripped')
})

check('_trimToolResultEvent keeps the 2000-char tail', () => {
  const e = {
    type: 'tool/result',
    data: { message: { content: [{ toolCallId: 'c1', content: [{ type: 'text', text: `h${'x'.repeat(5000)}t` }] }] } },
  }
  const out = m._trimToolResultEvent(e, new Map([['c1', 'bash']]))
  const text = out.data.message.content[0].content[0].text
  if (text.length !== 2000 || !text.endsWith('t')) throw new Error(`tail wrong: ${text.length}`)
})

check('_latestTitle finds the newest title', () => {
  if (m._latestTitle([{ type: 'session/title', data: { title: 'T' } }]) !== 'T') throw new Error('title missed')
})

check('_sessionPresetOf prefers selected events', () => {
  if (m._sessionPresetOf({ agentPreset: 'a' }, [{ type: 'agent-preset/selected', data: { agentPreset: 'b' } }]) !== 'b') {
    throw new Error('preset resolution wrong')
  }
})

let failed = 0
for (const [name, ok, error] of checks) {
  if (ok) {
    console.log(`PASS  ${name}`)
  } else {
    failed += 1
    console.error(`FAIL  ${name}: ${error}`)
  }
}
if (failed > 0) {
  console.error(`smoke-bridge: ${failed} contract(s) drifted — the bridge may be out of sync with the installed DSH version`)
  process.exit(1)
}
console.log(`smoke-bridge: ${checks.length} contracts OK (${entry})`)
