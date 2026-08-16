// Session-composition helpers (pure + one Cordis hook pair) — extracted
// from index.js. Everything here is independent of the WebSocket layer and
// covered by node --test test/compose.test.js.
import { readFileSync, statSync } from 'node:fs'
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

/**
 * Agent-scoped model selection, inlined from @deepseek-ai/dsh-agent's
 * `installModelSelection` so the bridge keeps its tiny dependency surface
 * (webServer + dsh-llm + ws). The tradeoff is deliberate: this copy is
 * pinned to the DSH version the bridge deploys against (0.1.0-rc.6) — after
 * every DSH upgrade run `node tools/smoke-bridge.mjs` against the live
 * profile, which asserts these pure contracts still match.
 *
 * Without it the selected provider/model never reach prompt assembly — the
 * deployment persona's `{{model}}` variable stays unbound and every turn
 * fails with "prompt variable {{model}} has no value". The host's own
 * entry points (web `session.create`, headless) install this in `setup`,
 * and the bridge must do the same for every session it creates or resumes.
 *
 * `selection` is the mutable `{ current, assembled }` pair: `current` feeds
 * the persona/assembly variables, `assembled` snapshots it for request
 * routing so a later switch never splits the two surfaces mid-step.
 */
export function installModelSelection(agentCtx, selection) {
  const disposeAssembly = agentCtx.on('system-prompt/assemble', async (_assembly, _context, next) => {
    const selected = selection.current
    const assembled = await next()
    selection.assembled = selected
    if (selected === undefined) return assembled
    return {
      ...assembled,
      variables: { ...assembled.variables, provider: selected.provider, model: selected.model },
    }
  })
  const disposeRequest = agentCtx.on('agent/request', async (_payload, next) => {
    const resolved = await next()
    const selected = selection.assembled
    if (selected === undefined) return resolved
    const { reasoningEffort: _inheritedEffort, ...withoutInheritedEffort } = resolved
    return {
      ...withoutInheritedEffort,
      provider: selected.provider,
      model: selected.model,
      ...(selected.reasoningEffort === undefined ? {} : { reasoningEffort: selected.reasoningEffort }),
    }
  })
  return () => { disposeAssembly(); disposeRequest() }
}

/** Default model selection the host composes, or undefined without the service. */
export function defaultModelSelection(ctx) {
  const service = ctx?.get?.('agentDefaultModel')
  return typeof service?.currentSelection === 'function' ? service.currentSelection() : undefined
}
