// Session-composition helpers (pure) — extracted from index.js. Everything
// here is independent of the WebSocket and model-selection adapter layers and
// covered by node --test test/compose.test.js.
import { statSync } from 'node:fs'
import { join } from 'node:path'

/** Root DSH home, matching the deployment's own resolution. */
export function dshHome() {
  return process.env.DSH_HOME
    ?? join(process.env.HOME ?? process.env.USERPROFILE ?? '.', '.dsh')
}

/** Whether `p` names an existing directory (the client's cwd claim). */
export function isExistingDirectory(p) {
  if (typeof p !== 'string' || p === '') return false
  try { return statSync(p).isDirectory() } catch { return false }
}

/** Latest `session/title` of a session's log, or undefined. */
export function latestTitle(events) {
  const list = Array.isArray(events) ? events : []
  for (let i = list.length - 1; i >= 0; i--) {
    if (list[i]?.type === 'session/title') return list[i].data?.title
  }
  return undefined
}

/** Preset id a session recorded: latest `agent-preset/selected`, else header. */
export function sessionPresetOf(meta, events) {
  const list = Array.isArray(events) ? events : []
  for (let i = list.length - 1; i >= 0; i--) {
    if (list[i]?.type === 'agent-preset/selected') return list[i].data?.agentPreset
  }
  return meta?.agentPreset
}
